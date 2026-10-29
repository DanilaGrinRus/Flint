// Create or restore a disposable RPC wallet through the same owner-API path as
// the application. Secrets arrive only through the process environment and are
// never printed. Used by testnet crash/recovery harnesses.
use flint_runtime::{BackendKind, WalletRuntime};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    let password = std::env::var("FLINT_WALLET_PASS").expect("FLINT_WALLET_PASS не задан");
    let runtime = WalletRuntime::new_with_backend(BackendKind::GrinWallet);
    let result = match command {
        "init" => runtime.create(&password).map(|created| created.address),
        "restore" => {
            let mnemonic = std::env::var("FLINT_WALLET_MNEMONIC")
                .expect("FLINT_WALLET_MNEMONIC не задан для restore");
            runtime
                .restore(&mnemonic, &password)
                .map(|restored| restored.address)
        }
        _ => {
            eprintln!("usage: rpc_wallet_init init|restore");
            runtime.shutdown();
            std::process::exit(2);
        }
    };
    runtime.shutdown();
    match result {
        Ok(address) => println!("address={address}"),
        Err(error) => {
            eprintln!("RPC {command} failed: {error}");
            std::process::exit(1);
        }
    }
}
