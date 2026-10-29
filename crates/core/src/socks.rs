//! Minimal SOCKS5 + HTTP/1.1 client for posting to a Tor onion.
//!
//! Deliberately dependency-free: the wallet only needs "POST this JSON to
//! `http://<onion>/v2/foreign` through a local Tor SOCKS port", and a full HTTP
//! client stack (with its TLS and proxy features) would be far more code than
//! the few hundred bytes on the wire.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::error::{FlintError, Result};

/// POST `body` to `http://{onion}{path}` through `proxy` (host:port).
pub fn http_post_onion(
    proxy: &str,
    onion: &str,
    path: &str,
    body: &str,
    timeout: Duration,
) -> Result<(u16, String)> {
    let addr: SocketAddr = proxy
        .parse()
        .map_err(|e| FlintError::Invalid(format!("socks: {e}")))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(FlintError::Io)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(FlintError::Io)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(FlintError::Io)?;

    socks5_connect(&mut stream, onion, 80)?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {onion}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(FlintError::Io)?;
    stream.flush().map_err(FlintError::Io)?;

    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => return Err(FlintError::Io(e)),
        }
        if raw.len() > 4 * 1024 * 1024 {
            return Err(FlintError::Invalid("socks: ответ слишком большой".into()));
        }
    }
    parse_response(&raw)
}

fn socks5_connect(stream: &mut TcpStream, host: &str, port: u16) -> Result<()> {
    stream
        .write_all(&[0x05, 0x01, 0x00])
        .map_err(FlintError::Io)?;
    let mut greeting = [0u8; 2];
    stream.read_exact(&mut greeting).map_err(FlintError::Io)?;
    if greeting != [0x05, 0x00] {
        return Err(FlintError::Invalid(format!(
            "socks: прокси отверг метод аутентификации {greeting:?}"
        )));
    }

    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(FlintError::Invalid("socks: слишком длинное имя".into()));
    }
    let mut request = vec![0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8];
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).map_err(FlintError::Io)?;

    let mut head = [0u8; 4];
    stream.read_exact(&mut head).map_err(FlintError::Io)?;
    if head[0] != 0x05 {
        return Err(FlintError::Invalid(
            "socks: неожиданная версия ответа".into(),
        ));
    }
    if head[1] != 0x00 {
        return Err(FlintError::Invalid(format!(
            "socks: соединение отклонено (код {})",
            head[1]
        )));
    }
    let addr_len = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).map_err(FlintError::Io)?;
            len[0] as usize
        }
        other => {
            return Err(FlintError::Invalid(format!(
                "socks: неизвестный тип адреса {other}"
            )))
        }
    };
    let mut skip = vec![0u8; addr_len + 2];
    stream.read_exact(&mut skip).map_err(FlintError::Io)?;
    Ok(())
}

fn parse_response(raw: &[u8]) -> Result<(u16, String)> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| FlintError::Invalid("http: нет заголовков в ответе".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| FlintError::Invalid(format!("http: плохой статус {status_line:?}")))?;

    let mut chunked = false;
    let mut content_length: Option<usize> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked") {
                chunked = true;
            }
            if name == "content-length" {
                content_length = value.parse().ok();
            }
        }
    }

    let body = &raw[split + 4..];
    let text = if chunked {
        decode_chunked(body)?
    } else if let Some(len) = content_length {
        String::from_utf8_lossy(&body[..body.len().min(len)]).to_string()
    } else {
        String::from_utf8_lossy(body).to_string()
    };
    Ok((status, text))
}

fn decode_chunked(mut body: &[u8]) -> Result<String> {
    let mut out = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| FlintError::Invalid("http: оборван chunked".into()))?;
        let size_text = String::from_utf8_lossy(&body[..line_end]);
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|e| FlintError::Invalid(format!("http: chunk size: {e}")))?;
        body = &body[line_end + 2..];
        if size == 0 {
            break;
        }
        if body.len() < size + 2 {
            return Err(FlintError::Invalid("http: chunk короче заявленного".into()));
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Pretends to be a Tor SOCKS proxy plus the onion endpoint behind it.
    fn stub_proxy(response: Vec<u8>) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut greeting = [0u8; 3];
            stream.read_exact(&mut greeting).expect("greeting");
            stream.write_all(&[0x05, 0x00]).expect("greeting reply");
            let mut head = [0u8; 5];
            stream.read_exact(&mut head).expect("connect");
            let mut host = vec![0u8; head[4] as usize];
            stream.read_exact(&mut host).expect("host");
            let mut port = [0u8; 2];
            stream.read_exact(&mut port).expect("port");
            stream
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .expect("connect reply");

            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).expect("request");
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let text = String::from_utf8_lossy(&request);
                    if let Some(len) = text
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                    {
                        let header_end = request
                            .windows(4)
                            .position(|w| w == b"\r\n\r\n")
                            .map(|p| p + 4)
                            .unwrap_or(0);
                        if request.len() >= header_end + len {
                            break;
                        }
                    }
                }
            }
            stream.write_all(&response).expect("response");
            String::from_utf8_lossy(&request).to_string()
        });
        (port, handle)
    }

    #[test]
    fn posts_json_through_socks() {
        let (port, handle) = stub_proxy(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 18\r\n\r\n{\"result\":\"hello\"}"
                .to_vec(),
        );
        let (status, body) = http_post_onion(
            &format!("127.0.0.1:{port}"),
            "abcdefghijklmnop.onion",
            "/v2/foreign",
            "{\"jsonrpc\":\"2.0\"}",
            Duration::from_secs(5),
        )
        .expect("post");
        assert_eq!(status, 200);
        assert_eq!(body, "{\"result\":\"hello\"}");
        let request = handle.join().expect("stub");
        assert!(request.contains("Host: abcdefghijklmnop.onion"));
        assert!(request.contains("{\"jsonrpc\":\"2.0\"}"));
    }

    #[test]
    fn decodes_chunked_responses() {
        let (port, _handle) = stub_proxy(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
                .to_vec(),
        );
        let (status, body) = http_post_onion(
            &format!("127.0.0.1:{port}"),
            "x.onion",
            "/v2/foreign",
            "{}",
            Duration::from_secs(5),
        )
        .expect("post");
        assert_eq!(status, 200);
        assert_eq!(body, "hello");
    }
}
