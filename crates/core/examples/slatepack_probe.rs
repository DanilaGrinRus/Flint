//! Diagnostic: is a slatepack encrypted (age) or sent in clear text?
//!
//! Tries to parse the armor without any decryption key. Success means the
//! payload is readable by anyone; failure is expected for an encrypted answer.

use grin_wallet_libwallet::slatepack::{Slatepacker, SlatepackerArgs};

fn main() {
    grin_core::global::set_local_chain_type(grin_core::global::ChainTypes::Testnet);
    let path = std::env::args()
        .nth(1)
        .expect("usage: slatepack_probe <file>");
    let armor = std::fs::read_to_string(&path).expect("read armor");
    let packer = Slatepacker::new(SlatepackerArgs {
        sender: None,
        recipients: vec![],
        dec_key: None,
    });
    match packer.deser_slatepack(armor.trim().as_bytes(), true) {
        Ok(slatepack) => println!("MODE={} (0=plaintext, 1=age)", slatepack.mode),
        Err(e) => println!("UNREADABLE ({e})"),
    }
}
