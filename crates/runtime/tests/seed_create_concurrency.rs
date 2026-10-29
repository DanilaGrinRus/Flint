//! Two independent sessions cannot both claim creation of the same wallet.

use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use flint_core::{AppPaths, CreatedWallet};
use flint_runtime::{BackendKind, WalletRuntime};
use uuid::Uuid;

const PASSWORD: &str = "Seed-create-concurrency!42";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("flint-seed-create-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create clean wallet root");
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

fn runtime(root: &TempRoot, backend: BackendKind) -> WalletRuntime {
    let runtime = WalletRuntime::open_at(root.paths(), backend);
    runtime
        .update_settings(None, None, None, Some(false), None, None, None)
        .expect("disable managed node");
    runtime
}

fn exercise_concurrent_create(backend: BackendKind) {
    let root = TempRoot::new(match backend {
        BackendKind::Embedded => "embedded",
        BackendKind::GrinWallet => "rpc",
    });
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let runtime = runtime(&root, backend);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let result = runtime.create(PASSWORD);
            (runtime, result)
        }));
    }

    let mut sessions = Vec::new();
    let mut created = Vec::<CreatedWallet>::new();
    let mut errors = Vec::new();
    for worker in workers {
        let (runtime, result) = worker.join().expect("create worker");
        sessions.push(runtime);
        match result {
            Ok(wallet) => created.push(wallet),
            Err(error) => errors.push(error),
        }
    }
    assert_eq!(
        created.len(),
        1,
        "both creates claimed success; errors: {errors:?}"
    );
    assert_eq!(
        errors.len(),
        1,
        "concurrent create did not reject the second writer"
    );
    let expected = created[0].address.clone();
    for session in sessions {
        session.shutdown();
    }

    let restarted = runtime(&root, backend);
    if backend.is_rpc() {
        restarted
            .start_rpc_background()
            .expect("start RPC after restart");
    }
    restarted
        .open(PASSWORD)
        .expect("open created wallet after restart");
    assert_eq!(
        restarted.address().expect("address after restart"),
        expected
    );
    assert_eq!(
        restarted.seed_status().expect("seed status").kind,
        "envelope"
    );
    restarted.shutdown();
}

#[test]
fn embedded_concurrent_create_has_one_recoverable_winner() {
    exercise_concurrent_create(BackendKind::Embedded);
}

#[test]
#[ignore = "spawns real grin-wallet processes for two competing creation sessions"]
fn rpc_concurrent_create_has_one_recoverable_winner() {
    exercise_concurrent_create(BackendKind::GrinWallet);
}
