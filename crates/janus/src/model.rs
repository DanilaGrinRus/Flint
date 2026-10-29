//! Offer and swap types plus the swap state machine.
//!
//! The protocol shape follows what Janus' author described publicly: a maker
//! posts an offer, takers prove anti-spam work (burn stamps / Equi-X / private
//! offers), the Grin leg is locked behind an *absolute* block-height timelock,
//! and a secret reveal settles both legs. Refunds are reachable only after the
//! timelock expires, which is why every state carries its height.

use serde::{Deserialize, Serialize};

/// An advertised swap. `rate` is "units of `get` per 1 unit of `give`".
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub id: String,
    pub give: String,
    pub get: String,
    pub rate: String,
    /// Minimum amount of `give`, in its smallest unit (piconero for XMR).
    pub min: String,
    /// Maximum amount of `give`.
    pub max: String,
    pub maker: String,
    /// Anti-spam: stamp tokens the taker must present.
    pub stamps: u32,
    /// Anti-spam: Monero burn required per take, in piconero.
    pub burn: String,
    /// Grin timelock for the maker's leg, in blocks.
    pub lock: u64,
    pub online: bool,
    pub takes: u32,
    pub created_at: i64,
}

impl Offer {
    pub fn within_bounds(&self, amount: u64) -> bool {
        let min = self.min.parse::<u64>().unwrap_or(0);
        let max = self.max.parse::<u64>().unwrap_or(u64::MAX);
        amount >= min && amount <= max
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapState {
    /// Offer selected locally, nothing has been committed.
    Taken,
    /// Anti-spam proofs accepted by the board.
    StampsVerified,
    /// Grin leg is in a timelocked kernel; money is on chain but not claimable.
    GrinLocked,
    /// Secret revealed, the other leg is claimable.
    SecretRevealed,
    Completed,
    /// Timelock expired without settlement.
    Refunded,
    Aborted,
}

impl SwapState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SwapState::Completed | SwapState::Refunded | SwapState::Aborted
        )
    }

    /// Human label used by the UI.
    pub fn label(self) -> &'static str {
        match self {
            SwapState::Taken => "оффер принят",
            SwapState::StampsVerified => "stamps проверены",
            SwapState::GrinLocked => "GRIN заблокированы",
            SwapState::SecretRevealed => "секрет раскрыт",
            SwapState::Completed => "завершён",
            SwapState::Refunded => "возвращён",
            SwapState::Aborted => "отменён",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwapStep {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub at: i64,
    pub done: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Swap {
    pub id: String,
    pub offer: Offer,
    /// `maker` or `taker` from this wallet's point of view.
    pub role: String,
    pub state: SwapState,
    pub amount: Option<u64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub steps: Vec<SwapStep>,
    /// Absolute Grin height after which the maker can reclaim the locked leg.
    pub lock_height: Option<u64>,
    /// Height at which the Grin leg was locked (tip when locking).
    pub locked_at_tip: Option<u64>,
    /// Slate id of the Grin leg, when known.
    pub slate_id: Option<String>,
    /// Monero leg reference (txid / address), when known.
    pub counter_leg: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub enum SwapEvent {
    /// Anti-spam proofs accepted.
    VerifyStamps {
        stamps_used: u32,
    },
    /// Grin leg locked at `tip` for `lock_blocks` blocks.
    LockGrin {
        slate_id: String,
        tip: u64,
        lock_blocks: u64,
    },
    /// Settlement secret revealed, counter leg claimable.
    RevealSecret {
        counter_leg: String,
    },
    Complete,
    /// Reclaim the Grin leg after the timelock expired.
    Refund {
        tip: u64,
    },
    Abort {
        reason: String,
    },
}

impl Swap {
    pub fn new(offer: Offer, role: &str, amount: Option<u64>, now: i64) -> Self {
        let mut swap = Self {
            id: format!("swap_{}", short_id(now)),
            offer,
            role: role.to_string(),
            state: SwapState::Taken,
            amount,
            created_at: now,
            updated_at: now,
            steps: Vec::new(),
            lock_height: None,
            locked_at_tip: None,
            slate_id: None,
            counter_leg: None,
            note: None,
        };
        swap.push_step(
            "taken",
            "Оффер принят",
            &format!("мейкер {}, {} stamp", swap.offer.maker, swap.offer.stamps),
            now,
        );
        swap
    }

    fn push_step(&mut self, key: &str, title: &str, detail: &str, now: i64) {
        self.steps.push(SwapStep {
            key: key.to_string(),
            title: title.to_string(),
            detail: detail.to_string(),
            at: now,
            done: true,
        });
        self.updated_at = now;
    }

    /// Apply a protocol event. Rejects out-of-order events instead of silently
    /// corrupting the swap.
    pub fn apply(&mut self, event: SwapEvent, now: i64) -> Result<(), String> {
        match (self.state, event) {
            (SwapState::Taken, SwapEvent::VerifyStamps { stamps_used }) => {
                if stamps_used < self.offer.stamps {
                    return Err(format!(
                        "нужно {} stamp-токенов, предъявлено {}",
                        self.offer.stamps, stamps_used
                    ));
                }
                self.state = SwapState::StampsVerified;
                self.push_step(
                    "stamps",
                    "Stamps проверены",
                    &format!(
                        "предъявлено {stamps_used}, сожжено {} piconero",
                        self.offer.burn
                    ),
                    now,
                );
                Ok(())
            }
            (
                SwapState::Taken | SwapState::StampsVerified,
                SwapEvent::LockGrin {
                    slate_id,
                    tip,
                    lock_blocks,
                },
            ) => {
                let lock_height = tip.saturating_add(lock_blocks);
                self.state = SwapState::GrinLocked;
                self.lock_height = Some(lock_height);
                self.locked_at_tip = Some(tip);
                self.slate_id = Some(slate_id);
                self.push_step(
                    "lock",
                    "GRIN заблокированы",
                    &format!("kernel с таймлоком до высоты {lock_height}"),
                    now,
                );
                Ok(())
            }
            (SwapState::GrinLocked, SwapEvent::RevealSecret { counter_leg }) => {
                self.state = SwapState::SecretRevealed;
                self.counter_leg = Some(counter_leg.clone());
                self.push_step(
                    "reveal",
                    "Секрет раскрыт",
                    &format!("контр-нога {counter_leg}"),
                    now,
                );
                Ok(())
            }
            (SwapState::SecretRevealed, SwapEvent::Complete) => {
                self.state = SwapState::Completed;
                self.push_step("complete", "Своп завершён", "обе ноги исполнены", now);
                Ok(())
            }
            (SwapState::GrinLocked, SwapEvent::Refund { tip }) => {
                let lock_height = self.lock_height.unwrap_or(u64::MAX);
                if tip < lock_height {
                    return Err(format!(
                        "таймлок ещё не истёк: высота {tip} < {lock_height} (осталось {})",
                        lock_height - tip
                    ));
                }
                self.state = SwapState::Refunded;
                self.push_step(
                    "refund",
                    "Возврат по таймлоку",
                    &format!("высота {tip}"),
                    now,
                );
                Ok(())
            }
            (SwapState::Taken | SwapState::StampsVerified, SwapEvent::Abort { reason }) => {
                self.state = SwapState::Aborted;
                self.push_step("abort", "Своп отменён", &reason, now);
                Ok(())
            }
            (state, event) => Err(format!("недопустимый переход: {:?} -> {:?}", state, event)),
        }
    }

    /// Blocks left before the refund path opens (`None` when not locked yet).
    pub fn blocks_to_refund(&self, tip: u64) -> Option<u64> {
        self.lock_height.map(|h| h.saturating_sub(tip))
    }

    /// Whether the refund path is currently legal.
    pub fn refund_available(&self, tip: u64) -> bool {
        matches!(self.state, SwapState::GrinLocked)
            && self.lock_height.map(|h| tip >= h).unwrap_or(false)
    }
}

fn short_id(now: i64) -> String {
    // Deterministic-ish, readable, no dependency on rng here.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", now as u64 & 0xffff_ffff, nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(lock: u64, stamps: u32) -> Offer {
        Offer {
            id: "of_test".into(),
            give: "XMR".into(),
            get: "GRIN".into(),
            rate: "118.4".into(),
            min: "1000".into(),
            max: "100000".into(),
            maker: "maker".into(),
            stamps,
            burn: "0".into(),
            lock,
            online: true,
            takes: 0,
            created_at: 0,
        }
    }

    #[test]
    fn happy_path_settles() {
        let mut s = Swap::new(offer(720, 2), "taker", Some(10_000), 0);
        s.apply(SwapEvent::VerifyStamps { stamps_used: 2 }, 1)
            .unwrap();
        s.apply(
            SwapEvent::LockGrin {
                slate_id: "sl".into(),
                tip: 1000,
                lock_blocks: 720,
            },
            2,
        )
        .unwrap();
        assert_eq!(s.lock_height, Some(1720));
        assert!(!s.refund_available(1719));
        s.apply(
            SwapEvent::RevealSecret {
                counter_leg: "xmr_tx".into(),
            },
            3,
        )
        .unwrap();
        s.apply(SwapEvent::Complete, 4).unwrap();
        assert_eq!(s.state, SwapState::Completed);
        assert!(s.state.is_terminal());
    }

    #[test]
    fn refund_only_after_timelock() {
        let mut s = Swap::new(offer(10, 0), "taker", None, 0);
        s.apply(
            SwapEvent::LockGrin {
                slate_id: "sl".into(),
                tip: 50,
                lock_blocks: 10,
            },
            1,
        )
        .unwrap();
        let err = s.apply(SwapEvent::Refund { tip: 59 }, 2).unwrap_err();
        assert!(err.contains("осталось 1"), "{err}");
        s.apply(SwapEvent::Refund { tip: 60 }, 3).unwrap();
        assert_eq!(s.state, SwapState::Refunded);
    }

    #[test]
    fn cannot_abort_after_lock() {
        let mut s = Swap::new(offer(10, 0), "taker", None, 0);
        s.apply(
            SwapEvent::LockGrin {
                slate_id: "sl".into(),
                tip: 50,
                lock_blocks: 10,
            },
            1,
        )
        .unwrap();
        assert!(s.apply(SwapEvent::Abort { reason: "x".into() }, 2).is_err());
    }

    #[test]
    fn insufficient_stamps_rejected() {
        let mut s = Swap::new(offer(10, 5), "taker", None, 0);
        let err = s
            .apply(SwapEvent::VerifyStamps { stamps_used: 3 }, 1)
            .unwrap_err();
        assert!(err.contains("нужно 5"), "{err}");
        assert_eq!(s.state, SwapState::Taken);
    }
}
