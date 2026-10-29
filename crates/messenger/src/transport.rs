//! Minimal HTTP transport for the loopback/Tor gateway (`/flint/v1/msg*`).
//!
//! Deliberately dependency-free: a small blocking HTTP/1.1 client over
//! `TcpStream`. The gateway is loopback behind the onion, so no TLS layer here;
//! Tor provides transport confidentiality. The envelope itself is the
//! signed/encrypted unit.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::Value;

use crate::ack::AckProof;
use crate::address::Address;
use crate::envelope::{unix_now, Envelope, Identity};
use crate::error::{Error, Result};
use crate::store::Transport;

const TIMEOUT: Duration = Duration::from_secs(15);
/// The first circuit to an onion can take noticeably longer than a clearnet
/// request: descriptor lookup and the rendezvous handshake happen before the
/// hidden service opens its local HTTP connection.
const ONION_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
/// Bound relay cleanup work per fetch; a hostile queue cannot make one inbox
/// call ack an unbounded number of ids.
const MAX_BAD_ACKS: usize = 32;

pub struct GatewayTransport {
    host: String,
    port: u16,
    /// Optional Tor SOCKS5 proxy (`host:port`). Required for `.onion` hosts.
    socks: Option<String>,
}

impl GatewayTransport {
    pub fn new(base_url: &str) -> Result<Self> {
        let rest = base_url
            .trim()
            .strip_prefix("http://")
            .ok_or_else(|| Error::Store("gateway: нужен http:// URL".into()))?;
        let rest = rest.trim_end_matches('/');
        let (host, port) = match rest.rsplit_once(':') {
            Some((host, port)) if port.parse::<u16>().is_ok() => {
                (host.to_string(), port.parse::<u16>().unwrap())
            }
            _ => (rest.to_string(), 80),
        };
        if host.is_empty() {
            return Err(Error::Store("gateway: пустой host".into()));
        }
        Ok(Self {
            host,
            port,
            socks: None,
        })
    }

    /// Route through a Tor SOCKS5 proxy. `socks` is e.g. `127.0.0.1:9050`.
    pub fn with_socks(base_url: &str, socks: &str) -> Result<Self> {
        let mut transport = Self::new(base_url)?;
        let socks = socks.trim();
        if socks.is_empty() {
            return Err(Error::Store("gateway: пустой SOCKS-адрес".into()));
        }
        transport.socks = Some(socks.to_string());
        Ok(transport)
    }

    pub fn is_onion(&self) -> bool {
        self.host.ends_with(".onion")
    }

    fn timeout(&self) -> Duration {
        if self.is_onion() {
            ONION_TIMEOUT
        } else {
            TIMEOUT
        }
    }

    fn connect(&self) -> Result<TcpStream> {
        if let Some(socks) = &self.socks {
            return socks5_connect(socks, &self.host, self.port, self.timeout());
        }
        if self.is_onion() {
            return Err(Error::Store(
                "gateway: для .onion нужен Tor SOCKS (with_socks)".into(),
            ));
        }
        let addr = format!("{}:{}", self.host, self.port);
        let sock = addr
            .to_socket_addrs()
            .map_err(|e| Error::Store(format!("gateway: {e}")))?
            .next()
            .ok_or_else(|| Error::Store("gateway: адрес не разрешился".into()))?;
        TcpStream::connect_timeout(&sock, self.timeout())
            .map_err(|e| Error::Store(format!("gateway: connect: {e}")))
    }

    fn request(&self, method: &str, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
        let timeout = self.timeout();
        let mut stream = self.connect()?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| Error::Store(format!("gateway: timeout: {e}")))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| Error::Store(format!("gateway: timeout: {e}")))?;

        let host_header = if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host_header}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| Error::Store(format!("gateway: write: {e}")))?;
        stream
            .write_all(body)
            .map_err(|e| Error::Store(format!("gateway: write: {e}")))?;

        let mut raw = Vec::new();
        stream
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|e| Error::Store(format!("gateway: read: {e}")))?;
        if raw.len() > MAX_RESPONSE {
            return Err(Error::Store("gateway: ответ слишком большой".into()));
        }
        let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
            return Err(Error::Store("gateway: нет заголовков ответа".into()));
        };
        let head = String::from_utf8_lossy(&raw[..pos]).to_string();
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| Error::Store("gateway: нет статуса".into()))?;
        Ok((status, raw[pos + 4..].to_vec()))
    }

    fn expect_ok(&self, status: u16, body: &[u8]) -> Result<()> {
        if (200..300).contains(&status) {
            return Ok(());
        }
        let text = String::from_utf8_lossy(body);
        Err(Error::Store(format!("gateway: HTTP {status}: {text}")))
    }
}

fn socks5_connect(socks: &str, target: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    let addr = socks
        .to_socket_addrs()
        .map_err(|e| Error::Store(format!("socks: {e}")))?
        .next()
        .ok_or_else(|| Error::Store("socks: адрес не разрешился".into()))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| Error::Store(format!("socks: connect: {e}")))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| Error::Store(format!("socks: timeout: {e}")))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| Error::Store(format!("socks: timeout: {e}")))?;

    // Greeting: SOCKS5, no authentication.
    stream
        .write_all(&[5, 1, 0])
        .map_err(|e| Error::Store(format!("socks: write: {e}")))?;
    let mut greeting = [0u8; 2];
    stream
        .read_exact(&mut greeting)
        .map_err(|e| Error::Store(format!("socks: read: {e}")))?;
    if greeting != [5, 0] {
        return Err(Error::Store("socks: прокси требует авторизацию".into()));
    }

    let target_bytes = target.as_bytes();
    if target_bytes.len() > 255 {
        return Err(Error::Store("socks: слишком длинный host".into()));
    }
    let mut request = Vec::with_capacity(7 + target_bytes.len());
    request.extend_from_slice(&[5, 1, 0, 3, target_bytes.len() as u8]);
    request.extend_from_slice(target_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .map_err(|e| Error::Store(format!("socks: write: {e}")))?;

    let mut reply = [0u8; 2];
    stream
        .read_exact(&mut reply)
        .map_err(|e| Error::Store(format!("socks: read: {e}")))?;
    if reply != [5, 0] {
        return Err(Error::Store(format!(
            "socks: CONNECT отклонён ({:#04x})",
            reply[1]
        )));
    }
    let mut reserved = [0u8; 1];
    stream
        .read_exact(&mut reserved)
        .map_err(|e| Error::Store(format!("socks: read: {e}")))?;
    let mut atyp = [0u8; 1];
    stream
        .read_exact(&mut atyp)
        .map_err(|e| Error::Store(format!("socks: read: {e}")))?;
    let skip = match atyp[0] {
        1 => 4 + 2,
        4 => 16 + 2,
        3 => {
            let mut len = [0u8; 1];
            stream
                .read_exact(&mut len)
                .map_err(|e| Error::Store(format!("socks: read: {e}")))?;
            len[0] as usize + 2
        }
        other => {
            return Err(Error::Store(format!(
                "socks: неизвестный ATYP {other:#04x}"
            )))
        }
    };
    let mut rest = vec![0u8; skip];
    stream
        .read_exact(&mut rest)
        .map_err(|e| Error::Store(format!("socks: read: {e}")))?;
    Ok(stream)
}

/// Direct peer-to-peer delivery over Tor.
///
/// Sending derives the recipient's v3 onion from the slatepack address and POSTs
/// the envelope to that onion. Fetching and acking use the wallet's own local
/// gateway, because the mailbox for this identity lives on this machine. No
/// third-party relay is involved; an offline recipient simply does not receive
/// the message (its gateway was not reachable).
pub struct DirectOnionTransport {
    local: String,
    socks: String,
}

impl DirectOnionTransport {
    /// `local` is the loopback gateway URL (`http://127.0.0.1:PORT`) that stores
    /// this identity's inbox; `socks` is the local Tor SOCKS5 endpoint.
    pub fn new(local: &str, socks: &str) -> Result<Self> {
        let local = local.trim().trim_end_matches('/').to_string();
        GatewayTransport::new(&local)?;
        let socks = socks.trim();
        if socks.is_empty() {
            return Err(Error::Store("direct onion: пустой SOCKS-адрес".into()));
        }
        Ok(Self {
            local,
            socks: socks.to_string(),
        })
    }

    fn local_transport(&self) -> Result<GatewayTransport> {
        GatewayTransport::new(&self.local)
    }
}

impl Transport for DirectOnionTransport {
    fn deliver(&self, envelope: &Envelope) -> Result<()> {
        let recipient = Address::parse(&envelope.recipient)?;
        let remote = GatewayTransport::with_socks(&recipient.onion_url(), &self.socks)?;
        remote.deliver(envelope)
    }

    fn fetch(&self, recipient: &Address, now: u64, limit: usize) -> Result<Vec<Envelope>> {
        self.local_transport()?.fetch(recipient, now, limit)
    }

    fn ack(&self, recipient: &Address, ids: &[String]) -> Result<usize> {
        self.local_transport()?.ack(recipient, ids)
    }

    fn ack_signed(&self, recipient: &Identity, ids: &[String]) -> Result<usize> {
        self.local_transport()?.ack_signed(recipient, ids)
    }
}

impl Transport for GatewayTransport {
    fn deliver(&self, envelope: &Envelope) -> Result<()> {
        let recipient: Address = envelope.recipient.parse()?;
        let payload = serde_json::to_string(envelope)
            .map_err(|e| Error::Envelope(format!("payload: {e}")))?;
        let body = serde_json::to_vec(&serde_json::json!({
            "id": envelope.id,
            "to": recipient.to_string(),
            "payload": payload,
            "ttl_secs": envelope.ttl,
        }))
        .map_err(|e| Error::Envelope(format!("json: {e}")))?;
        let (status, response) = self.request("POST", "/flint/v1/msg", &body)?;
        self.expect_ok(status, &response)
    }

    fn fetch(&self, recipient: &Address, now: u64, limit: usize) -> Result<Vec<Envelope>> {
        let to = recipient.to_string();
        let page = limit.max(1).min(100);
        let (status, response) =
            self.request("GET", &format!("/flint/v1/msg?to={to}&limit={page}"), b"")?;
        self.expect_ok(status, &response)?;
        let value: Value = serde_json::from_slice(&response)
            .map_err(|e| Error::Store(format!("gateway JSON: {e}")))?;
        let items = value
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        let mut rejected = Vec::new();
        // One malformed payload must not poison the whole batch. Parse every
        // item, skip only the bad one, and keep scanning until the requested
        // number of valid envelopes is reached; otherwise the first 20 broken
        // envelopes would hide good messages behind them forever. A bounded
        // number of malformed relay ids is acked as quarantine so they do not
        // occupy the queue until TTL.
        for item in items {
            let Some(payload) = item.get("payload").and_then(Value::as_str) else {
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    if rejected.len() < MAX_BAD_ACKS {
                        rejected.push(id.to_string());
                    }
                }
                continue;
            };
            let Ok(envelope) = serde_json::from_str::<Envelope>(payload) else {
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    if rejected.len() < MAX_BAD_ACKS {
                        rejected.push(id.to_string());
                    }
                }
                continue;
            };
            out.push(envelope);
            if out.len() >= limit.max(1) {
                break;
            }
        }
        // This is not a sender-authenticated ack: only malformed items are
        // removed and a relay that requires a signed ack rejects it harmlessly.
        if !rejected.is_empty() {
            let _ = self.ack(recipient, &rejected);
        }
        // `now` is the caller's clock; envelope TTL is enforced by `open`.
        let _ = now;
        Ok(out)
    }

    fn ack(&self, recipient: &Address, ids: &[String]) -> Result<usize> {
        let to = recipient.to_string();
        let body = serde_json::to_vec(&serde_json::json!({ "to": to }))
            .map_err(|e| Error::Store(format!("json: {e}")))?;
        self.ack_with_body(&body, ids)
    }

    fn ack_signed(&self, recipient: &Identity, ids: &[String]) -> Result<usize> {
        let mut acked = 0;
        for id in ids {
            let proof = AckProof::sign(recipient, id, unix_now());
            let body =
                serde_json::to_vec(&proof).map_err(|e| Error::Store(format!("ack proof: {e}")))?;
            acked += self.ack_one(id, &body)?;
        }
        Ok(acked)
    }
}

impl GatewayTransport {
    fn ack_with_body(&self, body: &[u8], ids: &[String]) -> Result<usize> {
        let mut acked = 0;
        for id in ids {
            acked += self.ack_one(id, body)?;
        }
        Ok(acked)
    }

    fn ack_one(&self, id: &str, body: &[u8]) -> Result<usize> {
        let (status, response) = self.request("POST", &format!("/flint/v1/msg/{id}/ack"), body)?;
        if status == 404 {
            return Ok(0);
        }
        self.expect_ok(status, &response)?;
        Ok(1)
    }
}
