//! Slatepack address ↔ Tor v3 onion helpers.
//!
//! A slatepack address is a bech32-encoded ed25519 public key. A Tor v3 onion
//! address is the same key, base32-encoded with a checksum and a version byte.
//! Upstream relies on this too: `grin-wallet listen` derives its onion from the
//! wallet's address key, and payment proofs encode the address as an onion.
//!
//! The point for Flint: knowing somebody's `grin1…`/`tgrin1…` is enough to
//! compute their `.onion`, so direct Tor delivery needs no directory.

use bech32::{self, FromBase32, ToBase32};
use grin_wallet_util::OnionV3Address;

use crate::error::{FlintError, Result};

/// Human-readable prefixes we accept (mainnet / testnet).
const HRPS: [&str; 2] = ["grin", "tgrin"];

/// Compute the Tor v3 onion address that corresponds to a slatepack address.
pub fn onion_from_address(address: &str) -> Result<String> {
    let (hrp, data) = bech32::decode(address.trim())
        .map_err(|e| FlintError::Invalid(format!("адрес: не bech32 ({e})")))?;
    if !HRPS.contains(&hrp.as_str()) {
        return Err(FlintError::Invalid(format!(
            "адрес: неизвестный префикс {hrp:?}"
        )));
    }
    let bytes = Vec::<u8>::from_base32(&data)
        .map_err(|e| FlintError::Invalid(format!("адрес: не base32 ({e})")))?;
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| FlintError::Invalid("адрес: ожидались 32 байта ключа".into()))?;
    Ok(OnionV3Address::from_bytes(key).to_ov3_str())
}

/// Inverse helper: onion → the ed25519 key bytes (useful for diagnostics).
pub fn onion_key_bytes(onion: &str) -> Result<[u8; 32]> {
    let parsed: OnionV3Address = onion
        .try_into()
        .map_err(|e| FlintError::Invalid(format!("onion: {e}")))?;
    Ok(*parsed.as_bytes())
}

/// Encode raw ed25519 key bytes into a slatepack address (tests, tooling).
pub fn address_from_key(hrp: &str, key: &[u8; 32]) -> Result<String> {
    bech32::encode(hrp, key.to_base32()).map_err(|e| FlintError::Invalid(format!("bech32: {e}")))
}

/// Derive the slatepack address at `m/0/1/<index>` from raw seed entropy,
/// exactly as upstream does (`address_from_derivation_path` + ed25519).
///
/// This lets Flint write the envelope before the wallet exists: the address is
/// what binds the envelope to a wallet, and it must be known offline.
pub fn address_from_entropy(entropy: &[u8], testnet: bool, index: u32) -> Result<String> {
    use grin_keychain::extkey_bip32::ChildNumber;
    use grin_keychain::{ExtKeychain, Identifier, Keychain, SwitchCommitmentType};

    let keychain = ExtKeychain::from_seed(entropy, testnet)
        .map_err(|e| FlintError::Invalid(format!("seed: ключи не выводятся ({e})")))?;
    // Upstream's default parent path is `m/0/0` (WalletBackend::default_path);
    // address paths replace the second child with 1 and append the index.
    let parent = <ExtKeychain as Keychain>::derive_key_id(2, 0, 0, 0, 0);
    let mut path = parent.to_path();
    path.path[1] = ChildNumber::from(1);
    path.depth += 1;
    path.path[path.depth as usize - 1] = ChildNumber::from(index);
    let id = Identifier::from_path(&path);
    let secret = keychain
        .derive_key(0, &id, SwitchCommitmentType::None)
        .map_err(|e| FlintError::Invalid(format!("seed: ключ адреса не выводится ({e})")))?;
    let hashed = blake2_rfc::blake2b::blake2b(32, &[], &secret.0);
    let seed: [u8; 32] = hashed
        .as_bytes()
        .try_into()
        .map_err(|_| FlintError::Invalid("seed: неверная длина ключа адреса".into()))?;
    let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public = signing.verifying_key().to_bytes();
    address_from_key(if testnet { "tgrin" } else { "grin" }, &public)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vector from upstream `ov3.rs`: this key must produce this onion.
    const KEY: [u8; 32] = [
        0xd0, 0x3c, 0x09, 0xe9, 0xc1, 0x9b, 0xb7, 0x4a, 0xa9, 0xea, 0x44, 0xe0, 0xfe, 0x5a, 0xe2,
        0x37, 0xa9, 0xbf, 0x40, 0xbd, 0xdf, 0x09, 0x41, 0x06, 0x4a, 0x80, 0x91, 0x3a, 0x44, 0x59,
        0xc8, 0xbb,
    ];
    const ONION: &str = "2a6at2obto3uvkpkitqp4wxcg6u36qf534eucbskqciturczzc5suyid";

    #[test]
    fn address_gives_the_same_onion_as_upstream() {
        let address = address_from_key("tgrin", &KEY).expect("encode");
        assert_eq!(onion_from_address(&address).expect("derive"), ONION);
    }

    #[test]
    fn onion_roundtrips_to_the_same_key() {
        assert_eq!(onion_key_bytes(ONION).expect("parse"), KEY);
    }

    #[test]
    fn junk_is_rejected() {
        assert!(onion_from_address("tgrin1notreally").is_err());
        assert!(onion_from_address("bc1qsomethingelse").is_err());
        assert!(onion_from_address("").is_err());
    }
}
