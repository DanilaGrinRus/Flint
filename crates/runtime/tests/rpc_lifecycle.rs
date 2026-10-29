//! Runtime-owned `grin-wallet` lifecycle.
//!
//! The non-ignored tests use only a throwaway data root and never touch
//! `FLINT_DATA_DIR`. The live test spawns the real binary from
//! `resources/wallet` and is ignored by default:
//!
//!   cargo test -p flint-runtime --test rpc_lifecycle -- --ignored --nocapture --test-threads=1

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flint_core::seed::SeedStore;
use flint_core::{AppPaths, ScanState};
use flint_runtime::{BackendKind, RpcPhase, WalletRuntime};
use uuid::Uuid;

struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("flint-runtime-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("temp root");
        Self { path }
    }

    fn paths(&self) -> AppPaths {
        AppPaths::at(&self.path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

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

#[test]
fn embedded_runtime_has_no_rpc_session() {
    let root = TempRoot::new("embedded");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    assert!(!runtime.is_rpc());
    assert_eq!(runtime.rpc().phase(), RpcPhase::Disabled);
    assert!(runtime.rpc().port().is_none());
}

#[test]
fn diagnostics_export_contains_no_secret_material() {
    let root = TempRoot::new("diagnostics");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let export = runtime.diagnostics_export().expect("diagnostics");
    let lower = export.to_ascii_lowercase();
    assert!(!lower.contains("password"));
    assert!(!lower.contains("api_secret"));
    assert!(!lower.contains("mnemonic"));
    assert!(!lower.contains("seed_phrase"));
    assert!(!lower.contains("private_key"));
}

#[test]
#[test]
fn unlock_grace_cache_and_recent_profile_round_trip() {
    let root = TempRoot::new("unlock-grace");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    assert!(runtime.cached_unlock("default").is_none());
    runtime.remember_unlock("grace-password");
    assert_eq!(
        runtime
            .cached_unlock("default")
            .map(|p| p.to_string())
            .as_deref(),
        Some("grace-password")
    );
    runtime.remember_recent_profile("default").expect("recent");
    assert_eq!(runtime.recent_profiles(), vec!["default".to_string()]);
    runtime.clear_unlock_cache();
    assert!(runtime.cached_unlock("default").is_none());
}

#[test]
fn ui_language_round_trips_and_defaults_to_russian() {
    let root = TempRoot::new("ui-language");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    assert_eq!(runtime.ui_language(), "ru");
    runtime.set_ui_language("en").expect("save en");
    assert_eq!(runtime.ui_language(), "en");
    assert!(
        runtime.set_ui_language("de").is_err(),
        "unknown language accepted"
    );
    assert_eq!(
        runtime.ui_language(),
        "en",
        "failed write changed the language"
    );
    assert!(!runtime.auto_process_slatepacks(), "auto must default off");
    runtime
        .set_auto_process_slatepacks(true)
        .expect("persist auto on");
    assert!(runtime.auto_process_slatepacks());
    let reopened = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    assert!(
        reopened.auto_process_slatepacks(),
        "auto did not survive reopen"
    );
    reopened
        .set_auto_process_slatepacks(false)
        .expect("persist auto off");
    assert!(!reopened.auto_process_slatepacks());
}

#[test]
fn rpc_read_paths_fail_without_starting_a_process() {
    let root = TempRoot::new("rpc-read-only");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    let status = runtime.status().expect("status without child");
    assert_eq!(status.status.mode, "grin-wallet");
    assert!(!status.status.initialized);
    let profiles = runtime.profiles().expect("profiles without child");
    assert!(profiles.profiles.is_empty());
    assert_eq!(profiles.active, "default");
    let err = runtime.open("password-123").unwrap_err();
    assert_eq!(err, "кошелёк не найден");
    assert!(runtime.rpc().port().is_none());
}

#[test]
fn embedded_tor_runtime_refuses_start() {
    let root = TempRoot::new("tor-embedded");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let err = runtime.tor().start().unwrap_err();
    assert!(err.contains("grin-wallet"), "unexpected: {err}");
    let status = runtime.tor().status().expect("tor status");
    assert!(!status.running);
    assert!(status.onion.is_none());
}

#[test]
fn tor_bridges_roundtrip() {
    let root = TempRoot::new("tor-bridges");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let saved = runtime
        .tor()
        .save_bridges("obfs4 192.0.2.1:443 FINGERPRINT cert=abc iat-mode=0")
        .expect("save bridges");
    assert!(saved.text.contains("obfs4"));
    let read = runtime.tor().bridges().expect("read bridges");
    assert_eq!(read.text, saved.text);
    runtime.tor().save_bridges("").expect("clear bridges");
    assert!(runtime.tor().bridges().expect("read empty").text.is_empty());
}

#[test]
fn messenger_status_reports_a_locked_wallet() {
    let root = TempRoot::new("messenger-status");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let status = runtime.messenger_status().expect("messenger status");
    assert_eq!(status["unlocked"], false);
    assert_eq!(status["history_key"], false);
    assert!(status["address"].is_null());
    assert!(status["error"].is_string());
}

#[test]
fn messenger_contacts_and_blocked_lists_share_the_history_key() {
    let root = TempRoot::new("messenger-contacts");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let password = "Tr7!kLm9#Qw2";
    runtime.create(password).expect("create wallet");
    runtime
        .unlock_history_key(password)
        .expect("unlock history key");
    let peer = flint_messenger::Identity::generate("tgrin")
        .expect("peer identity")
        .address()
        .to_string();

    let saved = runtime
        .messenger_contact_save(&peer, "peer")
        .expect("save contact");
    assert_eq!(saved["name"], "peer");
    let contacts = runtime.messenger_contacts().expect("list contacts");
    assert_eq!(contacts.as_array().map(Vec::len), Some(1));

    runtime.messenger_block(&peer, true).expect("block peer");
    let blocked = runtime.messenger_blocked().expect("list blocked");
    assert_eq!(blocked["addresses"].as_array().map(Vec::len), Some(1));
    runtime.messenger_block(&peer, false).expect("unblock peer");
    let unblocked = runtime.messenger_blocked().expect("list unblocked");
    assert!(unblocked["addresses"].as_array().unwrap().is_empty());

    let history = runtime.messenger_history(&peer).expect("empty history");
    assert!(history.as_array().unwrap().is_empty());
    runtime
        .messenger_contact_delete(&peer)
        .expect("delete contact");
    assert!(runtime
        .messenger_contacts()
        .expect("contacts after delete")
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn legacy_history_recovery_restores_the_old_key() {
    let root = TempRoot::new("legacy-history-recovery");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let password = "LegacyRecovery!42";
    runtime.create(password).expect("create wallet");
    let legacy = root
        .path
        .join("messenger")
        .join("history")
        .join("testnet")
        .join("default");
    std::fs::create_dir_all(&legacy).expect("legacy history dir");
    let wrapped = flint_messenger::keywrap::wrap(password, &[9u8; 32]).expect("wrap legacy key");
    std::fs::write(legacy.join(".history.key"), wrapped).expect("legacy key");

    let result = runtime
        .recover_legacy_history(password)
        .expect("recover legacy history");
    assert_eq!(result["restored"], true);
    assert!(runtime.history_key_ready());
    runtime.shutdown();
}

#[test]
fn rpc_start_refuses_after_shutdown() {
    let root = TempRoot::new("rpc-shutdown");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    runtime.rpc().begin_shutdown();
    let err = runtime.rpc().start().unwrap_err();
    assert!(err.contains("закрывается"), "unexpected: {err}");
    assert!(runtime.rpc().port().is_none());
}

#[test]
fn password_change_rewraps_the_profile_history_key() {
    let root = TempRoot::new("history-rewrap");
    let paths = root.paths();
    let root_path = paths.root().to_path_buf();
    let runtime = WalletRuntime::open_at(paths, BackendKind::Embedded);
    runtime.create("Tr7!kLm9#Qw2").expect("create wallet");
    let identity = runtime.handle().lock().unwrap().wallet_identity();
    let history_dir = root_path
        .join("messenger")
        .join("history")
        .join("testnet")
        .join("embedded")
        .join(identity)
        .join("default");
    std::fs::create_dir_all(&history_dir).expect("history dir");
    let key_path = history_dir.join(".history.key");
    let (_, wrapped) = flint_messenger::create_history_key("Tr7!kLm9#Qw2").expect("history key");
    std::fs::write(&key_path, &wrapped).expect("write history key");

    let outcome = runtime
        .change_password_with_history("Tr7!kLm9#Qw2", "Vb8$nM4!xR6Q")
        .expect("change password with history");
    assert!(outcome.history_rewrapped, "{outcome:?}");
    assert!(outcome.history_error.is_none());
    let bytes = std::fs::read(&key_path).expect("read rewrapped key");
    assert!(
        flint_messenger::unwrap_history_key("Vb8$nM4!xR6Q", &bytes).is_ok(),
        "history key does not open with the new password"
    );
    assert!(
        flint_messenger::unwrap_history_key("Tr7!kLm9#Qw2", &bytes).is_err(),
        "history key still opens with the old password"
    );
}

#[test]
fn weak_password_is_rejected_before_embedded_history_rewrap() {
    let root = TempRoot::new("weak-embedded");
    let paths = root.paths();
    let root_path = paths.root().to_path_buf();
    let runtime = WalletRuntime::open_at(paths, BackendKind::Embedded);
    runtime.create("Tr7!kLm9#Qw2").expect("create wallet");
    let identity = runtime.handle().lock().unwrap().wallet_identity();
    let history_dir = root_path
        .join("messenger")
        .join("history")
        .join("testnet")
        .join("embedded")
        .join(identity)
        .join("default");
    std::fs::create_dir_all(&history_dir).expect("history dir");
    let key_path = history_dir.join(".history.key");
    let (_, wrapped) = flint_messenger::create_history_key("Tr7!kLm9#Qw2").expect("history key");
    std::fs::write(&key_path, &wrapped).expect("write history key");

    let error = runtime
        .change_password_with_history("Tr7!kLm9#Qw2", "short")
        .expect_err("weak password must be rejected");
    assert!(error.contains("12"), "unexpected: {error}");
    assert_eq!(
        std::fs::read(&key_path).expect("key still present"),
        wrapped,
        "history key was rewrapped despite rejected password"
    );
}

#[test]
fn weak_password_is_rejected_for_rpc_envelope_before_history() {
    let root = TempRoot::new("weak-rpc-envelope");
    let dir = flint_core::rpc_backend::wallet_dir(&root.paths(), "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let store = SeedStore::new(&dir, "testnet");
    let generated = store.generate().expect("mnemonic");
    store
        .write_envelope(
            "Tr7!kLm9#Qw2",
            "tgrin1van75u9m9h8vtezrtjcjzwh0ez2gzjqpz2f2ltqrta3lcl2nexqqrpw2dl",
            &generated.entropy,
            None,
        )
        .expect("envelope");
    let before = std::fs::read(store.envelope_path()).expect("envelope bytes");

    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    let error = runtime
        .change_password_with_history("Tr7!kLm9#Qw2", "short")
        .expect_err("weak password must be rejected for RPC envelope");
    assert!(error.contains("12"), "unexpected: {error}");
    assert_eq!(
        std::fs::read(store.envelope_path()).expect("envelope still present"),
        before,
        "envelope changed despite rejected password"
    );
}

#[test]
fn weak_password_is_rejected_for_rpc_legacy() {
    let root = TempRoot::new("weak-rpc-legacy");
    let dir = flint_core::rpc_backend::wallet_dir(&root.paths(), "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let store = SeedStore::new(&dir, "testnet");
    std::fs::create_dir_all(store.legacy_path().parent().expect("legacy parent"))
        .expect("legacy parent");
    std::fs::write(store.legacy_path(), b"legacy seed placeholder").expect("legacy seed");
    let before = std::fs::read(store.legacy_path()).expect("legacy bytes");

    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    let error = runtime
        .change_password_with_history("Tr7!kLm9#Qw2", "short")
        .expect_err("weak password must be rejected for RPC legacy");
    assert!(error.contains("12"), "unexpected: {error}");
    assert_eq!(
        std::fs::read(store.legacy_path()).expect("legacy seed still present"),
        before,
        "legacy seed changed despite rejected password"
    );
}

#[test]
fn runtime_owns_embedded_switch_and_settings_rebuild() {
    let root = TempRoot::new("switch-settings");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let switched = runtime
        .switch(Some("mainnet".to_string()), Some(None))
        .expect("switch network");
    assert_eq!(switched.network, "mainnet");
    assert_eq!(switched.profile, "default");
    assert_eq!(runtime.settings_view().unwrap().network, "mainnet");

    let updated = runtime
        .update_settings(
            None,
            Some("http://127.0.0.1:1".to_string()),
            None,
            Some(false),
            Some(false),
            Some("".to_string()),
            Some(5),
        )
        .expect("update settings");
    assert_eq!(updated.view.network, "mainnet");
    assert_eq!(updated.view.min_confirmations, 5);
    assert_eq!(updated.view.node_url, "http://127.0.0.1:1");
    assert!(!updated.node_restarted);
    assert!(updated.restart_error.is_none());
}

#[test]
fn scan_runtime_starts_idle_and_refuses_a_stray_cancel() {
    let root = TempRoot::new("scan-idle");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    let scan = runtime.scan_runtime();
    assert!(!scan.busy());
    assert!(scan.ensure_idle("тест").is_ok());
    assert!(scan.busy_error("баланс").unwrap().is_none());
    assert_eq!(scan.cancel().unwrap_err(), "скан не идёт");
    let view = scan.view().unwrap();
    assert_eq!(view.state, ScanState::NotStarted);
    assert!(!view.cancellable());
}

#[test]
fn stopped_rpc_session_reports_unavailable_not_ready() {
    let root = TempRoot::new("stopped");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    assert!(runtime.is_rpc());
    runtime.rpc().stop_settled();
    assert!(matches!(runtime.rpc().phase(), RpcPhase::Failed(_)));
    assert!(runtime.rpc().port().is_none());
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn runtime_restores_an_rpc_wallet_from_a_mnemonic() {
    let root = TempRoot::new("restore");
    let paths = root.paths();
    let dir = flint_core::rpc_backend::wallet_dir(&paths, "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let store = flint_core::seed::SeedStore::new(&dir, "testnet");
    let generated = store.generate().expect("mnemonic");
    let password = "runtime-restore-pass-123";

    let runtime = WalletRuntime::open_at(paths, BackendKind::GrinWallet);
    let restored = runtime
        .restore(generated.mnemonic.as_str(), password)
        .expect("restore through runtime");
    assert!(restored.address.starts_with("tgrin1"));
    assert!(restored.needs_scan);
    assert!(!restored.history_key);

    runtime.open(password).expect("open restored wallet");
    assert!(runtime.rpc().unlocked());
    assert_eq!(runtime.address().unwrap(), restored.address);
    let status = runtime.seed_status().expect("seed status after restore");
    assert_eq!(status.kind, "envelope");
    runtime.lock().expect("lock restored wallet");

    let moved = runtime
        .delete_wallet("default")
        .expect("delete restored wallet");
    assert!(
        std::path::Path::new(&moved).is_dir(),
        "trash target missing"
    );
}

#[test]
#[ignore = "spawns grin-wallet and tor.exe; needs the Tor expert bundle"]
fn runtime_starts_and_stops_the_tor_listener() {
    let root = TempRoot::new("tor-live");
    let paths = root.paths();
    let dir = flint_core::rpc_backend::wallet_dir(&paths, "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let password = "runtime-tor-pass-123";
    init_wallet(&dir, password);

    let runtime = WalletRuntime::open_at(paths, BackendKind::GrinWallet);
    runtime.rpc().start().expect("start grin-wallet");
    runtime.open(password).expect("open wallet");
    let started = runtime.tor().start().expect("start Tor listener");
    assert!(!started.onion.is_empty(), "empty onion");
    assert!(started.relay_url.contains("http://"));
    let status = runtime.tor().status().expect("Tor status");
    assert!(status.running);
    assert_eq!(status.onion.as_deref(), Some(started.onion.as_str()));
    assert!(runtime.tor().stop(), "listener was not running");
    assert!(!runtime.tor().status().expect("stopped status").running);
    runtime.shutdown();
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn runtime_starts_opens_locks_and_stops_the_rpc_child() {
    let root = TempRoot::new("rpc");
    let paths = root.paths();
    let dir = flint_core::rpc_backend::wallet_dir(&paths, "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let password = "runtime-rpc-pass-123";
    init_wallet(&dir, password);

    let runtime = WalletRuntime::open_at(paths, BackendKind::GrinWallet);
    runtime.rpc().start().expect("runtime starts grin-wallet");
    assert_eq!(runtime.rpc().phase(), RpcPhase::Ready);

    let opened = runtime.open(password).expect("open through runtime");
    assert_eq!(opened.reconciled, 0);
    assert!(runtime.rpc().unlocked());
    let address = runtime.address().expect("address through runtime");
    assert!(
        address.starts_with("tgrin1"),
        "unexpected address: {address}"
    );
    let status = runtime.status().expect("status through runtime");
    assert_eq!(status.status.mode, "grin-wallet");
    assert!(status.status.unlocked);
    assert_eq!(status.status.address.as_deref(), Some(address.as_str()));
    let profiles = runtime.profiles().expect("profiles through runtime");
    assert_eq!(profiles.active, "default");
    assert_eq!(profiles.profiles, vec!["default".to_string()]);
    assert_eq!(runtime.rpc_slatepack_seed(0).unwrap().len(), 32);
    assert_eq!(runtime.messenger_seed().unwrap().len(), 32);
    runtime.activity(5).expect("activity through runtime");
    let pending = runtime.pending().expect("pending through runtime");
    assert!(pending.items.is_empty());
    assert!(pending.unreadable.is_empty());

    runtime.lock().expect("lock through runtime");
    assert!(!runtime.rpc().unlocked());

    runtime.rpc().stop();
    assert!(runtime.rpc().port().is_none());

    // Restart the child on the same wallet and reopen it: a stopped/restarted
    // RPC backend must come back Ready with the same address, not hang or
    // require a fresh data directory.
    runtime.rpc().start().expect("restart grin-wallet");
    assert_eq!(runtime.rpc().phase(), RpcPhase::Ready);
    runtime.open(password).expect("reopen after RPC restart");
    assert_eq!(
        runtime.address().expect("address after restart"),
        address,
        "RPC restart changed the wallet address"
    );
    runtime.lock().expect("lock after restart");
    runtime.shutdown();
}

#[test]
#[ignore = "spawns the real grin-wallet binary and changes a throwaway wallet password"]
fn rpc_legacy_password_change_reopens_only_with_new_password() {
    let root = TempRoot::new("password-legacy");
    let paths = root.paths();
    let dir = flint_core::rpc_backend::wallet_dir(&paths, "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let old = "OldLegacy!42";
    let new = "NewLegacy#84Q";
    init_wallet(&dir, old);

    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    runtime.rpc().start().expect("start grin-wallet");
    runtime.open(old).expect("open with old password");
    let address = runtime.address().expect("address before change");
    assert_eq!(
        runtime.seed_status().expect("seed status").kind,
        "legacy",
        "legacy fixture unexpectedly became an envelope"
    );
    let outcome = runtime
        .change_password_with_history(old, new)
        .expect("change legacy RPC password");
    assert!(outcome.history_error.is_none(), "{outcome:?}");
    runtime.lock().expect("lock after password change");
    assert!(
        runtime.open(old).is_err(),
        "old password still opens wallet"
    );
    runtime.open(new).expect("open with new password");
    assert_eq!(runtime.address().expect("address after change"), address);
    runtime.shutdown();

    let restarted = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    restarted.rpc().start().expect("restart grin-wallet");
    assert!(
        restarted.open(old).is_err(),
        "old password opens wallet after restart"
    );
    restarted
        .open(new)
        .expect("new password opens wallet after restart");
    assert_eq!(restarted.address().expect("address after restart"), address);
    restarted.shutdown();
}

#[test]
#[ignore = "spawns the real grin-wallet binary and changes a throwaway wallet password"]
fn rpc_envelope_password_change_reopens_only_with_new_password() {
    let root = TempRoot::new("password-envelope");
    let paths = root.paths();
    let dir = flint_core::rpc_backend::wallet_dir(&paths, "testnet", None);
    std::fs::create_dir_all(&dir).expect("wallet dir");
    let old = "OldEnvelope!42";
    let new = "NewEnvelope#84Q";
    let store = SeedStore::new(&dir, "testnet");
    let generated = store.generate().expect("mnemonic");

    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    runtime
        .restore(generated.mnemonic.as_str(), old)
        .expect("restore envelope wallet");
    runtime.open(old).expect("open with old password");
    let address = runtime.address().expect("address before change");
    assert_eq!(
        runtime.seed_status().expect("seed status").kind,
        "envelope",
        "fixture is not an envelope"
    );
    let outcome = runtime
        .change_password_with_history(old, new)
        .expect("change envelope password");
    assert!(outcome.history_error.is_none(), "{outcome:?}");
    runtime.lock().expect("lock after password change");
    assert!(
        runtime.open(old).is_err(),
        "old password still opens wallet"
    );
    runtime.open(new).expect("open with new password");
    assert_eq!(runtime.address().expect("address after change"), address);
    runtime.shutdown();

    let restarted = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    restarted.rpc().start().expect("restart grin-wallet");
    assert!(
        restarted.open(old).is_err(),
        "old password opens wallet after restart"
    );
    restarted
        .open(new)
        .expect("new password opens wallet after restart");
    assert_eq!(restarted.address().expect("address after restart"), address);
    restarted.shutdown();
}
