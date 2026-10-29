//! Janus layer: atomic swaps between Monero (or other fungible chains) and Grin.
//!
//! Janus itself is still pre-release (no public repository as of October 2026),
//! so this crate is deliberately written as an *adapter*: the swap state machine,
//! timelock accounting and offer model are ours, while discovery and anti-spam
//! are behind traits ([`Board`], [`StampAuthority`]) that can be swapped for the
//! real Janus services once they ship.

pub mod board;
pub mod model;
pub mod service;

pub use board::{Board, LocalBoard, LocalStamps, StampAuthority};
pub use model::{Offer, Swap, SwapEvent, SwapState, SwapStep};
pub use service::JanusService;

/// Janus is a Monero <-> Grin swap protocol; this build ships a demo board.
pub const PROTOCOL: &str = "janus-draft";
