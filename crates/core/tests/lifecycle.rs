//! Offline smoke tests: wallet creation, unlock and address derivation must work
//! without any Grin node running (node access is only needed for chain sync).

use std::path::PathBuf;

use flint_core::{AppPaths, WalletService};

fn temp_root(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "flint-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p
}

#[test]
fn wallet_lifecycle_offline() {
    let root = temp_root("lifecycle");
    let mut service = WalletService::open_at(AppPaths::at(&root));

    assert!(!service.initialized(), "fresh data dir must start empty");
    assert!(
        service.address().is_err(),
        "locked wallet must not hand out an address"
    );

    let created = service
        .create_wallet("correct horse battery")
        .expect("create wallet");
    assert!(
        created.mnemonic.split_whitespace().count() >= 12,
        "seed phrase expected"
    );
    // Testnet slatepack addresses carry the `tgrin1` HRP, mainnet `grin1`.
    assert!(
        created.address.starts_with("tgrin1") || created.address.starts_with("grin1"),
        "slatepack address expected, got {}",
        created.address
    );
    assert!(service.initialized());
    assert!(service.unlocked());
    assert!(root
        .join("testnet")
        .join("wallet")
        .join("wallet_data")
        .exists());

    // Locking drops keys; opening restores the exact same address.
    service.lock();
    assert!(!service.unlocked());
    assert!(service.summary().is_err());

    service
        .open_wallet("correct horse battery")
        .expect("reopen wallet");
    let again = service.address().expect("address after reopen");
    assert_eq!(created.address, again);

    // A wrong password must not unlock the wallet.
    service.lock();
    assert!(service.open_wallet("wrong password").is_err());
    assert!(!service.unlocked());

    let status = service.status();
    assert!(status.initialized);
    assert_eq!(status.network, "testnet");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rejects_short_password() {
    let root = temp_root("password");
    let mut service = WalletService::open_at(AppPaths::at(&root));

    let err = service
        .create_wallet("short")
        .expect_err("short password must be rejected");
    assert!(err.to_string().contains("12"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn settings_roundtrip() {
    let root = temp_root("settings");
    let mut service = WalletService::open_at(AppPaths::at(&root));

    let view = service
        .update_settings(
            Some("testnet".into()),
            Some("http://127.0.0.1:13413".into()),
            Some(String::new()), // keep the bundled node
            Some(true),
            Some(true), // proxy on
            None,
            Some(5),
        )
        .expect("save settings");

    assert_eq!(view.node_url, "http://127.0.0.1:13413");
    assert!(view.auto_start_node);
    assert_eq!(view.min_confirmations, 5);

    // A second service instance over the same root must see the persisted values.
    let reloaded = WalletService::open_at(AppPaths::at(&root));
    let view2 = reloaded.settings_view();
    assert!(view2.auto_start_node);
    assert_eq!(view2.min_confirmations, 5);
    assert_eq!(view2.node_url, "http://127.0.0.1:13413");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn network_switch_moves_default_node_port() {
    let root = temp_root("switchnet");
    let mut service = WalletService::open_at(AppPaths::at(&root));

    // Defaults to testnet port.
    assert_eq!(service.settings_view().node_url, "http://127.0.0.1:13413");

    // Switching without touching the URL must follow the new network's port.
    let view = service
        .update_settings(
            Some("mainnet".into()),
            None,
            None,
            Some(false),
            None,
            None,
            None,
        )
        .expect("switch to mainnet");
    assert_eq!(view.network, "mainnet");
    assert_eq!(view.node_url, "http://127.0.0.1:3413");

    // A custom URL is preserved across switches.
    service
        .update_settings(
            None,
            Some("http://127.0.0.1:9999".into()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("custom url");
    let view = service
        .update_settings(Some("testnet".into()), None, None, None, None, None, None)
        .expect("back to testnet");
    assert_eq!(view.node_url, "http://127.0.0.1:9999");

    let _ = std::fs::remove_dir_all(root);
}
