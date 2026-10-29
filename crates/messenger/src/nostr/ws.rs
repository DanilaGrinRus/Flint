//! Blocking WebSocket relay client for the Nostr transport (NIP-01 subset).
//!
//! The transport contract allows one `wss://` relay; plain `ws://` is accepted
//! only for loopback tests. Publishing waits for the relay's `OK` for the exact
//! event id, and a query stops at `EOSE` or on the read timeout — an absent EOSE
//! never turns into an infinite wait.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use super::event::Event;
use super::relay::{Relay, RelayFilter};
use crate::error::{Error, Result};

/// Read/write budget for one relay operation after the handshake.
const IO_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a publish waits for the matching `OK`.
const PUBLISH_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a query waits for `EOSE` after the last event.
const QUERY_TIMEOUT: Duration = Duration::from_secs(20);

fn store(message: impl Into<String>) -> Error {
    Error::Store(format!("nostr/ws: {}", message.into()))
}

/// One relay endpoint. Connections are opened per operation and never pooled.
pub struct WsRelay {
    url: String,
}

impl WsRelay {
    /// Accept `wss://`; `ws://` only for loopback (unit tests, local drills).
    pub fn new(url: &str) -> Result<Self> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(store("empty relay URL"));
        }
        let lower = trimmed.to_ascii_lowercase();
        let loopback = lower.starts_with("ws://127.0.0.1")
            || lower.starts_with("ws://localhost")
            || lower.starts_with("ws://[::1]");
        if !(lower.starts_with("wss://") || loopback) {
            return Err(store("relay URL must be wss:// (ws:// only for loopback)"));
        }
        Ok(Self {
            url: trimmed.to_string(),
        })
    }

    fn open(&self) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
        install_crypto_provider();
        let (socket, _response) =
            connect(self.url.as_str()).map_err(|error| store(format!("connect: {error}")))?;
        match socket.get_ref() {
            MaybeTlsStream::Plain(stream) => {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
            }
            MaybeTlsStream::Rustls(stream) => {
                let _ = stream.sock.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.sock.set_write_timeout(Some(IO_TIMEOUT));
            }
            _ => {}
        }
        Ok(socket)
    }

    fn send(&self, socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, text: String) -> Result<()> {
        socket
            .send(Message::Text(text.into()))
            .map_err(|error| store(format!("send: {error}")))?;
        socket
            .flush()
            .map_err(|error| store(format!("flush: {error}")))
    }

    fn close(mut socket: WebSocket<MaybeTlsStream<TcpStream>>) {
        let _ = socket.close(None);
        let _ = socket.flush();
    }
}

impl Relay for WsRelay {
    fn publish(&self, event: &Event) -> Result<()> {
        let mut socket = self.open()?;
        self.send(&mut socket, publish_payload(event))?;
        let deadline = Instant::now() + PUBLISH_TIMEOUT;
        let outcome = loop {
            if Instant::now() > deadline {
                break Err(store("timeout waiting for OK"));
            }
            let message = match socket.read() {
                Ok(message) => message,
                Err(error) => break Err(store(format!("read: {error}"))),
            };
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Close(_) => break Err(store("relay closed the connection")),
                _ => continue,
            };
            match parse_message(&text) {
                Ok(RelayMessage::Ok {
                    id,
                    accepted,
                    message,
                }) => {
                    if id != event.id {
                        continue;
                    }
                    if accepted {
                        break Ok(());
                    }
                    break Err(store(format!("relay rejected the event: {message}")));
                }
                Ok(RelayMessage::Notice(message)) => {
                    // A NOTICE is advisory; keep waiting for the matching OK.
                    let _ = message;
                }
                Ok(RelayMessage::Closed { message, .. }) => {
                    break Err(store(format!("relay closed the subscription: {message}")));
                }
                Ok(_) => continue,
                Err(error) => break Err(error),
            }
        };
        Self::close(socket);
        outcome
    }

    fn query(&self, filter: &RelayFilter) -> Result<Vec<Event>> {
        let mut socket = self.open()?;
        let subscription = format!("flint-{}", std::process::id());
        self.send(&mut socket, req_payload(filter, &subscription))?;
        let mut events = Vec::new();
        let deadline = Instant::now() + QUERY_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                break;
            }
            let message = match socket.read() {
                Ok(message) => message,
                Err(error) => {
                    Self::close(socket);
                    return Err(store(format!("read: {error}")));
                }
            };
            let text = match message {
                Message::Text(text) => text.to_string(),
                Message::Close(_) => break,
                _ => continue,
            };
            match parse_message(&text)? {
                RelayMessage::Event(event) => {
                    if events.len() < filter.limit.max(1) {
                        events.push(event);
                    }
                }
                RelayMessage::Eose(id) if id == subscription => break,
                RelayMessage::Closed {
                    subscription: id,
                    message,
                } if id == subscription => {
                    Self::close(socket);
                    return Err(store(format!("relay closed the subscription: {message}")));
                }
                _ => continue,
            }
        }
        let _ = self.send(&mut socket, json!(["CLOSE", subscription]).to_string());
        Self::close(socket);
        Ok(events)
    }
}

/// `rustls` cannot infer a provider from crate features here (the vendor tree
/// enables neither `aws-lc-rs` nor `ring` by default), so install `ring` once.
fn install_crypto_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// `["EVENT", event]` as sent by a client.
fn publish_payload(event: &Event) -> String {
    json!(["EVENT", event]).to_string()
}

/// `["REQ", subscription, filter]` with only the fields in use.
fn req_payload(filter: &RelayFilter, subscription: &str) -> String {
    let mut object = serde_json::Map::new();
    if !filter.kinds.is_empty() {
        object.insert("kinds".into(), json!(filter.kinds));
    }
    if let Some(recipient) = &filter.recipient {
        object.insert("#p".into(), json!([recipient]));
    }
    object.insert("since".into(), json!(filter.since));
    if filter.limit > 0 {
        object.insert("limit".into(), json!(filter.limit));
    }
    json!(["REQ", subscription, Value::Object(object)]).to_string()
}

/// Parsed relay-to-client message (the subset this transport uses).
#[derive(Debug, PartialEq)]
pub(crate) enum RelayMessage {
    Ok {
        id: String,
        accepted: bool,
        message: String,
    },
    Event(Event),
    Eose(String),
    Notice(String),
    Closed {
        subscription: String,
        message: String,
    },
    Other,
}

fn parse_message(text: &str) -> Result<RelayMessage> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| store(format!("json: {error}")))?;
    let array = value
        .as_array()
        .ok_or_else(|| store("relay message is not an array"))?;
    let kind = array.first().and_then(Value::as_str).unwrap_or_default();
    let string_at = |index: usize| {
        array
            .get(index)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(match kind {
        "OK" => RelayMessage::Ok {
            id: string_at(1),
            accepted: array.get(2).and_then(Value::as_bool).unwrap_or(false),
            message: string_at(3),
        },
        "EVENT" => {
            let raw = array
                .get(2)
                .cloned()
                .ok_or_else(|| store("EVENT without payload"))?;
            let event: Event =
                serde_json::from_value(raw).map_err(|error| store(format!("event: {error}")))?;
            RelayMessage::Event(event)
        }
        "EOSE" => RelayMessage::Eose(string_at(1)),
        "NOTICE" => RelayMessage::Notice(string_at(1)),
        "CLOSED" => RelayMessage::Closed {
            subscription: string_at(1),
            message: string_at(2),
        },
        _ => RelayMessage::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn url_policy() {
        assert!(WsRelay::new("wss://relay.example.com").is_ok());
        assert!(WsRelay::new("ws://127.0.0.1:8080").is_ok());
        assert!(WsRelay::new("ws://localhost:7777").is_ok());
        assert!(WsRelay::new("ws://relay.example.com").is_err());
        assert!(WsRelay::new("https://relay.example.com").is_err());
        assert!(WsRelay::new("").is_err());
    }

    #[test]
    fn payloads_and_messages() {
        let event = Event::unsigned(
            "ab".repeat(32),
            1_700_000_000,
            1059,
            vec![vec!["p".into(), "cd".repeat(32)]],
            "payload".into(),
        )
        .sign(
            &secp256k1::SecretKey::from_str(
                "0000000000000000000000000000000000000000000000000000000000000001",
            )
            .expect("secret"),
        )
        .expect("sign");

        let published: Value = serde_json::from_str(&publish_payload(&event)).expect("json");
        assert_eq!(published[0], "EVENT");
        assert_eq!(published[1]["id"], event.id);

        let filter = RelayFilter {
            kinds: vec![1059],
            recipient: Some("cd".repeat(32)),
            since: 5,
            limit: 7,
        };
        let request: Value = serde_json::from_str(&req_payload(&filter, "sub")).expect("json");
        assert_eq!(request[0], "REQ");
        assert_eq!(request[1], "sub");
        assert_eq!(request[2]["kinds"][0], 1059);
        assert_eq!(request[2]["#p"][0], "cd".repeat(32));
        assert_eq!(request[2]["limit"], 7);

        assert!(matches!(
            parse_message(r#"["OK","id",true,"saved"]"#).expect("ok"),
            RelayMessage::Ok { accepted: true, .. }
        ));
        assert!(matches!(
            parse_message(r#"["EOSE","sub"]"#).expect("eose"),
            RelayMessage::Eose(id) if id == "sub"
        ));
        assert!(matches!(
            parse_message(r#"["CLOSED","sub","error: not found"]"#).expect("closed"),
            RelayMessage::Closed { .. }
        ));
        assert!(matches!(
            parse_message(r#"["NOTICE","slow down"]"#).expect("notice"),
            RelayMessage::Notice(_)
        ));
        let wrapped = json!(["EVENT", "sub", serde_json::to_value(&event).expect("value")]);
        assert!(matches!(
            parse_message(&wrapped.to_string()).expect("event"),
            RelayMessage::Event(parsed) if parsed.id == event.id
        ));
        assert!(matches!(
            parse_message(r#"["AUTH","challenge"]"#).expect("other"),
            RelayMessage::Other
        ));
        assert!(parse_message("not json").is_err());
        assert!(parse_message("{}").is_err());
    }

    /// Read-only network probe: REQ → EOSE on a live relay. Not run by default.
    /// Set `FLINT_NOSTR_LIVE_RELAY=wss://…` and run with `--ignored`.
    #[test]
    #[ignore = "network: requires FLINT_NOSTR_LIVE_RELAY"]
    fn live_relay_req_returns_eose() {
        let Ok(url) = std::env::var("FLINT_NOSTR_LIVE_RELAY") else {
            panic!("FLINT_NOSTR_LIVE_RELAY must be set for the live relay probe");
        };
        // The probe verifies TLS, WebSocket, REQ and EOSE. Kind 1 (public notes)
        // is used by default because many relays answer kind 1059 filters with
        // `CLOSED auth-required` (NIP-42), which v1 does not implement. Set
        // FLINT_NOSTR_LIVE_RELAY_KIND=1059 to observe that behaviour instead.
        let kind = std::env::var("FLINT_NOSTR_LIVE_RELAY_KIND")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1u32);
        let relay = WsRelay::new(&url).expect("relay url");
        let filter = RelayFilter {
            kinds: vec![kind],
            recipient: None,
            since: super::super::unix_now_secs().saturating_sub(600),
            limit: 3,
        };
        let events = relay.query(&filter).expect("query");
        assert!(events.len() <= 3);
        assert!(events.iter().all(|event| event.kind == kind));
        for event in &events {
            event.verify().expect("relay event must verify");
        }
    }
}
