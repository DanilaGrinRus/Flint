// Small CLI over the RPC backend: useful for manual funding and end-to-end runs.
//
//   cargo run -q -p flint-core --example wallet_cli -- address  <dir> <pass>
//   cargo run -q -p flint-core --example wallet_cli -- balance  <dir> <pass>
//   cargo run -q -p flint-core --example wallet_cli -- receive  <dir> <pass> <slatepack-file>
//   cargo run -q -p flint-core --example wallet_cli -- send     <dir> <pass> <dest> <nano>
//   cargo run -q -p flint-core --example wallet_cli -- finalize <dir> <pass> <slate-id> <slatepack-file>
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::seed::{SeedKind, SeedStore};
use flint_core::RpcBackend;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wallet_exe() -> PathBuf {
    std::env::var("FLINT_WALLET_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("resources/wallet/grin-wallet.exe"))
}

fn read_text(path: &str) -> String {
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_string(&mut text))
        .unwrap_or_else(|e| panic!("не читается {path}: {e}"));
    text.trim().to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: wallet_cli <command> <dir> <pass> [...]");
        std::process::exit(2);
    }
    grin_core::global::set_local_chain_type(grin_core::global::ChainTypes::Testnet);
    let command = args[0].clone();
    let dir = PathBuf::from(&args[1]);
    let password = args[2].clone();

    if command == "init" {
        let status = Command::new(wallet_exe())
            .args([
                "--testnet",
                "-t",
                &dir.to_string_lossy(),
                "-p",
                &password,
                "init",
            ])
            .status()
            .expect("init");
        std::process::exit(if status.success() { 0 } else { 1 });
    }

    let port = free_port();
    let log_path = dir.join("cli-owner.log");
    let log = std::fs::File::create(&log_path).expect("log");
    let err = log.try_clone().expect("clone");
    let mut child = Command::new(wallet_exe())
        .args([
            "--testnet",
            "-t",
            &dir.to_string_lossy(),
            "owner_api",
            "--run_foreign",
            "-l",
            &port.to_string(),
        ])
        .current_dir(&dir)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .spawn()
        .expect("owner_api");

    let secret_path = dir.join(".owner_api_secret");
    let deadline = Instant::now() + Duration::from_secs(40);
    let secret = loop {
        if let Ok(text) = std::fs::read_to_string(&secret_path) {
            if !text.trim().is_empty() {
                break text.trim().to_string();
            }
        }
        if Instant::now() > deadline {
            panic!("кошелёк не создал секрет");
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let min_conf: u64 = std::env::var("FLINT_MIN_CONF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let mut backend = loop {
        match RpcBackend::connect(&format!("http://127.0.0.1:{port}"), &secret, min_conf) {
            Ok(b) => break b,
            Err(e) => {
                if Instant::now() > deadline {
                    panic!("backend: {e}");
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    };
    let store = SeedStore::new(&dir, "testnet");
    match store.kind() {
        SeedKind::Envelope => backend
            .open_with_envelope(&store, password.as_str(), Duration::from_secs(30))
            .expect("unlock по конверту"),
        _ => backend.open(&password).expect("unlock"),
    }
    backend.set_journal_dir(dir.join("rpc-pending"));
    backend.set_node_snapshot(flint_core::NodeSnapshot::new(
        "testnet",
        std::env::var("FLINT_NODE_URL").unwrap_or_else(|_| "http://127.0.0.1:13413".to_string()),
        std::env::var("FLINT_NODE_SECRET").ok(),
        min_conf,
    ));

    match command.as_str() {
        "address" => println!("{}", backend.address().expect("address")),
        "balance" => {
            let s = backend.summary().expect("summary");
            println!(
                "spendable={} total={}",
                s.balance.spendable, s.balance.total
            );
        }
        "scan" => {
            let start = args.get(3).and_then(|v| v.parse().ok());
            let height = backend.scan(start).expect("scan");
            println!("scanned_to={height}");
        }
        "receive" => {
            let slatepack = read_text(&args[3]);
            let (response, slate_id, amount) =
                backend.receive_prepare(&slatepack).expect("receive");
            println!("SLATE_ID={slate_id} AMOUNT={amount}");
            println!("{response}");
        }
        "send" => {
            let dest = args[3].clone();
            let amount: u64 = args[4].parse().expect("amount");
            match backend.send_prepare(&dest, amount) {
                Ok((slatepack, slate_id, fee)) => {
                    println!("SLATE_ID={slate_id} FEE={fee}");
                    println!("{slatepack}");
                    // Crash-test hook: the journal and the locked inputs exist,
                    // the caller "dies" before the slate is delivered.
                    if std::env::var("FLINT_CRASH_AFTER_SEND").is_ok() {
                        let _ = child.kill();
                        let _ = child.wait();
                        std::process::exit(9);
                    }
                }
                Err(e) => {
                    eprintln!("send failed: {e}");
                    if e.to_string().contains("CRASH_AFTER_PREPARE") {
                        let _ = child.kill();
                        let _ = child.wait();
                        std::process::exit(9);
                    }
                    std::process::exit(1);
                }
            }
        }
        "finalize" => {
            let slate_id = args[3].clone();
            let slatepack = read_text(&args[4]);
            match backend.send_finalize(&slate_id, &slatepack) {
                Ok(()) => println!("posted"),
                Err(e) => {
                    eprintln!("finalize failed: {e}");
                    if e.to_string().contains("CRASH_AFTER_FINALIZE") {
                        let _ = child.kill();
                        let _ = child.wait();
                        std::process::exit(9);
                    }
                    std::process::exit(1);
                }
            }
        }
        "send-onion" => {
            let dest = args[3].clone();
            let amount: u64 = args[4].parse().expect("amount");
            let seed = backend.slatepack_seed(0).expect("address seed");
            let tor_dir = dir.join("tor-cli");
            let listener =
                flint_core::TorListener::start(&seed, port, &tor_dir).expect("tor listener");
            println!(
                "ONION_PUBLISHED={} SOCKS={}",
                listener.onion(),
                listener.socks_addr()
            );
            match backend.send_over_tor(&dest, amount, &listener.socks_addr()) {
                Ok(tx) => println!("posted tx={tx}"),
                Err(e) => {
                    eprintln!("send-onion failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "send-self" => {
            let amount: u64 = args[3].parse().expect("amount");
            match backend.send_local(amount) {
                Ok(tx) => println!("posted tx={tx}"),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("send-self failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        "stored" => {
            let id = args[3].clone();
            match backend.stored_tx(&id) {
                Ok(v) => println!("{}", serde_json::to_string(&v).unwrap_or_default()),
                Err(e) => eprintln!("stored failed: {e}"),
            }
        }
        "republish" => {
            let id = args[3].clone();
            match backend.republish(&id) {
                Ok(()) => println!("republished"),
                Err(e) => eprintln!("republish failed: {e}"),
            }
        }
        "sender" => {
            let text = read_text(&args[3]);
            match backend.slatepack_sender(&text) {
                Some(addr) => println!("sender={addr}"),
                None => println!("sender=None"),
            }
        }
        "spkey" => {
            let index: u32 = args[3].parse().unwrap_or(0);
            match backend.slatepack_secret_key(index) {
                Ok(v) => println!("{}", serde_json::to_string(&v).unwrap_or_default()),
                Err(e) => eprintln!("spkey failed: {e}"),
            }
        }
        "activity" => match backend.activity(20) {
            Ok(items) => {
                for item in items {
                    println!(
                        "{} {} {} status={} type={} fee={}",
                        item.id, item.direction, item.amount, item.status, item.tx_type, item.fee
                    );
                }
            }
            Err(e) => eprintln!("activity failed: {e}"),
        },
        "pending" => {
            let items = backend.pending().expect("pending");
            if items.is_empty() {
                println!("pending: empty");
            }
            for item in items {
                println!(
                    "{} {} amount={} state={} created={}",
                    item.slate_id, item.kind, item.amount, item.state, item.created_at
                );
            }
        }
        "cancel" => {
            backend.cancel(&args[3]).expect("cancel");
            println!("cancelled");
        }
        other => {
            eprintln!("неизвестная команда: {other}");
            std::process::exit(2);
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[allow(dead_code)]
fn _path(_p: &Path) {}
