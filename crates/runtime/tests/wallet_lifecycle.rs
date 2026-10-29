//! Repeatable wallet lifecycle on independent clean testnet roots.
//!
//! The embedded test runs in the ordinary workspace suite. The RPC variant uses
//! the real `grin-wallet` binary and is ignored by default:
//!
//!   cargo test -p flint-runtime --test wallet_lifecycle -- --ignored --nocapture --test-threads=1

use std::path::PathBuf;

use flint_core::AppPaths;
use flint_runtime::{BackendKind, WalletRuntime};
use uuid::Uuid;
use zeroize::Zeroizing;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("flint-wallet-lifecycle-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create clean lifecycle root");
        Self(path)
    }

    fn paths(&self) -> AppPaths {
        AppPaths::at(&self.0)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn disable_managed_node(runtime: &WalletRuntime) {
    runtime
        .update_settings(None, None, None, Some(false), None, None, None)
        .expect("disable managed node");
}

fn open_runtime(root: &TempRoot, backend: BackendKind) -> WalletRuntime {
    let runtime = WalletRuntime::open_at(root.paths(), backend);
    disable_managed_node(&runtime);
    runtime
}

fn exercise_clean_lifecycle(backend: BackendKind, cycle: usize) {
    let tag = match backend {
        BackendKind::Embedded => format!("embedded-{cycle}"),
        BackendKind::GrinWallet => format!("rpc-{cycle}"),
    };
    let original = TempRoot::new(&tag);
    let restored_root = TempRoot::new(&format!("{tag}-restore"));
    let password = format!("Lifecycle!{cycle}#Pass");
    let runtime = open_runtime(&original, backend);
    assert!(!runtime.status().expect("fresh status").status.initialized);

    let created = runtime.create(&password).expect("create wallet");
    let address = created.address.clone();
    let mnemonic = Zeroizing::new(created.mnemonic.clone());
    assert!(address.starts_with("tgrin1"));
    assert!(runtime.status().expect("created status").status.unlocked);

    runtime.lock().expect("lock created wallet");
    assert!(!runtime.status().expect("locked status").status.unlocked);
    assert!(
        runtime.address().is_err(),
        "locked wallet returned an address"
    );
    runtime.open(&password).expect("reopen created wallet");
    assert_eq!(runtime.address().expect("address after reopen"), address);
    runtime.shutdown();

    let restarted = open_runtime(&original, backend);
    if backend.is_rpc() {
        restarted
            .start_rpc_background()
            .expect("start RPC after restart");
    }
    assert!(
        restarted
            .status()
            .expect("restart status")
            .status
            .initialized
    );
    assert!(!restarted.status().expect("restart locked").status.unlocked);
    restarted.open(&password).expect("open after restart");
    assert_eq!(restarted.address().expect("address after restart"), address);
    restarted.lock().expect("lock before shutdown");
    restarted.shutdown();

    let restored = open_runtime(&restored_root, backend);
    assert!(
        !restored
            .status()
            .expect("restore fresh status")
            .status
            .initialized
    );
    let outcome = restored
        .restore(mnemonic.as_str(), &password)
        .expect("restore into an independent clean root");
    assert_eq!(outcome.address, address);
    assert!(outcome.needs_scan);
    if restored.status().expect("restored status").status.unlocked {
        restored.lock().expect("lock restored wallet");
    }
    restored.open(&password).expect("open restored wallet");
    assert_eq!(restored.address().expect("restored address"), address);
    assert_eq!(
        restored.seed_status().expect("restored seed").kind,
        "envelope"
    );

    let moved = restored
        .delete_wallet("default")
        .expect("delete restored wallet");
    assert!(
        std::path::Path::new(&moved).is_dir(),
        "trash target missing"
    );
    assert!(
        !restored
            .status()
            .expect("deleted status")
            .status
            .initialized
    );
    restored.shutdown();
}

#[test]
fn embedded_wallet_lifecycle_is_repeatable_on_clean_roots() {
    for cycle in 0..2 {
        exercise_clean_lifecycle(BackendKind::Embedded, cycle);
    }
}

#[test]
#[ignore = "spawns the real grin-wallet binary for two independent lifecycle runs"]
fn rpc_wallet_lifecycle_is_repeatable_on_clean_roots() {
    for cycle in 0..2 {
        exercise_clean_lifecycle(BackendKind::GrinWallet, cycle);
    }
}
