//! CORS-stripping reverse proxy for the managed Grin node.
//!
//! The node binary (5.5.2) hardcodes `Access-Control-Allow-Origin: *` on its API
//! responses. We keep the node on an internal loopback port and expose a proxy on
//! the public loopback port instead:
//!
//! * only `/v2/owner` and `/v2/foreign` are forwarded;
//! * `Access-Control-*` response headers are removed, so a browser page cannot
//!   read answers cross-origin;
//! * `OPTIONS` (preflight) is refused without CORS headers;
//! * Authorization and bodies are passed through unchanged, so the app and
//!   `grin-wallet` keep working.
//!
//! Responses are streamed after the header block; the upstream connection is
//! forced to `Connection: close` so the body ends at EOF.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_HEADER: usize = 64 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);
const ALLOWED_PATHS: [&str; 2] = ["/v2/owner", "/v2/foreign"];

pub struct NodeCorsProxy {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NodeCorsProxy {
    pub fn start(listen_port: u16, upstream_port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", listen_port))?;
        let bound_port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let _ = client.set_nonblocking(false);
                        let _ = client.set_read_timeout(Some(TIMEOUT));
                        let _ = client.set_write_timeout(Some(TIMEOUT));
                        thread::spawn(move || {
                            let _ = handle(client, upstream_port);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            port: bound_port,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for NodeCorsProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

struct Head {
    method: String,
    path: String,
    content_length: usize,
    /// Client headers passed through unchanged (Authorization, Content-Type...).
    headers: Vec<String>,
}

fn handle(mut client: TcpStream, upstream_port: u16) -> std::io::Result<()> {
    let (head, rest) = match read_head(&mut client) {
        Some(parts) => parts,
        None => return respond(&mut client, 400, "bad request"),
    };
    if head.method == "OPTIONS" {
        // No CORS headers on purpose: browser preflight must fail.
        return respond(&mut client, 403, "preflight not allowed");
    }
    if !ALLOWED_PATHS
        .iter()
        .any(|allowed| head.path == *allowed || head.path.starts_with(&format!("{allowed}?")))
    {
        return respond(&mut client, 404, "not found");
    }
    if head.content_length > MAX_BODY {
        return respond(&mut client, 413, "payload too large");
    }

    let mut body = rest;
    while body.len() < head.content_length {
        let mut chunk = [0u8; 16 * 1024];
        match client.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    body.truncate(head.content_length);

    let mut upstream = TcpStream::connect(("127.0.0.1", upstream_port))?;
    upstream.set_read_timeout(Some(TIMEOUT))?;
    upstream.set_write_timeout(Some(TIMEOUT))?;
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1\r\n",
        head.method, head.path
    );
    for header in &head.headers {
        request.push_str(header);
        request.push_str("\r\n");
    }
    request.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    upstream.write_all(request.as_bytes())?;
    upstream.write_all(&body)?;

    // Upstream is forced to close, so the whole answer (headers + body) fits in
    // one buffered read and the body cannot be lost between header/body reads.
    let mut raw = Vec::new();
    if upstream.read_to_end(&mut raw).is_err() {
        return respond(&mut client, 502, "bad upstream");
    }
    let Some(header_end) = find_end(&raw) else {
        return respond(&mut client, 502, "bad upstream headers");
    };
    let head_text = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = head_text.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let mut out = String::new();
    out.push_str(status_line);
    out.push_str("\r\n");
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("access-control-") || lower.starts_with("connection:") {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    client.write_all(out.as_bytes())?;
    client.write_all(&raw[header_end + 4..])?;
    let _ = client.flush();
    let _ = client.shutdown(Shutdown::Write);
    Ok(())
}

/// Read request headers and whatever body bytes came with them.
fn read_head(stream: &mut TcpStream) -> Option<(Head, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEADER {
            return None;
        }
        if let Some(pos) = find_end(&buffer) {
            break pos;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let mut content_length = 0usize;
    let mut headers = Vec::new();
    for line in lines {
        match line.split_once(':') {
            Some((name, value)) if name.eq_ignore_ascii_case("content-length") => {
                content_length = value.trim().parse().unwrap_or(0);
            }
            Some((name, _))
                if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("connection") => {
            }
            Some(_) => headers.push(line.to_string()),
            None => {}
        }
    }
    Some((
        Head {
            method,
            path,
            content_length,
            headers,
        },
        buffer[header_end + 4..].to_vec(),
    ))
}

fn find_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

fn respond(stream: &mut TcpStream, status: u16, text: &str) -> std::io::Result<()> {
    let body = text.as_bytes();
    let head = format!(
        "HTTP/1.1 {status} Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_upstream() -> (u16, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut header_end = None;
                while let Ok(read) = stream.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(pos) = find_end(&buffer) {
                        header_end = Some(pos);
                        break;
                    }
                }
                // Drain the request body, otherwise closing with unread bytes
                // can reset the connection and lose the response we just wrote.
                if let Some(pos) = header_end {
                    let head = String::from_utf8_lossy(&buffer[..pos]).to_string();
                    let length = head
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    while buffer.len() < pos + 4 + length {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                    }
                }
                // Answer with a CORS header that must be stripped.
                let body = br#"{"ok":true}"#;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Credentials: true\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        (port, handle)
    }

    fn post(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    #[test]
    fn strips_cors_and_forwards_node_api() {
        let (upstream, handle) = stub_upstream();
        let proxy = NodeCorsProxy::start(0, upstream).unwrap();
        let request = "POST /v2/owner HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}";
        let response = post(proxy.port(), request);
        assert!(response.contains("200 OK"), "unexpected: {response}");
        assert!(response.contains("\"ok\":true"), "body lost: {response}");
        assert!(
            !response.to_ascii_lowercase().contains("access-control-"),
            "CORS must be stripped: {response}"
        );
        drop(proxy);
        let _ = handle.join();
    }

    #[test]
    fn preflight_and_foreign_paths_are_controlled() {
        // No upstream needed: OPTIONS and unknown paths are answered locally.
        let proxy = NodeCorsProxy::start(0, 9).unwrap();
        let options = post(
            proxy.port(),
            "OPTIONS /v2/owner HTTP/1.1\r\nHost: x\r\nOrigin: http://evil\r\n\r\n",
        );
        assert!(options.contains("403"), "preflight: {options}");
        assert!(!options.to_ascii_lowercase().contains("access-control-"));
        let other = post(
            proxy.port(),
            "POST /v1/status HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(other.contains("404"), "unknown path: {other}");
    }
}
