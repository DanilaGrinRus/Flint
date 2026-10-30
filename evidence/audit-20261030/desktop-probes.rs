use flint_core::{AppPaths, Settings, WalletService};
use flint_messenger::WsRelay;
fn main() {
    for url in ["ws://127.0.0.1.attacker.invalid/", "ws://localhost.attacker.invalid/", "ws://127.0.0.1@attacker.invalid/"] {
        println!("non_loopback_plaintext_accepted {} {}", url, WsRelay::validate_url(url).is_ok());
    }
    let root = std::env::temp_dir().join(format!("flint-audit-synthetic-{}",std::process::id()));
    let paths = AppPaths::at(&root);
    paths.ensure().unwrap();
    let mut settings = Settings::default();
    settings.network = "testnet".into();
    settings.node_url = "https://external-node.invalid".into();
    settings.node_api_secret = None;
    std::fs::write(paths.settings_file(),serde_json::to_vec(&settings).unwrap()).unwrap();
    let node = paths.node_dir("testnet");
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(node.join(".api_secret"),"synthetic-audit-value").unwrap();
    let service = WalletService::open_at(AppPaths::at(&root));
    println!("external_endpoint_inherits_local_secret {}", service.node_snapshot().secret.is_some());
    settings.node_url = "http://external-node.invalid".into();
    std::fs::write(paths.settings_file(),serde_json::to_vec(&settings).unwrap()).unwrap();
    let service = WalletService::open_at(paths);
    println!("persisted_plain_http_preserved {}",service.node_snapshot().url.starts_with("http://external-node.invalid"));
    println!("synthetic_test_data {}",root.display());
}
