//! Concurrent seed mutations must leave one recoverable wallet after restart.

use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use flint_core::seed::{LegacySeed, SeedEnvelope, SeedKind, SeedStore};
use uuid::Uuid;

const PASSWORD: &str = "Seed-concurrency!42";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("flint-seed-concurrency-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(path.join("wallet_data")).expect("create seed test root");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn legacy_store(root: &TempRoot) -> (SeedStore, Vec<u8>, String) {
    let store = SeedStore::new(&root.0, "testnet");
    let entropy: Vec<u8> = (0u8..32).collect();
    LegacySeed::write_atomic(&store.legacy_path(), &entropy, PASSWORD).expect("write legacy seed");
    let address = flint_core::address::address_from_entropy(&entropy, true, 0).expect("address");
    (store, entropy, address)
}

fn assert_envelope(store: &SeedStore, entropy: &[u8], address: &str) {
    assert_eq!(store.kind(), SeedKind::Envelope);
    assert!(!store.legacy_path().exists(), "weak legacy seed survived");
    let envelope = SeedEnvelope::read(&store.envelope_path()).expect("read envelope");
    assert_eq!(envelope.address(), address);
    let opened = envelope.open(PASSWORD).expect("open envelope");
    assert_eq!(opened.seed.as_slice(), entropy);
}

fn assert_legacy(store: &SeedStore, entropy: &[u8]) {
    assert_eq!(store.kind(), SeedKind::Legacy);
    assert!(!store.envelope_path().exists());
    let opened = LegacySeed::read_file(&store.legacy_path(), PASSWORD).expect("open legacy seed");
    assert_eq!(opened.as_slice(), entropy);
}

#[test]
fn concurrent_migrations_leave_one_valid_envelope_after_restart() {
    let root = TempRoot::new("migrate");
    let (store, entropy, address) = legacy_store(&root);
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let store = SeedStore::new(&root.0, "testnet");
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            store.migrate_legacy(PASSWORD, false)
        }));
    }
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().expect("migration worker"))
        .collect();
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "concurrent migrations must have exactly one winner: {results:?}"
    );

    assert_envelope(&store, &entropy, &address);
    drop(store);
    let restarted = SeedStore::new(&root.0, "testnet");
    let prepared = restarted
        .prepare_unlock(PASSWORD)
        .expect("prepare after restart");
    restarted
        .commit_unlock(&prepared, &address)
        .expect("commit after restart");
    drop(prepared);
    assert_envelope(&restarted, &entropy, &address);
}

#[test]
fn concurrent_disables_leave_one_valid_legacy_seed_after_restart() {
    let root = TempRoot::new("disable");
    let (store, entropy, address) = legacy_store(&root);
    store
        .migrate_legacy(PASSWORD, false)
        .expect("prepare envelope");
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let store = SeedStore::new(&root.0, "testnet");
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            store.disable_envelope(PASSWORD)
        }));
    }
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().expect("disable worker"))
        .collect();
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "concurrent disables must have exactly one winner: {results:?}"
    );

    assert_legacy(&store, &entropy);
    drop(store);
    let restarted = SeedStore::new(&root.0, "testnet");
    assert_legacy(&restarted, &entropy);
    restarted
        .migrate_legacy(PASSWORD, false)
        .expect("migrate after restart");
    assert_envelope(&restarted, &entropy, &address);
}

#[test]
fn concurrent_cleanup_and_disable_never_delete_the_last_seed() {
    let root = TempRoot::new("cleanup");
    let (store, entropy, _) = legacy_store(&root);
    store
        .migrate_legacy(PASSWORD, false)
        .expect("prepare envelope");
    let barrier = Arc::new(Barrier::new(2));

    let cleanup_store = SeedStore::new(&root.0, "testnet");
    let cleanup_barrier = Arc::clone(&barrier);
    let cleanup = std::thread::spawn(move || {
        cleanup_barrier.wait();
        cleanup_store.remove_verified_seed();
    });
    let disable_store = SeedStore::new(&root.0, "testnet");
    barrier.wait();
    let first_disable = disable_store.disable_envelope(PASSWORD);
    cleanup.join().expect("cleanup worker");

    // A cleanup that wins the lock may make the concurrent disable busy-fail.
    // Retrying after that operation is the same user-visible recovery path.
    if let Err(error) = first_disable {
        assert!(
            error.to_string().contains("другим процессом"),
            "unexpected disable error: {error}"
        );
        disable_store
            .disable_envelope(PASSWORD)
            .expect("retry disable after cleanup");
    }
    assert_legacy(&disable_store, &entropy);

    // This is the stale late-creator cleanup: with no readable envelope it must
    // leave the legacy copy installed by disable untouched, including after restart.
    disable_store.remove_verified_seed();
    drop(disable_store);
    let restarted = SeedStore::new(&root.0, "testnet");
    restarted.remove_verified_seed();
    assert_legacy(&restarted, &entropy);
}
