//! Headless selftest without Tauri.
//!
//! Creates its own marked directory, exercises the shared runtime (wallet,
//! Janus board, managed node) and writes `selftest.json`. `FLINT_DATA_DIR` is
//! deliberately not used as the target: a real wallet must never be deleted.

use std::path::{Path, PathBuf};

use flint_core::{AppPaths, NodeManager};
use flint_runtime::{SwapRuntime, WalletRuntime};

const MARKER: &str = ".flint-selftest";

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn create_root() -> Result<PathBuf, String> {
    let parent = std::env::var("FLINT_SELFTEST_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&parent).map_err(err)?;
    let root = parent.join(format!("flint-selftest-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    std::fs::write(root.join(MARKER), b"flint selftest\n").map_err(err)?;
    Ok(root)
}

fn cleanup(root: &Path) {
    let marked = root.join(MARKER).is_file()
        && root
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("flint-selftest-"));
    if !marked {
        eprintln!("selftest: refusing to delete {}", root.display());
        return;
    }
    if let Err(e) = std::fs::remove_dir_all(root) {
        eprintln!("selftest: cleanup failed for {}: {e}", root.display());
    }
}

fn check_node(root: &str) -> serde_json::Value {
    let attempt = || -> Result<serde_json::Value, String> {
        let paths = AppPaths::at(root);
        let mut manager = NodeManager::new(&paths, "testnet", None).map_err(err)?;
        manager.start().map_err(err)?;
        let running = manager.is_running();
        let addr = format!("127.0.0.1:{}", manager.api_port);
        let port_open = std::net::TcpStream::connect_timeout(
            &addr.parse().map_err(err)?,
            std::time::Duration::from_secs(2),
        )
        .is_ok();
        manager.stop().ok();
        Ok(serde_json::json!({
            "binary": manager.binary().display().to_string(),
            "started": running,
            "port": manager.api_port,
            "port_open": port_open,
        }))
    };
    attempt().unwrap_or_else(|e| serde_json::json!({"error": e}))
}

fn run(root: &Path) -> Result<(), String> {
    let root_text = root.display().to_string();
    std::env::set_var("FLINT_DATA_DIR", &root_text);
    let started = std::time::Instant::now();

    let wallet = WalletRuntime::new();
    let created = wallet.create_wallet("Flint-Selftest-123!")?;
    wallet.open_wallet("Flint-Selftest-123!")?;
    let reopened = wallet.address()?;
    let node = wallet.node_info()?;
    let summary = wallet.summary().ok();

    let swaps = SwapRuntime::new();
    let board = swaps.board("XMR/GRIN")?;
    let offer = board
        .offers
        .first()
        .ok_or_else(|| "selftest: Janus board is empty".to_string())?;
    let swap = swaps.take(&offer.id, None)?;
    let settled = swaps.simulate(&swap.id, 1_000)?;

    let node_check = check_node(&root_text);
    let report = serde_json::json!({
        "ok": true,
        "data_dir": root_text,
        "wallet": {
            "address": created.address,
            "reopened_matches": created.address == reopened,
            "seed_words": created.mnemonic.split_whitespace().count(),
            "spendable_offline": summary.as_ref().map(|s| s.balance.spendable.clone()),
        },
        "node": {
            "connected": node.connected,
            "height": node.height,
            "managed": node_check,
        },
        "janus": {
            "offers": board.offers.len(),
            "swap_state": format!("{:?}", settled.state),
            "steps": settled.steps.len(),
            "lock_height": settled.lock_height,
        },
        "elapsed_ms": started.elapsed().as_millis(),
    });
    let text = serde_json::to_string_pretty(&report).map_err(err)?;
    println!("{text}");
    let _ = std::fs::write("selftest.json", &text);
    Ok(())
}

fn main() {
    let root = match create_root() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("selftest failed: {e}");
            std::process::exit(2);
        }
    };
    let result = run(&root);
    if std::env::var("FLINT_KEEP_SELFTEST").ok().as_deref() != Some("1") {
        cleanup(&root);
    }
    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("selftest failed: {e}");
            std::process::exit(2);
        }
    }
}
