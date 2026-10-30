//! Funded restore lifecycle with a full chain scan and an independent backend oracle.
//!
//! Both tests are ignored by default. They operate only on explicitly marked
//! disposable testnet roots and never print mnemonics, passwords or slatepacks.
//! The work root survives between phase invocations, but its state contains no
//! secrets: the phrase is recovered from the original wallet's encrypted
//! envelope using the password supplied through the environment.
//!
//! Phases: source-restore/source-scan, setup, restore, oracle, verify. Setup is
//! resumable after a confirmed funding transaction and never creates a duplicate.
//!
//!   cargo test -p flint-runtime --test restore_lifecycle -- --ignored --nocapture --test-threads=1

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use flint_core::seed::SeedEnvelope;
use flint_core::{AppPaths, Balance, WalletSummary};
use flint_runtime::{BackendKind, WalletRuntime};
use serde_json::{json, Value};
use zeroize::{Zeroize, Zeroizing};

const AMOUNT: u64 = 100_000_000;

fn armed() -> bool {
    std::env::var("FLINT_LIVE_TESTNET").ok().as_deref() == Some("1")
}

fn password() -> Zeroizing<String> {
    Zeroizing::new(std::env::var("FLINT_LIVE_PASS").expect("FLINT_LIVE_PASS"))
}

fn work_root() -> PathBuf {
    PathBuf::from(std::env::var("FLINT_RESTORE_WORK").expect("FLINT_RESTORE_WORK"))
}

fn source_root() -> PathBuf {
    PathBuf::from(std::env::var("FLINT_RESTORE_SOURCE").expect("FLINT_RESTORE_SOURCE"))
}

fn source_backend() -> BackendKind {
    match std::env::var("FLINT_RESTORE_SOURCE_BACKEND").as_deref() {
        Ok("embedded") | Err(_) => BackendKind::Embedded,
        Ok("grin-wallet") | Ok("rpc") => BackendKind::GrinWallet,
        Ok(other) => panic!("unknown source backend: {other}"),
    }
}

fn opposite(backend: BackendKind) -> BackendKind {
    match backend {
        BackendKind::Embedded => BackendKind::GrinWallet,
        BackendKind::GrinWallet => BackendKind::Embedded,
    }
}

fn assert_disposable(root: &Path) {
    assert!(
        !root.to_string_lossy().to_lowercase().contains("mainnet"),
        "refusing a mainnet-labelled root: {}",
        root.display()
    );
    assert!(
        root.join(".crash-test-ok").is_file(),
        "refusing an unmarked wallet root: {}",
        root.display()
    );
}

fn runtime(root: &Path, backend: BackendKind) -> WalletRuntime {
    let paths = AppPaths::at(root);
    let secret_source = std::env::var("FLINT_NODE_SECRET_FILE")
        .unwrap_or_else(|_| ".node/testnet/node/.api_secret".to_string());
    let node_dir = paths.node_dir("testnet");
    std::fs::create_dir_all(&node_dir).expect("create test node-auth directory");
    std::fs::copy(&secret_source, node_dir.join(".api_secret"))
        .expect("copy testnet node API secret into disposable root");
    let runtime = WalletRuntime::open_at(paths, backend);
    let node_url =
        std::env::var("FLINT_NODE_URL").unwrap_or_else(|_| "http://127.0.0.1:13413".to_string());
    runtime
        .update_settings(None, Some(node_url), None, Some(false), None, None, Some(1))
        .expect("configure disposable testnet wallet");
    runtime
}

fn unlock(runtime: &WalletRuntime, backend: BackendKind, password: &str) {
    if backend.is_rpc() {
        runtime.start_rpc_background().expect("start grin-wallet");
    }
    runtime.open(password).expect("open wallet");
    runtime.ensure_node_ready().expect("testnet node ready");
}

fn open_existing(root: &Path, backend: BackendKind, password: &str) -> WalletRuntime {
    assert_disposable(root);
    let runtime = runtime(root, backend);
    unlock(&runtime, backend, password);
    runtime
}

fn wallet_home(root: &Path, backend: BackendKind) -> PathBuf {
    let paths = AppPaths::at(root);
    if backend.is_rpc() {
        flint_core::rpc_backend::wallet_dir(&paths, "testnet", None)
    } else {
        paths.wallet_home_for("testnet", None)
    }
}

fn original_mnemonic(work: &Path, backend: BackendKind, password: &str) -> Zeroizing<String> {
    let envelope = wallet_home(&work.join("original"), backend).join("wallet_data/wallet.seed.v2");
    let opened = SeedEnvelope::read(&envelope)
        .expect("read original envelope")
        .open(password)
        .expect("decrypt original envelope");
    let mnemonic = grin_keychain::mnemonic::from_entropy(opened.seed.as_slice())
        .expect("encode original mnemonic");
    Zeroizing::new(mnemonic)
}

fn expected_address(mnemonic: &str) -> String {
    let entropy = flint_core::seed::entropy_from_mnemonic(mnemonic).expect("mnemonic entropy");
    flint_core::address::address_from_entropy(entropy.as_slice(), true, 0)
        .expect("independent address from entropy")
}

fn state_path(work: &Path) -> PathBuf {
    work.join("state.json")
}

fn load_state(work: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(state_path(work)).expect("read phase state"))
        .expect("parse phase state")
}

fn save_state(work: &Path, state: &Value) {
    std::fs::write(
        state_path(work),
        serde_json::to_vec_pretty(state).expect("encode phase state"),
    )
    .expect("write phase state");
}

fn assert_balance_eq(left: &Balance, right: &Balance, context: &str) {
    assert_eq!(left.spendable, right.spendable, "{context}: spendable");
    assert_eq!(left.awaiting, right.awaiting, "{context}: awaiting");
    assert_eq!(left.total, right.total, "{context}: total");
    assert_eq!(left.immature, right.immature, "{context}: immature");
    assert_eq!(left.locked, right.locked, "{context}: locked");
}

fn assert_restored_summary(summary: &WalletSummary, expected: &Balance) {
    assert!(summary.validated, "restored summary is not chain-validated");
    assert!(!summary.needs_scan, "restored wallet still needs a scan");
    assert_balance_eq(&summary.balance, expected, "restored balance");
}

fn await_funding(runtime: &WalletRuntime) -> WalletSummary {
    if runtime.is_rpc() {
        let pending = runtime
            .scan_runtime()
            .view()
            .expect("funding scan view")
            .pending;
        runtime
            .scan_runtime()
            .scan(if pending { None } else { Some(1) }, |_, _| {})
            .expect("explicit full scan before RPC funding summary");
    }
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        let summary = runtime.summary().expect("funding summary");
        if summary.validated && summary.balance.spendable == AMOUNT.to_string() {
            return summary;
        }
        assert!(Instant::now() < deadline, "funding was not confirmed");
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn source_scan() {
    let source = source_root();
    let backend = source_backend();
    let password = password();
    let runtime = open_existing(&source, backend, password.as_str());
    let start = if runtime
        .scan_runtime()
        .view()
        .expect("source scan view")
        .pending
    {
        None
    } else {
        Some(1)
    };
    runtime
        .scan_runtime()
        .scan(start, |_, _| {})
        .expect("full funded-source scan");
    let summary = runtime.summary().expect("funded-source summary");
    assert!(summary.validated);
    assert!(
        summary
            .balance
            .spendable
            .parse::<u64>()
            .expect("source balance")
            > AMOUNT,
        "funded source has insufficient testnet balance"
    );
    eprintln!("{}: funded-source full scan passed", backend.as_str());
    runtime.shutdown();
}

fn source_restore() {
    let source = source_root();
    assert!(
        !source.exists(),
        "source restore requires a clean root: {}",
        source.display()
    );
    std::fs::create_dir(&source).expect("create funded-source restore root");
    std::fs::write(source.join(".crash-test-ok"), "testnet").expect("mark funded source");
    let mnemonic = Zeroizing::new(
        std::env::var("FLINT_LIVE_MNEMONIC").expect("FLINT_LIVE_MNEMONIC for source restore"),
    );
    let password = password();
    let runtime = runtime(&source, source_backend());
    let expected = expected_address(mnemonic.as_str());
    let restored = runtime
        .restore(mnemonic.as_str(), password.as_str())
        .expect("restore funded source from phrase");
    assert_eq!(restored.address, expected);
    assert!(restored.needs_scan);
    if runtime
        .status()
        .expect("source restore status")
        .status
        .unlocked
    {
        runtime.lock().expect("lock funded source");
    }
    unlock(&runtime, source_backend(), password.as_str());
    runtime
        .scan_runtime()
        .scan(None, |_, _| {})
        .expect("full funded-source restore scan");
    let summary = runtime.summary().expect("restored funded-source summary");
    assert!(summary.validated);
    assert!(!summary.needs_scan);
    assert_eq!(summary.address, expected);
    assert!(
        summary
            .balance
            .spendable
            .parse::<u64>()
            .expect("source balance")
            > AMOUNT,
        "restored funded source has insufficient testnet balance"
    );
    eprintln!("funded source restore and full scan passed");
    runtime.shutdown();
}

fn export_credentials(
    backend: BackendKind,
    original: &Path,
    address: &str,
    password: &str,
    mnemonic: &str,
) {
    let Ok(credentials) = std::env::var("FLINT_RESTORE_CREDENTIALS") else {
        return;
    };
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(credentials)
        .expect("open explicitly requested credentials file");
    writeln!(file, "[second-wallet-{}]", backend.as_str())
        .and_then(|()| writeln!(file, "profile={}", original.display()))
        .and_then(|()| writeln!(file, "network=testnet"))
        .and_then(|()| writeln!(file, "address={address}"))
        .and_then(|()| writeln!(file, "password={password}"))
        .and_then(|()| writeln!(file, "seed={mnemonic}"))
        .and_then(|()| writeln!(file))
        .expect("append explicitly requested wallet credentials");
}

fn save_setup(work: &Path, balance: &Balance) {
    save_state(
        work,
        &json!({
            "setup": true,
            "original_balance": balance,
            "amount_nano": AMOUNT.to_string(),
        }),
    );
}

fn finish_setup(backend: BackendKind) {
    let work = work_root();
    if state_path(&work).is_file() && load_state(&work)["setup"] == true {
        eprintln!("{}: setup already complete", backend.as_str());
        return;
    }
    let original = work.join("original");
    let password = password();
    let original_runtime = open_existing(&original, backend, password.as_str());
    let funded = await_funding(&original_runtime);
    assert_eq!(funded.balance.total, AMOUNT.to_string());
    let mnemonic = original_mnemonic(&work, backend, password.as_str());
    assert_eq!(funded.address, expected_address(mnemonic.as_str()));
    export_credentials(
        backend,
        &original,
        &funded.address,
        password.as_str(),
        mnemonic.as_str(),
    );
    save_setup(&work, &funded.balance);
    original_runtime.shutdown();
    eprintln!(
        "{}: resumed funded setup without a new transaction",
        backend.as_str()
    );
}

fn setup(backend: BackendKind) {
    let work = work_root();
    if work.exists() {
        finish_setup(backend);
        return;
    }
    assert!(
        !work.to_string_lossy().to_lowercase().contains("mainnet"),
        "refusing a mainnet-labelled work root"
    );
    std::fs::create_dir_all(&work).expect("create work root");
    std::fs::write(work.join(".crash-test-ok"), "testnet").expect("mark work root");

    let original = work.join("original");
    std::fs::create_dir(&original).expect("create original root");
    std::fs::write(original.join(".crash-test-ok"), "testnet").expect("mark original");
    let password = password();
    let original_runtime = runtime(&original, backend);
    let mut created = original_runtime
        .create(password.as_str())
        .expect("create original");
    let mnemonic = Zeroizing::new(created.mnemonic.clone());
    created.mnemonic.zeroize();
    let expected_address = expected_address(mnemonic.as_str());
    assert_eq!(created.address, expected_address, "created address oracle");
    original_runtime
        .ensure_node_ready()
        .expect("testnet node ready");

    let source = source_root();
    let source_runtime = open_existing(&source, source_backend(), password.as_str());
    assert!(
        !source_runtime
            .scan_runtime()
            .view()
            .expect("source scan state")
            .pending,
        "run source-scan before funding"
    );
    let slate = source_runtime
        .prepare_send(&created.address, AMOUNT)
        .expect("prepare restore funding");
    let response = original_runtime
        .prepare_receive(&slate.slatepack)
        .expect("receive restore funding");
    source_runtime
        .finalize_send(&slate.slate_id, &response.slatepack, &slate.confirmation)
        .expect("finalize and post restore funding");
    let funded = await_funding(&original_runtime);
    assert_eq!(funded.balance.total, AMOUNT.to_string());
    export_credentials(
        backend,
        &original,
        &created.address,
        password.as_str(),
        mnemonic.as_str(),
    );
    save_setup(&work, &funded.balance);
    eprintln!("{}: create/fund/confirm passed", backend.as_str());
    source_runtime.shutdown();
    original_runtime.shutdown();
}

fn restore_scan(
    root: &Path,
    backend: BackendKind,
    mnemonic: &str,
    password: &str,
) -> WalletSummary {
    assert!(
        !root.exists(),
        "restore root is not clean: {}",
        root.display()
    );
    std::fs::create_dir(root).expect("create clean restore root");
    std::fs::write(root.join(".crash-test-ok"), "testnet").expect("mark restore root");
    let runtime = runtime(root, backend);
    assert!(
        !runtime
            .status()
            .expect("fresh restore status")
            .status
            .initialized
    );
    let outcome = runtime
        .restore(mnemonic, password)
        .expect("restore from phrase");
    let expected_address = expected_address(mnemonic);
    assert_eq!(outcome.address, expected_address, "restore address oracle");
    assert!(outcome.needs_scan, "restore did not request a full scan");
    if runtime.status().expect("restore status").status.unlocked {
        runtime.lock().expect("lock restored wallet");
    }
    unlock(&runtime, backend, password);
    assert_eq!(
        runtime.address().expect("restored address"),
        expected_address
    );
    assert!(
        runtime.scan_runtime().scan(Some(1), |_, _| {}).is_err(),
        "partial scan was accepted for a restored wallet"
    );
    runtime
        .scan_runtime()
        .scan(None, |_, _| {})
        .expect("full restored-wallet chain scan");
    let summary = runtime.summary().expect("restored summary");
    assert_eq!(summary.address, expected_address);
    assert_eq!(summary.balance.spendable, AMOUNT.to_string());
    assert!(summary.validated, "restored summary is not chain-validated");
    assert!(!summary.needs_scan, "restored wallet still needs a scan");
    runtime.shutdown();
    summary
}

fn restore_phase(backend: BackendKind) {
    let work = work_root();
    assert_disposable(&work);
    assert_eq!(load_state(&work)["setup"], true);
    let password = password();
    let mnemonic = original_mnemonic(&work, backend, password.as_str());
    let expected = load_state(&work)["original_balance"].clone();
    let expected: Balance = serde_json::from_value(expected).expect("original balance");
    let summary = restore_scan(
        &work.join("restored"),
        backend,
        mnemonic.as_str(),
        password.as_str(),
    );
    assert_balance_eq(&summary.balance, &expected, "original versus restored");

    let restarted = open_existing(&work.join("restored"), backend, password.as_str());
    assert_eq!(
        restarted.address().expect("address after restore restart"),
        expected_address(mnemonic.as_str())
    );
    let after_restart = restarted.summary().expect("summary after restore restart");
    assert_restored_summary(&after_restart, &expected);
    restarted.shutdown();

    let mut state = load_state(&work);
    state["restored"] = json!(true);
    state["restored_balance"] = json!(summary.balance);
    save_state(&work, &state);
    eprintln!("{}: restore/full-scan/restart passed", backend.as_str());
}

fn oracle_phase(backend: BackendKind) {
    let work = work_root();
    assert_disposable(&work);
    assert_eq!(load_state(&work)["restored"], true);
    let password = password();
    let mnemonic = original_mnemonic(&work, backend, password.as_str());
    let state = load_state(&work);
    let restored: Balance =
        serde_json::from_value(state["restored_balance"].clone()).expect("restored balance");
    let oracle = opposite(backend);
    let summary = restore_scan(
        &work.join("oracle"),
        oracle,
        mnemonic.as_str(),
        password.as_str(),
    );
    assert_balance_eq(&summary.balance, &restored, "independent backend oracle");
    let mut state = load_state(&work);
    state["oracle"] = json!(true);
    state["oracle_balance"] = json!(summary.balance);
    save_state(&work, &state);
    eprintln!(
        "{}: independent {} oracle full scan and balance match passed",
        backend.as_str(),
        oracle.as_str()
    );
}

fn read_verified(
    root: &Path,
    backend: BackendKind,
    password: &str,
    expected_address: &str,
) -> WalletSummary {
    let runtime = open_existing(root, backend, password);
    assert_eq!(
        runtime.address().expect("verification address"),
        expected_address
    );
    let summary = runtime.summary().expect("verification summary");
    runtime.shutdown();
    summary
}

fn delete_restored(root: &Path, backend: BackendKind) {
    let runtime = runtime(root, backend);
    let moved = runtime
        .delete_wallet("default")
        .expect("delete restored wallet");
    assert!(Path::new(&moved).is_dir(), "wallet was not moved to trash");
    runtime.shutdown();
}

fn verify(backend: BackendKind) {
    let work = work_root();
    assert_disposable(&work);
    let state = load_state(&work);
    assert_eq!(state["oracle"], true);
    let password = password();
    let mnemonic = original_mnemonic(&work, backend, password.as_str());
    let expected_address = expected_address(mnemonic.as_str());
    let original = read_verified(
        &work.join("original"),
        backend,
        password.as_str(),
        &expected_address,
    );
    let restored = read_verified(
        &work.join("restored"),
        backend,
        password.as_str(),
        &expected_address,
    );
    let oracle_backend = opposite(backend);
    let oracle = read_verified(
        &work.join("oracle"),
        oracle_backend,
        password.as_str(),
        &expected_address,
    );
    assert_balance_eq(
        &original.balance,
        &restored.balance,
        "final original/restored",
    );
    assert_balance_eq(&restored.balance, &oracle.balance, "final restored/oracle");
    assert_eq!(restored.balance.spendable, AMOUNT.to_string());

    delete_restored(&work.join("restored"), backend);
    delete_restored(&work.join("oracle"), oracle_backend);
    delete_restored(&work.join("original"), backend);
    eprintln!(
        "{}: independent address and exact balance reconciliation passed ({} nano)",
        backend.as_str(),
        restored.balance.spendable
    );
    std::fs::remove_dir_all(&work).expect("remove disposable work root");
}

fn exercise(backend: BackendKind) {
    if !armed() {
        eprintln!("FLINT_LIVE_TESTNET is not set: nothing to do");
        return;
    }
    grin_core::global::set_local_chain_type(grin_core::global::ChainTypes::Testnet);
    grin_core::global::init_global_chain_type(grin_core::global::ChainTypes::Testnet);
    match std::env::var("FLINT_RESTORE_PHASE")
        .as_deref()
        .expect("FLINT_RESTORE_PHASE")
    {
        "source-restore" => source_restore(),
        "source-scan" => source_scan(),
        "setup" => setup(backend),
        "restore" => restore_phase(backend),
        "oracle" => oracle_phase(backend),
        "verify" => verify(backend),
        other => panic!("unknown restore phase: {other}"),
    }
}

#[test]
#[ignore = "funded disposable testnet restore with full scans"]
fn embedded_full_restore_lifecycle() {
    exercise(BackendKind::Embedded);
}

#[test]
#[ignore = "funded disposable testnet restore with full scans"]
fn rpc_full_restore_lifecycle() {
    exercise(BackendKind::GrinWallet);
}
