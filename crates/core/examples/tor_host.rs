// Hosts an onion service with the system Tor binary, using keys derived from
// the address seed, and keeps running.
//
//   cargo run -q -p flint-core --example tor_host -- <seed byte> <state dir>
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

fn main() {
    let mut args = std::env::args().skip(1);
    let seed_byte: u8 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0x23);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".tmp/tor-host".into()));
    let tor = args
        .next()
        .unwrap_or_else(|| "resources/tor/expert/tor/tor.exe".into());

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

    let seed = [seed_byte; 32];
    let hs_dir = dir.join("hs");
    flint_core::tor_hidden_service_keys(&hs_dir, &seed).expect("keys");
    std::fs::create_dir_all(dir.join("data")).expect("data");
    let torrc = dir.join("torrc");
    std::fs::write(
        &torrc,
        format!(
            "SocksPort 0\nDataDirectory {}\nHiddenServiceDir {}\nHiddenServicePort 80 127.0.0.1:{}\nLog info file {}\n",
            dir.join("data").display(),
            hs_dir.display(),
            port,
            dir.join("tor.log").display()
        ),
    )
    .expect("torrc");

    let log = std::fs::File::create(dir.join("tor.out")).expect("log");
    let log_err = log.try_clone().expect("log clone");
    let _tor = Command::new(&tor)
        .arg("-f")
        .arg(&torrc)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("tor");

    let hostname = hs_dir.join("hostname");
    for _ in 0..240 {
        if let Ok(text) = std::fs::read_to_string(&hostname) {
            let onion = text.trim().trim_end_matches(".onion").to_string();
            if !onion.is_empty() {
                println!("ONION={onion} LOCAL={port}");
                println!("READY");
                std::io::stdout().flush().ok();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
