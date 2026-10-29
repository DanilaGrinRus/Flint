//! Discovery (boards) and anti-spam (burn stamps) behind traits.
//!
//! Janus uses centralised boards only for discovery: a board cannot move funds.
//! The same is true here, so the local implementation is an honest demo rather
//! than a fake "trusted server".

use std::sync::Mutex;

use crate::model::Offer;

pub trait Board: Send + Sync {
    fn name(&self) -> &str;
    fn list(&self, pair: &str) -> Result<Vec<Offer>, String>;
    fn take(&self, id: &str) -> Result<Offer, String>;
    fn publish(&self, offer: Offer) -> Result<Offer, String>;
}

/// Blinded burn-stamp tokens. Unblinding makes them fungible, the authority only
/// stores nullifiers so it cannot link a stamp back to the on-chain burn.
pub trait StampAuthority: Send + Sync {
    fn name(&self) -> &str;
    fn balance(&self) -> u64;
    /// Burn `piconero` of Monero and receive stamps (1 stamp per 0.001 XMR here).
    fn redeem(&self, piconero: u64) -> Result<u64, String>;
    fn spend(&self, stamps: u32) -> Result<u64, String>;
}

/* ------------------------------------------------------------- local demo --- */

pub struct LocalBoard {
    offers: Mutex<Vec<Offer>>,
}

impl LocalBoard {
    pub fn demo() -> Self {
        Self {
            offers: Mutex::new(demo_offers()),
        }
    }

    pub fn empty() -> Self {
        Self {
            offers: Mutex::new(Vec::new()),
        }
    }
}

impl Board for LocalBoard {
    fn name(&self) -> &str {
        "local-demo-board"
    }

    fn list(&self, pair: &str) -> Result<Vec<Offer>, String> {
        let want = pair.trim().to_uppercase();
        let offers = self.offers.lock().map_err(|_| "борд заблокирован")?;
        let mut out: Vec<Offer> = if want.is_empty() {
            offers.clone()
        } else {
            offers
                .iter()
                .filter(|o| format!("{}/{}", o.give, o.get).eq_ignore_ascii_case(&want))
                .cloned()
                .collect()
        };
        out.sort_by(|a, b| {
            b.rate
                .parse::<f64>()
                .unwrap_or(0.0)
                .partial_cmp(&a.rate.parse::<f64>().unwrap_or(0.0))
                .unwrap()
        });
        Ok(out)
    }

    fn take(&self, id: &str) -> Result<Offer, String> {
        let mut offers = self.offers.lock().map_err(|_| "борд заблокирован")?;
        let idx = offers
            .iter()
            .position(|o| o.id == id)
            .ok_or_else(|| "оффер уже недоступен".to_string())?;
        let offer = offers[idx].clone();
        if !offer.online {
            return Err("мейкер офлайн".into());
        }
        offers[idx].takes = offers[idx].takes.saturating_add(1);
        Ok(offer)
    }

    fn publish(&self, offer: Offer) -> Result<Offer, String> {
        let mut offers = self.offers.lock().map_err(|_| "борд заблокирован")?;
        if offers.iter().any(|o| o.id == offer.id) {
            return Err("оффер с таким id уже есть".into());
        }
        offers.push(offer.clone());
        Ok(offer)
    }
}

pub struct LocalStamps {
    balance: Mutex<u64>,
}

impl LocalStamps {
    pub fn new(balance: u64) -> Self {
        Self {
            balance: Mutex::new(balance),
        }
    }
}

impl Default for LocalStamps {
    fn default() -> Self {
        Self::new(0)
    }
}

impl StampAuthority for LocalStamps {
    fn name(&self) -> &str {
        "local-demo-stamps"
    }

    fn balance(&self) -> u64 {
        self.balance.lock().map(|b| *b).unwrap_or(0)
    }

    fn redeem(&self, piconero: u64) -> Result<u64, String> {
        // 1 stamp per 0.001 XMR (1e9 piconero).
        let stamps = piconero / 1_000_000_000;
        if stamps == 0 {
            return Err("слишком маленький burn: минимум 0.001 XMR".into());
        }
        let mut b = self.balance.lock().map_err(|_| "stamps заблокированы")?;
        *b = b.saturating_add(stamps);
        Ok(*b)
    }

    fn spend(&self, stamps: u32) -> Result<u64, String> {
        let mut b = self.balance.lock().map_err(|_| "stamps заблокированы")?;
        if *b < stamps as u64 {
            return Err(format!("недостаточно stamps: {} из {stamps}", *b));
        }
        *b -= stamps as u64;
        Ok(*b)
    }
}

fn demo_offers() -> Vec<Offer> {
    let now = chrono::Utc::now().timestamp_millis();
    vec![
        Offer {
            id: "of_1".into(),
            give: "XMR".into(),
            get: "GRIN".into(),
            rate: "118.42".into(),
            min: "5_000_000_000".replace('_', ""),
            max: "40_000_000_000".replace('_', ""),
            maker: "eravsar…4f2".into(),
            stamps: 12,
            burn: "8_000_000_000".into(),
            lock: 1440,
            online: true,
            takes: 3,
            created_at: now - 1000 * 60 * 14,
        },
        Offer {
            id: "of_2".into(),
            give: "XMR".into(),
            get: "GRIN".into(),
            rate: "117.90".into(),
            min: "2_000_000_000".replace('_', ""),
            max: "15_000_000_000".replace('_', ""),
            maker: "monero-fox".into(),
            stamps: 3,
            burn: "2_000_000_000".into(),
            lock: 720,
            online: false,
            takes: 0,
            created_at: now - 1000 * 60 * 40,
        },
        Offer {
            id: "of_3".into(),
            give: "LTC".into(),
            get: "GRIN".into(),
            rate: "0.00214".into(),
            min: "1_000_000_000".replace('_', ""),
            max: "8_000_000_000".replace('_', ""),
            maker: "mweb-alice".into(),
            stamps: 40,
            burn: "0".into(),
            lock: 2880,
            online: true,
            takes: 11,
            created_at: now - 1000 * 60 * 90,
        },
        Offer {
            id: "of_4".into(),
            give: "XMR".into(),
            get: "GRIN".into(),
            rate: "119.05".into(),
            min: "10_000_000_000".replace('_', ""),
            max: "90_000_000_000".replace('_', ""),
            maker: "grin-whale".into(),
            stamps: 120,
            burn: "40_000_000_000".into(),
            lock: 4320,
            online: true,
            takes: 1,
            created_at: now - 1000 * 60 * 120,
        },
    ]
}
