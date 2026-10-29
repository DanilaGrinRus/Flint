// Prints the Tor v3 onion address derived from a slatepack address.
//
//   cargo run -q -p flint-core --example onion -- tgrin1...
fn main() {
    let address = match std::env::args().nth(1) {
        Some(a) => a,
        None => {
            eprintln!("usage: onion <slatepack address>");
            std::process::exit(2);
        }
    };
    match flint_core::onion_from_address(&address) {
        Ok(onion) => println!("{onion}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
