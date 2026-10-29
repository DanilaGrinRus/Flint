//! Flint wallet core: a thin, auditable service layer over the Grin wallet crates.
//!
//! The heavy lifting (Mimblewimble commitments, bulletproofs, slate construction,
//! chain scanning) is delegated to the official `grin_wallet_*` crates. This crate
//! only owns lifecycle, persistence of in-flight slates, DTO shaping for the UI and
//! the safety rails around spending.

pub mod address;
pub mod config;
pub mod crash;
pub mod error;
pub mod foreign_proxy;
pub mod gateway;
pub mod model;
pub mod node;
pub mod node_proxy;
pub mod onion;
pub mod rpc_backend;
pub mod seed;
pub mod service;
pub mod socks;
pub mod tor_listener;
pub mod wallet_rpc;
pub mod win_acl;

pub use address::{address_from_key, onion_from_address, onion_key_bytes};
pub use config::{portable_root, AppPaths, Settings};
pub use error::{FlintError, Result};
pub use gateway::{Gateway, GatewayConfig, Message};
pub use model::*;
pub use node::{NodeManager, NodeSnapshot};
pub use onion::{onion_address, tor_hidden_service_keys, OnionService};
pub use rpc_backend::{RpcBackend, WalletProcess};
pub use seed::{LegacySeed, OpenedSeed, SeedEnvelope, ENVELOPE_FILE, SEED_FILE};
pub use service::{
    pending_alerts, EmbeddedScanJob, PendingAlert, PendingSeverity, ScanRunOutcome, WalletService,
    SCAN_CANCELLED,
};
pub use socks::http_post_onion;
pub use tor_listener::{locate_tor_binary, TorListener};
pub use wallet_rpc::WalletRpc;

/// Human readable name of the wallet engine actually in use.
pub fn engine() -> &'static str {
    concat!("grin_wallet_libwallet ", env!("CARGO_PKG_VERSION"))
}
