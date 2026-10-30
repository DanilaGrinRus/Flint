//! Wire types shared with the desktop shell. Amounts are nanogrins encoded as
//! strings: they exceed `Number.MAX_SAFE_INTEGER` long before the UI would care.

use serde::{Deserialize, Serialize};

use crate::node::NodePhase;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeInfo {
    pub connected: bool,
    /// True only for `NodePhase::Ready` — never inferred from RPC alone.
    pub synced: bool,
    /// `not_running` | `starting` | `headers` | `blocks` | `ready` | `unknown`
    pub phase: NodePhase,
    pub height: u64,
    /// Header/block height reached, as reported by the node's own sync info.
    pub sync_current: Option<u64>,
    /// Height the node has heard about from peers (the sync target).
    pub sync_target: Option<u64>,
    pub version: Option<String>,
    pub peers: Option<u32>,
    /// Why the node's native status could not be read (auth, timeout, bad
    /// response). Kept separate from `phase` so an unknown phase can explain
    /// itself without pretending the node is merely "starting".
    pub status_error: Option<String>,
}

/// Lifecycle of a wallet-side chain scan.
///
/// The wallet API reports no progress and no "scanning" flag, so the state is
/// owned by Flint and persisted per profile. It is never inferred from log
/// lines: a missing log line is not evidence that the chain was scanned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanState {
    /// No scan has run for this profile (a freshly created wallet needs none).
    #[default]
    NotStarted,
    /// A scan is running in this process, or the wallet was busy scanning.
    Running,
    /// The last scan finished and the balance can be trusted.
    Completed,
    /// The last scan failed or was interrupted; the balance stays unknown.
    Failed,
}

impl ScanState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "скан не запускался",
            Self::Running => "скан идёт",
            Self::Completed => "скан завершён",
            Self::Failed => "скан не завершён",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletStatus {
    pub initialized: bool,
    pub unlocked: bool,
    pub network: String,
    pub mode: String,
    pub address: Option<String>,
    pub node: NodeInfo,
    pub data_dir: String,
    /// Explicit scan lifecycle; the UI polls while it is `running`.
    #[serde(default)]
    pub scan_state: ScanState,
    /// Why the last scan failed, for the banner. Never contains secrets.
    #[serde(default)]
    pub scan_error: Option<String>,
    /// True only while this process owns an RPC scan that can be aborted by
    /// killing the separate `grin-wallet` process. The embedded backend cannot
    /// stop `Owner::scan`, so it always reports `false`.
    #[serde(default)]
    pub scan_cancellable: bool,
    /// Set by the shell when the same wallet address is live in another copy.
    #[serde(default)]
    pub clone_warning: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Balance {
    /// Spendable now (>= min confirmations).
    pub spendable: String,
    /// In the mempool / not enough confirmations.
    pub awaiting: String,
    /// Total, including immature coinbase and locked outputs.
    pub total: String,
    /// Coinbase outputs still inside their lock height.
    pub immature: String,
    /// Locked by an in-flight transaction.
    pub locked: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletSummary {
    /// `false` when the node check behind the balance failed and the numbers may
    /// be cached. The UI must not present such a balance as verified.
    #[serde(default = "default_true")]
    pub validated: bool,
    pub address: String,
    pub account: String,
    pub balance: Balance,
    pub last_confirmed_height: u64,
    pub minimum_confirmations: u64,
    /// Restored wallet, chain not scanned yet: balance is unknown, not zero.
    pub needs_scan: bool,
    /// Explicit scan lifecycle, so the badge can say "иду" without guessing.
    #[serde(default)]
    pub scan_state: ScanState,
    #[serde(default)]
    pub scan_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxItem {
    /// Stable-ish identifier for the UI (slate uuid when known).
    pub id: String,
    /// `in` | `out`.
    pub direction: String,
    pub amount: String,
    pub fee: String,
    /// `confirmed` | `pending` | `canceled`.
    pub status: String,
    pub created_at: i64,
    pub confirmed_at: Option<i64>,
    pub counterparty: Option<String>,
    pub note: Option<String>,
    pub tx_type: String,
}

/// Confirmation shown when a send is prepared. The UI passes the serialized
/// token back at finalize; any mismatch (context, amount, fee, destination or
/// S1 digest) refuses a new signature. Legacy journals without it are
/// fail-closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendConfirmation {
    pub network: String,
    pub address_index: u32,
    pub destination: String,
    pub amount_nano: u64,
    pub fee_nano: u64,
    /// SHA-256 of the canonical JSON of the prepared S1 (inputs included).
    pub slate_digest: String,
}

impl SendConfirmation {
    pub fn token(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_token(token: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(token)
    }
}

/// Canonical JSON (sorted object keys) so a digest is stable across serializers.
pub fn canonical_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                out.insert(key.clone(), canonical_json(&map[key]));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_json).collect())
        }
        other => other.clone(),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Digest of a JSON value (used for the prepared S1).
pub fn digest_json(value: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let canonical = canonical_json(value);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hex(&hasher.finalize())
}

/// Digest of any serializable S1 payload (embedded `Slate` or RPC JSON value).
pub fn digest_slate<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(value)?;
    Ok(digest_json(&value))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendSlate {
    pub slate_id: String,
    pub slatepack: String,
    pub amount_nano: String,
    pub fee_nano: String,
    pub destination: String,
    /// `false` only when the user explicitly asked for `manual`: the slatepack is
    /// then unencrypted and the UI must say so.
    pub encrypted: bool,
    /// Present when the node was still in Headers/Blocks and the user accepted
    /// the explicit "may be rejected" warning.
    #[serde(default)]
    pub warning: Option<String>,
    /// Serialized [`SendConfirmation`] token the UI must send back to finalize.
    pub confirmation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiveSlate {
    pub slate_id: String,
    pub slatepack: String,
    pub amount_nano: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoredWallet {
    pub address: String,
    pub needs_scan: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedWallet {
    pub mnemonic: String,
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsView {
    pub network: String,
    pub node_url: String,
    /// Optional override for the node binary; `None` means "use the bundled one".
    pub node_bin: Option<String>,
    /// Whether a `grin` binary was found on disk at all.
    pub node_binary_found: bool,
    pub auto_start_node: bool,
    pub relay_url: String,
    /// Nostr relay for the optional Nostr transport (empty = disabled).
    pub nostr_relay_url: String,
    pub min_confirmations: u64,
    pub data_dir: String,
    pub app_version: String,
    pub exe_path: String,
    /// Set when settings.json was corrupt and defaults/backup were used.
    pub settings_warning: String,
}

fn default_true() -> bool {
    true
}
