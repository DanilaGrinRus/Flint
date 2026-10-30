//! `Transport` implementation over Nostr gift wraps (kind 1059).
//!
//! The recipient's Nostr key is resolved through [`Directory`]; the transport
//! never guesses a key from an address, because the address is an ed25519
//! slatepack key and the transport key is unrelated secp256k1 material.

use std::collections::HashMap;
use std::sync::Mutex;

use secp256k1::XOnlyPublicKey;

use super::event::{Event, KIND_DELETE, KIND_GIFT_WRAP};
use super::{gift_wrap, unwrap, NostrKeys, Relay, RelayFilter};
use crate::address::Address;
use crate::contacts::Contact;
use crate::envelope::Envelope;
use crate::error::{Error, Result};
use crate::store::Transport;
use crate::{DEFAULT_TTL_SECS, MAX_TTL_SECS};

/// Resolve the transport key of a peer by their Flint address.
pub trait Directory: Send + Sync {
    fn nostr_pubkey(&self, address: &Address) -> Option<XOnlyPublicKey>;
}

/// Nostr delivery for Flint envelopes with an explicit transport directory.
pub struct NostrTransport<R: Relay, D: Directory> {
    relay: R,
    keys: NostrKeys,
    directory: D,
    ttl: u64,
    /// Flint envelope id → Nostr event id, for the best-effort NIP-09 ACK.
    received: Mutex<HashMap<String, String>>,
}

/// Directory assembled from the encrypted contact book: the peer's address maps
/// to the Nostr transport key stored with that contact.
#[derive(Clone, Debug, Default)]
pub struct ContactDirectory {
    entries: HashMap<String, XOnlyPublicKey>,
}

impl ContactDirectory {
    /// Build from contacts; entries with an invalid or absent key are skipped.
    pub fn from_contacts(contacts: &[Contact]) -> Self {
        let mut entries = HashMap::new();
        for contact in contacts {
            let Some(hex) = contact.nostr.as_deref() else {
                continue;
            };
            if let Ok(key) = super::parse_public_hex(hex) {
                entries.insert(contact.address.clone(), key);
            }
        }
        Self { entries }
    }

    /// Add or replace one entry, validating the key.
    pub fn insert_hex(&mut self, address: &str, hex: &str) -> Result<()> {
        let key = super::parse_public_hex(hex)?;
        self.entries.insert(address.to_string(), key);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Directory for ContactDirectory {
    fn nostr_pubkey(&self, address: &Address) -> Option<XOnlyPublicKey> {
        self.entries.get(&address.to_string()).copied()
    }
}

impl<R: Relay, D: Directory> NostrTransport<R, D> {
    pub fn new(relay: R, keys: NostrKeys, directory: D) -> Self {
        Self {
            relay,
            keys,
            directory,
            ttl: DEFAULT_TTL_SECS,
            received: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_ttl(mut self, ttl: u64) -> Self {
        self.ttl = ttl.min(MAX_TTL_SECS);
        self
    }

    /// Transport identity other peers need to address this wallet.
    pub fn public_hex(&self) -> String {
        self.keys.public_hex()
    }
}

impl<R: Relay, D: Directory> Transport for NostrTransport<R, D> {
    fn deliver(&self, envelope: &Envelope) -> Result<()> {
        let recipient = Address::parse(&envelope.recipient)?;
        let peer = self.directory.nostr_pubkey(&recipient).ok_or_else(|| {
            Error::Store("nostr: неизвестный транспортный ключ получателя".into())
        })?;
        let content = serde_json::to_string(envelope)
            .map_err(|error| Error::Envelope(format!("nostr: envelope: {error}")))?;
        let event = gift_wrap(self.keys.secret(), &peer, content, super::unix_now_secs())?;
        self.relay.publish(&event)
    }

    fn fetch(&self, recipient: &Address, now: u64, limit: usize) -> Result<Vec<Envelope>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // NIP-59 randomizes the wrap timestamp by up to two days into the past,
        // so the subscription window must cover that jitter on top of the TTL.
        let window = self.ttl.saturating_add(172_800);
        let filter = RelayFilter {
            kinds: vec![KIND_GIFT_WRAP],
            recipient: Some(self.keys.public_hex()),
            since: now.saturating_sub(window),
            limit,
        };
        let events = self.relay.query(&filter)?;
        let mut received = self
            .received
            .lock()
            .map_err(|_| Error::Store("nostr: transport lock poisoned".into()))?;
        let mut out = Vec::new();
        for event in events {
            // A wrap we cannot open (foreign key, corrupt payload) is skipped:
            // one poison message must not lose the rest of the batch.
            let Ok((_sender, rumor)) = unwrap(&event, self.keys.secret()) else {
                continue;
            };
            let Ok(envelope) = serde_json::from_str::<Envelope>(&rumor.content) else {
                continue;
            };
            if envelope.recipient != recipient.to_string() {
                continue;
            }
            if envelope.ttl == 0 || envelope.ttl > MAX_TTL_SECS {
                continue;
            }
            if envelope.created_at.saturating_add(envelope.ttl) < now {
                continue;
            }
            if received.contains_key(&envelope.id) {
                continue;
            }
            received.insert(envelope.id.clone(), event.id.clone());
            out.push(envelope);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    fn ack(&self, _recipient: &Address, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut received = self
            .received
            .lock()
            .map_err(|_| Error::Store("nostr: transport lock poisoned".into()))?;
        let mut tags = Vec::new();
        for id in ids {
            if let Some(event_id) = received.remove(id) {
                tags.push(vec!["e".to_string(), event_id]);
            }
        }
        if tags.is_empty() {
            return Ok(0);
        }
        let count = tags.len();
        let event = Event::unsigned(
            self.keys.public_hex(),
            super::unix_now_secs(),
            KIND_DELETE,
            tags,
            String::new(),
        )
        .sign(self.keys.secret())?;
        self.relay.publish(&event)?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Identity, MessageKind};
    use crate::nostr::MemoryRelay;
    use std::str::FromStr;
    use std::sync::Arc;

    struct SharedRelay(Arc<MemoryRelay>);
    impl Relay for SharedRelay {
        fn publish(&self, event: &Event) -> Result<()> {
            self.0.publish(event)
        }
        fn query(&self, filter: &RelayFilter) -> Result<Vec<Event>> {
            self.0.query(filter)
        }
    }

    #[derive(Clone, Default)]
    struct MapDirectory(HashMap<String, XOnlyPublicKey>);
    impl Directory for MapDirectory {
        fn nostr_pubkey(&self, address: &Address) -> Option<XOnlyPublicKey> {
            self.0.get(&address.to_string()).copied()
        }
    }

    fn keys(hex: &str) -> NostrKeys {
        NostrKeys::from_hex(hex).expect("keys")
    }

    fn peer(keys: &NostrKeys) -> XOnlyPublicKey {
        keys.public()
    }

    #[test]
    fn deliver_fetch_ack() {
        let relay = Arc::new(MemoryRelay::new());
        let alice_keys = keys("0000000000000000000000000000000000000000000000000000000000000001");
        let bob_keys = keys("0000000000000000000000000000000000000000000000000000000000000002");

        let alice = Identity::from_seed([11u8; 32], "tgrin").expect("alice");
        let bob = Identity::from_seed([22u8; 32], "tgrin").expect("bob");

        let mut directory = MapDirectory::default();
        directory
            .0
            .insert(bob.address().to_string(), peer(&bob_keys));
        directory
            .0
            .insert(alice.address().to_string(), peer(&alice_keys));

        let alice_transport =
            NostrTransport::new(SharedRelay(relay.clone()), alice_keys, directory.clone());
        let bob_transport = NostrTransport::new(SharedRelay(relay.clone()), bob_keys, directory);

        assert_eq!(alice_transport.public_hex().len(), 64);

        let envelope = Envelope::seal_default(
            &alice,
            bob.address(),
            MessageKind::Text { text: "hi".into() },
        )
        .expect("seal");
        alice_transport.deliver(&envelope).expect("deliver");

        let now = envelope.created_at;
        let fetched = bob_transport.fetch(bob.address(), now, 10).expect("fetch");
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].id, envelope.id);

        // The same wrap is not returned twice.
        assert!(bob_transport
            .fetch(bob.address(), now, 10)
            .expect("fetch again")
            .is_empty());

        // Best-effort NIP-09 deletion removes it from the relay.
        assert_eq!(
            bob_transport
                .ack(bob.address(), std::slice::from_ref(&envelope.id))
                .expect("ack"),
            1
        );
        assert!(relay.stored().iter().any(|event| event.kind == KIND_DELETE));
    }

    #[test]
    fn contact_directory_maps_saved_keys() {
        use crate::envelope::Identity;
        let keys =
            NostrKeys::from_hex("0000000000000000000000000000000000000000000000000000000000000002")
                .expect("keys");
        let identity = Identity::from_seed([22u8; 32], "tgrin").expect("identity");
        let address = identity.address().to_string();
        let mut contact = Contact {
            address: address.clone(),
            name: "bob".into(),
            created_at: 0,
            updated_at: 0,
            nostr: Some(keys.public_hex()),
        };
        let directory = ContactDirectory::from_contacts(&[contact.clone()]);
        assert_eq!(directory.len(), 1);
        assert!(directory
            .nostr_pubkey(&address.parse().expect("address"))
            .is_some());

        contact.nostr = Some("zz".into());
        assert!(ContactDirectory::from_contacts(&[contact]).is_empty());
    }

    #[test]
    fn unknown_recipient_is_explicit() {
        let relay = Arc::new(MemoryRelay::new());
        let alice_keys =
            NostrKeys::from_hex("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("keys");
        let alice = Identity::from_seed([11u8; 32], "tgrin").expect("alice");
        let bob = Identity::from_seed([22u8; 32], "tgrin").expect("bob");
        let transport =
            NostrTransport::new(SharedRelay(relay), alice_keys, MapDirectory::default());
        let envelope = Envelope::seal_default(
            &alice,
            bob.address(),
            MessageKind::Text { text: "hi".into() },
        )
        .expect("seal");
        let error = transport.deliver(&envelope).expect_err("must fail");
        assert!(matches!(error, Error::Store(_)));
    }

    #[test]
    fn foreign_wrap_is_skipped() {
        let relay = Arc::new(MemoryRelay::new());
        let alice_keys =
            NostrKeys::from_hex("0000000000000000000000000000000000000000000000000000000000000001")
                .expect("keys");
        let eve_keys =
            NostrKeys::from_hex("0000000000000000000000000000000000000000000000000000000000000003")
                .expect("keys");
        let alice = Identity::from_seed([11u8; 32], "tgrin").expect("alice");

        // A wrap addressed to Eve, delivered into Alice's subscription window.
        let event = gift_wrap(
            &secp256k1::SecretKey::from_str(
                "0000000000000000000000000000000000000000000000000000000000000002",
            )
            .expect("secret"),
            &eve_keys.public(),
            "{}".into(),
            super::super::unix_now_secs(),
        )
        .expect("wrap");
        relay.publish(&event).expect("publish");

        let transport =
            NostrTransport::new(SharedRelay(relay), alice_keys, MapDirectory::default());
        let fetched = transport
            .fetch(alice.address(), super::super::unix_now_secs(), 10)
            .expect("fetch");
        assert!(fetched.is_empty());
    }
}
