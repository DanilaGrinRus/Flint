// CLI over the embedded wallet backend (the one used by the existing wallet
// data), for scripted end-to-end runs.
//
//   FLINT_DATA_DIR=dist/data FLINT_WALLET_PASS=... \
//   cargo run -q -p flint-core --example embedded_cli -- balance
//
//   ... -- address
//   ... -- send <slatepack-address> <nano>
//   ... -- finalize <slate-id> <file-with-response-slatepack>
//   ... -- pending
use std::io::Read;

use flint_core::WalletService;

fn read_text(path: &str) -> String {
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_string(&mut text))
        .unwrap_or_else(|e| panic!("не читается {path}: {e}"));
    text.trim().to_string()
}

fn main() {
    let password = std::env::var("FLINT_WALLET_PASS").expect("FLINT_WALLET_PASS не задан");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().cloned().unwrap_or_else(|| "balance".into());

    if command == "init" {
        let mut fresh = WalletService::new();
        match fresh.create_wallet(&password) {
            Ok(created) => println!("address={}", created.address),
            Err(e) => {
                eprintln!("init failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if command == "restore" {
        let mnemonic = std::env::var("FLINT_WALLET_MNEMONIC")
            .expect("FLINT_WALLET_MNEMONIC не задан для restore");
        let mut fresh = WalletService::new();
        match fresh.restore_wallet(&mnemonic, &password) {
            Ok(restored) => println!("address={}", restored.address),
            Err(e) => {
                eprintln!("restore failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let mut wallet = WalletService::new();
    wallet
        .open_wallet_and_reconcile(&password)
        .expect("не удалось открыть кошелёк");

    match command.as_str() {
        "balance" => {
            let s = wallet.summary().expect("summary");
            println!(
                "address={} spendable={} awaiting={} total={} height={}",
                s.address,
                s.balance.spendable,
                s.balance.awaiting,
                s.balance.total,
                s.last_confirmed_height
            );
        }
        "address" => println!("{}", wallet.address().expect("address")),
        "activity" => {
            for item in wallet.activity(20).expect("activity") {
                println!(
                    "{} {} {} status={} type={} fee={}",
                    item.id, item.direction, item.amount, item.status, item.tx_type, item.fee
                );
            }
        }
        "pending" => {
            for item in wallet.list_pending().expect("pending") {
                println!(
                    "{} {} amount={} state={:?} attempts={}",
                    item.slate_id, item.kind, item.amount, item.state, item.attempts
                );
            }
        }
        "send" => {
            let dest = args.get(1).cloned().expect("нужен адрес получателя");
            let amount: u64 = args
                .get(2)
                .and_then(|v| v.parse().ok())
                .expect("нужна сумма в нано");
            match wallet.prepare_send(&dest, amount) {
                Ok(slate) => {
                    println!(
                        "SLATE_ID={} FEE={} AMOUNT={}",
                        slate.slate_id, slate.fee_nano, slate.amount_nano
                    );
                    println!("{}", slate.slatepack);
                    // Crash-test hook: journal exists, slate never delivered.
                    if std::env::var("FLINT_CRASH_AFTER_SEND").is_ok() {
                        std::process::exit(9);
                    }
                }
                Err(e) => {
                    eprintln!("send failed: {e}");
                    if e.to_string().contains("CRASH_AFTER_PREPARE") {
                        std::process::exit(9);
                    }
                    std::process::exit(1);
                }
            }
        }
        "scan" => {
            let start: Option<u64> = args.get(1).and_then(|v| v.parse().ok());
            match wallet.scan_wallet(start) {
                Ok(height) => println!("scanned_to={height}"),
                Err(e) => eprintln!("scan failed: {e}"),
            }
        }
        "receive" => {
            let path = args.get(1).cloned().expect("нужен файл со slatepack");
            let text = read_text(&path);
            match wallet.prepare_receive(&text) {
                Ok(received) => {
                    println!(
                        "SLATE_ID={} AMOUNT={}",
                        received.slate_id, received.amount_nano
                    );
                    println!("{}", received.slatepack);
                }
                Err(e) => {
                    eprintln!("receive failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "finalize" => {
            let slate_id = args.get(1).cloned().expect("нужен slate id");
            let slatepack = read_text(args.get(2).expect("нужен файл с ответом"));
            match wallet.finalize_send(&slate_id, &slatepack) {
                Ok(tx) => println!("posted tx={tx}"),
                Err(e) => {
                    eprintln!("finalize failed: {e}");
                    if e.to_string().contains("CRASH_AFTER_FINALIZE") {
                        std::process::exit(9);
                    }
                    std::process::exit(1);
                }
            }
        }
        "kernel" => {
            // Chain-gated harnesses need the original binary-backed transaction:
            // compact get_stored_tx slates can lose signatures in a round-trip.
            let id =
                uuid::Uuid::parse_str(args.get(1).expect("нужен slate id")).expect("slate UUID");
            let path = std::path::PathBuf::from(
                std::env::var_os("FLINT_DATA_DIR").expect("FLINT_DATA_DIR"),
            )
            .join("testnet/wallet/wallet_data/saved_txs")
            .join(format!("{id}.grintx"));
            let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("не читается signed transaction {}: {e}", path.display())
            });
            let bytes = grin_util::from_hex(raw.trim()).expect("signed transaction hex");
            let transaction: grin_core::core::Transaction = grin_core::ser::deserialize(
                &mut &bytes[..],
                grin_core::ser::ProtocolVersion(1),
                grin_core::ser::DeserializationMode::default(),
            )
            .expect("signed transaction binary");
            use grin_util::ToHex;
            for kernel in &transaction.body.kernels {
                println!("KERNEL_EXCESS={}", kernel.excess.to_hex());
                println!(
                    "KERNEL_EXCESS_SIG={}",
                    kernel.excess_sig.to_raw_data().to_vec().to_hex()
                );
            }
        }
        "cancel" => {
            let slate_id = args.get(1).cloned().expect("нужен slate id");
            wallet.cancel_pending(&slate_id).expect("cancel");
            println!("cancelled");
        }
        other => {
            eprintln!("неизвестная команда: {other}");
            std::process::exit(2);
        }
    }
}
