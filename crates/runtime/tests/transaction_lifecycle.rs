//! Full backend-neutral transaction lifecycle on disposable funded testnet wallets.
//!
//! Both tests are ignored by default and must be armed explicitly. Each data root
//! must carry `.crash-test-ok`; the sender and receiver must be independent wallets.
//! Passwords come only from the environment and slatepacks are never logged.
//!
//!   cargo test -p flint-runtime --test transaction_lifecycle -- --ignored --nocapture --test-threads=1

use std::ops::Deref;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use flint_core::{AppPaths, Balance, WalletSummary};
use flint_runtime::{BackendKind, WalletRuntime};
use zeroize::Zeroizing;

struct LiveWallet {
    root: PathBuf,
    backend: BackendKind,
    password: Zeroizing<String>,
    runtime: WalletRuntime,
}

impl LiveWallet {
    fn open(root: PathBuf, backend: BackendKind, password: String) -> Result<Self, String> {
        let password = Zeroizing::new(password);
        let runtime = open_runtime(&root, backend, password.as_str())?;
        Ok(Self {
            root,
            backend,
            password,
            runtime,
        })
    }

    fn restart(&mut self) -> Result<(), String> {
        self.runtime.shutdown();
        self.runtime = open_runtime(&self.root, self.backend, self.password.as_str())?;
        Ok(())
    }

    fn summary(&self) -> WalletSummary {
        self.runtime.summary().expect("wallet summary")
    }

    fn spendable(&self) -> u128 {
        number(&self.summary().balance.spendable)
    }
}

impl Deref for LiveWallet {
    type Target = WalletRuntime;

    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

impl Drop for LiveWallet {
    fn drop(&mut self) {
        self.runtime.shutdown();
    }
}

fn open_runtime(
    root: &PathBuf,
    backend: BackendKind,
    password: &str,
) -> Result<WalletRuntime, String> {
    let runtime = WalletRuntime::open_at(AppPaths::at(root), backend);
    let result = (|| {
        let node_url = std::env::var("FLINT_NODE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:13413".to_string());
        runtime.update_settings(None, Some(node_url), None, Some(false), None, None, Some(1))?;
        if backend.is_rpc() {
            runtime.start_rpc_background()?;
        }
        runtime.open(password)?;
        runtime.ensure_node_ready()?;
        if runtime.scan_runtime().view()?.pending {
            runtime.scan_runtime().scan(None, |_, _| {})?;
        }
        Ok(())
    })();
    if result.is_err() {
        runtime.shutdown();
    }
    result.map(|()| runtime)
}

fn number(value: &str) -> u128 {
    value.parse().expect("numeric wallet balance")
}

fn await_confirmation(
    wallet: &LiveWallet,
    minimum_spendable: u128,
    timeout: Duration,
) -> WalletSummary {
    let deadline = Instant::now() + timeout;
    loop {
        let summary = wallet.summary();
        if summary.validated && number(&summary.balance.spendable) >= minimum_spendable {
            return summary;
        }
        assert!(
            Instant::now() < deadline,
            "transfer was not confirmed within {} seconds",
            timeout.as_secs()
        );
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn await_sender_settled(wallet: &LiveWallet, maximum_awaiting: u128, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let summary = wallet.summary();
        if summary.validated && number(&summary.balance.awaiting) <= maximum_awaiting {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "sender change was not confirmed within {} seconds",
            timeout.as_secs()
        );
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn assert_cancelled_balance(before: &Balance, after: &Balance) {
    assert_eq!(
        number(&after.spendable),
        number(&before.spendable),
        "cancellation did not restore spendable"
    );
    assert_eq!(
        number(&after.locked),
        number(&before.locked),
        "cancellation did not unlock the wallet inputs"
    );
}

fn exercise(backend: BackendKind, full_cycle: bool) {
    if std::env::var("FLINT_LIVE_TESTNET").ok().as_deref() != Some("1") {
        panic!("live test selected but FLINT_LIVE_TESTNET=1 is missing");
    }
    let scan_only = std::env::var("FLINT_LIVE_SCAN_ONLY").ok().as_deref() == Some("1");
    assert_eq!(
        scan_only, !full_cycle,
        "full-cycle test refuses FLINT_LIVE_SCAN_ONLY=1; scan-only test requires it"
    );
    grin_core::global::set_local_chain_type(grin_core::global::ChainTypes::Testnet);
    grin_core::global::init_global_chain_type(grin_core::global::ChainTypes::Testnet);
    let (env_a, env_b) = if matches!(backend, BackendKind::Rpc) {
        ("FLINT_LIVE_RPC_A", "FLINT_LIVE_RPC_B")
    } else {
        ("FLINT_LIVE_A", "FLINT_LIVE_B")
    };
    let root_a = PathBuf::from(std::env::var(env_a).unwrap_or_else(|_| panic!("{env_a}")));
    let root_b = PathBuf::from(std::env::var(env_b).unwrap_or_else(|_| panic!("{env_b}")));
    assert_ne!(root_a, root_b, "sender and receiver roots must differ");
    for root in [&root_a, &root_b] {
        assert!(
            root.join(".crash-test-ok").is_file(),
            "refusing unmarked wallet root: {}",
            root.display()
        );
        assert!(
            !root.to_string_lossy().to_lowercase().contains("mainnet"),
            "refusing a mainnet-labelled wallet root"
        );
    }
    let (pass_a_env, pass_b_env) = if matches!(backend, BackendKind::Rpc) {
        ("FLINT_LIVE_RPC_PASS", "FLINT_LIVE_RPC_PASS_B")
    } else {
        ("FLINT_LIVE_PASS", "FLINT_LIVE_PASS_B")
    };
    let password_a = std::env::var(pass_a_env).unwrap_or_else(|_| panic!("{pass_a_env}"));
    let password_b = std::env::var(pass_b_env).unwrap_or_else(|_| panic!("{pass_b_env}"));
    let timeout = Duration::from_secs(
        std::env::var("FLINT_LIVE_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(300),
    );
    let amount = 100_000_000_u64;

    let mut sender = LiveWallet::open(root_a, backend, password_a).expect("open sender");
    let mut receiver = LiveWallet::open(root_b, backend, password_b).expect("open receiver");
    // Existing direct-CLI fixtures do not have Flint's scan marker. Explicitly
    // scan the whole chain before trusting either funded or empty balance. A
    // scan-only preparation run keeps the money test within an outer timeout.
    if std::env::var("FLINT_LIVE_SKIP_INITIAL_FULL_SCAN")
        .ok()
        .as_deref()
        != Some("1")
    {
        sender
            .scan_runtime()
            .scan(Some(1), |_, _| {})
            .expect("full sender scan");
        receiver
            .scan_runtime()
            .scan(Some(1), |_, _| {})
            .expect("full receiver scan");
    }
    if !full_cycle {
        assert!(
            sender.spendable() > 0 || receiver.spendable() > 0,
            "scan-only run found no funded balance on either disposable wallet"
        );
        eprintln!(
            "{}: scan-only preparation passed; full transfer was NOT run",
            backend.as_str()
        );
        return;
    }
    let receiver_address = receiver.address().expect("receiver address");
    assert!(
        receiver_address.starts_with("tgrin1"),
        "receiver is not on testnet"
    );
    let receiver_before = receiver.spendable();
    let sender_initial = sender.summary();
    assert!(
        sender.spendable() > amount as u128,
        "sender has no funded testnet balance"
    );

    let slate = sender
        .prepare_send(&receiver_address, amount)
        .expect("prepare send");
    let response = receiver
        .prepare_receive(&slate.slatepack)
        .expect("prepare receive");
    sender
        .finalize_send(&slate.slate_id, &response.slatepack)
        .expect("finalize and post");
    let receiver_confirmed =
        await_confirmation(&receiver, receiver_before + amount as u128, timeout);
    await_sender_settled(&sender, number(&sender_initial.balance.awaiting), timeout);
    eprintln!(
        "{}: send/receive/finalize/post/confirm passed; receiver +{} nano",
        backend.as_str(),
        number(&receiver_confirmed.balance.spendable) - receiver_before
    );

    let sender_before_cancel = sender.summary().balance;
    let second = sender
        .prepare_send(&receiver_address, amount)
        .expect("prepare second send");
    let locked = sender.summary().balance;
    assert!(
        number(&locked.locked) > number(&sender_before_cancel.locked),
        "second send did not lock inputs"
    );
    sender.cancel_pending(&second.slate_id).expect("cancel");
    assert!(
        sender
            .pending()
            .expect("pending after cancel")
            .items
            .iter()
            .all(|item| item.slate_id != second.slate_id),
        "cancelled slate remains pending"
    );
    let sender_after_cancel = sender.summary().balance;
    assert_cancelled_balance(&sender_before_cancel, &sender_after_cancel);

    sender.restart().expect("restart sender");
    receiver.restart().expect("restart receiver");
    assert!(
        sender
            .pending()
            .expect("pending after restart")
            .items
            .iter()
            .all(|item| item.slate_id != second.slate_id),
        "cancelled slate returned after restart"
    );
    assert_cancelled_balance(&sender_before_cancel, &sender.summary().balance);
    assert!(
        receiver.spendable() >= receiver_before + amount as u128,
        "confirmed receiver balance was lost after restart"
    );
    eprintln!(
        "{}: cancel restored spendable and unlocked inputs before and after restart",
        backend.as_str()
    );
}

#[test]
#[ignore = "moves testnet coins on explicitly marked funded disposable wallets"]
fn embedded_full_transaction_lifecycle() {
    exercise(BackendKind::Embedded, true);
}

#[test]
#[ignore = "moves testnet coins on explicitly marked funded disposable wallets"]
fn rpc_full_transaction_lifecycle() {
    exercise(BackendKind::GrinWallet, true);
}

#[test]
#[ignore = "scan-only preparation for explicitly marked funded disposable wallets"]
fn embedded_scan_only_live() {
    exercise(BackendKind::Embedded, false);
}

#[test]
#[ignore = "scan-only preparation for explicitly marked funded disposable wallets"]
fn rpc_scan_only_live() {
    exercise(BackendKind::GrinWallet, false);
}
