//! Swap orchestration: offer selection, state ownership and persistence.
//!
//! The Grin side of a swap needs a real timelocked kernel; that part is delegated
//! to the wallet layer (see `flint-core`). This service owns the *protocol* side:
//! which offer, which state, when a refund becomes legal, and what the UI shows.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::board::{Board, LocalBoard, LocalStamps, StampAuthority};
use crate::model::{Offer, Swap, SwapEvent, SwapState};

#[derive(Serialize, Deserialize, Default)]
struct Snapshot {
    swaps: Vec<Swap>,
}

pub struct JanusService {
    board: Box<dyn Board>,
    stamps: Box<dyn StampAuthority>,
    swaps: HashMap<String, Swap>,
    /// Where swaps are persisted; `None` keeps everything in memory.
    path: Option<PathBuf>,
}

impl JanusService {
    pub fn demo(path: Option<PathBuf>) -> Self {
        let swaps = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str::<Snapshot>(&raw).ok())
            .map(|s| s.swaps.into_iter().map(|sw| (sw.id.clone(), sw)).collect())
            .unwrap_or_default();

        Self {
            board: Box::new(LocalBoard::demo()),
            stamps: Box::new(LocalStamps::default()),
            swaps,
            path,
        }
    }

    pub fn with_parts(
        board: Box<dyn Board>,
        stamps: Box<dyn StampAuthority>,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            board,
            stamps,
            swaps: HashMap::new(),
            path,
        }
    }

    pub fn board_name(&self) -> &str {
        self.board.name()
    }

    pub fn stamps_authority(&self) -> &str {
        self.stamps.name()
    }

    pub fn stamps_balance(&self) -> u64 {
        self.stamps.balance()
    }

    pub fn redeem_stamps(&self, piconero: u64) -> Result<u64, String> {
        self.stamps.redeem(piconero)
    }

    pub fn offers(&self, pair: &str) -> Result<Vec<Offer>, String> {
        self.board.list(pair)
    }

    pub fn take(&mut self, offer_id: &str, amount: Option<u64>) -> Result<Swap, String> {
        let offer = self.board.take(offer_id)?;
        if let Some(amount) = amount {
            if !offer.within_bounds(amount) {
                return Err(format!(
                    "сумма вне границ оффера: {}..{}",
                    offer.min, offer.max
                ));
            }
        }
        let swap = Swap::new(offer, "taker", amount, now_ms());
        self.swaps.insert(swap.id.clone(), swap.clone());
        self.save()?;
        Ok(swap)
    }

    pub fn apply(&mut self, swap_id: &str, event: SwapEvent) -> Result<Swap, String> {
        let swap = self
            .swaps
            .get_mut(swap_id)
            .ok_or_else(|| format!("своп {swap_id} не найден"))?;
        swap.apply(event, now_ms())?;
        let out = swap.clone();
        self.save()?;
        Ok(out)
    }

    pub fn get(&self, swap_id: &str) -> Option<Swap> {
        self.swaps.get(swap_id).cloned()
    }

    pub fn list(&self) -> Vec<Swap> {
        let mut all: Vec<Swap> = self.swaps.values().cloned().collect();
        all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        all
    }

    /// Demo-only: walk the whole state machine without touching another chain.
    /// Each step is labelled as simulated in the resulting timeline.
    pub fn simulate(&mut self, swap_id: &str, tip: u64) -> Result<Swap, String> {
        let swap = self
            .swaps
            .get(swap_id)
            .ok_or_else(|| format!("своп {swap_id} не найден"))?
            .clone();

        if swap.state == SwapState::Taken {
            let stamps = swap.offer.stamps;
            // Demo: top up stamps with a simulated burn so the flow can be walked
            // without a Monero node.
            if self.stamps.balance() < stamps as u64 {
                self.stamps.redeem(stamps as u64 * 1_000_000_000)?;
            }
            self.stamps.spend(stamps)?;
            self.apply(
                swap_id,
                SwapEvent::VerifyStamps {
                    stamps_used: stamps,
                },
            )?;
        }

        let swap = self.get(swap_id).expect("just checked");
        if swap.state == SwapState::StampsVerified {
            let lock_blocks = swap.offer.lock.clamp(60, 4320);
            self.apply(
                swap_id,
                SwapEvent::LockGrin {
                    slate_id: format!("sim-{}", &swap_id[swap_id.len().saturating_sub(6)..]),
                    tip,
                    lock_blocks,
                },
            )?;
        }

        let swap = self.get(swap_id).expect("just checked");
        if swap.state == SwapState::GrinLocked {
            self.apply(
                swap_id,
                SwapEvent::RevealSecret {
                    counter_leg: format!("sim-xmr-{}", now_ms()),
                },
            )?;
            self.apply(swap_id, SwapEvent::Complete)?;
        }

        self.get(swap_id)
            .ok_or_else(|| format!("своп {swap_id} потерялся"))
    }

    /// Swaps whose refund path is open at `tip`.
    pub fn refundable(&self, tip: u64) -> Vec<Swap> {
        self.swaps
            .values()
            .filter(|s| s.refund_available(tip))
            .cloned()
            .collect()
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let snapshot = Snapshot {
            swaps: self.swaps.values().cloned().collect(),
        };
        let raw = serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?;
        write_atomic(path, raw.as_bytes()).map_err(|e| e.to_string())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_and_simulate_settles() {
        let mut svc = JanusService::demo(None);
        let offers = svc.offers("XMR/GRIN").unwrap();
        assert!(!offers.is_empty());

        let swap = svc.take(&offers[0].id, None).unwrap();
        assert_eq!(swap.state, SwapState::Taken);

        let done = svc.simulate(&swap.id, 1_000).unwrap();
        assert_eq!(done.state, SwapState::Completed);
        assert!(done.steps.len() >= 4);
        assert_eq!(svc.list().len(), 1);
    }

    #[test]
    fn refund_is_blocked_until_timelock() {
        let mut svc = JanusService::demo(None);
        let offer = svc.offers("XMR/GRIN").unwrap().remove(0);
        let swap = svc.take(&offer.id, None).unwrap();

        svc.apply(
            &swap.id,
            SwapEvent::VerifyStamps {
                stamps_used: offer.stamps,
            },
        )
        .unwrap();
        svc.apply(
            &swap.id,
            SwapEvent::LockGrin {
                slate_id: "sl".into(),
                tip: 100,
                lock_blocks: 50,
            },
        )
        .unwrap();

        assert!(svc.refundable(149).is_empty());
        assert_eq!(svc.refundable(150).len(), 1);
    }

    #[test]
    fn amount_outside_bounds_rejected() {
        let mut svc = JanusService::demo(None);
        let offer = svc.offers("XMR/GRIN").unwrap().remove(0);
        let err = svc.take(&offer.id, Some(1)).unwrap_err();
        assert!(err.contains("вне границ"), "{err}");
    }
}
