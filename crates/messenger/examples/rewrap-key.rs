// Test utility for the history-recovery live check:
// take the current wrapped history key, unwrap it with the current password,
// wrap the same key under an old password, and write it to the legacy path.
//
//   cargo run -p flint-messenger --example rewrap-key -- <in> <current> <old> <out>
use std::{env, fs, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: rewrap-key <in> <current-password> <old-password> <out>");
        process::exit(2);
    }
    let bytes = fs::read(&args[1]).expect("read wrapped key");
    let key = flint_messenger::unwrap_history_key(&args[2], &bytes).expect("unwrap current key");
    let wrapped = flint_messenger::keywrap::wrap(&args[3], &key).expect("wrap old key");
    fs::write(&args[4], wrapped).expect("write old key");
}
