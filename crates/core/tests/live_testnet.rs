//! Live testnet lifecycle: send → receive → finalize → confirmation, then
//! cancel, on two real wallets with a real node.
//!
//! Ignored by default and armed explicitly:
//!
//!   FLINT_LIVE_TESTNET=1
//!   FLINT_LIVE_A=<wallet data dir (sender)>
//!   FLINT_LIVE_B=<wallet data dir (receiver)>
//!   FLINT_LIVE_PASS=<wallet password>        (optional, default below)
//!   FLINT_NODE_URL=<node url>                (optional, default testnet port)
//!   FLINT_LIVE_TIMEOUT_SECS=<confirmation timeout> (optional, default 300)
//!
//! The directories may already contain wallets: the scenario opens them instead
//! of creating anything. The sender must be funded; mining is out of scope.
//!
//! Node readiness comes from the wallet's own `node_info()`: after the P0 fix
//! the node requires owner-API auth, and the wallet knows how to present it.

use std::time::{Duration, Instant};

use flint_core::{AppPaths, WalletService};

fn env_root(name: &str) -> Option<std::path::PathBuf> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .map(Into::into)
}

fn spendable(service: &WalletService) -> u128 {
    service
        .summary()
        .expect("summary")
        .balance
        .spendable
        .parse()
        .unwrap_or(0)
}

#[test]
#[ignore = "live testnet scenario; needs a synced node and funded wallets"]
fn live_testnet_send_receive_confirm_cancel() {
    if std::env::var("FLINT_LIVE_TESTNET").ok().as_deref() != Some("1") {
        eprintln!("FLINT_LIVE_TESTNET is not set: nothing to do");
        return;
    }
    let node_url = std::env::var("FLINT_NODE_URL").ok();
    let root_a = env_root("FLINT_LIVE_A").expect("FLINT_LIVE_A must point to the sender data dir");
    let root_b =
        env_root("FLINT_LIVE_B").expect("FLINT_LIVE_B must point to the receiver data dir");
    let password =
        std::env::var("FLINT_LIVE_PASS").unwrap_or_else(|_| "live-testnet-scenario".to_string());
    let timeout: u64 = std::env::var("FLINT_LIVE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    // The receiver may be a fresh profile and therefore needs a password that
    // satisfies the current create-wallet policy even when the funded sender is
    // an older test fixture with a shorter password.
    let password_b = std::env::var("FLINT_LIVE_PASS_B").unwrap_or_else(|_| password.clone());

    let mut a = WalletService::open_at(AppPaths::at(root_a));
    let mut b = WalletService::open_at(AppPaths::at(root_b));
    // A pre-seeded fixture may already carry the correct testnet settings and
    // node ACL. Reusing them avoids rewriting settings in an environment where
    // the ACL helper cannot change the temporary file's DACL.
    let reuse_settings = std::env::var("FLINT_LIVE_REUSE_SETTINGS").ok().as_deref() == Some("1");
    if !reuse_settings {
        if let Some(url) = &node_url {
            a.update_settings(None, Some(url.clone()), None, None, None, None, Some(1))
                .expect("settings a");
            b.update_settings(None, Some(url.clone()), None, None, None, None, Some(1))
                .expect("settings b");
        } else {
            a.update_settings(None, None, None, None, None, None, Some(1))
                .expect("settings a");
            b.update_settings(None, None, None, None, None, None, Some(1))
                .expect("settings b");
        }
    }

    // Readiness is checked only after opening: the wallet fetches the node's own
    // status (with the managed secret) rather than trusting a TCP port.
    if !a.initialized() {
        a.create_wallet(&password).expect("create sender wallet");
    }
    a.open_wallet(&password).expect("open sender wallet");
    if !b.initialized() {
        b.create_wallet(&password_b)
            .expect("create receiver wallet");
    }
    b.open_wallet(&password_b).expect("open receiver wallet");

    let info = a.node_info();
    assert!(
        info.phase.ready(),
        "node is not ready: phase {:?}, height {:?}",
        info.phase,
        info.height
    );
    eprintln!("node ready: height {} peers {:?}", info.height, info.peers);

    let sender_address = a.address().expect("sender address");
    let receiver_address = b.address().expect("receiver address");
    eprintln!("sender {sender_address}");
    eprintln!("receiver {receiver_address}");

    let funded = spendable(&a);
    assert!(
        funded > 0,
        "sender wallet has no testnet coins: fund {sender_address} before running the scenario"
    );

    // 1. Send -> receive -> finalize -> post.
    let amount: u64 = 100_000_000; // 0.1 GRIN keeps the scenario cheap
    let slate = a
        .prepare_send(&receiver_address, amount)
        .expect("prepare send");
    let response = b
        .prepare_receive(&slate.slatepack)
        .expect("prepare receive");
    a.finalize_send(&slate.slate_id, &response.slatepack, &slate.confirmation)
        .expect("finalize and post");

    // 2. Wait until the receiver's spendable balance grows by the amount.
    let before_b = spendable(&b);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if spendable(&b) >= before_b + amount as u128 {
            eprintln!("receiver confirmed: {}", spendable(&b));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "transfer was not confirmed within {timeout} seconds"
        );
        std::thread::sleep(Duration::from_secs(5));
    }

    // 3. Cancellation must return the sender's inputs.
    let before_a = spendable(&a);
    let second = a
        .prepare_send(&receiver_address, amount)
        .expect("prepare second send");
    a.cancel_pending(&second.slate_id).expect("cancel");
    let after_a = spendable(&a);
    assert!(
        after_a >= before_a,
        "after cancellation spendable {after_a} < before {before_a}"
    );
    eprintln!("cancellation restored spendable: {before_a} -> {after_a}");
}
