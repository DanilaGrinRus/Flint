//! Live check of the exact desktop path: `TorListener` publishes an onion that
//! forwards to its local messenger gateway, and a direct envelope reaches it
//! through the same Tor SOCKS proxy.
//!
//! Ignored by default: spawns `tor.exe` and needs the Tor network.

use std::path::PathBuf;
use std::time::Duration;

use flint_core::{onion_address, TorListener};
use flint_messenger::{
    DirectOnionTransport, Envelope, Identity, MessageKind, Messenger, Transport, DEFAULT_TTL_SECS,
};

#[test]
#[ignore = "spawns tor.exe and needs the Tor network"]
fn tor_listener_publishes_a_gateway_reachable_by_direct_onion() {
    let seed = [0x41u8; 32];
    let expected = onion_address(&seed).expect("onion");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!("tor-listener-{}", uuid::Uuid::new_v4()));

    let listener = TorListener::start(&seed, 0, &root).expect("start Tor listener with gateway");
    eprintln!("onion: {}", listener.onion());
    eprintln!("gateway: {}", listener.gateway_url());
    eprintln!("socks: {}", listener.socks_addr());
    assert_eq!(listener.onion(), expected);

    let identity = Identity::from_seed(seed, "tgrin").expect("identity");
    let transport = DirectOnionTransport::new(&listener.gateway_url(), &listener.socks_addr())
        .expect("direct transport");
    let envelope = Envelope::seal_now(
        &identity,
        identity.address(),
        MessageKind::Text {
            text: "through the desktop listener".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal");

    let mut last = String::new();
    let mut delivered = false;
    for attempt in 1..=8 {
        match transport.deliver(&envelope) {
            Ok(()) => {
                eprintln!("delivered on attempt {attempt}");
                delivered = true;
                break;
            }
            Err(error) => {
                last = format!("{error}");
                eprintln!("attempt {attempt} failed: {last}");
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    }
    assert!(delivered, "direct onion delivery failed: {last}");

    let inbox = Messenger::new(transport)
        .receive(&identity, 10)
        .expect("receive");
    assert_eq!(inbox.len(), 1);
    assert!(matches!(
        inbox[0].kind,
        MessageKind::Text { ref text } if text == "through the desktop listener"
    ));

    drop(listener);
    let _ = std::fs::remove_dir_all(&root);
}
