//! The RPC backend with the Argon2id envelope (`docs/history/ARGON2ID.md` §6.3).
//!
//! The `grin-wallet` process starts from the envelope alone — there is no
//! `wallet.seed` at rest — and `open_with_envelope` materializes a transient
//! seed under a random password, checks the index-0 address against the
//! envelope, and removes the file again.
//!
//! Ignored by default: it spawns `resources/wallet/grin-wallet.exe`.
//!
//!   cargo test -p flint-core --test wallet_rpc_envelope -- --ignored --nocapture --test-threads=1

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use flint_core::rpc_backend::wallet_dir;
use flint_core::seed::SeedStore;
use flint_core::{address, AppPaths, LegacySeed, RpcBackend, WalletProcess};
use uuid::Uuid;

fn wallet_exe() -> PathBuf {
    if let Ok(path) = std::env::var("FLINT_WALLET_EXE") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/wallet/grin-wallet.exe")
}

fn init_wallet(dir: &Path, password: &str) {
    let status = Command::new(wallet_exe())
        .args([
            "--testnet",
            "-t",
            &dir.to_string_lossy(),
            "-p",
            password,
            "init",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("grin-wallet init");
    assert!(status.success(), "grin-wallet init завершился с ошибкой");
}

/// A wallet created by upstream, then converted to the envelope: upstream's
/// seed is read (the bootstrap password is known), the envelope written, and
/// the legacy file removed — the state a new Flint RPC wallet is in.
struct Fixture {
    root: PathBuf,
    paths: AppPaths,
    dir: PathBuf,
    store: SeedStore,
    address: String,
    user_password: String,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("flint-rpc-env-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        let paths = AppPaths::at(&root);
        let dir = wallet_dir(&paths, "testnet", None);
        std::fs::create_dir_all(&dir).expect("wallet dir");

        let bootstrap = "bootstrap-pass-123";
        init_wallet(&dir, bootstrap);
        let entropy =
            LegacySeed::read_file(&dir.join("wallet_data").join("wallet.seed"), bootstrap)
                .expect("читаем seed, созданный upstream");
        let address = address::address_from_entropy(entropy.as_slice(), true, 0).expect("адрес");

        let user_password = "user-envelope-pass".to_string();
        let store = SeedStore::new(&dir, "testnet");
        store
            .write_envelope(user_password.as_str(), &address, entropy.as_slice(), None)
            .expect("конверт");
        LegacySeed::remove(&dir.join("wallet_data").join("wallet.seed")).expect("удаляем seed");

        Self {
            root,
            paths,
            dir,
            store,
            address,
            user_password,
        }
    }

    fn legacy_seed(&self) -> PathBuf {
        self.dir.join("wallet_data").join("wallet.seed")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn envelope_only_wallet_starts_and_unlocks() {
    let f = Fixture::new("open");
    assert!(
        !f.legacy_seed().exists(),
        "в покое seed-файла быть не должно"
    );

    // The launch gate must accept the envelope with no wallet.seed present.
    let process = WalletProcess::start(&f.paths, "testnet", None).expect("процесс из конверта");
    let mut backend = RpcBackend::connect(&process.url(), process.secret(), 10).expect("connect");

    backend
        .open_with_envelope(&f.store, f.user_password.as_str(), Duration::from_secs(30))
        .expect("открытие по конверту");

    assert_eq!(backend.address_at(0).unwrap(), f.address);
    assert!(
        !f.legacy_seed().exists(),
        "временный wallet.seed должен быть удалён после сверки адреса"
    );

    backend.lock().expect("lock");
    assert!(!backend.unlocked());
    assert!(!f.legacy_seed().exists(), "lock не должен оставлять seed");
    drop(process);
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn wrong_password_leaves_no_seed_and_keeps_the_envelope() {
    let f = Fixture::new("wrongpw");
    let process = WalletProcess::start(&f.paths, "testnet", None).expect("процесс из конверта");
    let mut backend = RpcBackend::connect(&process.url(), process.secret(), 10).expect("connect");

    let err = backend
        .open_with_envelope(&f.store, "definitely-wrong", Duration::from_secs(30))
        .expect_err("неверный пароль должен быть отвергнут");
    eprintln!("rejected: {err}");
    assert!(
        !f.legacy_seed().exists(),
        "неверный пароль не должен оставлять временный seed"
    );
    assert_eq!(f.store.kind(), flint_core::seed::SeedKind::Envelope);

    // The right password still opens the same wallet afterwards.
    backend
        .open_with_envelope(&f.store, f.user_password.as_str(), Duration::from_secs(30))
        .expect("открытие после неудачной попытки");
    assert_eq!(backend.address_at(0).unwrap(), f.address);
    drop(process);
}

/// The restore path of the app (`restore_rpc`): the envelope is written first
/// from a generated phrase, upstream then creates the wallet under a random
/// password, and the transient seed is removed after the address check.
#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn restore_writes_the_envelope_before_upstream_creates_the_wallet() {
    let root = std::env::temp_dir().join(format!("flint-rpc-restore-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("root");
    let paths = AppPaths::at(&root);
    let dir = wallet_dir(&paths, "testnet", None);
    std::fs::create_dir_all(dir.join("wallet_data")).expect("wallet dir");
    let store = SeedStore::new(&dir, "testnet");

    let user_password = "user-restore-pass";
    let new_seed = store.generate().expect("фраза");
    let address =
        address::address_from_entropy(new_seed.entropy.as_slice(), true, 0).expect("адрес");
    store
        .write_envelope(user_password, &address, new_seed.entropy.as_slice(), None)
        .expect("конверт до создания кошелька");

    // Upstream creates the wallet from the phrase under a random password.
    let upstream = SeedStore::new_upstream_password().expect("случайный пароль");
    let words = new_seed.mnemonic.split_whitespace().count() as u32;
    let process =
        WalletProcess::start_uninitialized(&paths, "testnet", None).expect("empty process");
    {
        let rpc =
            flint_core::WalletRpc::connect(&process.url(), process.secret()).expect("connect");
        rpc.create_wallet(new_seed.mnemonic.as_str(), upstream.as_str(), words)
            .expect("create_wallet");
    }
    drop(process);
    assert!(
        dir.join("wallet_data").join("wallet.seed").is_file(),
        "upstream создаёт seed под случайным паролем"
    );

    // The process starts from the envelope; the transient file is removed only
    // after the address matches.
    let process = WalletProcess::start(&paths, "testnet", None).expect("процесс из конверта");
    let mut backend = RpcBackend::connect(&process.url(), process.secret(), 10).expect("connect");
    backend
        .open_with_envelope(&store, user_password, Duration::from_secs(30))
        .expect("открытие восстановленного кошелька");
    assert_eq!(backend.address_at(0).unwrap(), address);
    assert!(!dir.join("wallet_data").join("wallet.seed").exists());
    assert_eq!(store.kind(), flint_core::seed::SeedKind::Envelope);
    drop(process);

    let _ = std::fs::remove_dir_all(&root);
}
