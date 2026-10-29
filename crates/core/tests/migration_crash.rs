//! Crash drill for the legacy → Argon2id envelope migration (`docs/history/ARGON2ID.md`
//! §6.6, §8 test 25).
//!
//! Each step of `SeedStore::migrate_legacy` is followed by a failpoint that
//! aborts the process when `FLINT_MIGRATION_CRASH_AT=M0..M5` is set. The test
//! runs the migration in a child process, lets it die at the selected point,
//! then checks the on-disk state and proves that the wallet still opens with
//! the same password, seed and address and that no PBKDF2 blob survives.
//!
//! Only built with `--features crash-injection`; the feature is never enabled
//! in a release build.

#![cfg(feature = "crash-injection")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use flint_core::address::address_from_entropy;
use flint_core::seed::{LegacySeed, SeedEnvelope, SeedKind, SeedStore};

const PASSWORD: &str = "crash-drill-user-password";
const STEPS: [&str; 6] = ["M0", "M1", "M2", "M3", "M4", "M5"];

fn unique_dir(step: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "flint-migration-crash-{step}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// A legacy wallet in the state migration starts from, plus one insecure
/// `wallet.seed.bak*` copy so every crash point is checked for weak leftovers.
fn setup(dir: &Path) -> (Vec<u8>, String) {
    fs::create_dir_all(dir.join("wallet_data")).unwrap();
    let store = SeedStore::new(dir, "testnet");
    let entropy: Vec<u8> = (0u8..32).collect();
    LegacySeed::write_atomic(&store.legacy_path(), &entropy, PASSWORD).unwrap();
    fs::write(
        dir.join("wallet_data").join("wallet.seed.bak-old"),
        b"weak copy",
    )
    .unwrap();
    let address = address_from_entropy(&entropy, true, 0).unwrap();
    (entropy, address)
}

fn expected(step: &str) -> (bool, bool, usize) {
    match step {
        // envelope, wallet.seed, wallet.seed.bak*
        "M0" => (false, true, 1),
        "M1" | "M2" => (true, true, 1),
        "M3" => (true, true, 0),
        "M4" | "M5" => (true, false, 0),
        other => panic!("unknown crash point {other}"),
    }
}

#[test]
fn crash_at_every_migration_step_is_recoverable() {
    for step in STEPS {
        let dir = unique_dir(step);
        let (entropy, address) = setup(&dir);
        let store = SeedStore::new(&dir, "testnet");
        let marker = dir.join("crash-marker");

        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("child_worker")
            .arg("--nocapture")
            .env("FLINT_MIGRATION_CRASH_DIR", &dir)
            .env("FLINT_MIGRATION_CRASH_AT", step)
            .env("FLINT_MIGRATION_CRASH_MARKER", &marker)
            .status()
            .expect("spawn the crash worker");
        assert!(
            !status.success(),
            "{step}: migration must die at the failpoint, not finish (status {status})"
        );
        assert_eq!(
            fs::read_to_string(&marker).ok().as_deref(),
            Some(step),
            "{step}: the child did not reach the expected failpoint (status {status})"
        );

        let envelope = store.envelope_path().is_file();
        let legacy = store.legacy_path().is_file();
        let backups = store.legacy_backups().len();
        assert_eq!(
            (envelope, legacy, backups),
            expected(step),
            "{step}: unexpected files after the crash"
        );

        if envelope {
            let envelope = SeedEnvelope::read(&store.envelope_path()).unwrap();
            assert_eq!(envelope.address(), address, "{step}: envelope address");
            let opened = envelope.open(PASSWORD).unwrap();
            assert_eq!(opened.seed.as_slice(), entropy, "{step}: envelope seed");
            assert!(
                opened.legacy.is_some(),
                "{step}: the original wallet.seed bytes must be kept for rollback"
            );
        }
        if legacy {
            assert_eq!(
                LegacySeed::read_file(&store.legacy_path(), PASSWORD)
                    .unwrap()
                    .as_slice(),
                entropy,
                "{step}: leftover wallet.seed must still be the user's seed"
            );
        }

        // Recovery, exactly as the app would do it: without an envelope the
        // user retries the migration; with an envelope the next unlock removes
        // the leftover weak files after checking the address.
        if envelope {
            let prepared = store.prepare_unlock(PASSWORD).expect("prepare unlock");
            assert_eq!(prepared.address(), address);
            assert_eq!(
                LegacySeed::read_file(&store.legacy_path(), prepared.password())
                    .unwrap()
                    .as_slice(),
                entropy,
                "{step}: transient seed must carry the wallet seed"
            );
            store
                .commit_unlock(&prepared, &address)
                .expect("commit unlock");
        } else {
            let outcome = store
                .migrate_legacy(PASSWORD, true)
                .expect("retry the migration");
            assert_eq!(outcome.address, address);
        }

        assert_eq!(store.kind(), SeedKind::Envelope, "{step}: after recovery");
        assert!(
            !store.legacy_path().exists(),
            "{step}: no weak wallet.seed may survive recovery"
        );
        assert!(
            store.legacy_backups().is_empty(),
            "{step}: no weak wallet.seed.bak* may survive recovery"
        );
        let opened = SeedEnvelope::read(&store.envelope_path())
            .unwrap()
            .open(PASSWORD)
            .unwrap();
        assert_eq!(opened.seed.as_slice(), entropy);

        // The explicit downgrade must still restore a readable legacy file.
        store.disable_envelope(PASSWORD).expect("disable envelope");
        assert_eq!(store.kind(), SeedKind::Legacy);
        assert_eq!(
            LegacySeed::read_file(&store.legacy_path(), PASSWORD)
                .unwrap()
                .as_slice(),
            entropy
        );

        let _ = fs::remove_dir_all(&dir);
    }
}

/// Runs in the child process. With the env vars set by the parent it starts the
/// migration and must be aborted by the failpoint; reaching the end is a test
/// failure.
#[test]
fn child_worker() {
    let Ok(dir) = std::env::var("FLINT_MIGRATION_CRASH_DIR") else {
        return;
    };
    let Ok(step) = std::env::var("FLINT_MIGRATION_CRASH_AT") else {
        return;
    };
    let store = SeedStore::new(PathBuf::from(dir), "testnet");
    match store.migrate_legacy(PASSWORD, true) {
        Ok(_) => panic!("migration finished without hitting crash point {step}"),
        Err(e) => panic!("migration failed before crash point {step}: {e}"),
    }
}
