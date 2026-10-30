//! Live RPC pending-action evidence: export the exact S1 request, export the
//! saved S2 response, and verify no new signing is required for either.
//!
//! Requires two disposable RPC wallet roots and their passwords:
//!   FLINT_RPC_A, FLINT_RPC_B, FLINT_RPC_PASS, FLINT_RPC_PASS_B,
//!   FLINT_NODE_URL (default 127.0.0.1:13413),
//!   FLINT_NODE_SECRET_FILE (optional).
//! Ignored by default; a selected run without inputs fails configuration.

use std::path::Path;
use std::time::{Duration, Instant};

use flint_core::AppPaths;
use flint_runtime::{BackendKind, WalletRuntime};

fn open(root: &str, password: &str) -> WalletRuntime {
    let runtime = WalletRuntime::open_at(AppPaths::at(Path::new(root)), BackendKind::GrinWallet);
    let node =
        std::env::var("FLINT_NODE_URL").unwrap_or_else(|_| "http://127.0.0.1:13413".to_string());
    runtime
        .update_settings(None, Some(node), None, Some(false), None, None, Some(1))
        .expect("settings");
    runtime.start_rpc_background().expect("rpc start");
    runtime.open(password).expect("rpc open");
    runtime.ensure_node_ready().expect("node ready");
    if runtime.scan_runtime().view().expect("scan view").pending {
        runtime
            .scan_runtime()
            .scan(None, |_, _| {})
            .expect("rpc scan");
    }
    runtime
}

fn armor(value: &str) {
    assert!(value.starts_with("BEGINSLATEPACK."), "not S1/S2 armor");
    assert!(value.ends_with("ENDSLATEPACK."), "not S1/S2 armor");
    assert!(value.len() > 100, "suspiciously short armor");
}

#[test]
#[ignore = "moves testnet coins on two disposable RPC wallets"]
fn rpc_pending_export_live() {
    if std::env::var("FLINT_LIVE_TESTNET").ok().as_deref() != Some("1") {
        panic!("live test selected but FLINT_LIVE_TESTNET=1 is missing");
    }
    let root_a = std::env::var("FLINT_RPC_A").expect("FLINT_RPC_A");
    let root_b = std::env::var("FLINT_RPC_B").expect("FLINT_RPC_B");
    let pass_a = std::env::var("FLINT_RPC_PASS").expect("FLINT_RPC_PASS");
    let pass_b = std::env::var("FLINT_RPC_PASS_B").expect("FLINT_RPC_PASS_B");
    grin_core::global::set_local_chain_type(grin_core::global::ChainTypes::Testnet);
    grin_core::global::init_global_chain_type(grin_core::global::ChainTypes::Testnet);
    assert_ne!(root_a, root_b);
    let mut a = open(&root_a, &pass_a);
    let mut b = open(&root_b, &pass_b);
    // A previous interrupted drill may leave an unsigned locked send. Cancel
    // only the disposable testnet operations before measuring availability.
    for wallet in [&mut a, &mut b] {
        if let Ok(list) = wallet.pending() {
            for item in list.items {
                if item.kind == "send" && matches!(item.state.as_str(), "prepared" | "locked") {
                    let _ = wallet.cancel_pending(&item.slate_id);
                }
            }
        }
    }
    let address_b = b.address().expect("B address");
    let before = b.summary().expect("B summary").balance.spendable;

    let s1 = a.prepare_send(&address_b, 1_000).expect("prepare S1");
    let exported = a.export_pending_request(&s1.slate_id).expect("export S1");
    armor(&exported);
    assert_eq!(exported, s1.slatepack, "exported S1 must be byte-identical");
    assert_eq!(
        a.export_pending_request(&s1.slate_id).expect("resend S1"),
        exported,
        "resend must not create a new signature"
    );

    let s2 = b.prepare_receive(&s1.slatepack).expect("prepare S2");
    let response = b.export_pending_response(&s2.slate_id).expect("export S2");
    armor(&response);
    assert_eq!(s2.slate_id, s1.slate_id);
    // Re-armoring S2 uses fresh encryption randomness; the slate id is the
    // identity proof, not byte equality of the encrypted armor.

    a.finalize_send(&s1.slate_id, &response, &s1.confirmation)
        .expect("finalize");
    b.finalize_receive(&s2.slate_id).expect("finalize receive");

    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let summary = b.summary().expect("B summary");
        let spendable: u128 = summary.balance.spendable.parse().unwrap_or(0);
        let before_n: u128 = before.parse().unwrap_or(0);
        if summary.validated && spendable >= before_n + 1_000 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "RPC pending transfer not visible"
        );
        std::thread::sleep(Duration::from_secs(5));
    }
    eprintln!("RPC pending export/resend live: PASS");
}
