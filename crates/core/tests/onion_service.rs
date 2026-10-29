//! Live check that the onion service really is derived from the address key.
//!
//! Ignored by default: it bootstraps Tor and needs network access.
//!
//!   RUST_LOG=tor_hsservice=debug,tor_hsclient=debug \
//!   cargo test -p flint-core --test onion_service -- --ignored --nocapture

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use flint_core::{onion_address, OnionService};

#[test]
#[ignore = "bootstraps Tor, needs network"]
fn onion_service_matches_the_derived_address() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let _ = tracing_log::LogTracer::init();

    let seed = [0x23u8; 32];
    let expected = onion_address(&seed).expect("derive onion");
    eprintln!("expected onion: {expected}");

    // A tiny local endpoint the hidden service will forward to.
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = b"{\"ok\":true}";
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    let dir = std::env::temp_dir().join(format!("flint-onion-{}", uuid::Uuid::new_v4()));
    let service =
        OnionService::start(&seed, &format!("127.0.0.1:{port}"), &dir).expect("start onion");
    eprintln!("service onion: {}", service.onion());
    assert_eq!(
        service.onion(),
        expected,
        "onion must come from the address key"
    );

    // Full loop: an arti client reaches the onion, which forwards to the stub.
    // The descriptor takes a while to publish, so retry instead of failing fast.
    let config = grin_wallet_config::TorConfig {
        send_config_dir: dir.to_string_lossy().to_string(),
        ..grin_wallet_config::TorConfig::default()
    };
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});
    let url = format!("http://{}/", service.onion());
    let mut last = String::new();
    let mut ok = false;
    for attempt in 1..=12 {
        match grin_wallet_impls::tor::arti::tor_post(&config, &body, &url) {
            Ok(text) => {
                eprintln!("through onion (attempt {attempt}): {text}");
                assert!(text.contains("ok"), "unexpected onion response: {text}");
                ok = true;
                break;
            }
            Err(e) => {
                last = format!("{e}");
                eprintln!("attempt {attempt} failed: {last}");
                std::thread::sleep(Duration::from_secs(15));
            }
        }
    }
    assert!(ok, "onion request failed after retries: {last}");

    service.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
