//! Live embedded scan cancellation against a real testnet node.
//!
//! Ignored by default. It creates a disposable wallet in `.tmp`, copies the
//! managed node's API secret into that data root, and cancels a full scan at a
//! batch boundary.
//!
//!   FLINT_LIVE_EMBEDDED_SCAN=1
//!   FLINT_NODE_URL=http://127.0.0.1:13413       (optional)
//!   FLINT_NODE_SECRET_FILE=C:\DHW\.node\testnet\node\.api_secret (optional)
//!
//!   cargo test -p flint-core --test live_embedded_scan_cancel -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flint_core::{AppPaths, FlintError, ScanRunOutcome, WalletService, SCAN_CANCELLED};

fn armed() -> bool {
    std::env::var("FLINT_LIVE_EMBEDDED_SCAN").ok().as_deref() == Some("1")
}

fn temp_root() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".tmp")
        .join(format!("live-embedded-cancel-{stamp}"))
}

fn install_node_secret(root: &Path) {
    let source = std::env::var("FLINT_NODE_SECRET_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(".node")
                .join("testnet")
                .join("node")
                .join(".api_secret")
        });
    let dir = root.join("testnet").join("node");
    std::fs::create_dir_all(&dir).expect("node dir");
    std::fs::copy(&source, dir.join(".api_secret"))
        .unwrap_or_else(|e| panic!("copy {}: {e}", source.display()));
}

fn wait_for_ready(service: &WalletService) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let info = service.node_info();
        if info.phase.ready() {
            eprintln!("node ready: height {}", info.height);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "node is not ready: phase {:?}, height {:?}",
            info.phase,
            info.height
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "live embedded scan; needs a synced testnet node"]
fn embedded_scan_cancels_between_batches() {
    if !armed() {
        eprintln!("FLINT_LIVE_EMBEDDED_SCAN is not set: nothing to do");
        return;
    }

    let root = temp_root();
    std::fs::create_dir_all(&root).expect("temp root");
    install_node_secret(&root);

    let mut service = WalletService::open_at(AppPaths::at(&root));
    if let Ok(url) = std::env::var("FLINT_NODE_URL") {
        service
            .update_settings(None, Some(url), None, None, None, None, Some(1))
            .expect("node url");
    }
    service
        .create_wallet("live-embedded-cancel-123")
        .expect("create wallet");
    service.require_scan().expect("restore-like pending scan");
    wait_for_ready(&service);

    let job = service.prepare_scan(None).expect("prepare scan");
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    let handle = std::thread::spawn(move || job.run(thread_cancel.as_ref()));

    // Full scan from genesis is long enough to reach a batch boundary; cancel
    // after the first couple of batches rather than before the scan starts.
    std::thread::sleep(Duration::from_secs(2));
    cancel.store(true, Ordering::SeqCst);
    let outcome = handle.join().expect("scan thread").expect("scan run");
    assert_eq!(
        outcome,
        ScanRunOutcome::Cancelled,
        "the scan must stop at a batch boundary"
    );

    let err = service
        .finish_scan(Ok(outcome))
        .expect_err("cancelled scan reports an error");
    assert!(matches!(err, FlintError::ScanCancelled));
    assert_eq!(service.scan_state(), flint_core::ScanState::Failed);
    assert_eq!(service.scan_error(), Some(SCAN_CANCELLED));
    assert!(
        service.pending_scan(),
        "a cancelled restore scan leaves the balance unknown"
    );

    // A retry after a cancelled restore must cover the full history; a partial
    // tail scan cannot prove the middle outputs were seen.
    let job = service.prepare_scan(None).expect("prepare full retry");
    let no_cancel = AtomicBool::new(false);
    let outcome = job.run(&no_cancel).expect("retry run");
    service.finish_scan(Ok(outcome)).expect("completed retry");
    assert_eq!(service.scan_state(), flint_core::ScanState::Completed);
    assert!(!service.pending_scan());

    drop(service);
    let _ = std::fs::remove_dir_all(root);
}
