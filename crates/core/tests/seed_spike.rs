//! Spike S1 from `docs/history/ARGON2ID.md`: the seed is the transport, the wallet
//! database is disposable.
//!
//! An interrupted creation (or a deleted `db/`) must reopen from the envelope,
//! recreate the database and derive the same address — with the balance marked
//! as unknown, not as a confirmed zero.

use std::path::{Path, PathBuf};

use flint_core::{AppPaths, WalletService};

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        // Only the uniquely named test directory is removed.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl AsRef<Path> for TempRoot {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

fn temp_root(tag: &str) -> TempRoot {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "flint-spike-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    TempRoot(p)
}

#[test]
fn wallet_without_a_database_reopens_from_the_envelope() {
    let root = temp_root("s1");
    let mut service = WalletService::open_at(AppPaths::at(root.as_ref()));
    let created = service
        .create_wallet("correct horse battery")
        .expect("create");
    service.lock();

    let data = root
        .as_ref()
        .join("testnet")
        .join("wallet")
        .join("wallet_data");
    assert!(data.join("db").exists(), "upstream created a database");
    assert!(
        !data.join("wallet.seed").exists(),
        "no seed file at rest while locked"
    );

    // A crash after the envelope was written, before the database existed,
    // leaves exactly this: an envelope and no db/.
    std::fs::remove_dir_all(data.join("db")).expect("remove db");

    service = WalletService::open_at(AppPaths::at(root.as_ref()));
    assert!(service.initialized());
    service
        .open_wallet("correct horse battery")
        .expect("open without db");
    assert_eq!(
        service.address().expect("address"),
        created.address,
        "recreated database must derive the same address"
    );
    assert!(
        data.join("db").exists(),
        "upstream must have recreated the database"
    );
    assert!(
        !data.join("wallet.seed").exists(),
        "the transient seed file is removed after the address check"
    );

    // TempRoot's Drop removes only this unique test directory.
}
