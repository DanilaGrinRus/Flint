//! Live check: the address-derived key files are accepted by real Tor and
//! produce exactly the onion that the slatepack address implies.
//!
//! Ignored by default: spawns `tor.exe` (Tor expert bundle) and needs network.
//!
//!   cargo test -p flint-core --test tor_hidden_service -- --ignored --nocapture

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flint_core::{onion_address, tor_hidden_service_keys};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tor_exe() -> PathBuf {
    if let Ok(path) = std::env::var("FLINT_TOR_BIN") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/tor/expert/tor/tor.exe")
}

#[test]
#[ignore = "spawns tor.exe and needs network"]
fn tor_publishes_the_address_derived_onion() {
    let tor = tor_exe();
    assert!(
        tor.is_file(),
        "нет {} — скачайте tor expert bundle в resources/tor",
        tor.display()
    );

    let seed = [0x23u8; 32];
    let expected = onion_address(&seed).expect("derive onion");
    eprintln!("expected onion: {expected}");

    let dir = std::env::temp_dir().join(format!("flint-tor-{}", uuid::Uuid::new_v4()));
    let hs_dir = dir.join("hs");
    tor_hidden_service_keys(&hs_dir, &seed).expect("write keys");

    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
        }
    });

    std::fs::create_dir_all(dir.join("data")).expect("data dir");
    let torrc = dir.join("torrc");
    std::fs::write(
        &torrc,
        format!(
            "SocksPort 0\nDataDirectory {}\nHiddenServiceDir {}\nHiddenServicePort 80 127.0.0.1:{}\nLog notice file {}\n",
            dir.join("data").display(),
            hs_dir.display(),
            port,
            dir.join("tor.log").display()
        ),
    )
    .expect("torrc");

    let log = std::fs::File::create(dir.join("tor.out")).expect("log");
    let log_err = log.try_clone().expect("log clone");
    let child = Command::new(&tor)
        .arg("-f")
        .arg(&torrc)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("tor");
    let _guard = ChildGuard(child);

    let hostname_path = hs_dir.join("hostname");
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut published = None;
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&hostname_path) {
            if !text.trim().is_empty() {
                published = Some(text.trim().to_string());
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let published = published.unwrap_or_else(|| {
        let log_text = std::fs::read_to_string(dir.join("tor.log")).unwrap_or_default();
        panic!("tor не опубликовал сервис; лог:\n{}", log_text)
    });
    eprintln!("tor hostname: {published}");
    let published = published.trim_end_matches(".onion").to_string();
    assert_eq!(published, expected, "tor must use the address-derived key");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "spawns tor.exe and needs network"]
fn tor_listener_starts_from_a_unicode_data_dir() {
    let seed = [0x25u8; 32];
    let dir = std::env::temp_dir().join(format!(
        "flint-tor-listener проверка {}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("data root");
    let wallet = TcpListener::bind(("127.0.0.1", 0)).expect("bind wallet port");
    let port = wallet.local_addr().expect("addr").port();

    let listener = flint_core::TorListener::start(&seed, port, &dir)
        .expect("Tor must start from a Unicode data root via the short path");
    assert!(!listener.onion().is_empty(), "empty onion");
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);
}
