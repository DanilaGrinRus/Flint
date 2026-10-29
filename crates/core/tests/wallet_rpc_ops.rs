//! Live plumbing check for the RPC write path (send/receive/scan) against the
//! real `grin-wallet` binary.
//!
//! Ignored by default: it spawns the wallet and needs a node for scanning.
//!
//!   cargo test -p flint-core --test wallet_rpc_ops -- --ignored --nocapture --test-threads=1

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::RpcBackend;
use uuid::Uuid;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wallet_exe() -> PathBuf {
    if let Ok(path) = std::env::var("FLINT_WALLET_EXE") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/wallet/grin-wallet.exe")
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

fn start(dir: &PathBuf, password: &str) -> (u16, ChildGuard, String) {
    let exe = wallet_exe();
    assert!(
        exe.is_file(),
        "нет {} — tools/fetch-wallet.ps1",
        exe.display()
    );
    let top = dir.to_string_lossy().to_string();
    let init = Command::new(&exe)
        .args(["--testnet", "-t", &top, "-p", password, "init"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("init");
    assert!(init.success(), "init failed");

    let secret = std::fs::read_to_string(dir.join(".owner_api_secret"))
        .expect("secret")
        .trim()
        .to_string();
    let port = free_port();
    let log = std::fs::File::create(dir.join("owner.log")).expect("log");
    let err = log.try_clone().expect("clone");
    let child = Command::new(&exe)
        .args([
            "--testnet",
            "-t",
            &top,
            "owner_api",
            "--run_foreign",
            "-l",
            &port.to_string(),
        ])
        .current_dir(dir)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .spawn()
        .expect("owner_api");
    (port, ChildGuard(child), secret)
}

fn connect(port: u16, secret: &str) -> RpcBackend {
    let url = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        match RpcBackend::connect(&url, secret, 10) {
            Ok(backend) => return backend,
            Err(e) => {
                if Instant::now() > deadline {
                    panic!("backend не поднялся: {e}");
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

#[test]
#[ignore = "spawns grin-wallet; scanning needs the testnet node"]
fn write_path_talks_to_the_real_wallet() {
    let dir = std::env::temp_dir().join(format!("flint-wallet-ops-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("dir");
    let password = "spike-pass-123";
    let (port, _guard, secret) = start(&dir, password);
    let mut backend = connect(port, &secret);
    backend.set_journal_dir(dir.join("rpc-pending"));
    backend.open(password).expect("open_wallet");

    // Foreign API is mounted on the owner port thanks to --run_foreign. It only
    // exposes a few methods; `build_coinbase` proves the route exists because it
    // answers an argument error instead of "method not found".
    let err = backend
        .foreign("build_coinbase")
        .err()
        .expect("foreign route must answer");
    eprintln!("foreign route alive: {err}");
    assert!(
        err.to_string().contains("Expected 1"),
        "unexpected foreign error: {err}"
    );

    // Scan is the safest write operation on an empty wallet.
    let height = backend.scan(Some(3_300_000)).expect("scan must succeed");
    eprintln!("scanned to height {height}");
    assert!(height > 0);

    // A send with an empty wallet must fail as a wallet error, not a protocol one.
    let destination = backend.address().expect("address");
    let err = backend
        .send_prepare(&destination, 1_000_000_000)
        .err()
        .expect("empty wallet cannot send");
    eprintln!("send rejected as expected: {err}");
    let text = err.to_string().to_lowercase();
    assert!(
        text.contains("fund") || text.contains("not enough") || text.contains("no outputs"),
        "unexpected send error: {err}"
    );

    // Cancelling an unknown transaction must be a wallet error, not a protocol one.
    let cancel_err = backend
        .cancel(&uuid::Uuid::new_v4().to_string())
        .err()
        .expect("unknown tx cannot be cancelled");
    eprintln!("cancel rejected as expected: {cancel_err}");
    let text = cancel_err.to_string().to_lowercase();
    assert!(
        text.contains("not found") || text.contains("unknown") || text.contains("tx"),
        "unexpected cancel error: {cancel_err}"
    );

    // Junk slatepack must not panic the backend.
    let bad = backend.receive_prepare("BEGINSLATEPACK. not really");
    assert!(bad.is_err(), "junk slatepack must be rejected");

    let _ = std::fs::remove_dir_all(&dir);
}
