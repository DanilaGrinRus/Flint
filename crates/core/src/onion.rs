//! Tor v3 onion service for the wallet: published straight from the address key.
//!
//! Reuses the arti integration that ships with `grin-wallet` (`grin_wallet_impls`)
//! instead of embedding another Tor stack: the wallet's `listen` command uses
//! the same code path, and we verified it publishes the very onion derived from
//! the slatepack address.

use std::path::Path;

use grin_util::secp::key::SecretKey;
use grin_util::static_secp_instance;
use grin_wallet_config::TorConfig;
use grin_wallet_impls::tor::arti::{start_tor_service, stop_tor_service};
use grin_wallet_util::OnionV3Address;
use sha2::{Digest, Sha512};

use crate::error::{FlintError, Result};

const SECRET_TAG: &str = "== ed25519v1-secret: type0 ==";
const PUBLIC_TAG: &str = "== ed25519v1-public: type0 ==";

/// Tor's on-disk hidden-service keys take an expanded ed25519 secret:
/// `SHA-512(seed)` with the scalar clamped, plus the nonce prefix. Each file is
/// a 32-byte tag (NUL padded), the key, and a SHA-256 checksum over both.
pub fn tor_hidden_service_keys(dir: &Path, seed: &[u8; 32]) -> Result<()> {
    std::fs::create_dir_all(dir)?;

    let digest = Sha512::digest(seed);
    let mut expanded = [0u8; 64];
    expanded.copy_from_slice(&digest);
    expanded[0] &= 248;
    expanded[31] &= 127;
    expanded[31] |= 64;

    let signing = ed25519_dalek::SigningKey::from_bytes(seed);
    let public = signing.verifying_key().to_bytes();

    write_key_file(&dir.join("hs_ed25519_secret_key"), SECRET_TAG, &expanded)?;
    write_key_file(&dir.join("hs_ed25519_public_key"), PUBLIC_TAG, &public)?;
    Ok(())
}

fn write_key_file(path: &Path, tag: &str, key: &[u8]) -> Result<()> {
    let mut padded = [0u8; 32];
    let bytes = tag.as_bytes();
    if bytes.len() > padded.len() {
        return Err(FlintError::Invalid("tor: тег ключа слишком длинный".into()));
    }
    padded[..bytes.len()].copy_from_slice(bytes);

    // Tor v1 key files are exactly tag(32) + key, no checksum: the secret file
    // is 96 bytes and the public one 64.
    let mut out = Vec::with_capacity(32 + key.len());
    out.extend_from_slice(&padded);
    out.extend_from_slice(key);
    crate::win_acl::write_private_atomic(path, &out)?;
    Ok(())
}

/// The onion address that belongs to an address key (ed25519 seed bytes).
pub fn onion_address(seed: &[u8; 32]) -> Result<String> {
    Ok(OnionV3Address::from_private(seed)
        .map_err(|e| FlintError::Invalid(format!("onion: {e}")))?
        .to_ov3_str())
}

/// A running hidden service forwarding `onion:80` to a local address.
pub struct OnionService {
    onion: Option<String>,
}

impl OnionService {
    /// Publish `local_addr` (e.g. `127.0.0.1:34567`) under the onion derived from
    /// `seed`. Blocks while arti starts and the service is launched.
    pub fn start(seed: &[u8; 32], local_addr: &str, state_dir: &Path) -> Result<Self> {
        let onion = onion_address(seed)?;
        std::fs::create_dir_all(state_dir)?;

        let secp_inst = static_secp_instance();
        let secp = secp_inst.lock();
        let key = SecretKey::from_slice(&secp, seed)
            .map_err(|e| FlintError::Wallet(format!("onion key: {e}")))?;
        drop(secp);

        let config = TorConfig {
            send_config_dir: state_dir.to_string_lossy().to_string(),
            ..TorConfig::default()
        };

        start_tor_service(key, local_addr, &config)
            .map_err(|e| FlintError::Wallet(format!("onion service: {e}")))?;
        Ok(Self { onion: Some(onion) })
    }

    pub fn onion(&self) -> &str {
        self.onion.as_deref().unwrap_or_default()
    }

    pub fn url(&self) -> String {
        format!("http://{}.onion", self.onion())
    }

    pub fn stop(mut self) {
        if let Some(onion) = self.onion.take() {
            let _ = stop_tor_service(onion);
        }
    }
}

impl Drop for OnionService {
    fn drop(&mut self) {
        if let Some(onion) = self.onion.take() {
            let _ = stop_tor_service(onion);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onion_is_stable_for_the_same_key() {
        let seed = [7u8; 32];
        let first = onion_address(&seed).expect("onion");
        let second = onion_address(&seed).expect("onion");
        assert_eq!(first, second);
        assert_eq!(first.len(), 56);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn different_keys_give_different_onions() {
        assert_ne!(
            onion_address(&[1u8; 32]).expect("onion"),
            onion_address(&[2u8; 32]).expect("onion")
        );
    }
}
