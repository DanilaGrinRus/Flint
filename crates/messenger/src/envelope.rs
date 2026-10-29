//! Signed, encrypted envelope v1.
//!
//! Wire layout (all binary fields are base64 in JSON):
//! `version, id, recipient, created_at, ttl, ephemeral_key, nonce, ciphertext,
//! signature`.
//!
//! The sender address is **not** in the clear: it lives inside the encrypted
//! payload, and the recipient verifies the signature after decryption. The relay
//! therefore knows only the recipient, size and timestamps.

use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::address::Address;
use crate::crypto;
use crate::error::{Error, Result};
use crate::{DEFAULT_TTL_SECS, MAX_TTL_SECS, VERSION};

const DOMAIN: &[u8] = b"flint-messenger-envelope-v1";
const POW_DOMAIN: &[u8] = b"flint-messenger-pow-v1";

/// Work required for a new envelope. Debug builds use a lower value so the test
/// suite stays fast; release binaries use the production difficulty.
pub const POW_BITS: u8 = if cfg!(debug_assertions) { 12 } else { 20 };
/// Minimum accepted by a gateway/recipient that enforces proof of work.
pub const POW_MIN_BITS: u8 = if cfg!(debug_assertions) { 12 } else { 18 };
const POW_MAX_BITS: u8 = 24;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    pub version: u8,
    pub id: String,
    pub recipient: String,
    pub created_at: u64,
    pub ttl: u64,
    #[serde(with = "b64::arr32")]
    pub ephemeral_key: [u8; 32],
    #[serde(with = "b64::arr12")]
    pub nonce: [u8; 12],
    #[serde(with = "b64::vec")]
    pub ciphertext: Vec<u8>,
    /// Sender proof-of-work nonce. Missing/zero marks a pre-PoW envelope.
    #[serde(default)]
    pub pow_nonce: u64,
    #[serde(default)]
    pub pow_bits: u8,
    #[serde(with = "b64::arr64")]
    pub signature: [u8; 64],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Payload {
    pub sender: String,
    #[serde(flatten)]
    pub kind: MessageKind,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageKind {
    /// Slatepack armor. Plaintext armor (`encrypted == false`) is refused.
    Slatepack { armor: String, encrypted: bool },
    /// Short service text. No attachments/groups in v1.
    Text { text: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub id: String,
    pub sender: Address,
    pub kind: MessageKind,
    pub created_at: u64,
}

#[derive(Clone)]
pub struct Identity {
    signing: SigningKey,
    address: Address,
}

impl Identity {
    pub fn from_seed(seed: [u8; 32], hrp: &str) -> Result<Self> {
        let signing = SigningKey::from_bytes(&seed);
        let address = Address::from_key(hrp, signing.verifying_key().to_bytes())?;
        Ok(Self { signing, address })
    }

    pub fn generate(hrp: &str) -> Result<Self> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| Error::Crypto(format!("RNG: {e}")))?;
        Self::from_seed(seed, hrp)
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    /// Sign an arbitrary domain-separated transcript. Used by mailbox acks;
    /// envelope signatures use their own transcript in [`Envelope`].
    pub fn sign_bytes(&self, bytes: &[u8]) -> [u8; 64] {
        self.signing.sign(bytes).to_bytes()
    }

    fn seed(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }
}

impl Envelope {
    pub fn seal(
        sender: &Identity,
        recipient: &Address,
        kind: MessageKind,
        ttl: u64,
        now: u64,
    ) -> Result<Self> {
        validate_ttl(ttl)?;
        if matches!(
            kind,
            MessageKind::Slatepack {
                encrypted: false,
                ..
            }
        ) {
            return Err(Error::PlaintextSlatepack);
        }

        let id = Uuid::new_v4().to_string();
        let payload = Payload {
            sender: sender.address().to_string(),
            kind,
            created_at: now,
        };
        let plaintext =
            serde_json::to_vec(&payload).map_err(|e| Error::Envelope(format!("payload: {e}")))?;
        let recipient_text = recipient.to_string();
        let aad = header_bytes(VERSION, &id, &recipient_text, now, ttl);
        let sealed = crypto::seal(recipient, &plaintext, &aad)?;

        let mut envelope = Self {
            version: VERSION,
            id,
            recipient: recipient_text,
            created_at: now,
            ttl,
            ephemeral_key: sealed.ephemeral_key,
            nonce: sealed.nonce,
            ciphertext: sealed.ciphertext,
            pow_nonce: 0,
            pow_bits: POW_BITS,
            signature: [0u8; 64],
        };
        envelope.mine_pow();
        envelope.signature = sender.signing.sign(&envelope.transcript()).to_bytes();
        Ok(envelope)
    }

    fn mine_pow(&mut self) {
        let mut nonce = 0u64;
        loop {
            self.pow_nonce = nonce;
            if pow_is_valid(&self.pow_digest(nonce, self.pow_bits), self.pow_bits) {
                return;
            }
            nonce = nonce.wrapping_add(1);
        }
    }

    /// Verify a sender proof-of-work against `min_bits`. Envelopes without a
    /// proof are legacy and return `Ok(())` here; a gateway that requires PoW
    /// must check `pow_bits` itself.
    pub fn verify_pow(&self, min_bits: u8) -> Result<()> {
        if self.pow_bits == 0 {
            return Ok(());
        }
        if self.pow_bits < min_bits || self.pow_bits > POW_MAX_BITS {
            return Err(Error::Envelope(format!(
                "PoW: сложность {} вне диапазона {min_bits}..={POW_MAX_BITS}",
                self.pow_bits
            )));
        }
        let digest = self.pow_digest(self.pow_nonce, self.pow_bits);
        if !pow_is_valid(&digest, self.pow_bits) {
            return Err(Error::Envelope("PoW: не выполнена".into()));
        }
        Ok(())
    }

    fn pow_digest(&self, nonce: u64, bits: u8) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(POW_DOMAIN);
        hasher.update([self.version]);
        push_str_bytes(&mut hasher, self.id.as_bytes());
        push_str_bytes(&mut hasher, self.recipient.as_bytes());
        hasher.update(self.created_at.to_le_bytes());
        hasher.update(self.ttl.to_le_bytes());
        hasher.update(self.ephemeral_key);
        hasher.update(self.nonce);
        // The ciphertext is committed once; the mining loop only hashes the
        // small header/hash, not the whole payload.
        hasher.update(Sha256::digest(&self.ciphertext));
        hasher.update(nonce.to_le_bytes());
        hasher.update([bits]);
        let out = hasher.finalize();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&out);
        digest
    }

    pub fn seal_now(
        sender: &Identity,
        recipient: &Address,
        kind: MessageKind,
        ttl: u64,
    ) -> Result<Self> {
        Self::seal(sender, recipient, kind, ttl, unix_now())
    }

    pub fn seal_default(sender: &Identity, recipient: &Address, kind: MessageKind) -> Result<Self> {
        Self::seal_now(sender, recipient, kind, DEFAULT_TTL_SECS)
    }

    pub fn is_expired(&self, now: u64) -> bool {
        now > self.created_at.saturating_add(self.ttl)
    }

    pub fn open(&self, recipient: &Identity, now: u64) -> Result<Opened> {
        if self.version != VERSION {
            return Err(Error::Version(self.version));
        }
        if self.pow_bits > 0 {
            self.verify_pow(POW_MIN_BITS)?;
        }
        if Address::parse(&self.recipient)? != *recipient.address() {
            return Err(Error::WrongRecipient);
        }
        validate_ttl(self.ttl)?;
        if self.is_expired(now) {
            return Err(Error::Expired);
        }

        let aad = header_bytes(
            self.version,
            &self.id,
            &self.recipient,
            self.created_at,
            self.ttl,
        );
        let plaintext = crypto::open(
            &recipient.seed(),
            self.ephemeral_key,
            self.nonce,
            &self.ciphertext,
            &aad,
        )?;
        let payload: Payload = serde_json::from_slice(&plaintext)
            .map_err(|e| Error::Envelope(format!("payload: {e}")))?;
        if payload.created_at != self.created_at {
            return Err(Error::Envelope("время payload не совпадает".into()));
        }
        if matches!(
            payload.kind,
            MessageKind::Slatepack {
                encrypted: false,
                ..
            }
        ) {
            return Err(Error::PlaintextSlatepack);
        }
        let sender = Address::parse(&payload.sender)?;
        let verifying = VerifyingKey::from_bytes(&sender.key())
            .map_err(|e| Error::Envelope(format!("ключ отправителя: {e}")))?;
        let signature = Signature::from_bytes(&self.signature);
        verifying
            .verify(&self.transcript(), &signature)
            .map_err(|_| Error::BadSignature)?;

        Ok(Opened {
            id: self.id.clone(),
            sender,
            kind: payload.kind,
            created_at: self.created_at,
        })
    }

    /// Bytes covered by the sender signature. Also binds the ciphertext.
    fn transcript(&self) -> Vec<u8> {
        let mut out = DOMAIN.to_vec();
        out.push(self.version);
        push_str(&mut out, &self.id);
        push_str(&mut out, &self.recipient);
        out.extend_from_slice(&self.created_at.to_le_bytes());
        out.extend_from_slice(&self.ttl.to_le_bytes());
        out.extend_from_slice(&self.ephemeral_key);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&(self.ciphertext.len() as u64).to_le_bytes());
        out.extend_from_slice(&self.ciphertext);
        if self.pow_bits != 0 {
            out.extend_from_slice(&self.pow_nonce.to_le_bytes());
            out.push(self.pow_bits);
        }
        out
    }
}

fn validate_ttl(ttl: u64) -> Result<()> {
    if ttl == 0 || ttl > MAX_TTL_SECS {
        return Err(Error::BadTtl(ttl));
    }
    Ok(())
}

fn header_bytes(version: u8, id: &str, recipient: &str, created_at: u64, ttl: u64) -> Vec<u8> {
    let mut out = DOMAIN.to_vec();
    out.push(version);
    push_str(&mut out, id);
    push_str(&mut out, recipient);
    out.extend_from_slice(&created_at.to_le_bytes());
    out.extend_from_slice(&ttl.to_le_bytes());
    out
}

fn push_str(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u64).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn push_str_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn pow_is_valid(digest: &[u8; 32], bits: u8) -> bool {
    let full = (bits / 8) as usize;
    let rem = bits % 8;
    if digest[..full].iter().any(|byte| *byte != 0) {
        return false;
    }
    rem == 0 || digest[full] >> (8 - rem) == 0
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

mod b64 {
    use base64::{decode, encode};
    use serde::{Deserialize, Deserializer, Serializer};

    pub mod arr32 {
        use super::*;
        pub fn serialize<S>(value: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode(value))
        }
        pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
        where
            D: Deserializer<'de>,
        {
            let text = String::deserialize(deserializer)?;
            let bytes = decode(text).map_err(serde::de::Error::custom)?;
            bytes
                .try_into()
                .map_err(|_| serde::de::Error::custom("ожидались 32 байта"))
        }
    }

    pub mod arr12 {
        use super::*;
        pub fn serialize<S>(value: &[u8; 12], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode(value))
        }
        pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 12], D::Error>
        where
            D: Deserializer<'de>,
        {
            let text = String::deserialize(deserializer)?;
            let bytes = decode(text).map_err(serde::de::Error::custom)?;
            bytes
                .try_into()
                .map_err(|_| serde::de::Error::custom("ожидались 12 байт"))
        }
    }

    pub mod arr64 {
        use super::*;
        pub fn serialize<S>(value: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode(value))
        }
        pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
        where
            D: Deserializer<'de>,
        {
            let text = String::deserialize(deserializer)?;
            let bytes = decode(text).map_err(serde::de::Error::custom)?;
            bytes
                .try_into()
                .map_err(|_| serde::de::Error::custom("ожидались 64 байта"))
        }
    }

    pub mod vec {
        use super::*;
        pub fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(&encode(value))
        }
        pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let text = String::deserialize(deserializer)?;
            decode(text).map_err(serde::de::Error::custom)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_760_000_000;

    fn alice() -> Identity {
        Identity::from_seed([7u8; 32], "tgrin").expect("alice")
    }

    fn bob() -> Identity {
        Identity::from_seed([9u8; 32], "tgrin").expect("bob")
    }

    fn carol() -> Identity {
        Identity::from_seed([11u8; 32], "tgrin").expect("carol")
    }

    #[test]
    fn proof_of_work_is_mined_and_checked() {
        let alice = alice();
        let bob = bob();
        let envelope = Envelope::seal(
            &alice,
            bob.address(),
            MessageKind::Text { text: "pow".into() },
            DEFAULT_TTL_SECS,
            NOW,
        )
        .expect("seal");
        assert!(envelope.pow_bits >= POW_MIN_BITS);
        envelope
            .verify_pow(POW_MIN_BITS)
            .expect("mined proof must verify");
        assert!(envelope.open(&bob, NOW + 1).is_ok());

        let mut tampered = envelope.clone();
        tampered.pow_nonce ^= 1;
        assert!(tampered.verify_pow(POW_MIN_BITS).is_err());

        let mut stripped = envelope;
        stripped.pow_bits = 0;
        // Gateway policy rejects `pow_bits == 0`; `open` keeps reading legacy.
        assert!(stripped.open(&bob, NOW + 1).is_err());
    }

    #[test]
    fn roundtrip_hides_sender_and_body_from_the_relay() {
        let alice = alice();
        let bob = bob();
        let envelope = Envelope::seal(
            &alice,
            bob.address(),
            MessageKind::Text {
                text: "привет".into(),
            },
            DEFAULT_TTL_SECS,
            NOW,
        )
        .expect("seal");

        let wire = serde_json::to_string(&envelope).expect("json");
        assert!(!wire.contains(&alice.address().to_string()), "{wire}");
        assert!(!wire.contains("привет"), "{wire}");
        assert!(
            wire.contains(&bob.address().to_string()),
            "recipient is metadata"
        );

        let opened = envelope.open(&bob, NOW + 1).expect("open");
        assert_eq!(opened.sender, *alice.address());
        assert_eq!(
            opened.kind,
            MessageKind::Text {
                text: "привет".into()
            }
        );
    }

    #[test]
    fn encrypted_slatepack_is_accepted_plaintext_is_not() {
        let sealed = Envelope::seal(
            &alice(),
            bob().address(),
            MessageKind::Slatepack {
                armor: "BEGINSLATEPACK...".into(),
                encrypted: true,
            },
            DEFAULT_TTL_SECS,
            NOW,
        );
        assert!(sealed.is_ok());
        let plain = Envelope::seal(
            &alice(),
            bob().address(),
            MessageKind::Slatepack {
                armor: "BEGINSLATEPACK...".into(),
                encrypted: false,
            },
            DEFAULT_TTL_SECS,
            NOW,
        );
        assert!(matches!(plain, Err(Error::PlaintextSlatepack)));
    }

    #[test]
    fn wrong_recipient_cannot_open() {
        let envelope = Envelope::seal(
            &alice(),
            bob().address(),
            MessageKind::Text { text: "x".into() },
            DEFAULT_TTL_SECS,
            NOW,
        )
        .unwrap();
        assert!(matches!(
            envelope.open(&carol(), NOW + 1),
            Err(Error::WrongRecipient)
        ));
    }

    #[test]
    fn tampering_is_rejected() {
        let mut envelope = Envelope::seal(
            &alice(),
            bob().address(),
            MessageKind::Text { text: "x".into() },
            DEFAULT_TTL_SECS,
            NOW,
        )
        .unwrap();
        envelope.ciphertext[0] ^= 1;
        assert!(envelope.open(&bob(), NOW + 1).is_err());

        let mut envelope = Envelope::seal(
            &alice(),
            bob().address(),
            MessageKind::Text { text: "x".into() },
            DEFAULT_TTL_SECS,
            NOW,
        )
        .unwrap();
        envelope.signature[0] ^= 1;
        assert!(matches!(
            envelope.open(&bob(), NOW + 1),
            Err(Error::BadSignature)
        ));
    }

    #[test]
    fn expired_and_bad_ttl_are_rejected() {
        let envelope = Envelope::seal(
            &alice(),
            bob().address(),
            MessageKind::Text { text: "x".into() },
            60,
            NOW,
        )
        .unwrap();
        assert!(matches!(
            envelope.open(&bob(), NOW + 61),
            Err(Error::Expired)
        ));
        assert!(matches!(
            Envelope::seal(
                &alice(),
                bob().address(),
                MessageKind::Text { text: "x".into() },
                0,
                NOW
            ),
            Err(Error::BadTtl(0))
        ));
        assert!(matches!(
            Envelope::seal(
                &alice(),
                bob().address(),
                MessageKind::Text { text: "x".into() },
                MAX_TTL_SECS + 1,
                NOW
            ),
            Err(Error::BadTtl(_))
        ));
    }

    #[test]
    fn forged_sender_signature_is_rejected() {
        let alice = alice();
        let bob = bob();
        let mallory = Identity::from_seed([13u8; 32], "tgrin").unwrap();

        // Mallory encrypts a payload that *claims* to be Alice, but signs with
        // her own key. Bob must reject it after decryption.
        let payload = Payload {
            sender: alice.address().to_string(),
            kind: MessageKind::Text {
                text: "fake".into(),
            },
            created_at: NOW,
        };
        let plaintext = serde_json::to_vec(&payload).unwrap();
        let id = Uuid::new_v4().to_string();
        let recipient = bob.address().to_string();
        let aad = header_bytes(VERSION, &id, &recipient, NOW, DEFAULT_TTL_SECS);
        let sealed = crypto::seal(bob.address(), &plaintext, &aad).unwrap();
        let mut envelope = Envelope {
            version: VERSION,
            id,
            recipient,
            created_at: NOW,
            ttl: DEFAULT_TTL_SECS,
            ephemeral_key: sealed.ephemeral_key,
            nonce: sealed.nonce,
            ciphertext: sealed.ciphertext,
            pow_nonce: 0,
            pow_bits: 0,
            signature: [0u8; 64],
        };
        envelope.signature = mallory.signing.sign(&envelope.transcript()).to_bytes();
        assert!(matches!(
            envelope.open(&bob, NOW + 1),
            Err(Error::BadSignature)
        ));
    }

    #[test]
    fn default_ttl_is_26_hours() {
        assert_eq!(DEFAULT_TTL_SECS, 26 * 60 * 60);
    }

    #[test]
    fn known_address_vector() {
        // seed [0x07; 32], hrp "tgrin" — must stay stable across releases.
        assert_eq!(
            alice().address().to_string(),
            "tgrin1af9xcclzn3fq40h42pa3xtk9lx25wa4wh6l8hyjzrm4xj9zx6gkqcpfcf7"
        );
    }
}
