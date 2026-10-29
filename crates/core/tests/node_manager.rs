//! Starts and stops the real bundled node binary. Skipped when `grin.exe` is not
//! present so the suite still passes on machines without it.

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use flint_core::node::{locate_binary, NodeManager};
use flint_core::AppPaths;

/// The real node tests share the same binary and ports; running them in
/// parallel makes `taskkill` race a process that already exited.
static NODE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free port");
    listener.local_addr().expect("addr").port()
}

fn temp_root(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "flint-node-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p
}

#[test]
fn starts_and_stops_bundled_node() {
    let _guard = NODE_TEST_LOCK.lock().unwrap();
    let Some(binary) = locate_binary() else {
        eprintln!("пропуск: grin.exe не найден рядом с проектом");
        return;
    };

    let root = temp_root("start");
    let paths = AppPaths::at(&root);
    let mut manager =
        NodeManager::new(&paths, "testnet", Some(binary.to_str().unwrap())).expect("manager");
    let api_port = free_port();
    let internal_port = free_port();
    manager.set_api_ports(api_port, internal_port);

    assert_eq!(manager.api_port, api_port);
    manager.start().expect("node start");
    assert!(manager.is_running(), "node must stay alive after start");
    assert!(
        TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", manager.api_port).parse().unwrap(),
            Duration::from_secs(2)
        )
        .is_ok(),
        "RPC port must accept connections"
    );
    assert!(root
        .join("testnet")
        .join("node")
        .join("grin-server.toml")
        .exists());

    manager.stop().expect("node stop");
    assert!(!manager.is_running());
    let log = std::fs::read_to_string(root.join("testnet").join("node").join("node.log"))
        .expect("node log");
    assert!(
        log.contains("Shutdown complete"),
        "node must close the chain gracefully; tail:\n{}",
        log.lines().rev().take(12).collect::<Vec<_>>().join("\n")
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn restarts_cleanly_after_a_normal_stop() {
    let _guard = NODE_TEST_LOCK.lock().unwrap();
    let Some(binary) = locate_binary() else {
        eprintln!("пропуск: grin.exe не найден рядом с проектом");
        return;
    };
    let root = temp_root("restart-after-stop");
    let paths = AppPaths::at(&root);
    let mut manager =
        NodeManager::new(&paths, "testnet", Some(binary.to_str().unwrap())).expect("manager");
    manager.set_api_ports(free_port(), free_port());
    manager.start().expect("first node start");
    manager.stop().expect("first node stop");
    assert!(!manager.is_running());
    manager
        .start()
        .expect("node must start again after a normal stop");
    assert!(manager.is_running());
    manager.stop().expect("second node stop");
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
fn starts_again_after_a_hard_kill() {
    let _guard = NODE_TEST_LOCK.lock().unwrap();
    use std::process::Command;

    let Some(binary) = locate_binary() else {
        eprintln!("пропуск: grin.exe не найден рядом с проектом");
        return;
    };
    let root = temp_root("kill-restart");
    let paths = AppPaths::at(&root);
    let mut manager =
        NodeManager::new(&paths, "testnet", Some(binary.to_str().unwrap())).expect("manager");
    let api_port = free_port();
    let internal_port = free_port();
    manager.set_api_ports(api_port, internal_port);
    manager.start().expect("node start");
    let pid = manager.pid().expect("managed pid");
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status()
        .expect("taskkill");
    assert!(status.success(), "taskkill failed");

    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while manager.is_running() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!manager.is_running(), "killed node still reports running");

    manager.stop().ok();
    let mut started = false;
    for _ in 0..20 {
        if manager.start().is_ok() {
            started = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(started, "node did not restart after a hard kill");
    assert!(manager.is_running());
    manager.stop().expect("clean stop after restart");
    let _ = std::fs::remove_dir_all(root);
}
