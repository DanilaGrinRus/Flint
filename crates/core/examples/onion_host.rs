// Hosts an onion service for probing: starts a tiny HTTP responder, publishes
// it under the onion derived from the given seed byte, prints the onion and
// keeps running.
//
//   cargo run -q -p flint-core --example onion_host -- <seed byte> <state dir>
use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed_byte: u8 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0x23);
    let dir = args.next().unwrap_or_else(|| ".tmp/onion-host".into());

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
    let service =
        flint_core::OnionService::start(&seed, &format!("127.0.0.1:{port}"), dir.as_ref())
            .expect("start onion");
    println!("ONION={} LOCAL={}", service.onion(), port);
    println!("READY");
    std::io::stdout().flush().ok();

    // Keep the service alive until the process is killed.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
