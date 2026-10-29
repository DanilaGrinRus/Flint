//! Loopback proxy between a Tor hidden service and the wallet's foreign API.
//!
//! The hidden service must never point at the owner API: anyone who learns the
//! onion address would otherwise reach administrative methods (`open_wallet`,
//! `init_send_tx`, `close_wallet`). This proxy forwards exactly one route —
//! `POST /v2/foreign` — and answers everything else with `403`.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_BODY: usize = 2 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Running proxy. Dropping it stops the accept loop.
pub struct ForeignProxy {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ForeignProxy {
    /// Start on a free loopback port, forwarding to `upstream_port` on
    /// `127.0.0.1`.
    pub fn start(upstream_port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let thread = thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        // On Windows an accepted socket inherits the listener's
                        // nonblocking flag; make it blocking before serving.
                        let _ = client.set_nonblocking(false);
                        let _ = client.set_read_timeout(Some(IO_TIMEOUT));
                        let _ = client.set_write_timeout(Some(IO_TIMEOUT));
                        thread::spawn(move || handle(client, upstream_port));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for ForeignProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Unblock `accept` so the thread notices the flag promptly.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn handle(mut client: TcpStream, upstream_port: u16) {
    let Some(request) = read_request(&mut client) else {
        let _ = respond(&mut client, 400, "text/plain", b"bad request");
        return;
    };
    if !request.allowed() {
        let _ = respond(
            &mut client,
            403,
            "application/json",
            br#"{"error":"only POST /v2/foreign is exposed"}"#,
        );
        return;
    }

    let mut upstream = match TcpStream::connect(("127.0.0.1", upstream_port)) {
        Ok(stream) => stream,
        Err(_) => {
            let _ = respond(&mut client, 502, "text/plain", b"wallet unavailable");
            return;
        }
    };
    let _ = upstream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = upstream.set_write_timeout(Some(IO_TIMEOUT));

    let forward = format!(
        "POST /v2/foreign HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        request.body.len()
    );
    if upstream.write_all(forward.as_bytes()).is_err() || upstream.write_all(&request.body).is_err()
    {
        let _ = respond(&mut client, 502, "text/plain", b"wallet unavailable");
        return;
    }

    let mut response = Vec::new();
    let _ = upstream.read_to_end(&mut response);
    let _ = upstream.shutdown(Shutdown::Both);
    if response.is_empty() {
        let _ = respond(&mut client, 502, "text/plain", b"empty wallet response");
        return;
    }
    let _ = client.write_all(&response);
    let _ = client.flush();
    // Half-close instead of a full shutdown: closing both directions at once can
    // reset the connection on Windows and truncate the response.
    let _ = client.shutdown(Shutdown::Write);
}

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

impl Request {
    /// The one route the hidden service is allowed to expose.
    fn allowed(&self) -> bool {
        self.method == "POST" && self.path == "/v2/foreign"
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > 64 * 1024 {
            return None;
        }
        if let Some(pos) = find_header_end(&buffer) {
            header_end = pos;
            break;
        }
    }

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();

    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().ok()?;
            }
        }
    }
    if content_length > MAX_BODY {
        return None;
    }

    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > MAX_BODY {
            return None;
        }
    }
    body.truncate(content_length);

    Some(Request { method, path, body })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        502 => "Bad Gateway",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// One-shot upstream: serve exactly one request and record what arrived.
    fn stub_upstream() -> (u16, Arc<Mutex<Vec<u8>>>, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_thread = seen.clone();
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            let header_end = loop {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                }
                if let Some(pos) = find_header_end(&buffer) {
                    break pos;
                }
            };
            let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
            let content_length: usize = head
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse().ok())
                .unwrap_or(0);
            while buffer.len() < header_end + 4 + content_length {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                }
            }
            *seen_thread.lock().unwrap() = buffer;
            let body = br#"{"jsonrpc":"2.0","result":{"Ok":"stub"}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        });
        (port, seen, handle)
    }

    /// Send a request and read the full response using Content-Length.
    fn call(port: u16, request: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut chunk).unwrap_or(0);
            if read == 0 {
                return (0, String::new());
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(pos) = find_header_end(&buffer) {
                break pos;
            }
        };
        let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let status: u16 = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let content_length: usize = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse().ok())
            .unwrap_or(0);
        while buffer.len() < header_end + 4 + content_length {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            }
        }
        let body = String::from_utf8_lossy(&buffer[header_end + 4..]).to_string();
        (status, body)
    }

    #[test]
    fn forwards_only_post_to_foreign_api() {
        let (upstream, seen, handle) = stub_upstream();
        let proxy = ForeignProxy::start(upstream).unwrap();

        let body = r#"{"method":"receive_tx","params":[]}"#;
        let (status, response) = call(
            proxy.port(),
            &format!(
                "POST /v2/foreign HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            ),
        );
        assert_eq!(status, 200, "foreign request must be forwarded: {response}");
        assert!(response.contains("stub"), "body not forwarded: {response}");
        let forwarded = String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        assert!(
            forwarded.contains("receive_tx") && forwarded.contains("/v2/foreign"),
            "upstream did not receive the foreign request: {forwarded}"
        );

        let (status, _) = call(
            proxy.port(),
            "POST /v3/owner HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}",
        );
        assert_eq!(status, 403, "owner route must be refused");

        let (status, _) = call(proxy.port(), "GET /v2/foreign HTTP/1.1\r\nHost: x\r\n\r\n");
        assert_eq!(status, 403, "GET must be refused");

        drop(proxy);
        let _ = handle.join();
    }
}
