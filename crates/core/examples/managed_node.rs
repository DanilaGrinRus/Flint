//! Diagnostic: run the managed node through the CORS-stripping proxy.
//!
//! `FLINT_NODE_ROOT` selects the data root (default `.tmp/cors-node`). Prints
//! `API_PORT=<public> INTERNAL_PORT=<node>` once ready and keeps running until
//! killed; dropping the manager stops both the proxy and the node.

use std::time::Duration;

use flint_core::{AppPaths, NodeManager};

fn main() {
    let root = std::env::var("FLINT_NODE_ROOT").unwrap_or_else(|_| ".tmp/cors-node".to_string());
    let paths = AppPaths::at(root);
    let mut manager = NodeManager::new(&paths, "testnet", None).expect("node manager");
    let proxy = std::env::var("FLINT_NODE_PROXY")
        .map(|v| v != "0")
        .unwrap_or(true);
    manager.set_proxy_enabled(proxy);
    manager.start().expect("node start");
    println!(
        "API_PORT={} PROXY={} EXTERNAL={}",
        manager.api_port,
        proxy,
        manager.is_external()
    );
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
