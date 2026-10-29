//! Integration test against the **real** grin-wallet binary.
//!
//! Ignored by default: it downloads nothing, but it does spawn
//! `resources/wallet/grin-wallet.exe` (fetch with `tools/fetch-wallet.ps1`) and
//! creates a throwaway wallet directory.
//!
//!   cargo test -p flint-core --test wallet_rpc -- --ignored --nocapture --test-threads=1
//!
//! This covers what cannot be unit-tested: the ECDH handshake, the encrypted
//! session, the auth header and the tuple/string encoding quirks, all against
//! the actual server.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::WalletRpc;
use uuid::Uuid;

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

/// Kills the wallet process even when the test panics half-way.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_wallet(dir: &PathBuf, password: &str) -> (u16, ChildGuard, String) {
    let exe = wallet_exe();
    assert!(
        exe.is_file(),
        "нет {} — запустите tools/fetch-wallet.ps1",
        exe.display()
    );
    let top = dir.to_string_lossy().to_string();

    let init = Command::new(&exe)
        .args(["--testnet", "-t", &top, "-p", password, "init"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("init");
    assert!(init.success(), "grin-wallet init завершился с ошибкой");

    let secret = std::fs::read_to_string(dir.join(".owner_api_secret"))
        .expect("секрет owner API не создан")
        .trim()
        .to_string();

    let port = free_port();
    let log = std::fs::File::create(dir.join("owner.log")).expect("log");
    let log_err = log.try_clone().expect("log clone");
    let child = Command::new(&exe)
        .args([
            "--testnet",
            "-t",
            &top,
            "owner_api",
            "-l",
            &port.to_string(),
        ])
        .current_dir(dir)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("owner_api");

    (port, ChildGuard(child), secret)
}

fn connect_retry(url: &str, secret: &str) -> WalletRpc {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        match WalletRpc::connect(url, secret) {
            Ok(rpc) => return rpc,
            Err(e) => {
                if Instant::now() > deadline {
                    panic!("owner API не поднялся: {e}");
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn temp_wallet_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flint-wallet-rpc-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn handshake_unlock_and_reads() {
    let dir = temp_wallet_dir();
    let password = "spike-pass-123";
    let (port, _guard, secret) = start_wallet(&dir, password);
    let url = format!("http://127.0.0.1:{port}");

    // The handshake is the riskiest part: wrong framing or a wrong shared key
    // makes every later call fail with "decryption error".
    let rpc = connect_retry(&url, &secret);

    let token = rpc.open_wallet(password).expect("open_wallet");
    let address = rpc.address(&token).expect("get_slatepack_address");
    eprintln!("wallet address: {address}");
    assert!(
        address.starts_with("tgrin1") || address.starts_with("grin1"),
        "unexpected address: {address}"
    );

    let (validated, summary) = rpc.summary(&token, true, 10).expect("summary");
    eprintln!("summary (validated={validated}): {summary}");
    assert!(
        summary.get("amount_currently_spendable").is_some(),
        "WalletInfo без amount_currently_spendable: {summary}"
    );

    let txs = rpc.txs(&token, true).expect("txs");
    assert!(txs.is_empty(), "новый кошелёк не должен иметь транзакций");

    let height = rpc.node_height(&token).expect("node_height");
    eprintln!("node height through wallet api: {height}");
    assert!(height > 0, "нода должна сообщить высоту");

    rpc.close_wallet().expect("close_wallet");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn a_wrong_secret_is_rejected() {
    let dir = temp_wallet_dir();
    let (port, _guard, secret) = start_wallet(&dir, "spike-pass-123");
    let url = format!("http://127.0.0.1:{port}");

    // Wait for the listener with the correct secret first, then try a bad one.
    connect_retry(&url, &secret);
    let err = WalletRpc::connect(&url, "definitely-not-the-secret")
        .err()
        .expect("wrong secret must be rejected");
    eprintln!("rejected as expected: {err}");
    assert!(
        err.to_string().contains("секрет") || err.to_string().contains("401"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
