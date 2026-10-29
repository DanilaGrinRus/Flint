// Probes Tor connectivity: posts to the given URL through arti.
//
//   RUST_LOG=tor_hsclient=debug \
//   cargo run -q -p flint-core --example tor_probe -- <state dir> <url>
fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| ".tmp/tor-probe".to_string());
    let url = args
        .next()
        .unwrap_or_else(|| "http://example.com/".to_string());

    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
    let _ = tracing_log::LogTracer::init();

    let config = grin_wallet_config::TorConfig {
        send_config_dir: dir,
        ..grin_wallet_config::TorConfig::default()
    };
    let started = std::time::Instant::now();
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});
    match grin_wallet_impls::tor::arti::tor_post(&config, &body, &url) {
        Ok(text) => println!(
            "OK after {:?}: {}",
            started.elapsed(),
            text.chars().take(120).collect::<String>()
        ),
        Err(e) => println!("FAILED after {:?}: {e}", started.elapsed()),
    }
}
