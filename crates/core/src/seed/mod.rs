//! Seed storage for Flint.
//!
//! Two formats meet here:
//!
//! * [`envelope`] — Flint's own versioned Argon2id container
//!   (`wallet.seed.v2`), the source of truth once a wallet uses it
//!   (design: `docs/history/ARGON2ID.md`).
//! * [`legacy`] — the exact `wallet.seed` format of upstream `grin-wallet`
//!   5.5.x (PBKDF2-HMAC-SHA512, 100 iterations). It is still read for wallets
//!   that were created before the envelope, and it is written only as a
//!   transient file, with a random high-entropy password, while the wallet is
//!   open. The upstream KDF is never the protection of a user password in a
//!   migrated or newly created wallet.
//!
//! No function here logs or formats a seed, a password or a derived key.

pub mod envelope;
pub mod legacy;
pub mod store;

pub use envelope::{OpenedSeed, SeedEnvelope, ENVELOPE_FILE};
pub use legacy::{LegacySeed, SEED_FILE};
pub use store::{MigrationOutcome, NewSeed, PreparedUnlock, SeedKind, SeedStore};

use zeroize::Zeroizing;

/// Entropy behind a BIP39 phrase, validated exactly as upstream does it. The
/// phrase itself is never stored; the envelope keeps this entropy.
pub fn entropy_from_mnemonic(phrase: &str) -> crate::error::Result<Zeroizing<Vec<u8>>> {
    grin_keychain::mnemonic::to_entropy(phrase.trim())
        .map(Zeroizing::new)
        .map_err(|_| crate::error::FlintError::Invalid("фраза не является мнемоникой Grin".into()))
}
