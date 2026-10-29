//! Cross-checks between Flint's seed storage and the upstream `grin-wallet` code.
//!
//! Upstream keeps its seed types private, so compatibility is tested through the
//! real lifecycle: a wallet created by Flint must hold the mnemonic entropy in
//! its Argon2id envelope, and a legacy `wallet.seed` written by Flint must be
//! opened by upstream with the password Flint used.
//!
//! No node is needed — creation and unlock are offline operations.

use std::path::PathBuf;

use flint_core::{AppPaths, LegacySeed, SeedEnvelope, WalletService};

fn temp_root(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "flint-codec-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p
}

fn wallet_home(root: &PathBuf) -> PathBuf {
    root.join("testnet").join("wallet")
}

fn legacy_seed_path(root: &PathBuf) -> PathBuf {
    wallet_home(root).join("wallet_data").join("wallet.seed")
}

const PASSWORD: &str = "correct horse battery";

#[test]
fn envelope_holds_the_mnemonic_entropy_and_no_seed_file() {
    let root = temp_root("envelope");
    let mut service = WalletService::open_at(AppPaths::at(&root));
    let created = service.create_wallet(PASSWORD).expect("create wallet");

    assert!(
        !legacy_seed_path(&root).exists(),
        "a new wallet must not keep wallet.seed at rest"
    );
    let envelope =
        SeedEnvelope::read(&SeedEnvelope::path_in(&wallet_home(&root))).expect("envelope on disk");
    let opened = envelope.open(PASSWORD).expect("envelope password");
    let from_words = grin_keychain::mnemonic::to_entropy(&created.mnemonic).expect("mnemonic");
    assert_eq!(opened.seed.as_slice(), from_words.as_slice());
    assert_eq!(envelope.address(), created.address);

    // Locking must still leave no seed file behind.
    service.lock();
    assert!(!legacy_seed_path(&root).exists());

    // The same password reopens the wallet and derives the same address.
    service.open_wallet(PASSWORD).expect("reopen");
    assert_eq!(service.address().expect("address"), created.address);
    assert!(service.open_wallet("wrong password").is_err());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn upstream_opens_the_legacy_seed_file_flint_wrote() {
    let root = temp_root("legacy");
    let mut service = WalletService::open_at(AppPaths::at(&root));
    let created = service.create_wallet(PASSWORD).expect("create wallet");
    service.lock();

    let envelope_path = SeedEnvelope::path_in(&wallet_home(&root));
    let opened = SeedEnvelope::read(&envelope_path)
        .expect("envelope")
        .open(PASSWORD)
        .expect("open envelope");
    // Simulate the legacy branch: drop the envelope, write upstream's format
    // with the same entropy under a different password, and let upstream open it.
    std::fs::remove_file(&envelope_path).expect("remove envelope");
    LegacySeed::write_atomic(
        &legacy_seed_path(&root),
        opened.seed.as_slice(),
        "another passphrase 42",
    )
    .expect("write legacy seed");
    let written = std::fs::read(legacy_seed_path(&root)).expect("seed bytes");

    assert!(
        service.open_wallet(PASSWORD).is_err(),
        "the user password must not open the legacy file it did not encrypt"
    );
    service
        .open_wallet("another passphrase 42")
        .expect("upstream opens Flint's seed file");
    assert_eq!(
        service.address().expect("address"),
        created.address,
        "the same seed must derive the same address"
    );
    // The hard rule: a legacy seed is read, never rewritten by an unlock.
    service.lock();
    assert_eq!(
        std::fs::read(legacy_seed_path(&root)).expect("seed bytes"),
        written,
        "opening and locking a legacy wallet must not touch wallet.seed"
    );

    let _ = std::fs::remove_dir_all(root);
}
