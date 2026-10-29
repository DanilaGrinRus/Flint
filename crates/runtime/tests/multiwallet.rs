//! Independent seed/profile lifecycle in one runtime. Live RPC coverage is
//! ignored by default; all directories are unique throwaway testnet roots.

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::AppPaths;
use flint_messenger::MessageKind;
use flint_runtime::{BackendKind, WalletRuntime};
use serde_json::Value;
use uuid::Uuid;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("flint-multiwallet-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("temp root");
        Self(path)
    }

    fn paths(&self) -> AppPaths {
        AppPaths::at(&self.0)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn disable_managed_node(runtime: &WalletRuntime) {
    runtime
        .update_settings(None, None, None, Some(false), None, None, None)
        .expect("disable managed node for profile test");
}

#[test]
fn embedded_profiles_keep_seeds_addresses_and_scan_state_isolated() {
    let root = TempRoot::new("embedded");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::Embedded);
    disable_managed_node(&runtime);
    let initial_generation = runtime.context_generation();
    let first = runtime.create("FirstWallet!42").expect("create default");
    let first_address = runtime.address().expect("default address");
    assert_eq!(first.address, first_address);

    runtime
        .switch(None, Some(Some("alice".into())))
        .expect("select new profile");
    assert!(runtime.context_generation() > initial_generation);
    assert!(!runtime.status().expect("new status").status.initialized);
    let second = runtime.create("SecondWallet#84").expect("create alice");
    let second_address = runtime.address().expect("alice address");
    assert_eq!(second.address, second_address);
    assert_ne!(first_address, second_address);
    assert_ne!(first.mnemonic, second.mnemonic);

    runtime
        .handle()
        .lock()
        .expect("wallet lock")
        .require_scan()
        .expect("mark only alice unknown");
    let profiles = runtime.profiles().expect("profile list");
    assert_eq!(profiles.active, "alice");
    assert_eq!(profiles.profiles, vec!["alice", "default"]);
    assert!(root
        .paths()
        .wallet_home_for("testnet", None)
        .join("wallet_data")
        .exists());
    assert!(root
        .paths()
        .wallet_home_for("testnet", Some("alice"))
        .join("wallet_data")
        .exists());

    runtime.switch(None, Some(None)).expect("back to default");
    assert!(!runtime.status().expect("default status").status.unlocked);
    assert!(!runtime.scan_runtime().view().expect("default scan").pending);
    runtime.open("FirstWallet!42").expect("reopen default");
    assert_eq!(runtime.address().expect("default restored"), first_address);

    runtime
        .switch(None, Some(Some("alice".into())))
        .expect("back to alice");
    assert!(runtime.scan_runtime().view().expect("alice scan").pending);
    runtime.open("SecondWallet#84").expect("reopen alice");
    assert_eq!(runtime.address().expect("alice restored"), second_address);
    runtime.shutdown();
}

#[test]
#[ignore = "spawns the real grin-wallet binary and creates two throwaway profiles"]
fn rpc_create_supports_two_independent_profiles() {
    let root = TempRoot::new("rpc");
    let runtime = WalletRuntime::open_at(root.paths(), BackendKind::GrinWallet);
    disable_managed_node(&runtime);
    let first = runtime
        .create("FirstRpcWallet!42")
        .expect("create RPC default");
    assert!(runtime.rpc().unlocked());
    assert_eq!(
        runtime.address().expect("RPC default address"),
        first.address
    );

    runtime
        .switch(None, Some(Some("alice".into())))
        .expect("select RPC alice");
    assert!(!runtime.status().expect("RPC new status").status.initialized);
    let second = runtime
        .create("SecondRpcWallet#84")
        .expect("create RPC alice");
    assert!(runtime.rpc().unlocked());
    assert_eq!(
        runtime.address().expect("RPC alice address"),
        second.address
    );
    assert_ne!(first.address, second.address);
    assert_ne!(first.mnemonic, second.mnemonic);
    assert_eq!(
        runtime.profiles().expect("RPC profile list").profiles,
        vec!["alice", "default"]
    );

    runtime.switch(None, Some(None)).expect("RPC default again");
    assert!(!runtime.rpc().unlocked());
    runtime
        .open("FirstRpcWallet!42")
        .expect("reopen RPC default");
    assert_eq!(
        runtime.address().expect("RPC default restored"),
        first.address
    );
    runtime.shutdown();
}

struct Relay {
    child: Child,
    dir: PathBuf,
}

impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind free relay port")
        .local_addr()
        .expect("relay local address")
        .port()
}

fn relay_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/flint-relay.exe")
}

fn spawn_relay() -> (Relay, String) {
    let dir = std::env::temp_dir().join(format!("flint-multiwallet-relay-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("relay store");
    let port = free_port();
    let child = Command::new(relay_exe())
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--store",
            &dir.to_string_lossy(),
            "--max-per",
            "200",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn real flint-relay");
    let deadline = Instant::now() + Duration::from_secs(8);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "flint-relay did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    (Relay { child, dir }, format!("http://127.0.0.1:{port}"))
}

fn configure_relay(runtime: &WalletRuntime, url: &str) {
    runtime
        .update_settings(None, None, None, Some(false), None, Some(url.into()), None)
        .expect("configure loopback relay");
}

fn open_with_history(runtime: &WalletRuntime, password: &str) {
    let profile = runtime.profile();
    runtime
        .open(password)
        .unwrap_or_else(|error| panic!("open profile {profile}: {error}"));
    runtime
        .unlock_history_key(password)
        .expect("unlock active profile history");
}

fn messenger_address(runtime: &WalletRuntime) -> String {
    runtime.messenger_status().expect("messenger status")["address"]
        .as_str()
        .expect("unlocked messenger address")
        .to_string()
}

fn history(runtime: &WalletRuntime, peer: &str) -> Vec<Value> {
    runtime
        .messenger_history(peer)
        .expect("profile history")
        .as_array()
        .expect("history array")
        .clone()
}

fn assert_message(record: &Value, id: &str, direction: &str, text: &str) {
    assert_eq!(record["id"], id);
    assert_eq!(record["direction"], direction);
    assert_eq!(
        record["kind"],
        serde_json::to_value(MessageKind::Text { text: text.into() }).expect("message kind")
    );
}

fn live_profile_messenger_history_restart_and_errors(backend: BackendKind) {
    let root = TempRoot::new(match backend {
        BackendKind::Embedded => "messenger-embedded",
        BackendKind::GrinWallet => "messenger-rpc",
    });
    let (_relay, relay_url) = spawn_relay();
    let mut runtime = WalletRuntime::open_at(root.paths(), backend);
    disable_managed_node(&runtime);
    configure_relay(&runtime, &relay_url);
    let default_password = "DefaultProfile!42";
    let alice_password = "AliceProfile#84";

    runtime.create(default_password).expect("create default");
    runtime
        .unlock_history_key(default_password)
        .expect("unlock default history");
    let default_address = messenger_address(&runtime);

    runtime
        .switch(None, Some(Some("alice".into())))
        .expect("select alice");
    configure_relay(&runtime, &relay_url);
    runtime.create(alice_password).expect("create alice");
    runtime
        .unlock_history_key(alice_password)
        .expect("unlock alice history");
    let alice_address = messenger_address(&runtime);
    assert_ne!(default_address, alice_address);
    assert!(history(&runtime, &default_address).is_empty());

    runtime.switch(None, Some(None)).expect("select default");
    open_with_history(&runtime, default_password);
    let first_id = runtime
        .messenger_send(&alice_address, "default → alice")
        .expect("send default to alice")["id"]
        .as_str()
        .expect("first message id")
        .to_string();
    assert_eq!(history(&runtime, &alice_address).len(), 1);

    runtime
        .switch(None, Some(Some("alice".into())))
        .expect("receive as alice");
    open_with_history(&runtime, alice_password);
    let inbox = runtime.messenger_inbox().expect("alice inbox");
    assert_eq!(inbox["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(inbox["messages"][0]["id"], first_id);
    let second_id = runtime
        .messenger_send(&default_address, "alice → default")
        .expect("send alice to default")["id"]
        .as_str()
        .expect("second message id")
        .to_string();

    let generation = runtime.context_generation();
    let error = runtime
        .switch(None, Some(Some("../invalid".into())))
        .expect_err("invalid profile must be rejected");
    assert!(!error.is_empty());
    assert_eq!(runtime.profile(), "alice");
    assert_eq!(runtime.context_generation(), generation);
    assert_eq!(history(&runtime, &default_address).len(), 2);

    runtime.switch(None, Some(None)).expect("default again");
    let error = runtime
        .open("IncorrectDefault!84")
        .expect_err("wrong password must fail");
    assert!(!error.is_empty());
    assert!(!runtime.history_key_ready());
    assert!(runtime.messenger_history(&alice_address).is_err());
    open_with_history(&runtime, default_password);
    let inbox = runtime.messenger_inbox().expect("default inbox");
    assert_eq!(inbox["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(inbox["messages"][0]["id"], second_id);

    let default_history = history(&runtime, &alice_address);
    assert_eq!(default_history.len(), 2);
    assert_message(
        &default_history[0],
        &first_id,
        "outgoing",
        "default → alice",
    );
    assert_message(
        &default_history[1],
        &second_id,
        "incoming",
        "alice → default",
    );

    runtime.shutdown();
    runtime = WalletRuntime::open_at(root.paths(), backend);
    disable_managed_node(&runtime);
    if backend.is_rpc() {
        runtime
            .start_rpc_background()
            .expect("start RPC after runtime restart");
    }
    assert_eq!(runtime.profile(), "default");
    open_with_history(&runtime, default_password);
    assert_eq!(messenger_address(&runtime), default_address);
    assert_eq!(history(&runtime, &alice_address), default_history);

    runtime
        .switch(None, Some(Some("alice".into())))
        .expect("alice after restart");
    open_with_history(&runtime, alice_password);
    assert_eq!(messenger_address(&runtime), alice_address);
    let alice_history = history(&runtime, &default_address);
    assert_eq!(alice_history.len(), 2);
    assert_message(&alice_history[0], &first_id, "incoming", "default → alice");
    assert_message(&alice_history[1], &second_id, "outgoing", "alice → default");
    assert!(
        runtime.messenger_inbox().expect("empty alice inbox")["messages"]
            .as_array()
            .expect("empty messages")
            .is_empty()
    );
    runtime.shutdown();
}

#[test]
#[ignore = "spawns the real flint-relay and exercises two live embedded profiles"]
fn embedded_profiles_messenger_history_restart_and_switch_errors() {
    live_profile_messenger_history_restart_and_errors(BackendKind::Embedded);
}

#[test]
#[ignore = "spawns real flint-relay and grin-wallet processes for two live RPC profiles"]
fn rpc_profiles_messenger_history_restart_and_switch_errors() {
    live_profile_messenger_history_restart_and_errors(BackendKind::GrinWallet);
}
