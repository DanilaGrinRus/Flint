//! Diagnostic: expose the foreign-only proxy for an already running wallet and
//! print its loopback port. Used to verify the Tor path without Tor itself.
//!
//! `cargo run -p flint-core --example proxy_probe -- <wallet_port>`

use std::time::Duration;

use flint_core::foreign_proxy::ForeignProxy;

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .expect("usage: proxy_probe <wallet_port>")
        .parse()
        .expect("wallet_port must be a number");
    let proxy = ForeignProxy::start(port).expect("proxy start");
    println!("PROXY_PORT={}", proxy.port());
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
