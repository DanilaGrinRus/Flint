//! S4 from `docs/history/ARGON2ID.md` §7: the official `grin-wallet owner_api` process
//! starts in a directory that has no seed yet, and opens the wallet once the
//! seed file appears — without a restart.
//!
//! This is what lets Flint start the RPC process before the user has typed the
//! password, and materialize the transient `wallet.seed` only afterwards.
//!
//! Ignored by default: it spawns `resources/wallet/grin-wallet.exe`.
//!
//!   cargo test -p flint-core --test wallet_rpc_seedless -- --ignored --nocapture

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::WalletRpc;
use serde_json::json;
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

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flint-{tag}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Kills the wallet process even when the test panics half-way.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn create_wallet(dir: &Path, password: &str) {
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

/// Start `owner_api` with whatever is in `dir`; never runs `init`.
fn start_owner_api(dir: &Path) -> (u16, ChildGuard, String) {
    let exe = wallet_exe();
    assert!(
        exe.is_file(),
        "нет {} — запустите tools/fetch-wallet.ps1",
        exe.display()
    );
    let port = free_port();
    let log = std::fs::File::create(dir.join("owner.log")).expect("log");
    let log_err = log.try_clone().expect("log clone");
    let child = Command::new(&exe)
        .args([
            "--testnet",
            "-t",
            &dir.to_string_lossy(),
            "owner_api",
            "-l",
            &port.to_string(),
        ])
        .current_dir(dir)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
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
        assert!(
            Instant::now() < deadline,
            "grin-wallet не создал .owner_api_secret за 40 секунд"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    (port, ChildGuard(child), secret)
}

fn connect_retry(url: &str, secret: &str) -> WalletRpc {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        match WalletRpc::connect(url, secret) {
            Ok(rpc) => return rpc,
            Err(e) => {
                assert!(
                    Instant::now() < deadline,
                    "owner API не поднялся в {url}: {e}"
                );
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates a throwaway wallet"]
fn owner_api_starts_without_seed_and_opens_after_the_seed_appears() {
    let password = "s4-seedless-pass";
    let source = temp_dir("s4-source");
    create_wallet(&source, password);
    let seed = std::fs::read(source.join("wallet_data").join("wallet.seed")).expect("seed");
    assert!(!seed.is_empty(), "grin-wallet init не записал seed");

    // Measure the address on a wallet that had the seed from the start.
    let (port_a, _guard_a, secret_a) = start_owner_api(&source);
    let rpc_a = connect_retry(&format!("http://127.0.0.1:{port_a}"), &secret_a);
    let token_a = rpc_a
        .open_wallet(password)
        .expect("open of the source wallet");
    let expected = rpc_a.address(&token_a).expect("address");
    assert!(expected.starts_with("tgrin1") || expected.starts_with("grin1"));
    rpc_a.close_wallet().expect("close_wallet");

    // The target directory is empty: the process must still come up.
    let target = temp_dir("s4-target");
    let (port_b, _guard_b, secret_b) = start_owner_api(&target);
    let target_seed = target.join("wallet_data").join("wallet.seed");
    assert!(
        !target_seed.exists(),
        "до появления файла в каталоге не должно быть seed"
    );
    let rpc_b = connect_retry(&format!("http://127.0.0.1:{port_b}"), &secret_b);

    // Opening before the seed exists fails; it must not hang the process.
    let before = rpc_b.call_with_timeout(
        "open_wallet",
        json!([null, password]),
        Duration::from_secs(20),
    );
    assert!(
        before.is_err(),
        "открытие кошелька без seed не должно удаваться"
    );
    eprintln!("seedless open rejected: {}", before.unwrap_err());

    // The seed appears while the same process keeps running.
    std::fs::create_dir_all(target.join("wallet_data")).expect("wallet_data");
    std::fs::write(&target_seed, &seed).expect("write seed");

    let token_b = rpc_b
        .open_wallet(password)
        .expect("open_wallet после появления seed");
    let actual = rpc_b.address(&token_b).expect("address");
    assert_eq!(
        actual, expected,
        "один и тот же seed должен давать один и тот же адрес"
    );
    rpc_b.close_wallet().expect("close_wallet");

    let _ = std::fs::remove_dir_all(&source);
    let _ = std::fs::remove_dir_all(&target);
}
