//! Messenger CLI for a real two-wallet exchange (M2).
//!
//! Usage:
//!   messenger_cli <wallet-root> <password> init
//!   messenger_cli <wallet-root> <password> identity
//!   messenger_cli <wallet-root> <password> send <relay-url> <recipient> <text>
//!   messenger_cli <wallet-root> <password> inbox <relay-url>
//!   messenger_cli gateway
//!
//! The relay is a loopback `gateway` instance (`gateway` command) or an HTTP
//! relay. This is a test/tool binary; the desktop UI comes later.

use flint_core::gateway::{wait_ready, Gateway, GatewayConfig};
use flint_core::{AppPaths, WalletService};
use flint_messenger::{GatewayTransport, Identity, MessageKind, Messenger, DEFAULT_TTL_SECS};
use serde_json::json;

fn open_wallet(root: &str, password: &str) -> Result<WalletService, String> {
    let mut wallet = WalletService::open_at(AppPaths::at(root));
    wallet.open_wallet(password).map_err(|e| e.to_string())?;
    Ok(wallet)
}

fn identity(wallet: &WalletService) -> Result<Identity, String> {
    let seed = wallet.messenger_seed().map_err(|e| e.to_string())?;
    let hrp = if wallet.settings().network == "mainnet" {
        "grin"
    } else {
        "tgrin"
    };
    Identity::from_seed(*seed, hrp).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: messenger_cli <wallet-root> <password> <init|identity|send|inbox> ... | gateway");
        std::process::exit(2);
    }
    if args[1] == "gateway" {
        let gateway = Gateway::start(GatewayConfig::default()).expect("gateway");
        if !wait_ready(&gateway, std::time::Duration::from_secs(5)) {
            eprintln!("gateway did not start");
            std::process::exit(1);
        }
        println!("RELAY={}", gateway.url());
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }

    if args.len() < 4 {
        eprintln!("usage: messenger_cli <wallet-root> <password> <command> ...");
        std::process::exit(2);
    }
    let root = &args[1];
    let password = &args[2];
    let command = &args[3];

    let result = match command.as_str() {
        "init" => {
            let mut wallet = WalletService::open_at(AppPaths::at(root));
            match wallet.create_wallet(password) {
                Ok(created) => Ok(json!({"address": created.address})),
                Err(e) => Err(e.to_string()),
            }
        }
        "identity" => open_wallet(root, password).and_then(|wallet| {
            let id = identity(&wallet)?;
            Ok(json!({"address": id.address().to_string()}))
        }),
        "send" => {
            if args.len() < 7 {
                eprintln!("usage: send <relay-url> <recipient> <text>");
                std::process::exit(2);
            }
            let relay = &args[4];
            let recipient = &args[5];
            let text = &args[6];
            open_wallet(root, password).and_then(|wallet| {
                let id = identity(&wallet)?;
                let transport = GatewayTransport::new(relay).map_err(|e| e.to_string())?;
                let messenger = Messenger::new(transport);
                let to = recipient.parse().map_err(|e| format!("{e}"))?;
                let envelope = messenger
                    .send(
                        &id,
                        &to,
                        MessageKind::Text { text: text.clone() },
                        DEFAULT_TTL_SECS,
                    )
                    .map_err(|e| e.to_string())?;
                Ok(json!({"id": envelope.id, "to": recipient}))
            })
        }
        "inbox" => {
            if args.len() < 5 {
                eprintln!("usage: inbox <relay-url>");
                std::process::exit(2);
            }
            let relay = &args[4];
            open_wallet(root, password).and_then(|wallet| {
                let id = identity(&wallet)?;
                let transport = GatewayTransport::new(relay).map_err(|e| e.to_string())?;
                let messenger = Messenger::new(transport);
                let inbox = messenger.receive(&id, 20).map_err(|e| e.to_string())?;
                let items: Vec<_> = inbox
                    .into_iter()
                    .map(|item| {
                        let body = match item.kind {
                            MessageKind::Text { text } => json!({"type": "text", "text": text}),
                            MessageKind::Slatepack { armor, encrypted } => {
                                json!({"type": "slatepack", "armor": armor, "encrypted": encrypted})
                            }
                        };
                        json!({"id": item.id, "from": item.sender.to_string(), "body": body})
                    })
                    .collect();
                Ok(json!({"messages": items}))
            })
        }
        other => Err(format!("unknown command: {other}")),
    };

    match result {
        Ok(value) => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
