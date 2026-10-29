//! Live two-wallet messenger exchange over real Tor hidden services.
//!
//! One Tor process publishes two address-derived onions (Alice and Bob), each
//! pointing at its own local gateway. Alice sends directly to Bob's onion
//! through the same Tor SOCKS proxy; no relay is involved.
//!
//! Ignored by default: spawns `tor.exe` and needs the Tor network.
//!
//!   cargo test -p flint-core --test direct_onion -- --ignored --nocapture

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::gateway::{wait_ready, Gateway, GatewayConfig};
use flint_core::{locate_tor_binary, onion_address, tor_hidden_service_keys};
use flint_messenger::{
    DirectOnionTransport, Envelope, Identity, MessageKind, Messenger, Transport, DEFAULT_TTL_SECS,
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("free port");
    listener.local_addr().expect("addr").port()
}

fn retry_delivery(transport: &DirectOnionTransport, envelope: &Envelope, label: &str) {
    let mut last = String::new();
    for attempt in 1..=8 {
        match transport.deliver(envelope) {
            Ok(()) => {
                eprintln!("{label}: delivered on attempt {attempt}");
                return;
            }
            Err(error) => {
                last = format!("{error}");
                eprintln!("{label}: attempt {attempt} failed: {last}");
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    }
    panic!("{label}: delivery failed after retries: {last}");
}

#[test]
#[ignore = "spawns tor.exe and needs the Tor network"]
fn two_wallets_exchange_directly_over_onion() {
    let tor = locate_tor_binary().expect("tor.exe в resources/tor");
    let seed_a = [0x31u8; 32];
    let seed_b = [0x32u8; 32];
    let onion_a = onion_address(&seed_a).expect("Alice onion");
    let onion_b = onion_address(&seed_b).expect("Bob onion");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!("direct-onion-{}", uuid::Uuid::new_v4()));

    let gateway_a = Gateway::start(GatewayConfig {
        store_dir: Some(root.join("mailbox-a")),
        ..GatewayConfig::default()
    })
    .expect("Alice gateway");
    let gateway_b = Gateway::start(GatewayConfig {
        store_dir: Some(root.join("mailbox-b")),
        ..GatewayConfig::default()
    })
    .expect("Bob gateway");
    assert!(wait_ready(&gateway_a, Duration::from_secs(5)));
    assert!(wait_ready(&gateway_b, Duration::from_secs(5)));

    let hs_a = root.join("hs-a");
    let hs_b = root.join("hs-b");
    tor_hidden_service_keys(&hs_a, &seed_a).expect("Alice keys");
    tor_hidden_service_keys(&hs_b, &seed_b).expect("Bob keys");
    std::fs::create_dir_all(root.join("data")).expect("data dir");
    let socks_port = free_port();
    let torrc = root.join("torrc");
    std::fs::write(
        &torrc,
        format!(
            "SocksPort 127.0.0.1:{socks_port}\nDataDirectory {}\n\
             HiddenServiceDir {}\nHiddenServicePort 80 127.0.0.1:{}\n\
             HiddenServiceDir {}\nHiddenServicePort 80 127.0.0.1:{}\n\
             Log info file {}\n",
            root.join("data").display(),
            hs_a.display(),
            gateway_a.addr().port(),
            hs_b.display(),
            gateway_b.addr().port(),
            root.join("tor.log").display()
        ),
    )
    .expect("torrc");

    let log = std::fs::File::create(root.join("tor.out")).expect("tor log");
    let log_err = log.try_clone().expect("log clone");
    let mut command = Command::new(&tor);
    command
        .arg("-f")
        .arg(&torrc)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command.spawn().expect("start tor");
    let guard = ChildGuard(child);

    // Both hostnames must be the ones derived from the slatepack keys.
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let a = std::fs::read_to_string(hs_a.join("hostname")).unwrap_or_default();
        let b = std::fs::read_to_string(hs_b.join("hostname")).unwrap_or_default();
        if !a.trim().is_empty() && !b.trim().is_empty() {
            assert_eq!(a.trim().trim_end_matches(".onion"), onion_a);
            assert_eq!(b.trim().trim_end_matches(".onion"), onion_b);
            eprintln!("Alice onion: {onion_a}");
            eprintln!("Bob onion: {onion_b}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "tor не опубликовал оба сервиса; лог:\n{}",
            std::fs::read_to_string(root.join("tor.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(300));
    }

    let alice = Identity::from_seed(seed_a, "tgrin").expect("Alice identity");
    let bob = Identity::from_seed(seed_b, "tgrin").expect("Bob identity");
    assert_eq!(alice.address().onion(), onion_a);
    let socks = format!("127.0.0.1:{socks_port}");
    let alice_transport =
        DirectOnionTransport::new(&gateway_a.url(), &socks).expect("Alice direct transport");
    let bob_transport =
        DirectOnionTransport::new(&gateway_b.url(), &socks).expect("Bob direct transport");

    let hello = Envelope::seal_now(
        &alice,
        bob.address(),
        MessageKind::Text {
            text: "hello over direct onion".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal hello");
    retry_delivery(&alice_transport, &hello, "Alice -> Bob");

    let bob_messenger = Messenger::new(bob_transport);
    let inbox = bob_messenger.receive(&bob, 10).expect("Bob receives");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].sender, *alice.address());
    assert!(matches!(
        inbox[0].kind,
        MessageKind::Text { ref text } if text == "hello over direct onion"
    ));

    // Bob's own outbound path: seal and deliver through Bob's transport.
    let bob_out =
        DirectOnionTransport::new(&gateway_b.url(), &socks).expect("Bob outbound transport");
    let reply = Envelope::seal_now(
        &bob,
        alice.address(),
        MessageKind::Text {
            text: "reply over direct onion".into(),
        },
        DEFAULT_TTL_SECS,
    )
    .expect("seal reply");
    retry_delivery(&bob_out, &reply, "Bob -> Alice");

    let alice_messenger = Messenger::new(
        DirectOnionTransport::new(&gateway_a.url(), &socks).expect("Alice receive transport"),
    );
    let inbox = alice_messenger.receive(&alice, 10).expect("Alice receives");
    assert_eq!(inbox.len(), 1);
    assert!(matches!(
        inbox[0].kind,
        MessageKind::Text { ref text } if text == "reply over direct onion"
    ));

    drop(guard);
    let _ = std::fs::remove_dir_all(&root);
}
