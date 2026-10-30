//! Wallet lifecycle and operations on top of the official Grin wallet crates.
//!
//! All calls here are blocking (Grin's wallet API is synchronous); the desktop
//! shell invokes them through `spawn_blocking`.

use std::cmp;
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use grin_core::global::ChainTypes;
use grin_keychain::ExtKeychain;
use grin_util::{Mutex as GrinMutex, ZeroingString};
use grin_wallet_api::{Foreign, Owner};
use grin_wallet_config::WALLET_CONFIG_FILE_NAME;
use grin_wallet_impls::{DefaultLCProvider, DefaultWalletImpl, HTTPNodeClient};
use grin_wallet_libwallet::slatepack::{Slatepacker, SlatepackerArgs};
use grin_wallet_libwallet::{
    InitTxArgs, NodeClient, Slate, SlateVersion, SlatepackAddress, VersionedSlate,
    WalletInitStatus, WalletInst, WalletLCProvider,
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::config::{AppPaths, Settings};
use crate::error::{FlintError, Result};
use crate::model::*;
use crate::node::NodeSnapshot;
use crate::seed::envelope::SeedEnvelope;
use crate::seed::store::{SeedKind, SeedStore};

type Lc = DefaultLCProvider<HTTPNodeClient, ExtKeychain>;
type WalletDyn = dyn WalletInst<'static, Lc, HTTPNodeClient, ExtKeychain>;
type OwnerApi = Owner<Lc, HTTPNodeClient, ExtKeychain>;
type ForeignApi = Foreign<'static, Lc, HTTPNodeClient, ExtKeychain>;

/// Persisted reason for a scan stopped by the user.
pub const SCAN_CANCELLED: &str = "скан отменён пользователем";

/// Result of driving the embedded scan batches to their end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanRunOutcome {
    Completed,
    Cancelled,
}

/// A prepared embedded scan.
///
/// It owns a clone of the same wallet instance the `Owner` uses, so the
/// `WalletService` mutex can be released while batches run. That is what lets
/// the UI poll `wallet_status` and request cancellation during the scan: the
/// long work happens here, not while the service is locked.
pub struct EmbeddedScanJob {
    wallet: Arc<GrinMutex<Box<WalletDyn>>>,
    tip_height: u64,
    tip_hash: String,
    start_height: u64,
}

impl EmbeddedScanJob {
    /// Last height this job will scan, as reported by the node at preparation.
    pub fn tip_height(&self) -> u64 {
        self.tip_height
    }

    /// Continue the scan from where the previous call stopped.
    ///
    /// Mirrors the orchestration in upstream `owner::scan`: refresh known
    /// outputs once, then call the public batch `scan` (10 001 blocks) and save
    /// `last_scanned_block` after each batch. The cancellation flag is checked
    /// only at batch boundaries, so no thread is killed mid-write.
    pub fn run(&self, cancel: &AtomicBool) -> Result<ScanRunOutcome> {
        self.run_with_progress(cancel, |_, _| {})
    }

    /// Same as [`Self::run`], with `(scanned_to_height, target_height)` after
    /// each completed batch. The callback is called on the scan thread.
    pub fn run_with_progress<F>(&self, cancel: &AtomicBool, progress: F) -> Result<ScanRunOutcome>
    where
        F: Fn(u64, u64),
    {
        self.run_with_progress_pausable(cancel, None, progress)
    }

    /// Pausable variant for the embedded batch loop. A pause is applied only
    /// between batches; no wallet write is interrupted. Cancellation wins over
    /// pause and is checked while waiting.
    pub fn run_with_progress_pausable<F>(
        &self,
        cancel: &AtomicBool,
        pause: Option<&AtomicBool>,
        progress: F,
    ) -> Result<ScanRunOutcome>
    where
        F: Fn(u64, u64),
    {
        let cancelled = || cancel.load(Ordering::SeqCst);
        let wait_if_paused = || {
            while pause.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                if cancelled() {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            !cancelled()
        };
        if !wait_if_paused() {
            return Ok(ScanRunOutcome::Cancelled);
        }
        progress(self.start_height, self.tip_height);

        // Upstream's `update_outputs` is a thin wrapper around this call and
        // ignores node failures here; the batch scan reports the real error.
        {
            let mut guard = self.wallet.lock();
            let w = guard
                .lc_provider()
                .map_err(FlintError::wallet)?
                .wallet_inst()
                .map_err(FlintError::wallet)?;
            let parent = w.parent_key_id();
            let _ = grin_wallet_libwallet::refresh_outputs(w, None, &parent, true);
        }
        if !wait_if_paused() {
            return Ok(ScanRunOutcome::Cancelled);
        }

        let status = None::<std::sync::mpsc::Sender<grin_wallet_libwallet::StatusMessage>>;
        let mut total_pmmr_range = None;
        for h in (self.start_height..=self.tip_height).step_by(10_001) {
            if !wait_if_paused() {
                return Ok(ScanRunOutcome::Cancelled);
            }
            let batch_end = cmp::min(self.tip_height, h + 10_000);
            let (mut info, range) = grin_wallet_libwallet::scan(
                self.wallet.clone(),
                None,
                false,
                h,
                batch_end,
                self.start_height,
                self.tip_height,
                total_pmmr_range,
                &status,
            )
            .map_err(FlintError::wallet)?;
            info.hash = if batch_end == self.tip_height {
                self.tip_hash.clone()
            } else {
                String::new()
            };
            total_pmmr_range = Some(range);

            let mut guard = self.wallet.lock();
            let w = guard
                .lc_provider()
                .map_err(FlintError::wallet)?
                .wallet_inst()
                .map_err(FlintError::wallet)?;
            let mut batch = w.batch(None).map_err(FlintError::wallet)?;
            batch
                .save_last_scanned_block(info)
                .map_err(FlintError::wallet)?;
            progress(batch_end, self.tip_height);
            batch.commit().map_err(FlintError::wallet)?;
        }
        Ok(ScanRunOutcome::Completed)
    }
}

/// Seed entropy: 32 bytes -> 24 words.
const SEED_ENTROPY: usize = 32;
/// Upper bound for address derivation: far beyond any realistic rotation, but
/// it keeps decryption bounded.
const MAX_ADDRESS_INDEX: u32 = 63;

struct UnlockedWallet {
    owner: OwnerApi,
    foreign: ForeignApi,
    config_path: PathBuf,
}

/// A remote node may only be reached over TLS. Plain HTTP is allowed only on
/// loopback, where the owner API secret never leaves the host.
fn validate_node_url(raw: &str) -> Result<()> {
    let url = reqwest::Url::parse(raw.trim())
        .map_err(|e| FlintError::Invalid(format!("node_url: {e}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(FlintError::Invalid(
            "node_url: поддерживаются только http:// (loopback) и https://".into(),
        ));
    }
    if url.scheme() == "http" {
        let host = url.host_str().unwrap_or_default();
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host == "::1"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if !loopback {
            return Err(FlintError::Invalid(
                "node_url: удалённый HTTP-нод запрещён — используйте HTTPS или туннель".into(),
            ));
        }
    }
    Ok(())
}

pub struct WalletService {
    paths: AppPaths,
    settings: Settings,
    settings_warning: Option<String>,
    unlocked: Option<UnlockedWallet>,
    /// Per-wallet state, not shared between profiles of one network.
    address_index: u32,
    pending_scan: bool,
    scan_state: ScanState,
    scan_error: Option<String>,
    scan_started_at: Option<i64>,
    scan_finished_at: Option<i64>,
}

impl WalletService {
    pub fn new() -> Self {
        Self::open_at(AppPaths::resolve())
    }

    /// Open the service over an explicit data root (used by tests and portable installs).
    pub fn open_at(paths: AppPaths) -> Self {
        let (settings, settings_warning) = match paths.load_settings_with_warning() {
            Ok((settings, warning)) => (settings, warning),
            Err(error) => {
                tracing::error!("{error}; используются настройки по умолчанию");
                (Settings::default(), Some(error))
            }
        };
        let _ = paths.ensure();
        let state = paths.load_wallet_state(
            &settings.network,
            settings.wallet_name.as_deref(),
            (settings.address_index, settings.pending_scan),
        );
        Self {
            paths,
            settings,
            settings_warning,
            unlocked: None,
            address_index: state.address_index,
            pending_scan: state.pending_scan,
            scan_state: state.scan_state,
            scan_error: state.scan_error,
            scan_started_at: state.scan_started_at,
            scan_finished_at: state.scan_finished_at,
        }
    }

    /* ------------------------------------------------------------ settings --- */

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_view(&self) -> SettingsView {
        SettingsView {
            network: self.settings.network.clone(),
            node_url: self.settings.node_url.clone(),
            node_bin: self.settings.node_bin.clone(),
            node_binary_found: self.settings.node_bin.is_some()
                || crate::node::locate_binary().is_some(),
            auto_start_node: self.settings.auto_start_node,
            relay_url: self.settings.relay_url.clone(),
            nostr_relay_url: self.settings.nostr_relay_url.clone(),
            min_confirmations: self.settings.min_confirmations,
            data_dir: self.paths.root().display().to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            exe_path: std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            settings_warning: self.settings_warning.clone().unwrap_or_default(),
        }
    }

    /// Set or clear the Nostr relay (`wss://`, loopback `ws://` for tests).
    ///
    /// Separate from `update_settings`/`relay_url`: the Flint relay and the Nostr
    /// transport must never silently substitute for each other.
    pub fn update_nostr_relay(&mut self, url: Option<String>) -> Result<SettingsView> {
        if let Some(value) = url {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                flint_messenger::nostr::WsRelay::validate_url(&trimmed)
                    .map_err(|error| FlintError::Invalid(error.to_string()))?;
            }
            self.settings.nostr_relay_url = trimmed;
            self.paths.save_settings(&self.settings)?;
        }
        Ok(self.settings_view())
    }

    /// Update persisted settings. Changing network or node drops the unlocked
    /// wallet because the instance is bound to a node client.
    pub fn update_settings(
        &mut self,
        network: Option<String>,
        node_url: Option<String>,
        node_bin: Option<String>,
        auto_start_node: Option<bool>,
        node_proxy: Option<bool>,
        relay_url: Option<String>,
        min_confirmations: Option<u64>,
    ) -> Result<SettingsView> {
        let before = self.settings.clone();

        if let Some(n) = network {
            self.settings.network = n;
        }
        if let Some(u) = node_url {
            validate_node_url(&u)?;
            self.settings.node_url = u;
        }
        if let Some(proxy) = node_proxy {
            self.settings.node_proxy = proxy;
        }
        if let Some(relay) = relay_url {
            self.settings.relay_url = relay.trim().to_string();
        }
        // Switching network must not leave the old network's port behind: if the
        // URL is still the previous network's default, move it to the new one.
        let old_default = crate::node::default_node_url(&before.network);
        if before.network != self.settings.network && self.settings.node_url == old_default {
            self.settings.node_url = crate::node::default_node_url(&self.settings.network);
        }
        if let Some(bin) = node_bin {
            self.settings.node_bin = if bin.trim().is_empty() {
                None
            } else {
                Some(bin)
            };
        }
        if let Some(auto) = auto_start_node {
            self.settings.auto_start_node = auto;
        }
        if let Some(c) = min_confirmations {
            self.settings.min_confirmations = c.clamp(1, 1000);
        }
        self.settings = self.settings.clone().sanitized();
        self.paths.save_settings(&self.settings)?;

        if before.network != self.settings.network || before.node_url != self.settings.node_url {
            self.unlocked = None;
        }
        Ok(self.settings_view())
    }
    /* ---------------------------------------------------------- lifecycle --- */

    fn chain(&self) -> ChainTypes {
        if self.settings.is_testnet() {
            ChainTypes::Testnet
        } else {
            ChainTypes::Mainnet
        }
    }

    /// Grin keeps the active chain in a thread-local (and a one-time global).
    /// Set both before any call into the wallet/chain crates.
    fn ensure_chain(&self) {
        let chain = self.chain();
        grin_core::global::set_local_chain_type(chain);
        grin_core::global::init_global_chain_type(chain);
    }

    fn wallet_home(&self) -> PathBuf {
        self.paths
            .wallet_home_for(&self.settings.network, self.settings.wallet_name.as_deref())
    }

    /// Stable, non-secret identifier of the seed behind this profile. Used by
    /// the app to keep one profile's messenger history away from a different
    /// seed that happens to reuse the same directory name. Legacy wallets get a
    /// constant id; after migration the envelope address takes over.
    pub fn wallet_identity(&self) -> String {
        let store = self.seed_store();
        if store.kind() == SeedKind::Envelope {
            if let Ok(envelope) = SeedEnvelope::read(&store.envelope_path()) {
                use sha2::{Digest, Sha256};
                let digest = Sha256::digest(envelope.address().as_bytes());
                return digest[..8]
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
            }
        }
        "legacy".to_string()
    }

    fn seed_store(&self) -> SeedStore {
        SeedStore::new(self.wallet_home(), &self.settings.network)
    }

    pub fn initialized(&self) -> bool {
        self.seed_store().is_initialized()
    }

    /// Display-only seed reveal: verifies the password against the envelope and
    /// returns the BIP39 phrase. It does not unlock the wallet and never logs the
    /// phrase; the UI must clear it when the dialog closes.
    pub fn reveal_mnemonic(&self, password: &str) -> Result<String> {
        Ok(self.seed_store().reveal_mnemonic(password)?.to_string())
    }

    pub fn unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    pub fn status(&self) -> WalletStatus {
        self.status_inner(true, true)
    }

    /// Status for the first UI paint: no node RPC, no owner-API address. The
    /// caller fills `node` from a separate background poll so a slow/offline
    /// node cannot hold the window behind a 2+ second network timeout.
    pub fn status_fast(&self) -> WalletStatus {
        self.status_inner(true, false)
    }

    /// Status without the owner-API address call. Used while an embedded scan
    /// holds the wallet lock for a batch: asking for the address would block
    /// the status poll that must show the cancel button.
    pub fn status_without_address(&self) -> WalletStatus {
        self.status_inner(false, true)
    }

    fn status_inner(&self, include_address: bool, include_node: bool) -> WalletStatus {
        self.ensure_chain();
        let address = if include_address {
            self.unlocked
                .as_ref()
                .and_then(|u| u.owner.get_slatepack_address(None, self.address_index).ok())
                .map(|a| a.to_string())
        } else {
            None
        };

        WalletStatus {
            initialized: self.initialized(),
            unlocked: self.unlocked(),
            network: self.settings.network.clone(),
            mode: "live".to_string(),
            address,
            node: if include_node {
                self.node_info()
            } else {
                NodeInfo::default()
            },
            data_dir: self.paths.root().display().to_string(),
            scan_state: self.scan_state,
            scan_error: self.scan_error.clone(),
            // The app overlays this with its in-process scan flag: embedded
            // scans are cancelled cooperatively between batches, RPC scans by
            // ending the child process.
            scan_cancellable: false,
        }
    }

    /// Secret for the node API: an explicit setting wins, otherwise the managed
    /// node's file is used **only when the configured endpoint is loopback**.
    /// A local `.api_secret` is a bearer credential and must never travel to an
    /// external node just because the URL happens to be configured.
    fn node_api_secret(&self) -> Option<String> {
        let explicit = self
            .settings
            .node_api_secret
            .clone()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if explicit.is_some() {
            return explicit;
        }
        if !crate::node::is_loopback_node_url(&self.settings.node_url) {
            return None;
        }
        fs::read_to_string(
            self.paths
                .node_dir(&self.settings.network)
                .join(".api_secret"),
        )
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    }

    fn node_client(&self) -> Result<HTTPNodeClient> {
        self.node_snapshot().client()
    }

    /// One frozen view of endpoint, auth, network and confirmation threshold.
    /// Readiness and wallet operations built from the same snapshot cannot
    /// silently describe different nodes.
    pub fn node_snapshot(&self) -> NodeSnapshot {
        NodeSnapshot::new(
            self.settings.network.clone(),
            self.settings.node_url.clone(),
            self.node_api_secret(),
            self.settings.min_confirmations,
        )
    }

    /// Node height, version and — importantly — what it is doing right now.
    ///
    /// `get_tip` answers from the local chain: a node that is still downloading
    /// headers answers too, with a tiny height. The phase therefore comes from the
    /// node's own `get_status`, and `synced` is true only for `Ready`.
    pub fn node_info(&self) -> NodeInfo {
        self.ensure_chain();
        let snapshot = self.node_snapshot();
        let node_dir = self.paths.node_dir(&snapshot.network);
        Self::node_info_for(&snapshot, &node_dir)
    }

    /// Same work as [`WalletService::node_info`] but without the wallet lock:
    /// the UI background poll must not block unlock/settings while an offline
    /// node burns its timeouts.
    pub fn node_info_for(snapshot: &NodeSnapshot, node_dir: &Path) -> NodeInfo {
        let mut info = NodeInfo::default();
        // A managed node keeps writing its log while starting; an external node
        // has none, and then we simply cannot call it ready.
        let log_fresh = crate::node::log_age_secs(node_dir).is_some_and(|age| age < 120.0);

        let mut client = match snapshot.client() {
            Ok(c) => c,
            Err(_) => {
                info.phase = crate::node::classify_phase(log_fresh, false, None);
                return info;
            }
        };

        match client.get_chain_tip() {
            Ok((height, _hash)) => {
                info.connected = true;
                info.height = height;
            }
            Err(_) => {
                info.phase = crate::node::classify_phase(log_fresh, false, None);
                return info;
            }
        }

        if let Some(v) = client.get_version_info() {
            info.version = Some(v.node_version);
        }

        // The node's own sync state is the only authority for "ready". Without it
        // the phase stays unknown: a quiet log and a non-zero tip is exactly what
        // a stalled node looks like.
        let native_result = crate::node::fetch_native_status_result(
            &snapshot.url,
            Duration::from_millis(1500),
            snapshot.secret.as_deref(),
        );
        if let Err(error) = &native_result {
            info.status_error = Some(error.clone());
        }
        let native = native_result.ok();
        if let Some(status) = &native {
            // A lookalike on the same port must not be taken for the node.
            if !crate::node::identity_matches(&snapshot.network, status) {
                info.phase = crate::node::NodePhase::Unknown;
                info.status_error = Some("нода не подтвердила сеть/версию Grin".into());
                return info;
            }
            if status.connections > 0 {
                info.peers = Some(status.connections);
            }
            if status.current_height > 0 {
                info.sync_current = Some(status.current_height);
            }
            if status.highest_height > 0 {
                info.sync_target = Some(status.highest_height);
            }
        } else {
            // Only the log fallback needs the (expensive) log scan; the native
            // status already carries the peer count.
            info.peers = Some(crate::node::sync_stats(node_dir).peers);
        }

        info.phase = crate::node::classify_phase(true, true, native.as_ref());
        info.synced = info.phase.ready();
        info
    }

    /// Create a new wallet (refuses if one already exists) and leave it unlocked.
    ///
    /// The Argon2id envelope is written **before** upstream creates the wallet.
    /// A crash then leaves a recoverable envelope instead of a seed file whose
    /// password existed only in memory; the upstream database is recreated on
    /// the next unlock (spike S1 in `docs/history/ARGON2ID.md`).
    pub fn create_wallet(&mut self, password: &str) -> Result<CreatedWallet> {
        self.ensure_chain();
        validate_password_strength(password)?;

        if self.initialized() {
            return Err(FlintError::Invalid("кошелёк уже создан".into()));
        }

        let home = self.wallet_home();
        fs::create_dir_all(&home)?;
        let store = SeedStore::new(&home, &self.settings.network);
        let new_seed = store.generate()?;
        let address = crate::address::address_from_entropy(
            new_seed.entropy.as_slice(),
            self.settings.is_testnet(),
            0,
        )?;
        store.write_envelope(password, &address, new_seed.entropy.as_slice(), None)?;

        let mut wallet = DefaultWalletImpl::<HTTPNodeClient>::new(self.node_client()?)
            .map_err(FlintError::wallet)?;
        let config_path = home.join(WALLET_CONFIG_FILE_NAME);
        // Upstream never sees the user password: it opens the transient seed
        // file with a random one, materialized from the envelope.
        let upstream_password = SeedStore::new_upstream_password()?;
        {
            let lc = lc_mut(&mut wallet)?;
            lc.set_top_level_directory(&home.to_string_lossy())
                .map_err(FlintError::wallet)?;
            if !config_path.exists() {
                lc.create_config(&self.chain(), WALLET_CONFIG_FILE_NAME, None, None, None)
                    .map_err(FlintError::wallet)?;
            }
            lc.create_wallet(
                None,
                Some(ZeroingString::from(new_seed.mnemonic.as_str())),
                SEED_ENTROPY,
                ZeroingString::from(upstream_password.as_str()),
                false,
            )
            .map_err(FlintError::wallet)?;
        }

        if let Err(e) = self.unlock_with(wallet, &config_path, upstream_password.as_str()) {
            store.remove_verified_seed();
            return Err(e);
        }
        let actual = self
            .owner()?
            .get_slatepack_address(None, 0)
            .map_err(FlintError::wallet)?
            .to_string();
        if actual != address {
            store.remove_verified_seed();
            return Err(FlintError::Invalid(
                "адрес нового кошелька не совпадает с конвертом".into(),
            ));
        }
        // Upstream needs the seed file only while opening; the envelope holds
        // the seed from now on.
        store.remove_verified_seed();
        self.save_wallet_init_status(WalletInitStatus::InitNoScanning)?;

        Ok(CreatedWallet {
            mnemonic: new_seed.mnemonic.to_string(),
            address,
        })
    }

    /// Replace upstream's "needs a full scan on next update" flag.
    ///
    /// Flint passes an explicit mnemonic to upstream's `create_wallet`, so
    /// upstream marks the fresh wallet `InitNeedsScanning`. New and restored
    /// wallets are scanned by Flint's explicit `EmbeddedScanJob`, not by
    /// `update_wallet_state` inside `wallet_summary`; marking them
    /// `InitNoScanning` stops a silent full scan outside `ScanState` and outside
    /// the cancel button. `InitComplete` is written after a successful explicit
    /// scan so later summaries use the saved `last_scanned_block` normally.
    fn save_wallet_init_status(&self, status: WalletInitStatus) -> Result<()> {
        let owner = self.owner()?;
        let mut guard = owner.wallet_inst.lock();
        let w = guard
            .lc_provider()
            .map_err(FlintError::wallet)?
            .wallet_inst()
            .map_err(FlintError::wallet)?;
        let mut batch = w.batch_no_mask().map_err(FlintError::wallet)?;
        batch.save_init_status(status).map_err(FlintError::wallet)?;
        batch.commit().map_err(FlintError::wallet)
    }

    /// Restore a wallet from a mnemonic into a **completely empty** wallet home.
    ///
    /// `recover_from_mnemonic` happily replaces `wallet.seed`, so the emptiness
    /// check comes before anything is written, and a refused import leaves no
    /// half-created directory behind.
    pub fn restore_wallet(&mut self, mnemonic: &str, password: &str) -> Result<RestoredWallet> {
        self.ensure_chain();
        validate_password_strength(password)?;

        let mnemonic = mnemonic.trim();
        if mnemonic.split_whitespace().count() < 12 {
            return Err(FlintError::Invalid(
                "фраза должна содержать не меньше 12 слов".into(),
            ));
        }

        let home = self.wallet_home();
        if home.exists() {
            let mut entries = fs::read_dir(&home)?;
            if entries.next().is_some() {
                return Err(FlintError::Invalid(
                    "каталог кошелька не пуст: восстановление разрешено только в чистый каталог"
                        .into(),
                ));
            }
        }
        // Validate the phrase before writing anything: a bad phrase must not
        // leave files, and its entropy is what the envelope stores.
        let entropy = Zeroizing::new(
            grin_keychain::mnemonic::to_entropy(mnemonic)
                .map_err(|_| FlintError::Invalid("фраза не является мнемоникой Grin".into()))?,
        );
        let address = crate::address::address_from_entropy(
            entropy.as_slice(),
            self.settings.is_testnet(),
            0,
        )?;

        fs::create_dir_all(home.join(grin_wallet_config::GRIN_WALLET_DIR))?;
        let store = SeedStore::new(&home, &self.settings.network);
        store.write_envelope(password, &address, entropy.as_slice(), None)?;
        // Mark the wallet as unscanned *before* upstream creates the database:
        // a crash after this point must never look like a confirmed zero.
        self.require_scan()?;

        let mut wallet = DefaultWalletImpl::<HTTPNodeClient>::new(self.node_client()?)
            .map_err(FlintError::wallet)?;
        let config_path = home.join(WALLET_CONFIG_FILE_NAME);
        let upstream_password = SeedStore::new_upstream_password()?;
        {
            let lc = lc_mut(&mut wallet)?;
            lc.set_top_level_directory(&home.to_string_lossy())
                .map_err(FlintError::wallet)?;
            if !config_path.exists() {
                lc.create_config(&self.chain(), WALLET_CONFIG_FILE_NAME, None, None, None)
                    .map_err(FlintError::wallet)?;
            }
            // recover_from_phrase refuses to work unless the seed directory exists;
            // create_wallet normally makes it, recovery must do it itself.
            fs::create_dir_all(home.join(grin_wallet_config::GRIN_WALLET_DIR))?;
            lc.recover_from_mnemonic(
                ZeroingString::from(mnemonic),
                ZeroingString::from(upstream_password.as_str()),
            )
            .map_err(FlintError::wallet)?;
        }

        if let Err(e) = self.unlock_with(wallet, &config_path, upstream_password.as_str()) {
            store.remove_verified_seed();
            return Err(e);
        }
        let actual = self
            .owner()?
            .get_slatepack_address(None, 0)
            .map_err(FlintError::wallet)?
            .to_string();
        if actual != address {
            store.remove_verified_seed();
            return Err(FlintError::Invalid(
                "адрес восстановленного кошелька не совпадает с конвертом".into(),
            ));
        }
        store.remove_verified_seed();
        self.save_wallet_init_status(WalletInitStatus::InitNoScanning)?;

        Ok(RestoredWallet {
            address,
            needs_scan: true,
        })
    }

    /// Full UTXO scan. Only allowed on a ready node: scanning a half-synced chain
    /// would produce a balance nobody can trust.
    ///
    /// The explicit state is persisted *before* the call, so a crash or a kill
    /// leaves `running`/`failed` — never "completed" without a scan. Only a
    /// successful return clears `pending_scan`.
    pub fn scan_wallet(&mut self, start_height: Option<u64>) -> Result<u64> {
        let job = self.prepare_scan(start_height)?;
        let cancel = AtomicBool::new(false);
        let outcome = job.run(&cancel);
        self.finish_scan(outcome)
    }

    /// Same as [`Self::scan_wallet`], but the caller can stop it at a batch
    /// boundary with `cancel`. The scan keeps its own progress; cancelling does
    /// not corrupt LMDB and does not drop the wallet.
    pub fn scan_wallet_cancellable(
        &mut self,
        start_height: Option<u64>,
        cancel: &AtomicBool,
    ) -> Result<u64> {
        let job = self.prepare_scan(start_height)?;
        let outcome = job.run(cancel);
        self.finish_scan(outcome)
    }

    /// Persist `running`, freeze the node tip and return a job that can run
    /// outside the `WalletService` mutex. Node readiness is checked before the
    /// state transition so a refused scan never looks like it started.
    pub fn prepare_scan(&mut self, start_height: Option<u64>) -> Result<EmbeddedScanJob> {
        self.ensure_chain();
        let start = start_height.unwrap_or(1);
        if self.pending_scan && start > 1 {
            return Err(FlintError::Invalid(
                "неполное восстановление нельзя завершить частичным сканом: запустите полный скан"
                    .into(),
            ));
        }
        let node = self.node_info();
        if !node.phase.ready() {
            return Err(FlintError::Invalid(format!(
                "скан запрещён: {}. Дождитесь полной синхронизации ноды",
                node.phase.label()
            )));
        }

        let (wallet, tip) = {
            let owner = self.owner()?;
            let tip = owner.node_height(None).map_err(FlintError::wallet)?;
            (owner.wallet_inst.clone(), tip)
        };
        if !tip.updated_from_node {
            return Err(FlintError::Node(
                "нода не сообщила tip для скана — повторите позже".into(),
            ));
        }
        if start >= tip.height {
            return Err(FlintError::Invalid(
                "скан не покрывает ни одного блока: выберите высоту ниже tip".into(),
            ));
        }

        self.begin_scan()?;
        Ok(EmbeddedScanJob {
            wallet,
            tip_height: tip.height,
            tip_hash: tip.header_hash,
            start_height: start,
        })
    }

    /// Apply a job outcome to the persisted lifecycle. A late cancellation
    /// never overwrites `Completed`: the job only reports `Cancelled` when it
    /// stopped before its last batch, and this method does not touch a scan
    /// that already completed.
    pub fn finish_scan(&mut self, result: Result<ScanRunOutcome>) -> Result<u64> {
        match result {
            Ok(ScanRunOutcome::Completed) => {
                let height = self.node_info().height;
                self.complete_scan()?;
                // The scan is authoritative now; let later summaries use the
                // saved `last_scanned_block` instead of the "no scan needed"
                // shortcut. A failure here must not turn a finished scan into an
                // error: the batch markers are already on disk.
                let _ = self.save_wallet_init_status(WalletInitStatus::InitComplete);
                Ok(height)
            }
            Ok(ScanRunOutcome::Cancelled) => {
                self.cancel_scan(SCAN_CANCELLED)?;
                Err(FlintError::ScanCancelled)
            }
            Err(e) => {
                // The balance stays unknown: a failed scan is not an empty chain.
                self.fail_scan(&e.to_string())?;
                Err(e)
            }
        }
    }

    /// Open a legacy wallet **only** to compare its index-0 address with the
    /// legacy seed inside the explicit migration operation. The normal
    /// `open_wallet` mainnet gate still refuses legacy wallets everywhere else.
    pub fn verify_legacy_address(&mut self, password: &str) -> Result<String> {
        self.ensure_chain();
        let home = self.wallet_home();
        let store = SeedStore::new(&home, &self.settings.network);
        if store.kind() != SeedKind::Legacy {
            return Err(FlintError::Invalid(
                "для миграции нужен legacy wallet.seed".into(),
            ));
        }
        self.open_legacy(&home, password)?;
        let address = self.address_at(0);
        self.lock();
        address
    }

    /// Open an existing wallet. Legacy wallets are opened exactly as before;
    /// wallets with an Argon2id envelope materialize a transient seed file under
    /// a random password, open upstream with it, verify the address against the
    /// envelope, and remove the file again.
    pub fn open_wallet(&mut self, password: &str) -> Result<()> {
        self.ensure_chain();
        let home = self.wallet_home();
        let store = SeedStore::new(&home, &self.settings.network);
        store.ensure_mainnet_ready()?;
        let opened = match store.kind() {
            SeedKind::Missing => Err(FlintError::NotInitialized),
            SeedKind::Legacy => self.open_legacy(&home, password),
            SeedKind::Envelope => self.open_envelope(&home, &store, password),
        };
        // Older pending profiles may still carry `InitNeedsScanning`; prevent
        // `wallet_summary` from starting an untracked full scan before the user
        // reaches the explicit scan. Failures here must not block unlocking.
        if opened.is_ok() && self.pending_scan {
            let _ = self.save_wallet_init_status(WalletInitStatus::InitNoScanning);
        }
        opened
    }

    fn open_legacy(&mut self, home: &Path, password: &str) -> Result<()> {
        let config_path = home.join(WALLET_CONFIG_FILE_NAME);
        // Managed node requires auth; the wallet's own node client reads this
        // path from its config, not from our settings.
        let node_secret = self
            .paths
            .node_dir(&self.settings.network)
            .join(".api_secret");
        crate::rpc_backend::patch_node_secret(&config_path, &node_secret);

        let mut wallet = DefaultWalletImpl::<HTTPNodeClient>::new(self.node_client()?)
            .map_err(FlintError::wallet)?;
        lc_mut(&mut wallet)?
            .set_top_level_directory(&home.to_string_lossy())
            .map_err(FlintError::wallet)?;

        self.unlock_with(wallet, &config_path, password)
    }

    fn open_envelope(&mut self, home: &Path, store: &SeedStore, password: &str) -> Result<()> {
        let config_path = home.join(WALLET_CONFIG_FILE_NAME);
        let mut wallet = DefaultWalletImpl::<HTTPNodeClient>::new(self.node_client()?)
            .map_err(FlintError::wallet)?;
        {
            let lc = lc_mut(&mut wallet)?;
            lc.set_top_level_directory(&home.to_string_lossy())
                .map_err(FlintError::wallet)?;
            // An interrupted creation or migration may have no config yet.
            if !config_path.exists() {
                lc.create_config(&self.chain(), WALLET_CONFIG_FILE_NAME, None, None, None)
                    .map_err(FlintError::wallet)?;
            }
        }
        let node_secret = self
            .paths
            .node_dir(&self.settings.network)
            .join(".api_secret");
        crate::rpc_backend::patch_node_secret(&config_path, &node_secret);

        // Wrong password fails here, before any file is written.
        let prepared = store.prepare_unlock(password)?;
        if let Err(e) = self.unlock_with(wallet, &config_path, prepared.password()) {
            store.rollback_unlock(&prepared);
            return Err(FlintError::Wallet(format!(
                "конверт seed не открылся (повреждён или это другой кошелёк): {e}"
            )));
        }

        let actual = match self.owner().and_then(|owner| {
            owner
                .get_slatepack_address(None, 0)
                .map_err(FlintError::wallet)
        }) {
            Ok(address) => address.to_string(),
            Err(e) => {
                self.unlocked = None;
                store.rollback_unlock(&prepared);
                return Err(e);
            }
        };
        if let Err(e) = store.commit_unlock(&prepared, &actual) {
            self.unlocked = None;
            return Err(e);
        }
        // A missing database is recreated empty by upstream: the balance is
        // unknown until a scan, not zero.
        if prepared.fresh_db() {
            self.pending_scan = true;
            let _ = self.save_wallet_state();
        }
        Ok(())
    }

    /// Open an existing wallet and immediately reconcile anything a previous crash
    /// left in `finalizing` — that state must not wait for a manual click.
    pub fn open_wallet_and_reconcile(&mut self, password: &str) -> Result<usize> {
        self.open_wallet(password)?;
        match self.reconcile_pending() {
            Ok(n) => Ok(n),
            Err(e) => {
                tracing::warn!("pending reconciliation failed: {e}");
                Ok(0)
            }
        }
    }

    fn unlock_with(
        &mut self,
        mut wallet: DefaultWalletImpl<HTTPNodeClient>,
        config_path: &Path,
        password: &str,
    ) -> Result<()> {
        {
            let lc = lc_mut(&mut wallet)?;
            lc.open_wallet(None, ZeroingString::from(password), false, false)
                .map_err(|e| {
                    FlintError::Wallet(format!(
                        "не удалось открыть кошелёк (неверный пароль?): {e}"
                    ))
                })?;
        }

        let boxed: Box<WalletDyn> = Box::new(wallet);
        let arc: Arc<GrinMutex<Box<WalletDyn>>> = Arc::new(GrinMutex::new(boxed));
        let owner = Owner::new(arc.clone(), None, config_path.to_path_buf());
        let foreign = Foreign::new(arc, config_path.to_path_buf(), None, None, false);

        self.unlocked = Some(UnlockedWallet {
            owner,
            foreign,
            config_path: config_path.to_path_buf(),
        });
        Ok(())
    }

    pub fn lock(&mut self) {
        // A transient seed file is useless without its random password, but it
        // should not outlive the session either. Legacy seeds are untouched:
        // `remove_transient_seed` acts only when an envelope exists.
        self.seed_store().remove_transient_seed();
        self.unlocked = None;
    }

    fn owner(&self) -> Result<&OwnerApi> {
        if !self.initialized() {
            return Err(FlintError::NotInitialized);
        }
        self.unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)
    }

    /// Sender address from an incoming slatepack, so the response can be
    /// encrypted to the sender instead of being handed over in clear text.
    fn slatepack_sender(&self, armor: &str, indices: &[u32]) -> Option<SlatepackAddress> {
        let owner = self.unlocked.as_ref().map(|u| &u.owner)?;
        for index in indices {
            let secret = match owner.get_slatepack_secret_key(None, *index) {
                Ok(secret) => secret,
                Err(e) => {
                    eprintln!("sender: key {index} failed: {e}");
                    continue;
                }
            };
            let signing = ed25519_dalek::SigningKey::from_bytes(&secret.to_bytes());
            let packer = Slatepacker::new(SlatepackerArgs {
                sender: None,
                recipients: vec![],
                dec_key: Some(&signing),
            });
            match packer.deser_slatepack(armor.as_bytes(), true) {
                Ok(slatepack) => {
                    eprintln!("sender: index {index} sender={:?}", slatepack.sender);
                    return slatepack.sender;
                }
                Err(e) => eprintln!("sender: index {index} decode failed: {e}"),
            }
        }
        None
    }

    /* ------------------------------------------------------------- reads --- */

    pub fn summary(&self) -> Result<WalletSummary> {
        self.ensure_chain();
        let owner = self.owner()?;
        let min_conf = self.settings.min_confirmations;

        let (validated, info) = match owner.retrieve_summary_info(None, true, min_conf) {
            Ok((validated, info)) => (validated, info),
            Err(node_err) => owner
                .retrieve_summary_info(None, false, min_conf)
                .map(|(validated, info)| (validated, info))
                .map_err(|_| FlintError::node(node_err))?,
        };

        let address = owner
            .get_slatepack_address(None, self.address_index)
            .map(|a| a.to_string())
            .unwrap_or_default();

        Ok(WalletSummary {
            validated,
            address,
            account: "default".to_string(),
            balance: Balance {
                spendable: info.amount_currently_spendable.to_string(),
                awaiting: (info.amount_awaiting_confirmation + info.amount_awaiting_finalization)
                    .to_string(),
                total: info.total.to_string(),
                immature: info.amount_immature.to_string(),
                locked: info.amount_locked.to_string(),
            },
            last_confirmed_height: info.last_confirmed_height,
            minimum_confirmations: info.minimum_confirmations,
            needs_scan: self.pending_scan,
            scan_state: self.scan_state,
            scan_error: self.scan_error.clone(),
        })
    }

    pub fn wallet_name(&self) -> Option<&str> {
        self.settings.wallet_name.as_deref()
    }

    pub fn wallet_profiles(&self) -> Vec<String> {
        self.paths.list_wallets(&self.settings.network)
    }

    /// Switch the active wallet profile. `None` selects the legacy directory.
    /// The caller must restart the app: the wallet is opened at startup.
    pub fn set_wallet_name(&mut self, name: Option<String>) -> Result<()> {
        self.settings.wallet_name = normalize_wallet_name(name)?;
        self.paths.save_settings(&self.settings)?;
        // The new profile has its own rotation index and scan flag; load them so
        // an in-process switch behaves exactly like a fresh start.
        let state = self.paths.load_wallet_state(
            &self.settings.network,
            self.settings.wallet_name.as_deref(),
            (self.settings.address_index, self.settings.pending_scan),
        );
        self.address_index = state.address_index;
        self.pending_scan = state.pending_scan;
        self.scan_state = state.scan_state;
        self.scan_error = state.scan_error;
        self.scan_started_at = state.scan_started_at;
        self.scan_finished_at = state.scan_finished_at;
        Ok(())
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    pub fn address_index(&self) -> u32 {
        self.address_index
    }

    pub fn pending_scan(&self) -> bool {
        self.pending_scan
    }

    pub fn scan_state(&self) -> ScanState {
        self.scan_state
    }

    pub fn scan_error(&self) -> Option<&str> {
        self.scan_error.as_deref()
    }

    /// A restore wrote a seed but no balance is known yet: mark the profile
    /// before the wallet database is created, so a crash cannot look scanned.
    pub fn require_scan(&mut self) -> Result<()> {
        self.pending_scan = true;
        self.scan_state = ScanState::NotStarted;
        self.scan_error = None;
        self.scan_started_at = None;
        self.scan_finished_at = None;
        self.save_wallet_state()
    }

    /// Persist `running` before the wallet call: a kill during the scan must
    /// leave an explicit interrupted state, not a stale "nothing happened".
    pub fn begin_scan(&mut self) -> Result<()> {
        if self.scan_state == ScanState::Running {
            return Err(FlintError::Invalid("скан уже идёт".into()));
        }
        self.scan_state = ScanState::Running;
        self.scan_error = None;
        self.scan_started_at = Some(Utc::now().timestamp());
        self.scan_finished_at = None;
        self.save_wallet_state()
    }

    /// The wallet confirmed the scan. This is the only place that clears the
    /// unknown-balance flag; an open or a log line is not proof.
    pub fn complete_scan(&mut self) -> Result<()> {
        self.scan_state = ScanState::Completed;
        self.scan_error = None;
        self.scan_finished_at = Some(Utc::now().timestamp());
        self.pending_scan = false;
        self.save_wallet_state()
    }

    pub fn fail_scan(&mut self, error: &str) -> Result<()> {
        self.scan_state = ScanState::Failed;
        self.scan_error = Some(error.to_string());
        self.scan_finished_at = Some(Utc::now().timestamp());
        // `pending_scan` stays as it was: a failed rescan must not pretend the
        // balance is unknown (it was known), and a failed restore scan must not
        // pretend it is zero (it was unknown).
        self.save_wallet_state()
    }

    /// The user aborted the scan. Only a running scan can be cancelled: a
    /// completed one must never be relabelled `failed`, and `pending_scan` is
    /// deliberately left alone because a cancelled restore scan leaves the
    /// balance just as unknown as a failed one.
    pub fn cancel_scan(&mut self, reason: &str) -> Result<()> {
        if self.scan_state != ScanState::Running {
            return Err(FlintError::Invalid("скан не идёт".into()));
        }
        self.fail_scan(reason)
    }

    /// The wallet was busy scanning when an open was attempted. This is an
    /// observation from the owner API, not a log heuristic; the next successful
    /// open finishes the scan.
    pub fn note_scan_running(&mut self) -> Result<()> {
        if self.scan_state == ScanState::Running {
            return Ok(());
        }
        self.scan_state = ScanState::Running;
        self.scan_error = None;
        self.scan_started_at = Some(Utc::now().timestamp());
        self.scan_finished_at = None;
        self.save_wallet_state()
    }

    fn save_wallet_state(&self) -> Result<()> {
        self.paths.save_wallet_state(
            &self.settings.network,
            self.settings.wallet_name.as_deref(),
            &crate::config::WalletState {
                address_index: self.address_index,
                pending_scan: self.pending_scan,
                scan_state: self.scan_state,
                scan_error: self.scan_error.clone(),
                scan_started_at: self.scan_started_at,
                scan_finished_at: self.scan_finished_at,
            },
        )
    }

    /// Indices tried when decrypting: every address this wallet has used stays
    /// able to receive offline slatepacks.
    fn decrypt_indices(&self) -> Vec<u32> {
        (0..=self.address_index.min(MAX_ADDRESS_INDEX)).collect()
    }

    /// `m/0/1/x`: rotate to the next address without touching the seed phrase.
    ///
    /// Old addresses keep working for offline slatepacks (decryption walks all
    /// indices up to the current one); online delivery via Tor onion does not,
    /// because the onion is derived from the current address key.
    pub fn next_address_index(&mut self) -> Result<u32> {
        if self.address_index >= MAX_ADDRESS_INDEX {
            return Err(FlintError::Invalid(format!(
                "исчерпан предел индексов адреса ({MAX_ADDRESS_INDEX})"
            )));
        }
        self.address_index += 1;
        self.save_wallet_state()?;
        Ok(self.address_index)
    }

    pub fn address(&self) -> Result<String> {
        self.ensure_chain();
        Ok(self
            .owner()?
            .get_slatepack_address(None, self.address_index)
            .map_err(FlintError::wallet)?
            .to_string())
    }

    /// Slatepack address at an explicit derivation index. The envelope always
    /// binds index 0, independent of the UI's rotating address index.
    pub fn address_at(&self, index: u32) -> Result<String> {
        self.ensure_chain();
        Ok(self
            .owner()?
            .get_slatepack_address(None, index)
            .map_err(FlintError::wallet)?
            .to_string())
    }

    /// 32-byte ed25519 secret of the current slatepack address, for the
    /// messenger identity. Callers must zeroize it after use.
    pub fn messenger_seed(&self) -> Result<zeroize::Zeroizing<[u8; 32]>> {
        self.ensure_chain();
        let secret = self
            .owner()?
            .get_slatepack_secret_key(None, self.address_index)
            .map_err(FlintError::wallet)?;
        Ok(zeroize::Zeroizing::new(secret.to_bytes()))
    }

    pub fn activity(&self, limit: usize) -> Result<Vec<TxItem>> {
        self.ensure_chain();
        let owner = self.owner()?;
        let (_, entries) = match owner.retrieve_txs(None, true, None, None, None) {
            Ok(v) => v,
            Err(_) => owner
                .retrieve_txs(None, false, None, None, None)
                .map_err(FlintError::wallet)?,
        };

        let mut items: Vec<TxItem> = entries
            .into_iter()
            .map(|t| {
                let is_in = matches!(
                    t.tx_type,
                    grin_wallet_libwallet::TxLogEntryType::TxReceived
                        | grin_wallet_libwallet::TxLogEntryType::ConfirmedCoinbase
                );
                let canceled = matches!(
                    t.tx_type,
                    grin_wallet_libwallet::TxLogEntryType::TxSentCancelled
                        | grin_wallet_libwallet::TxLogEntryType::TxReceivedCancelled
                );
                let fee = t.fee.map(|f| f.fee()).unwrap_or(0);
                let amount = if is_in {
                    t.amount_credited
                } else {
                    t.amount_debited.saturating_sub(fee)
                };
                TxItem {
                    id: t
                        .tx_slate_id
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| format!("tx-{}", t.id)),
                    direction: if is_in { "in".into() } else { "out".into() },
                    amount: amount.to_string(),
                    fee: fee.to_string(),
                    status: if canceled {
                        "canceled".into()
                    } else if t.confirmed {
                        "confirmed".into()
                    } else {
                        "pending".into()
                    },
                    created_at: t.creation_ts.timestamp_millis(),
                    confirmed_at: t.confirmation_ts.map(|c| c.timestamp_millis()),
                    counterparty: t
                        .payment_proof
                        .as_ref()
                        .map(|p| SlatepackAddress::new(&p.receiver_address).to_string()),
                    note: None,
                    tx_type: format!("{:?}", t.tx_type),
                }
            })
            .collect();

        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        items.truncate(limit);
        Ok(items)
    }

    /* ------------------------------------------------------------ writes --- */

    pub fn prepare_send(&mut self, destination: &str, amount_nano: u64) -> Result<SendSlate> {
        self.ensure_chain();
        // Spending against a chain that is still downloading produces unspendable
        // change and false balances. Refuse until the node reports ready.
        let node = self.node_info();
        if !node.phase.ready() {
            return Err(FlintError::Invalid(format!(
                "отправка заблокирована: {}. Дождитесь полной синхронизации ноды — иначе \
                 баланс и подтверждения недостоверны",
                node.phase.label()
            )));
        }
        if amount_nano == 0 {
            return Err(FlintError::Invalid("сумма должна быть больше нуля".into()));
        }
        let destination = destination.trim().to_string();
        if destination.is_empty() {
            return Err(FlintError::Invalid("укажите адрес получателя".into()));
        }

        // A malformed address used to fall through to an unencrypted slatepack
        // (`unwrap_or_default`), silently leaking amount and graph to a channel
        // the user believed was encrypted. Encryption is now the only default;
        // plaintext requires the explicit word `manual`.
        let plaintext = destination.eq_ignore_ascii_case("manual");
        let recipients = if plaintext {
            Vec::new()
        } else {
            vec![
                SlatepackAddress::try_from(destination.as_str()).map_err(|e| {
                    FlintError::Invalid(format!(
                        "адрес получателя не распознан ({e}). Если передача идёт вручную, \
                     введите manual — тогда slatepack уйдёт без шифрования"
                    ))
                })?,
            ]
        };

        let min_conf = self.settings.min_confirmations;
        let owner = self
            .unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)?;

        let args = InitTxArgs {
            amount: amount_nano,
            minimum_confirmations: min_conf,
            max_outputs: 500,
            num_change_outputs: 1,
            selection_strategy_is_use_all: true,
            ..Default::default()
        };

        let slate = owner.init_send_tx(None, args).map_err(FlintError::wallet)?;

        // No wallet state changes yet: a failure here leaves nothing to undo.
        let slatepack = owner
            .create_slatepack_message(None, &slate, Some(0), recipients)
            .map_err(FlintError::wallet)?;

        let slate_id = slate.id.to_string();
        let fee_nano = slate.fee_fields.fee().to_string();
        let pending = PendingSlate {
            kind: "send".into(),
            destination: Some(destination.clone()),
            created_at: Utc::now().timestamp_millis(),
            amount: amount_nano,
            slate: VersionedSlate::into_version(slate.clone(), SlateVersion::V4)
                .map_err(FlintError::wallet)?,
            request: None,
            state: Some(TxState::Prepared),
            attempts: 0,
            last_error: None,
        };
        // Record first, lock second. A crash between the two must leave a pending
        // entry the user can resume or cancel — never invisible locked coins.
        self.save_pending(&slate_id, &pending)?;
        #[cfg(debug_assertions)]
        if std::env::var("FLINT_CRASH_AFTER_PREPARE").is_ok() {
            return Err(FlintError::Invalid("CRASH_AFTER_PREPARE".into()));
        }

        if let Err(lock_err) = owner.tx_lock_outputs(None, &slate) {
            return match owner.cancel_tx(None, None, Some(slate.id)) {
                // Nothing is locked and nothing is signed: dropping the record is safe.
                Ok(()) => {
                    let _ = self.remove_pending(&slate_id);
                    Err(FlintError::Wallet(format!(
                        "не удалось заблокировать входы: {lock_err}"
                    )))
                }
                // The lock may be partial and the cancel failed. The record is the
                // only evidence of what happened — keep it, never delete.
                Err(cancel_err) => {
                    let _ = self.advance_pending(&slate_id, TxState::FailedRecoverable);
                    Err(FlintError::Wallet(format!(
                        "не удалось заблокировать входы: {lock_err}; отмена тоже не прошла: \
                         {cancel_err}. Запись сохранена как failed_recoverable — не начинайте \
                         новую отправку, пока не сверите состояние с Grin"
                    )))
                }
            };
        }

        // Inputs are locked now; say so, otherwise a crash before the receiver
        // answers would look like a prepared-but-not-locked transaction.
        self.advance_pending(&slate_id, TxState::Locked)?;

        Ok(SendSlate {
            slate_id,
            slatepack,
            amount_nano: amount_nano.to_string(),
            fee_nano,
            destination,
            encrypted: !plaintext,
        })
    }

    /// Complete a send after the receiver returned their signed slate.
    ///
    /// Rebuild the original S1 request armor for a still-unsigned send.
    /// Finalize/publish are explicitly refused; no transaction is mutated.
    pub fn export_send_request(&mut self, slate_id: &str) -> Result<String> {
        self.ensure_chain();
        let pending = self.load_pending(slate_id)?;
        if pending.kind != "send"
            || !matches!(
                pending.resolved_state(),
                TxState::Prepared | TxState::Locked | TxState::FailedRecoverable
            )
        {
            return Err(FlintError::Invalid(
                "экспорт S1 запрещён: операция уже финализирована или не является отправкой".into(),
            ));
        }
        let destination = pending
            .destination
            .clone()
            .ok_or_else(|| FlintError::Invalid("S1: адрес получателя не сохранён".into()))?;
        let recipients = if destination.eq_ignore_ascii_case("manual") {
            Vec::new()
        } else {
            vec![SlatepackAddress::try_from(destination.as_str())
                .map_err(|e| FlintError::Invalid(format!("S1: адрес получателя: {e}")))?]
        };
        let slate: Slate = pending.slate.into();
        let owner = self
            .unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)?;
        owner
            .create_slatepack_message(None, &slate, Some(0), recipients)
            .map_err(FlintError::wallet)
    }

    /// If a previous attempt already signed the transaction but failed to publish
    /// it, this resumes at the broadcast step instead of signing again: the record
    /// holds `FinalizedNotPosted`, and re-finalizing a signed slate is not safe.
    pub fn finalize_send(&mut self, slate_id: &str, response_slatepack: &str) -> Result<String> {
        self.ensure_chain();
        let mut pending = match self.load_pending(slate_id) {
            Ok(pending) => pending,
            Err(FlintError::UnknownSlate(_)) => {
                // The journal is removed once the chain confirms the tx. Only an
                // explicit not-found may use this fallback; unreadable journals
                // must never be treated as permission to finalize again.
                let id = parse_slate_id(slate_id)?;
                let owner = self
                    .unlocked
                    .as_ref()
                    .map(|u| &u.owner)
                    .ok_or(FlintError::Locked)?;
                let (_, txs) = owner
                    .retrieve_txs(None, true, None, Some(id), None)
                    .map_err(FlintError::wallet)?;
                if txs.iter().any(|t| t.tx_slate_id == Some(id)) {
                    return Ok(slate_id.to_string());
                }
                return Err(FlintError::Invalid(format!("нет такого slate: {slate_id}")));
            }
            Err(error) => return Err(error),
        };
        if pending.kind != "send" {
            return Err(FlintError::Invalid("это не исходящий slate".into()));
        }
        if matches!(
            pending.resolved_state(),
            TxState::Finalizing | TxState::FinalizedNotPosted
        ) {
            // The signature may already exist. Never re-sign: try to publish what
            // Grin has, and report if that is not possible.
            return self.retry_post(slate_id);
        }
        if pending.resolved_state() == TxState::Posted {
            // Already published: repeating finalize must be a no-op, not a
            // second signature attempt (upstream drops the private context).
            return Ok(slate_id.to_string());
        }
        if !matches!(
            pending.resolved_state(),
            TxState::Prepared | TxState::Locked
        ) {
            return Err(FlintError::Invalid(format!(
                "финализация запрещена в состоянии {:?}; требуется ручное восстановление",
                pending.resolved_state()
            )));
        }

        let owner = self
            .unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)?;

        let indices = self.decrypt_indices();
        let response = owner
            .slate_from_slatepack_message(None, response_slatepack.trim().to_string(), indices)
            .map_err(|e| FlintError::Wallet(format!("не удалось разобрать ответ: {e}")))?;

        if response.id.to_string() != slate_id {
            return Err(FlintError::Invalid(
                "ответный slate относится к другой транзакции".into(),
            ));
        }

        // Persist the intent *before* the irreversible call. A crash inside
        // `finalize_tx` then leaves `Finalizing`, not a stale `locked` that would
        // invite a second signature on the next run.
        pending.state = Some(TxState::Finalizing);
        self.save_pending(slate_id, &pending)?;

        let final_slate = match owner.finalize_tx(None, &response) {
            Ok(slate) => slate,
            Err(e) => {
                let text = e.to_string();
                // No error string proves a signature. In particular, a missing
                // private context may equally mean that signing never happened.
                mark_finalize_failed(&mut pending, text);
                self.save_pending(slate_id, &pending)?;
                return Err(FlintError::Invalid(
                    "результат finalize_tx неизвестен; повторная подпись и автоматическая публикация запрещены, запись оставлена для ручного восстановления"
                        .into(),
                ));
            }
        };

        // Signed. Record it before touching the network: a crash during `post_tx`
        // must resume at the broadcast, never at the signature.
        pending.state = Some(TxState::FinalizedNotPosted);
        self.save_pending(slate_id, &pending)?;
        #[cfg(debug_assertions)]
        if std::env::var("FLINT_CRASH_AFTER_FINALIZE").is_ok() {
            return Err(FlintError::Invalid("CRASH_AFTER_FINALIZE".into()));
        }

        if let Err(e) = owner.post_tx(None, &final_slate, false) {
            // A failed publish is not a refusal: the transaction stays signed and
            // retryable, and the attempt is recorded for the user.
            pending.attempts += 1;
            pending.last_error = Some(e.to_string());
            self.save_pending(slate_id, &pending)?;
            return Err(FlintError::Node(format!(
                "узел не принял транзакцию (попытка {}): {e}. Транзакция подписана и сохранена — \
                 повторите публикацию; дубликат сеть отклонит",
                pending.attempts
            )));
        }

        // Published. The record stays until the chain confirms it: after a crash
        // the journal must answer "signed? published? confirmed?" without guessing.
        pending.state = Some(TxState::Posted);
        self.save_pending(slate_id, &pending)?;
        Ok(slate_id.to_string())
    }

    /// Re-broadcast a transaction that was signed but never reached the node.
    ///
    /// The signed transaction comes from Grin's own store (`get_stored_tx`), not
    /// from the pending JSON: upstream is the authority on what was signed.
    ///
    /// `Finalizing` is reconciled to `FailedRecoverable` without any network
    /// publish: `get_stored_tx` can contain an unsigned partial S1, so its mere
    /// presence is not proof that finalization succeeded.
    pub fn retry_post(&mut self, slate_id: &str) -> Result<String> {
        self.ensure_chain();
        let mut pending = self.load_pending(slate_id)?;
        if pending.kind != "send" {
            return Err(FlintError::Invalid("это не исходящий slate".into()));
        }
        match pending.resolved_state() {
            TxState::Finalizing => {
                pending.state = Some(TxState::FailedRecoverable);
                pending.last_error = Some(
                    "finalizing: результат finalize_tx неизвестен; автоматическая публикация запрещена"
                        .into(),
                );
                self.save_pending(slate_id, &pending)?;
                return Err(FlintError::Invalid(
                    "нельзя доказать, что сохранённая сделка подписана; запись оставлена для ручного восстановления"
                        .into(),
                ));
            }
            TxState::FinalizedNotPosted => {}
            TxState::Posted => {
                return Err(FlintError::Invalid(
                    "транзакция уже отправлена в сеть".into(),
                ))
            }
            other => {
                return Err(FlintError::Invalid(format!(
                    "публиковать нечего: транзакция в состоянии {other:?}"
                )))
            }
        }

        let id = parse_slate_id(slate_id)?;
        let owner = self
            .unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)?;

        // The node may have accepted the transaction while our RPC call timed out.
        // Ask the wallet's own log first: if it is already confirmed, publishing
        // again is pointless and the error would only confuse.
        let (_, txs) = owner
            .retrieve_txs(None, true, None, Some(id), None)
            .map_err(FlintError::wallet)?;
        if txs.iter().any(|t| t.confirmed) {
            pending.state = Some(TxState::Posted);
            self.save_pending(slate_id, &pending)?;
            self.archive_pending(slate_id, TxState::Confirmed)?;
            return Ok(slate_id.to_string());
        }

        let stored = owner
            .get_stored_tx(None, None, Some(&id))
            .map_err(FlintError::wallet)?
            .ok_or_else(|| {
                FlintError::Wallet(
                    "Grin не нашёл подписанную транзакцию — восстановить публикацию нельзя".into(),
                )
            })?;

        if let Err(e) = owner.post_tx(None, &stored, false) {
            pending.attempts += 1;
            pending.last_error = Some(e.to_string());
            self.save_pending(slate_id, &pending)?;
            return Err(FlintError::Node(format!(
                "узел не принял транзакцию (попытка {}): {e}. Транзакция подписана и сохранена — \
                 повторите позже",
                pending.attempts
            )));
        }

        pending.state = Some(TxState::Posted);
        self.save_pending(slate_id, &pending)?;
        Ok(slate_id.to_string())
    }

    /// Drop journal records the chain has already confirmed: `posted` sends and
    /// answered receives whose output appeared on chain.
    ///
    /// Deliberately does not refresh from the node: it runs on every pending fetch
    /// and reads the local tx log, which the summary refresh already updates.
    pub fn reconcile_posted(&mut self) -> Result<usize> {
        if self.unlocked.is_none() {
            return Ok(0);
        }
        self.ensure_chain();
        let mut done = 0;
        for id in self.pending_ids()? {
            let Ok(mut p) = self.load_pending(&id) else {
                continue;
            };
            let state = p.resolved_state();
            // A receive is final only when the sender's transaction is confirmed;
            // handing over the response proves nothing.
            let trackable = state == TxState::Posted
                || (state == TxState::ResponseReceived && p.kind == "receive");
            if !trackable {
                continue;
            }
            let Ok(uuid) = parse_slate_id(&id) else {
                continue;
            };
            let confirmed = {
                let owner = match self.unlocked.as_ref() {
                    Some(u) => &u.owner,
                    None => break,
                };
                owner
                    .retrieve_txs(None, false, None, Some(uuid), None)
                    .map(|(_, txs)| {
                        txs.iter().find(|t| t.confirmed).map(|t| {
                            let fee = t.fee.map(|f| f.fee()).unwrap_or(0);
                            amount_matches(
                                &p.kind,
                                p.amount,
                                t.amount_credited,
                                t.amount_debited,
                                fee,
                            )
                        })
                    })
                    .unwrap_or(None)
            };
            match confirmed {
                Some(true) => {
                    self.archive_pending(&id, TxState::Confirmed)?;
                    done += 1;
                }
                Some(false) => {
                    // The chain says a different amount than the journal: do not
                    // drop the record, surface it instead.
                    p.state = Some(TxState::FailedRecoverable);
                    p.last_error = Some(format!(
                        "сумма в цепи не совпадает с записью: ожидалось {} нано",
                        p.amount
                    ));
                    self.save_pending(&id, &p)?;
                }
                None => {}
            }
        }
        Ok(done)
    }

    /// Parse an incoming slate and add our output. Returns the response slatepack.
    pub fn prepare_receive(&mut self, slatepack: &str) -> Result<ReceiveSlate> {
        self.ensure_chain();
        let text = slatepack.trim();
        if text.is_empty() {
            return Err(FlintError::Invalid("вставьте slatepack".into()));
        }
        let indices = self.decrypt_indices();
        // Encrypt the answer to the sender when their address is present in the
        // incoming slatepack; otherwise fall back to the old cleartext answer.
        let sender = self.slatepack_sender(text, &indices);
        let recipients: Vec<SlatepackAddress> = sender.clone().into_iter().collect();

        let unlocked = self.unlocked.as_ref().ok_or(FlintError::Locked)?;
        let slate = unlocked
            .owner
            .slate_from_slatepack_message(None, text.to_string(), indices)
            .map_err(|e| FlintError::Wallet(format!("не удалось разобрать slatepack: {e}")))?;

        let amount = slate.amount;
        if amount == 0 {
            return Err(FlintError::Invalid("slate не содержит суммы".into()));
        }

        let slate_id = slate.id.to_string();
        let request = serde_json::to_value(&slate).map_err(FlintError::Json)?;
        // Idempotent receive: a completed response can be re-armored. An
        // ambiguous attempt must never call `receive_tx` again; a request that
        // reuses the slate id with different contents is not a repeat.
        match self.load_pending(&slate_id) {
            Ok(existing) => {
                if existing.kind == "receive" {
                    check_receive_repeat(&existing, &request)?;
                }
                match existing.resolved_state() {
                    TxState::ResponseReceived => {
                        if let Some(reply) = self.resume_receive(&slate_id)? {
                            return Ok(reply);
                        }
                    }
                    TxState::ResponseLost => {
                        return Err(FlintError::Invalid(
                        "входящая сделка уже принята, но ответ был потерян; повторный receive запрещён"
                            .into(),
                    ));
                    }
                    TxState::ReceiveIntent => {
                        // Повторяем тот же S1: patched upstream либо вернёт тот же
                        // S2 из atomic recovery, либо выполнит первый commit. Старый
                        // TxReceived без recovery станет ResponseLost, без подписи.
                    }
                    other => {
                        return Err(FlintError::Invalid(format!(
                            "запись receive уже существует в состоянии {other:?}"
                        )));
                    }
                }
            }
            Err(FlintError::UnknownSlate(_)) => {}
            Err(error) => return Err(error),
        }

        // Durable intent before the wallet mutation. If we crash after this
        // point, the record says exactly which incoming slate was being
        // processed.
        let intent = PendingSlate {
            kind: "receive".into(),
            destination: sender.as_ref().map(|a| a.to_string()),
            created_at: Utc::now().timestamp_millis(),
            amount,
            slate: VersionedSlate::into_version(slate.clone(), SlateVersion::V4)
                .map_err(FlintError::wallet)?,
            request: Some(request.clone()),
            state: Some(TxState::ReceiveIntent),
            attempts: 0,
            last_error: None,
        };
        self.save_pending(&slate_id, &intent)?;
        // DF-J preparation: intent is durable, but no wallet mutation happened.
        // The destructible harness fills only the journal volume after this exit.
        crate::crash::receive_boundary("intent");

        let response = match unlocked.foreign.receive_tx(&slate, None, None) {
            Ok(response) => response,
            Err(error) => {
                let text = error.to_string();
                let response_unavailable = crate::error::receive_tx_already_mutated(&text);
                let mut pending = self.load_pending(&slate_id)?;
                mark_receive_failed(&mut pending, text.clone());
                self.save_pending(&slate_id, &pending)?;
                if response_unavailable {
                    return Err(FlintError::Wallet(format!(
                        "входящая сделка уже записана, но сохранённый ответ недоступен или повреждён ({text}); повторный receive запрещён"
                    )));
                }
                return Err(FlintError::Wallet(format!(
                    "receive_tx не выполнен ({text}); запись сохранена как receive_intent, повтор допустим"
                )));
            }
        };

        // OS-level F-04/C failpoint: patched upstream has committed S2, while
        // Flint still has only receive_intent. No-op without crash-injection.
        crate::crash::receive_boundary("upstream");

        // Save the response **before** armoring: `receive_tx` already mutated the
        // wallet, and this record is the only way to hand the same answer to the
        // sender again after a crash.
        let pending = PendingSlate {
            kind: "receive".into(),
            destination: sender.as_ref().map(|a| a.to_string()),
            created_at: Utc::now().timestamp_millis(),
            amount,
            slate: VersionedSlate::into_version(response.clone(), SlateVersion::V4)
                .map_err(FlintError::wallet)?,
            request: Some(request.clone()),
            state: Some(TxState::ResponseReceived),
            attempts: 0,
            last_error: None,
        };
        crate::crash::receive_journal_write()?;
        self.save_pending(&slate_id, &pending)?;
        crate::crash::receive_boundary("journal");

        let out = unlocked
            .owner
            .create_slatepack_message(None, &response, Some(0), recipients)
            .map_err(FlintError::wallet)?;

        Ok(ReceiveSlate {
            slate_id,
            slatepack: out,
            amount_nano: amount.to_string(),
        })
    }

    /// Rebuild the response slatepack for a receive transaction that is already
    /// stored in this wallet (a previous attempt got at least that far).
    fn resume_receive(&self, slate_id: &str) -> Result<Option<ReceiveSlate>> {
        let record = self.load_pending(slate_id)?;
        if record.kind != "receive" || record.resolved_state() != TxState::ResponseReceived {
            return Ok(None);
        }
        let response: Slate = record.slate.into();
        let owner = self
            .unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)?;
        let recipients: Vec<SlatepackAddress> = record
            .destination
            .as_deref()
            .and_then(|s| SlatepackAddress::try_from(s).ok())
            .into_iter()
            .collect();
        let slatepack = owner
            .create_slatepack_message(None, &response, Some(0), recipients)
            .map_err(FlintError::wallet)?;
        Ok(Some(ReceiveSlate {
            slate_id: slate_id.to_string(),
            slatepack,
            amount_nano: record.amount.to_string(),
        }))
    }

    /// Rebuild the response armor for a receive transaction that is already
    /// waiting for the sender. This is the explicit export path for the
    /// unfinished-transfers center; it does not sign or mutate the wallet.
    pub fn export_receive_response(&mut self, slate_id: &str) -> Result<ReceiveSlate> {
        self.ensure_chain();
        self.resume_receive(slate_id)?.ok_or_else(|| {
            FlintError::Invalid("ответ для этого входящего slate ещё не готов".into())
        })
    }

    /// Receiving is finished once the response slatepack is handed back; the payer
    /// posts the transaction, so there is nothing left to broadcast here.
    ///
    /// The record is **not** removed here: handing over the response proves
    /// nothing about the chain. `reconcile_posted` drops it when the wallet's
    /// tx-log shows the incoming transaction confirmed.
    pub fn finalize_receive(&mut self, slate_id: &str) -> Result<()> {
        let pending = self.load_pending(slate_id)?;
        if pending.kind != "receive" {
            return Err(FlintError::Invalid("это не входящий slate".into()));
        }
        Ok(())
    }

    /// Cancel an outgoing transaction in the Grin wallet and only then drop the
    /// pending record. Without the wallet-side cancel the inputs stay locked
    /// forever, which is worse than leaving the record in place.
    ///
    /// Grin needs a reachable node for this (it refreshes wallet state first), so
    /// cancellation is impossible while the node is down.
    pub fn cancel_pending(&mut self, slate_id: &str) -> Result<()> {
        self.ensure_chain();
        let pending = self.load_pending(slate_id)?;
        if pending.kind != "send" {
            return Err(FlintError::Invalid(
                "входящий slate нельзя отменить: отправитель мог уже опубликовать транзакцию"
                    .into(),
            ));
        }
        match pending.resolved_state() {
            // Finalizing may already be signed; the UI/command layer offers only
            // retry for it. FailedRecoverable is ambiguous, so cancel is not
            // offered there either.
            TxState::Prepared | TxState::Locked => {}
            TxState::Finalizing => return Err(FlintError::Invalid(
                "операция финализируется: отмена запрещена, доступна только повторная публикация"
                    .into(),
            )),
            TxState::FailedRecoverable | TxState::ReceiveIntent | TxState::ResponseLost => {
                return Err(FlintError::Invalid(
                    "состояние операции не определено: отмена запрещена".into(),
                ))
            }
            TxState::FinalizedNotPosted => {
                return Err(FlintError::Invalid(
                    "транзакция уже подписана — отменить нельзя. Опубликуйте её или дождитесь \
                     истечения: подпись необратима"
                        .into(),
                ))
            }
            TxState::Posted => {
                return Err(FlintError::Invalid(
                    "транзакция уже отправлена в сеть".into(),
                ))
            }
            TxState::ResponseReceived => {
                return Err(FlintError::Invalid("это входящий slate".into()))
            }
            TxState::Confirmed | TxState::Cancelled => {
                return Err(FlintError::Invalid(
                    "запись уже завершена и лежит в архиве".into(),
                ))
            }
        }
        let id = parse_slate_id(slate_id)?;
        let owner = self
            .unlocked
            .as_ref()
            .map(|u| &u.owner)
            .ok_or(FlintError::Locked)?;
        owner.cancel_tx(None, None, Some(id)).map_err(|e| {
            FlintError::Wallet(format!(
                "Grin не отменил транзакцию: {e}. Запись оставлена, входы всё ещё \
                 заблокированы — проверьте, что нода запущена"
            ))
        })?;
        self.archive_pending(slate_id, TxState::Cancelled)
    }

    /// Pending files that cannot be parsed.
    ///
    /// They are kept on disk for forensics — silently skipping a broken record is
    /// how a transaction in flight becomes invisible.
    pub fn pending_problems(&self) -> Result<Vec<String>> {
        let dir = self.pending_dir_path();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&dir).map_err(|e| {
            FlintError::Wallet(format!(
                "pending: не удалось прочитать каталог {}: {e}",
                dir.display()
            ))
        })?;
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                FlintError::Wallet(format!("pending: не удалось прочитать запись: {e}"))
            })?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let readable = fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<PendingSlate>(&raw).ok())
                .is_some();
            if !readable {
                out.push(
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            }
        }
        Ok(out)
    }

    pub fn pending_dir_path(&self) -> std::path::PathBuf {
        self.paths
            .pending_dir_for(&self.settings.network, self.settings.wallet_name.as_deref())
    }

    fn pending_ids(&self) -> Result<Vec<String>> {
        let dir = self
            .paths
            .pending_dir_for(&self.settings.network, self.settings.wallet_name.as_deref());
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if parse_slate_id(stem).is_ok() {
                    out.push(stem.to_string());
                }
            }
        }
        Ok(out)
    }

    /// Resume only records whose signature is durable (`finalized_not_posted`).
    /// `finalizing` is ambiguous and is left for explicit manual recovery.
    pub fn reconcile_pending(&mut self) -> Result<usize> {
        if self.unlocked.is_none() {
            return Ok(0);
        }
        self.ensure_chain();
        let mut resolved = 0;
        for id in self.pending_ids()? {
            let Ok(p) = self.load_pending(&id) else {
                continue;
            };
            if p.resolved_state() == TxState::FinalizedNotPosted && self.retry_post(&id).is_ok() {
                resolved += 1;
            }
        }
        Ok(resolved)
    }

    pub fn list_pending(&self) -> Result<Vec<PendingView>> {
        let dir = self
            .paths
            .pending_dir_for(&self.settings.network, self.settings.wallet_name.as_deref());
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            crate::win_acl::restrict(&path)?;
            let raw = fs::read_to_string(&path)?;
            let Ok(p) = serde_json::from_str::<PendingSlate>(&raw) else {
                continue;
            };
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let state = p.resolved_state().as_str().to_string();
            out.push(PendingView {
                slate_id: id,
                kind: p.kind,
                amount: p.amount.to_string(),
                destination: p.destination,
                created_at: p.created_at,
                state,
                attempts: p.attempts,
                last_error: p.last_error,
            });
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    /// `<data>/<network>/pending/<uuid>.json`.
    ///
    /// `slate_id` arrives from the renderer over IPC and used to be concatenated
    /// straight into a path, so `wallet_cancel("../../settings")` reached files
    /// outside the pending folder. Every access goes through this function now.
    fn pending_path(&self, id: &str) -> Result<PathBuf> {
        let uuid = parse_slate_id(id)?;
        Ok(self
            .paths
            .pending_dir_for(&self.settings.network, self.settings.wallet_name.as_deref())
            .join(format!("{uuid}.json")))
    }

    /// Durable owner-only write. A crash mid-write leaves either the old record
    /// or the new one — never a truncated JSON document under an inherited ACL.
    fn save_pending(&self, id: &str, pending: &PendingSlate) -> Result<()> {
        let path = self.pending_path(id)?;
        let raw = serde_json::to_vec_pretty(pending)?;
        crate::win_acl::write_private_atomic(&path, &raw)?;
        Ok(())
    }

    /// Move a pending record to a new state, keeping every other field.
    fn advance_pending(&self, id: &str, state: TxState) -> Result<()> {
        let mut pending = self.load_pending(id)?;
        pending.state = Some(state);
        self.save_pending(id, &pending)
    }

    fn load_pending(&self, id: &str) -> Result<PendingSlate> {
        let path = self.pending_path(id)?;
        if !path.exists() {
            return Err(FlintError::UnknownSlate(id.to_string()));
        }
        crate::win_acl::restrict(&path)?;
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Move a finished record into `archive/` instead of deleting it. This is
    /// the only place `confirmed`/`cancelled` are persisted: the live journal
    /// stays small, the audit trail survives.
    fn archive_pending(&self, id: &str, state: TxState) -> Result<()> {
        let mut pending = self.load_pending(id)?;
        pending.state = Some(state);
        let path = self.pending_path(id)?;
        let dir = path
            .parent()
            .ok_or_else(|| FlintError::Invalid("у pending нет каталога".into()))?
            .join("archive");
        let target = dir.join(
            path.file_name()
                .ok_or_else(|| FlintError::Invalid("у pending нет имени".into()))?,
        );
        let raw = serde_json::to_vec_pretty(&pending)?;
        crate::win_acl::write_private_atomic(&target, &raw)?;
        fs::remove_file(&path)?;
        Ok(())
    }

    fn remove_pending(&self, id: &str) -> Result<()> {
        let path = self.pending_path(id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Change the wallet password. The new one must pass the same strength
    /// policy as creation.
    pub fn change_password(&mut self, old: &str, new: &str) -> Result<()> {
        self.ensure_chain();
        validate_password_strength(new)?;
        let store = self.seed_store();
        if store.kind() == SeedKind::Envelope {
            // The envelope is what the password protects: it is re-encrypted
            // with a fresh salt, upstream is not called, `wallet.seed` is not
            // rewritten.
            return store.change_password(old, new);
        }
        let owner = self.owner()?;
        owner
            .change_password(
                None,
                grin_util::ZeroingString::from(old),
                grin_util::ZeroingString::from(new),
            )
            .map_err(FlintError::wallet)?;
        Ok(())
    }

    /// Remove the current wallet from the live tree without destroying it: the
    /// directory is moved to `trash/<network>/`. Returns the new location.
    pub fn delete_wallet(&mut self, confirm_name: &str) -> Result<String> {
        self.ensure_chain();
        let name = self
            .settings
            .wallet_name
            .clone()
            .unwrap_or_else(|| "default".to_string());
        if confirm_name.trim() != name {
            return Err(FlintError::Invalid(format!(
                "для удаления введите имя кошелька: {name}"
            )));
        }
        let home = self.wallet_home();
        if !home.exists() {
            return Err(FlintError::Invalid("кошелёк не найден".into()));
        }
        self.lock();
        let trash = self.paths.root().join("trash").join(&self.settings.network);
        fs::create_dir_all(&trash)?;
        let target = trash.join(format!("{name}-{}", Utc::now().timestamp_millis()));
        fs::rename(&home, &target)
            .map_err(|e| FlintError::Invalid(format!("не удалось убрать кошелёк: {e}")))?;
        self.settings.wallet_name = None;
        self.paths.save_settings(&self.settings)?;
        self.address_index = 0;
        self.pending_scan = false;
        self.scan_state = ScanState::NotStarted;
        self.scan_error = None;
        self.scan_started_at = None;
        self.scan_finished_at = None;
        Ok(target.display().to_string())
    }

    /// Exposed for tooling: the raw config path of the active wallet.
    pub fn config_path(&self) -> Option<&Path> {
        self.unlocked.as_ref().map(|u| u.config_path.as_path())
    }
}

impl Default for WalletService {
    fn default() -> Self {
        Self::new()
    }
}

/// `WalletInst` is implemented for `DefaultWalletImpl` generically over the
/// lifecycle provider, so the trait method needs disambiguation.
fn lc_mut(
    wallet: &mut DefaultWalletImpl<HTTPNodeClient>,
) -> Result<&mut dyn WalletLCProvider<'static, HTTPNodeClient, ExtKeychain>> {
    <DefaultWalletImpl<HTTPNodeClient> as WalletInst<'static, Lc, HTTPNodeClient, ExtKeychain>>::lc_provider(
        wallet,
    )
    .map_err(FlintError::wallet)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PendingSlate {
    kind: String,
    destination: Option<String>,
    created_at: i64,
    amount: u64,
    slate: VersionedSlate,
    /// Canonical JSON of the incoming S1 for receive records. `slate` is
    /// replaced with S2 when the receive succeeds, so it cannot prove that a
    /// repeated request carries the same transaction; this fingerprint can.
    #[serde(default)]
    request: Option<serde_json::Value>,
    /// `None` in records written before the journal existed; `resolved_state`
    /// reconstructs the only plausible value from `kind`.
    #[serde(default)]
    state: Option<TxState>,
    /// Publish attempts that ended in a node error. A failed publish is not a
    /// refusal: the transaction stays signed and retryable.
    #[serde(default)]
    attempts: u32,
    #[serde(default)]
    last_error: Option<String>,
}

/// A repeated receive must carry the same S1 that produced the stored response.
/// Records written before the fingerprint existed have `None`: refusing the
/// repeat is safer than reusing an S2 for a transaction we cannot identify.
fn check_receive_repeat(existing: &PendingSlate, request: &serde_json::Value) -> Result<()> {
    let expected = existing.request.as_ref().ok_or_else(|| {
        FlintError::Invalid("журнал receive без отпечатка исходного S1: повтор запрещён".into())
    })?;
    if expected != request {
        return Err(FlintError::Invalid(
            "повтор receive с тем же slate id, но другим содержимым отклонён".into(),
        ));
    }
    Ok(())
}

impl PendingSlate {
    fn resolved_state(&self) -> TxState {
        self.state.unwrap_or(if self.kind == "send" {
            TxState::Locked
        } else {
            TxState::ResponseReceived
        })
    }
}

fn mark_finalize_failed(pending: &mut PendingSlate, error: String) {
    // Only a successful finalize_tx return may create FinalizedNotPosted.
    pending.state = Some(TxState::FailedRecoverable);
    pending.last_error = Some(error);
}

fn mark_receive_failed(pending: &mut PendingSlate, error: String) {
    pending.state = Some(if crate::error::receive_tx_already_mutated(&error) {
        TxState::ResponseLost
    } else {
        TxState::ReceiveIntent
    });
    pending.attempts += 1;
    pending.last_error = Some(error);
}

/// Durable transaction state, persisted next to the slate.
///
/// The point is that a crash at any boundary leaves a record that says exactly
/// how far the transaction got — so the next start can finish it instead of
/// guessing (or signing twice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxState {
    /// Slate built, nothing locked yet.
    Prepared,
    /// Inputs are locked in the wallet.
    Locked,
    /// Incoming slate accepted for processing; intent is durable before
    /// `receive_tx`.
    ReceiveIntent,
    /// `receive_tx` may have mutated the wallet but the response was not
    /// persisted. Never re-sign/receive automatically.
    ResponseLost,
    /// Incoming slate answered: our output is locked, response handed over.
    ResponseReceived,
    /// `finalize_tx` was started. A crash here leaves the signature in doubt, so
    /// the record must never be re-signed blindly — only probed by publishing.
    Finalizing,
    /// Signed by us, never accepted by a node. Retry the broadcast, never re-sign.
    FinalizedNotPosted,
    /// Handed to the node; confirmation is the chain's business now.
    Posted,
    /// Lock and cancel both failed. The record is kept as evidence for
    /// reconciliation against Grin; starting a new send is not safe yet.
    FailedRecoverable,
    /// Terminal, archived: the chain confirmed the transaction.
    Confirmed,
    /// Terminal, archived: the transaction was cancelled and inputs released.
    Cancelled,
}

impl TxState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TxState::Prepared => "prepared",
            TxState::Locked => "locked",
            TxState::ReceiveIntent => "receive_intent",
            TxState::ResponseLost => "response_lost",
            TxState::ResponseReceived => "response_received",
            TxState::Finalizing => "finalizing",
            TxState::FinalizedNotPosted => "finalized_not_posted",
            TxState::Posted => "posted",
            TxState::FailedRecoverable => "failed_recoverable",
            TxState::Confirmed => "confirmed",
            TxState::Cancelled => "cancelled",
        }
    }
}

/// Slate ids are canonical UUIDs (`Uuid::to_string`).
///
/// `Uuid::parse_str` also accepts braced, urn and hyphen-less spellings; those
/// would widen the set of filenames we are willing to touch, so only the exact
/// spelling Grin itself writes is accepted.
/// Does the confirmed chain entry match what the journal recorded?
fn amount_matches(kind: &str, expected: u64, credited: u64, debited: u64, fee: u64) -> bool {
    if kind == "receive" {
        credited == expected
    } else {
        debited.saturating_sub(credited).saturating_sub(fee) == expected
    }
}

/// Validate and normalize a profile before a runtime stops the old wallet.
/// `None`, an empty value and `default` all select the legacy default directory.
pub fn normalize_wallet_name(name: Option<String>) -> Result<Option<String>> {
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && n != "default");
    if let Some(name) = &name {
        let valid = name.len() <= 32
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Err(FlintError::Invalid(
                "имя кошелька: только латиница, цифры, - и _, до 32 символов".into(),
            ));
        }
    }
    Ok(name)
}

/// Public wrapper so the RPC CLI path uses the same password policy as the
/// embedded create/restore/change paths.
pub fn validate_password(password: &str) -> Result<()> {
    validate_password_strength(password)
}

/// Password policy: a mitigation for the upstream PBKDF2-100 KDF. Length 12 is
/// mandatory; 3 character classes are required below 16 characters; the most
/// common weak substrings are rejected outright.
fn validate_password_strength(password: &str) -> Result<()> {
    let pw = password.trim();
    if pw.chars().count() < 12 {
        return Err(FlintError::Invalid("пароль короче 12 символов".into()));
    }
    let lower = pw.to_lowercase();
    if lower.contains("password") || lower.contains("qwerty") || lower.contains("123456") {
        return Err(FlintError::Invalid(
            "пароль содержит простое слово (password/qwerty/123456) — выберите другой".into(),
        ));
    }
    let classes = [
        pw.chars().any(|c| c.is_ascii_lowercase()),
        pw.chars().any(|c| c.is_ascii_uppercase()),
        pw.chars().any(|c| c.is_ascii_digit()),
        pw.chars().any(|c| !c.is_ascii_alphanumeric()),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if pw.chars().count() < 16 && classes < 3 {
        return Err(FlintError::Invalid(
            "пароль слабый: короче 16 символов требует минимум 3 класса (строчные, \
             заглавные, цифры, символы)"
                .into(),
        ));
    }
    Ok(())
}

fn parse_slate_id(id: &str) -> Result<Uuid> {
    let id = id.trim();
    let uuid = Uuid::parse_str(id)
        .map_err(|_| FlintError::Invalid("некорректный идентификатор slate".into()))?;
    if uuid.to_string() != id {
        return Err(FlintError::Invalid(
            "некорректный идентификатор slate: ожидается UUID".into(),
        ));
    }
    Ok(uuid)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingView {
    pub slate_id: String,
    pub kind: String,
    pub amount: String,
    pub destination: Option<String>,
    pub created_at: i64,
    /// `prepared` | `locked` | `response_received` | `finalizing` |
    /// `finalized_not_posted` | `posted` | `failed_recoverable`
    pub state: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// How long an unfinished slate may sit before the UI warns about it. There is
/// no protocol TTL on a pending slate; this is an age threshold, not expiry.
pub const PENDING_STALE_AFTER_MS: i64 = 30 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingSeverity {
    Warning,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PendingAlert {
    pub slate_id: String,
    pub severity: PendingSeverity,
    pub message: String,
}

/// Alerts for slates that need a human: a failed/recoverable step is always
/// critical, an ordinary in-flight slate becomes a warning after 30 minutes.
pub fn pending_alerts(items: &[PendingView], now_ms: i64) -> Vec<PendingAlert> {
    let mut alerts = Vec::new();
    for item in items {
        let age_min = (now_ms - item.created_at).max(0) / 60_000;
        let critical = match item.state.as_str() {
            "finalizing" => Some("подпись начата, публикация не завершена; нажмите «Опубликовать»"),
            "finalized_not_posted" => {
                Some("сделка подписана, но не опубликована; повторите публикацию")
            }
            "failed_recoverable" => {
                Some("требует ручного восстановления; не начинайте новую сделку")
            }
            "response_lost" => {
                Some("ответ мог быть потерян; повторный приём запрещён, проверьте журнал")
            }
            _ => None,
        };
        if let Some(reason) = critical {
            alerts.push(PendingAlert {
                slate_id: item.slate_id.clone(),
                severity: PendingSeverity::Critical,
                message: format!("{} на {} nano: {reason}", item.kind, item.amount),
            });
            continue;
        }
        let unfinished = matches!(
            item.state.as_str(),
            "prepared" | "locked" | "receive_intent" | "response_received"
        );
        if unfinished && age_min >= 30 {
            alerts.push(PendingAlert {
                slate_id: item.slate_id.clone(),
                severity: PendingSeverity::Warning,
                message: format!(
                    "{} на {} nano не завершена {age_min} мин; проверьте очередь сделок",
                    item.kind, item.amount
                ),
            });
        }
    }
    alerts.sort_by_key(|alert| match alert.severity {
        PendingSeverity::Critical => 0,
        PendingSeverity::Warning => 1,
    });
    alerts
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn is_send<T: Send>() {}
    is_send::<WalletService>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppPaths;
    use grin_wallet_libwallet::Slate;

    fn fresh_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("flint-pending-{tag}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn pending(kind: &str, amount: u64) -> PendingSlate {
        PendingSlate {
            kind: kind.to_string(),
            destination: Some("tgrin1qexample".into()),
            created_at: 1,
            amount,
            slate: VersionedSlate::into_version(Slate::blank(2, false), SlateVersion::V4)
                .expect("blank slate is versionable"),
            request: None,
            state: Some(TxState::Locked),
            attempts: 0,
            last_error: None,
        }
    }

    fn pending_in(kind: &str, state: TxState) -> PendingSlate {
        let mut p = pending(kind, 1);
        p.state = Some(state);
        p
    }

    #[test]
    fn legacy_records_get_a_state_from_their_kind() {
        let root = fresh_root("legacy");
        let service = WalletService::open_at(AppPaths::at(&root));

        let mut send = pending("send", 3);
        send.state = None;
        service
            .save_pending(&Uuid::new_v4().to_string(), &send)
            .expect("save");

        let mut receive = pending("receive", 4);
        receive.state = None;
        service
            .save_pending(&Uuid::new_v4().to_string(), &receive)
            .expect("save");

        let mut states: Vec<String> = service
            .list_pending()
            .expect("list")
            .into_iter()
            .map(|p| p.state)
            .collect();
        states.sort();
        assert_eq!(states, vec!["locked", "response_received"]);
    }

    #[test]
    fn signed_transactions_cannot_be_cancelled_or_blank_retried() {
        let root = fresh_root("signed");
        let mut service = WalletService::open_at(AppPaths::at(&root));

        let signed = Uuid::new_v4().to_string();
        service
            .save_pending(&signed, &pending_in("send", TxState::FinalizedNotPosted))
            .expect("save");
        // Refused before touching the wallet, which is locked in this test.
        let err = service.cancel_pending(&signed).expect_err("must refuse");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert!(service.load_pending(&signed).is_ok(), "record must survive");

        let prepared = Uuid::new_v4().to_string();
        service
            .save_pending(&prepared, &pending_in("send", TxState::Prepared))
            .expect("save");
        let err = service
            .retry_post(&prepared)
            .expect_err("nothing to publish");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
    }

    #[test]
    fn finalizing_records_fail_closed_without_publish() {
        let root = fresh_root("finalizing");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::Finalizing))
            .expect("save");

        // A finalizing record is ambiguous: fail closed before parsing the
        // response or calling finalize_tx again.
        let err = service
            .finalize_send(&id, "BEGINSLATEPACK. payload ENDSLATEPACK.")
            .expect_err("ambiguous finalize must fail closed");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert_eq!(
            service.load_pending(&id).expect("record").resolved_state(),
            TxState::FailedRecoverable,
            "ambiguous signature must require explicit manual recovery"
        );
    }

    #[test]
    fn receive_intent_is_not_mistaken_for_a_saved_response() {
        let root = fresh_root("receive-intent");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("receive", TxState::ReceiveIntent))
            .expect("save intent");
        assert!(
            service.resume_receive(&id).expect("read intent").is_none(),
            "an unsent response must not be armored from the incoming slate"
        );
        assert_eq!(
            service.load_pending(&id).expect("record").resolved_state(),
            TxState::ReceiveIntent
        );
    }

    #[test]
    fn receive_errors_preserve_retry_unless_mutation_is_proven() {
        for error in ["timeout", "invalid slate", "DB Not Found"] {
            let mut record = pending_in("receive", TxState::ReceiveIntent);
            mark_receive_failed(&mut record, error.to_string());
            assert_eq!(record.resolved_state(), TxState::ReceiveIntent, "{error}");
            assert_eq!(record.attempts, 1);
            assert_eq!(record.last_error.as_deref(), Some(error));
        }
        for error in [
            "TransactionAlreadyReceived",
            "ReceiveRecoveryUnavailable: slate-id",
            "ReceiveRecoveryIntegrity: mismatch",
        ] {
            let mut record = pending_in("receive", TxState::ReceiveIntent);
            mark_receive_failed(&mut record, error.into());
            assert_eq!(record.resolved_state(), TxState::ResponseLost, "{error}");
        }
    }

    #[test]
    fn local_node_secret_never_goes_to_an_external_node() {
        let root = fresh_root("node-secret");
        let paths = AppPaths::at(&root);
        std::fs::create_dir_all(paths.node_dir("testnet")).expect("node dir");
        std::fs::write(paths.node_dir("testnet").join(".api_secret"), "local-only")
            .expect("write secret");
        let mut service = WalletService::open_at(paths);

        service.settings.node_url = "https://node.example.com:13413".into();
        assert!(
            service.node_api_secret().is_none(),
            "a remote node must not receive the managed node's local secret"
        );

        service.settings.node_api_secret = Some("explicit-remote".into());
        assert_eq!(
            service.node_api_secret().as_deref(),
            Some("explicit-remote")
        );

        service.settings.node_url = "http://127.0.0.1:13413".into();
        service.settings.node_api_secret = None;
        assert_eq!(service.node_api_secret().as_deref(), Some("local-only"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn receive_request_fingerprint_survives_restart() {
        let root = fresh_root("receive-restart");
        let id = Uuid::new_v4().to_string();
        let original = serde_json::json!({"id": id, "amount": 1});
        let changed = serde_json::json!({"id": id, "amount": 2});

        let service = WalletService::open_at(AppPaths::at(&root));
        let mut record = pending_in("receive", TxState::ResponseReceived);
        record.request = Some(original.clone());
        service.save_pending(&id, &record).expect("save");
        drop(service);

        let reopened = WalletService::open_at(AppPaths::at(&root));
        let loaded = reopened.load_pending(&id).expect("record survives restart");
        assert_eq!(loaded.request, Some(original.clone()));
        assert!(check_receive_repeat(&loaded, &original).is_ok());
        assert!(
            check_receive_repeat(&loaded, &changed).is_err(),
            "a changed S1 must stay rejected after restart"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_receive_must_match_the_original_request() {
        let original = serde_json::json!({"id": "same", "amount": 1});
        let changed = serde_json::json!({"id": "same", "amount": 2});
        let mut record = pending("receive", 1);
        assert!(
            check_receive_repeat(&record, &original).is_err(),
            "legacy record without a fingerprint must refuse replay"
        );
        record.request = Some(original.clone());
        assert!(check_receive_repeat(&record, &original).is_ok());
        assert!(
            check_receive_repeat(&record, &changed).is_err(),
            "same slate id with other contents is not a repeat"
        );
    }

    #[test]
    fn resume_receive_does_not_hide_journal_errors() {
        let root = fresh_root("receive-journal-error");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("receive", TxState::ResponseReceived))
            .expect("save response");
        crate::win_acl::inject_acl_failures(1);
        let error = service
            .resume_receive(&id)
            .expect_err("ACL error must escape");
        crate::win_acl::inject_acl_failures(0);
        assert!(error.to_string().contains("injected ACL failure"));
    }

    #[test]
    fn failed_recoverable_cannot_be_finalized() {
        let root = fresh_root("failed-finalize");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::FailedRecoverable))
            .expect("save");
        let err = service
            .finalize_send(&id, "BEGINSLATEPACK. payload ENDSLATEPACK.")
            .expect_err("ambiguous record must not be signed");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert_eq!(
            service.load_pending(&id).expect("record").resolved_state(),
            TxState::FailedRecoverable
        );
    }

    #[test]
    fn no_finalize_error_text_is_treated_as_a_signature() {
        for error in ["DB Not Found", "Slate id", "timeout", "invalid slate"] {
            let mut record = pending_in("send", TxState::Finalizing);
            mark_finalize_failed(&mut record, error.to_string());
            assert_eq!(record.resolved_state(), TxState::FailedRecoverable);
            assert_eq!(record.last_error.as_deref(), Some(error));
        }
    }

    #[test]
    fn reconcile_is_a_noop_while_locked() {
        let root = fresh_root("reconcile");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::Finalizing))
            .expect("save");

        assert_eq!(service.reconcile_pending().expect("reconcile"), 0);
        assert_eq!(
            service.load_pending(&id).expect("record").resolved_state(),
            TxState::Finalizing
        );
    }

    #[test]
    fn pending_directory_read_error_is_not_an_empty_queue() {
        let root = fresh_root("pending-dir-error");
        let service = WalletService::open_at(AppPaths::at(&root));
        let dir = service.paths.pending_dir("testnet");
        fs::create_dir_all(dir.parent().expect("parent")).expect("parent");
        // A file where the journal directory is expected must be an error, not
        // an empty pending list.
        fs::write(&dir, "not a directory").expect("blocking file");
        assert!(service.pending_problems().is_err());
        assert!(service.list_pending().is_err());
    }

    #[test]
    fn broken_pending_files_are_reported_not_dropped() {
        let root = fresh_root("broken");
        let service = WalletService::open_at(AppPaths::at(&root));
        let dir = service.paths.pending_dir("testnet");
        fs::create_dir_all(&dir).expect("pending dir");
        let broken = dir.join(format!("{}.json", Uuid::new_v4()));
        fs::write(&broken, "{ not json").expect("write broken");

        let problems = service.pending_problems().expect("problems");
        assert_eq!(problems.len(), 1, "got {problems:?}");
        assert!(broken.exists(), "the file must stay on disk");
        assert!(service.list_pending().expect("list").is_empty());
    }

    /// Pin the service to a closed port: tests must not depend on whatever node
    /// happens to be running on the default one.
    fn offline(service: &mut WalletService) {
        service
            .update_settings(
                None,
                Some("http://127.0.0.1:1".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("pin node url");
    }

    fn wallet_init_status(service: &WalletService) -> WalletInitStatus {
        let owner = service.owner().expect("unlocked wallet");
        let mut guard = owner.wallet_inst.lock();
        let w = guard
            .lc_provider()
            .expect("lifecycle provider")
            .wallet_inst()
            .expect("wallet instance");
        w.init_status().expect("init status")
    }

    #[test]
    fn sending_is_blocked_until_the_node_reports_ready() {
        let root = fresh_root("gate");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        offline(&mut service);
        let err = service
            .prepare_send("tgrin1qexample", 1)
            .expect_err("no node, no send");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert!(
            err.to_string().contains("заблокирована"),
            "the message must say why: {err}"
        );
    }

    #[test]
    fn restore_requires_an_empty_wallet_directory() {
        let root = fresh_root("restore-dirty");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let home = service.paths().wallet_home("testnet");
        fs::create_dir_all(&home).expect("home");
        fs::write(home.join("leftover.txt"), "not a wallet").expect("leftover");

        let err = service
            .restore_wallet(
                "one two three four five six seven eight nine ten eleven twelve",
                "password-123",
            )
            .expect_err("must refuse");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert!(home.join("leftover.txt").exists(), "nothing may be touched");
    }

    #[test]
    fn restore_rejects_an_invalid_phrase_without_leaving_files() {
        let root = fresh_root("restore-bad");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let home = service.paths().wallet_home("testnet");

        let err = service
            .restore_wallet(
                "one two three four five six seven eight nine ten eleven twelve",
                "password-123",
            )
            .expect_err("must refuse");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert!(!service.initialized(), "no seed may be written");
        assert!(
            !home.exists(),
            "a refused import must not leave the directory"
        );
    }

    #[test]
    fn restore_recovers_the_same_address_as_the_original_wallet() {
        let source = fresh_root("restore-src");
        let target = fresh_root("restore-dst");
        let mut original = WalletService::open_at(AppPaths::at(&source));
        let created = original
            .create_wallet("Flint-Strong-1234!")
            .expect("create");

        let mut restored = WalletService::open_at(AppPaths::at(&target));
        let back = restored
            .restore_wallet(&created.mnemonic, "Another-Pass-123!")
            .expect("restore");
        assert_eq!(back.address, created.address);
        assert!(back.needs_scan);
        assert!(
            restored.pending_scan(),
            "a restored wallet must remember that the chain is unscanned"
        );
    }

    #[test]
    fn new_and_restored_wallets_skip_upstream_full_scan() {
        let source = fresh_root("init-status-src");
        let target = fresh_root("init-status-dst");
        let mut original = WalletService::open_at(AppPaths::at(&source));
        let created = original
            .create_wallet("Flint-Strong-1234!")
            .expect("create");
        assert!(
            matches!(
                wallet_init_status(&original),
                WalletInitStatus::InitNoScanning
            ),
            "a freshly created wallet must not trigger a full scan in summary"
        );

        let mut restored = WalletService::open_at(AppPaths::at(&target));
        restored
            .restore_wallet(&created.mnemonic, "Another-Pass-123!")
            .expect("restore");
        assert!(
            matches!(
                wallet_init_status(&restored),
                WalletInitStatus::InitNoScanning
            ),
            "Flint owns the explicit scan; a restored wallet must not full-scan in summary"
        );
        assert!(
            restored.pending_scan(),
            "the restored wallet still needs the explicit scan"
        );
    }

    #[test]
    fn scan_is_blocked_without_a_ready_node() {
        let root = fresh_root("scan-gate");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        offline(&mut service);
        let err = service.scan_wallet(None).expect_err("must refuse");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        assert!(err.to_string().contains("скан запрещён"));
        assert_eq!(
            service.scan_state(),
            ScanState::NotStarted,
            "a refused scan must not look started"
        );
    }

    #[test]
    fn a_partial_scan_cannot_finish_a_restore() {
        let root = fresh_root("scan-partial-restore");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        offline(&mut service);
        service.require_scan().expect("restore flag");
        let err = match service.prepare_scan(Some(10_000)) {
            Ok(_) => panic!("partial scan must not finish a restore"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("полный"),
            "must demand a full scan: {err}"
        );
        assert_eq!(service.scan_state(), ScanState::NotStarted);
        assert!(service.pending_scan());
    }

    #[test]
    fn finish_scan_applies_cooperative_outcomes() {
        let root = fresh_root("scan-finish");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        offline(&mut service);
        service.require_scan().expect("restore flag");

        service.begin_scan().expect("start");
        let err = service
            .finish_scan(Ok(ScanRunOutcome::Cancelled))
            .expect_err("cancelled job must report it");
        assert!(matches!(err, FlintError::ScanCancelled));
        assert_eq!(service.scan_state(), ScanState::Failed);
        assert_eq!(service.scan_error(), Some(SCAN_CANCELLED));
        assert!(
            service.pending_scan(),
            "a cancelled restore scan keeps the balance unknown"
        );

        service.begin_scan().expect("retry");
        service
            .finish_scan(Ok(ScanRunOutcome::Completed))
            .expect("completed job");
        assert_eq!(service.scan_state(), ScanState::Completed);
        assert!(!service.pending_scan());

        service.begin_scan().expect("rescan");
        let err = service
            .finish_scan(Err(FlintError::Wallet("нет узла".into())))
            .expect_err("failed job must report it");
        assert!(err.to_string().contains("нет узла"));
        assert_eq!(service.scan_state(), ScanState::Failed);
        assert_eq!(service.scan_error(), Some("ошибка кошелька: нет узла"));
        assert!(
            !service.pending_scan(),
            "a failed rescan must not pretend a known balance is unknown"
        );
    }

    #[test]
    fn a_late_batch_cancel_does_not_overwrite_completed() {
        let root = fresh_root("scan-late-cancel");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        service.begin_scan().expect("start");
        service.complete_scan().expect("finish");

        let err = service
            .finish_scan(Ok(ScanRunOutcome::Cancelled))
            .expect_err("completed scan cannot be cancelled");
        assert!(err.to_string().contains("скан не идёт"));
        assert_eq!(service.scan_state(), ScanState::Completed);
        assert!(service.scan_error().is_none());
    }

    #[test]
    fn node_snapshot_is_one_view_of_the_settings() {
        let root = fresh_root("node-snapshot");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        service
            .update_settings(
                Some("testnet".to_string()),
                Some("http://127.0.0.1:9999".to_string()),
                None,
                None,
                None,
                None,
                Some(3),
            )
            .expect("update settings");

        let snapshot = service.node_snapshot();
        assert_eq!(snapshot.network, "testnet");
        assert_eq!(snapshot.url, "http://127.0.0.1:9999");
        assert_eq!(snapshot.min_confirmations, 3);
        assert!(
            snapshot.secret.is_none(),
            "no managed node secret was configured in this temp root"
        );
    }

    #[test]
    fn scan_state_is_explicit_and_survives_restarts() {
        let root = fresh_root("scan-state");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(service.scan_state(), ScanState::NotStarted);

        service.require_scan().expect("restore flag");
        assert!(service.pending_scan(), "a restore needs a scan");
        service.begin_scan().expect("start");
        assert_eq!(service.scan_state(), ScanState::Running);
        assert!(
            service.begin_scan().is_err(),
            "a second scan must not start over the first"
        );

        // A persisted `running` cannot outlive the process that owned it: a
        // restart must report an interrupted scan, not a completed one, and
        // must not clear the unknown-balance flag.
        let restarted = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(restarted.scan_state(), ScanState::Failed);
        assert!(
            restarted.pending_scan(),
            "an interrupted scan leaves the balance unknown"
        );
        assert!(restarted.scan_error().is_some());

        let mut restarted = restarted;
        restarted.begin_scan().expect("retry");
        restarted.complete_scan().expect("finish");
        assert_eq!(restarted.scan_state(), ScanState::Completed);
        assert!(!restarted.pending_scan());
        assert!(restarted.scan_error().is_none());

        let after = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(after.scan_state(), ScanState::Completed);
        assert!(
            !after.pending_scan(),
            "only a completed scan clears the flag"
        );
    }

    #[test]
    fn a_failed_scan_keeps_the_balance_unknown() {
        let root = fresh_root("scan-failed");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        service.require_scan().expect("restore flag");
        service.begin_scan().expect("start");
        service.fail_scan("нода недоступна").expect("fail");

        assert_eq!(service.scan_state(), ScanState::Failed);
        assert!(service.pending_scan(), "a failure is not an empty chain");
        assert_eq!(service.scan_error(), Some("нода недоступна"));

        service.begin_scan().expect("retry");
        service.complete_scan().expect("finish");
        assert!(!service.pending_scan());
    }

    #[test]
    fn cancelling_a_running_scan_marks_it_failed_and_keeps_pending_scan() {
        let root = fresh_root("scan-cancel");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        service.require_scan().expect("restore flag");
        service.begin_scan().expect("start");

        service
            .cancel_scan("скан отменён пользователем")
            .expect("cancel");
        assert_eq!(service.scan_state(), ScanState::Failed);
        assert_eq!(service.scan_error(), Some("скан отменён пользователем"));
        assert!(
            service.pending_scan(),
            "a cancelled restore scan leaves the balance unknown"
        );

        // A second cancel must not invent a successful cancellation.
        let err = service.cancel_scan("ещё раз").expect_err("must refuse");
        assert!(err.to_string().contains("скан не идёт"));
        assert_eq!(service.scan_state(), ScanState::Failed);

        // The failure survives a restart; the next scan can still start.
        let mut restarted = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(restarted.scan_state(), ScanState::Failed);
        assert_eq!(restarted.scan_error(), Some("скан отменён пользователем"));
        assert!(restarted.pending_scan());
        restarted.begin_scan().expect("retry after cancel");
    }

    #[test]
    fn cancelling_a_completed_scan_does_not_overwrite_it() {
        let root = fresh_root("scan-cancel-completed");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        service.require_scan().expect("restore flag");
        service.begin_scan().expect("start");
        service.complete_scan().expect("finish");
        assert!(!service.pending_scan());

        let err = service
            .cancel_scan("скан отменён пользователем")
            .expect_err("a completed scan cannot be cancelled");
        assert!(err.to_string().contains("скан не идёт"));
        assert_eq!(service.scan_state(), ScanState::Completed);
        assert!(service.scan_error().is_none());
        assert!(!service.pending_scan());

        let after = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(after.scan_state(), ScanState::Completed);
    }

    #[test]
    fn failed_publish_attempts_are_visible_in_the_journal() {
        let root = fresh_root("attempts");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        let mut record = pending_in("send", TxState::FinalizedNotPosted);
        record.attempts = 3;
        record.last_error = Some("timeout".into());
        service.save_pending(&id, &record).expect("save");

        let items = service.list_pending().expect("list");
        assert_eq!(items[0].attempts, 3);
        assert_eq!(items[0].last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn a_failed_pending_write_leaves_the_previous_record_intact() {
        let root = fresh_root("pending-write-fail");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        let first = pending("send", 7);
        service.save_pending(&id, &first).expect("initial save");

        crate::win_acl::inject_acl_failures(1);
        let err = service.load_pending(&id).unwrap_err();
        crate::win_acl::inject_acl_failures(0);
        assert!(err.to_string().contains("injected ACL failure"), "{err}");

        // A pre-write ACL failure must leave the durable record untouched. The
        // fault is thread-local, so parallel tests keep using the real ACL path.
        crate::win_acl::inject_acl_failures(1);
        let second = pending("send", 999);
        let err = service.save_pending(&id, &second).unwrap_err();
        crate::win_acl::inject_acl_failures(0);
        assert!(err.to_string().contains("injected ACL failure"), "{err}");
        let loaded = service
            .load_pending(&id)
            .expect("old record still readable");
        assert_eq!(loaded.amount, first.amount);
        assert_eq!(
            service.pending_problems().expect("problems"),
            Vec::<String>::new()
        );

        service.save_pending(&id, &second).expect("write recovers");
        assert_eq!(service.load_pending(&id).unwrap().amount, 999);
    }

    #[test]
    fn receives_are_reconciled_only_with_an_unlocked_wallet() {
        let root = fresh_root("recv-reconcile");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("receive", TxState::ResponseReceived))
            .expect("save");

        assert_eq!(service.reconcile_posted().expect("noop"), 0);
        assert!(
            service.load_pending(&id).is_ok(),
            "a receive record must survive while the wallet is locked"
        );
    }

    #[test]
    fn wallet_names_are_validated_and_persisted() {
        let root = fresh_root("wallet-name");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        assert!(service.set_wallet_name(Some("../evil".into())).is_err());
        service
            .set_wallet_name(Some("alice".into()))
            .expect("valid name");
        assert_eq!(service.wallet_name(), Some("alice"));
        let reloaded = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(
            reloaded.wallet_name(),
            Some("alice"),
            "profile must persist"
        );
    }

    #[test]
    fn address_index_rotates_and_persists() {
        let root = fresh_root("rotate");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(service.address_index(), 0);
        assert_eq!(service.next_address_index().expect("rotate"), 1);
        assert_eq!(service.address_index(), 1);

        let reloaded = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(reloaded.address_index(), 1, "index must survive restart");
        let indices = reloaded.decrypt_indices();
        assert_eq!(indices.first(), Some(&0), "old addresses stay decryptable");
        assert_eq!(indices.last(), Some(&1));
    }

    #[test]
    fn address_index_is_isolated_per_profile() {
        let root = fresh_root("rotate-profiles");
        let mut default = WalletService::open_at(AppPaths::at(&root));
        default.next_address_index().expect("rotate default");

        let mut alice = WalletService::open_at(AppPaths::at(&root));
        alice.set_wallet_name(Some("alice".into())).expect("switch");
        assert_eq!(alice.address_index(), 0, "new profile starts at index 0");
        alice.next_address_index().expect("rotate alice");
        assert_eq!(alice.address_index(), 1);

        alice.set_wallet_name(None).expect("back to default");
        let default_again = WalletService::open_at(AppPaths::at(&root));
        assert_eq!(
            default_again.address_index(),
            1,
            "default keeps its own index"
        );

        let mut alice_again = WalletService::open_at(AppPaths::at(&root));
        alice_again
            .set_wallet_name(Some("alice".into()))
            .expect("switch");
        assert_eq!(alice_again.address_index(), 1, "alice keeps her own index");
    }

    #[test]
    fn chain_amount_must_match_the_journal() {
        // Receiving: credited must be exactly the recorded amount.
        assert!(amount_matches("receive", 100, 100, 0, 0));
        assert!(!amount_matches("receive", 100, 99, 0, 0));
        // Sending: debited - change - fee is the amount that left the wallet.
        assert!(amount_matches("send", 100, 40, 163, 23));
        assert!(!amount_matches("send", 100, 40, 164, 23));
        assert!(!amount_matches("send", 100, 0, 99, 0));
    }

    #[test]
    fn posted_records_stay_until_the_chain_confirms_them() {
        let root = fresh_root("posted");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::Posted))
            .expect("save");

        // Without an unlocked wallet confirmation cannot be checked — and must
        // not be assumed: the record stays and is visible.
        assert_eq!(service.reconcile_posted().expect("reconcile"), 0);
        let items = service.list_pending().expect("list");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, "posted");
    }

    #[test]
    fn failed_recoverable_records_stay_visible() {
        let root = fresh_root("recoverable");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::FailedRecoverable))
            .expect("save");

        let items = service.list_pending().expect("list");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, "failed_recoverable");
    }

    #[test]
    fn finalizing_an_already_signed_transaction_resumes_at_the_broadcast() {
        let root = fresh_root("resume");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::FinalizedNotPosted))
            .expect("save");

        // The resume path reaches the wallet and stops at the lock, instead of
        // trying to finalize a slate that was already signed.
        let err = service
            .finalize_send(&id, "BEGINSLATEPACK. payload ENDSLATEPACK.")
            .expect_err("wallet is locked");
        assert!(matches!(err, FlintError::Locked), "got {err:?}");
    }

    #[test]
    fn pending_ids_must_be_canonical_uuids() {
        let root = fresh_root("ids");
        let service = WalletService::open_at(AppPaths::at(&root));
        let good = Uuid::new_v4();
        let upper = good.to_string().to_uppercase();
        let simple = good.simple().to_string();

        for bad in [
            "../../settings",
            "../wallet_data/wallet",
            "..",
            "",
            "abc",
            "2f3f6a3e-8d8f-4e0c-9d4b-6b3b8f0f8f0",
            upper.as_str(),
            simple.as_str(),
        ] {
            assert!(service.pending_path(bad).is_err(), "accepted {bad:?}");
        }
        assert!(service.pending_path(&good.to_string()).is_ok());
    }

    #[test]
    fn password_policy_rejects_weak_and_accepts_strong() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("lowercaseonly").is_err());
        assert!(validate_password_strength("correct horse battery").is_ok());
        assert!(validate_password_strength("Password-1234!").is_err());
        assert!(validate_password_strength("GoodPass-1234!").is_ok());
        assert!(validate_password_strength("lower-UPPER-1").is_ok());
    }

    #[test]
    fn finished_records_are_archived_not_deleted() {
        let root = fresh_root("archive");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending_in("send", TxState::Locked))
            .expect("save");
        service
            .archive_pending(&id, TxState::Cancelled)
            .expect("archive");
        assert!(
            matches!(service.load_pending(&id), Err(FlintError::UnknownSlate(_))),
            "live journal must no longer show the record"
        );
        let archive = service
            .pending_path(&id)
            .unwrap()
            .parent()
            .unwrap()
            .join("archive")
            .join(format!("{id}.json"));
        let raw = fs::read_to_string(&archive).expect("archived file");
        assert!(
            raw.contains("\"cancelled\""),
            "state must be persisted: {raw}"
        );
    }

    #[test]
    fn pending_paths_stay_inside_the_pending_dir() {
        let root = fresh_root("paths");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        let path = service.pending_path(&id).expect("canonical uuid");
        let dir = service.paths.pending_dir("testnet");
        assert_eq!(path.parent().expect("parent"), dir);
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{id}.json")
        );
    }

    #[test]
    fn pending_access_rejects_traversal_before_touching_disk() {
        let root = fresh_root("traversal");
        let service = WalletService::open_at(AppPaths::at(&root));
        // Without validation this id becomes <root>/settings.json via `../..`.
        let settings = root.join("settings.json");
        fs::write(&settings, "{}").expect("fixture");

        assert!(matches!(
            service.load_pending("../../settings"),
            Err(FlintError::Invalid(_))
        ));
        assert!(matches!(
            service.remove_pending("../../settings"),
            Err(FlintError::Invalid(_))
        ));
        assert!(settings.exists(), "traversal must not reach the file");
    }

    #[test]
    fn pending_survives_an_atomic_round_trip() {
        let root = fresh_root("roundtrip");
        let service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();

        service
            .save_pending(&id, &pending("send", 7))
            .expect("save");
        assert_eq!(service.load_pending(&id).expect("load").amount, 7);

        // Replacing an existing record must not leave temp files behind.
        service
            .save_pending(&id, &pending("send", 9))
            .expect("overwrite");
        assert_eq!(service.load_pending(&id).expect("load").amount, 9);

        let leftovers: Vec<String> = fs::read_dir(service.paths.pending_dir("testnet"))
            .expect("pending dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");

        service.remove_pending(&id).expect("remove");
        assert!(matches!(
            service.load_pending(&id),
            Err(FlintError::UnknownSlate(_))
        ));
        service.remove_pending(&id).expect("remove is idempotent");
    }

    #[test]
    fn cancel_refuses_signed_or_ambiguous_send_states() {
        let root = fresh_root("cancel-states");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        for state in [
            TxState::Finalizing,
            TxState::FinalizedNotPosted,
            TxState::Posted,
            TxState::FailedRecoverable,
        ] {
            let id = Uuid::new_v4().to_string();
            service
                .save_pending(&id, &pending_in("send", state))
                .expect("save");
            let error = service.cancel_pending(&id).expect_err("must refuse");
            assert!(matches!(error, FlintError::Invalid(_)), "got {error:?}");
            assert!(
                service.load_pending(&id).is_ok(),
                "refused cancel must keep the record"
            );
        }
    }

    #[test]
    fn incoming_slates_cannot_be_cancelled() {
        let root = fresh_root("cancel");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let id = Uuid::new_v4().to_string();
        service
            .save_pending(&id, &pending("receive", 5))
            .expect("save");

        let err = service.cancel_pending(&id).expect_err("must refuse");
        assert!(matches!(err, FlintError::Invalid(_)), "got {err:?}");
        // A refused cancellation must keep the record intact.
        assert!(service.load_pending(&id).is_ok());
    }

    #[test]
    fn remote_plain_http_node_url_is_rejected() {
        assert!(validate_node_url("http://127.0.0.1:3413").is_ok());
        assert!(validate_node_url("http://localhost:3413").is_ok());
        assert!(validate_node_url("https://node.example:3413").is_ok());
        assert!(validate_node_url("http://node.example:3413").is_err());
        assert!(validate_node_url("ftp://node.example").is_err());
    }

    #[test]
    fn service_surfaces_a_settings_backup_warning() {
        let root = fresh_root("settings-warning");
        let paths = AppPaths::at(&root);
        paths.ensure().expect("ensure");
        let settings = crate::config::Settings::default();
        paths.save_settings(&settings).expect("first save");
        paths.save_settings(&settings).expect("backup save");
        fs::write(paths.settings_file(), "{ not json").expect("corrupt settings");

        let service = WalletService::open_at(AppPaths::at(&root));
        let warning = service.settings_view().settings_warning;
        assert!(warning.contains(".corrupt-"), "unexpected: {warning}");
        assert!(warning.contains("json.bak"), "unexpected: {warning}");
    }

    #[test]
    fn node_info_is_not_synced_when_the_node_is_unreachable() {
        let root = fresh_root("node");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        offline(&mut service);
        let info = service.node_info();
        assert!(!info.connected);
        assert!(!info.synced, "an unreachable node is not synchronized");
    }

    /// Minimal owner/foreign JSON-RPC stub: answers get_tip, get_version and
    /// get_status for one node_info call and then closes the listener.
    fn stub_node(chain: &'static str) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("stub bind");
        let port = listener.local_addr().expect("stub addr").port();
        let handle = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().expect("stub accept");
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).expect("stub read");
                let text = String::from_utf8_lossy(&buffer[..read]);
                let body = if text.contains("get_tip") {
                    r#"{"result":{"Ok":{"height":42,"last_block_pushed":"aa","prev_block_to_last":"bb","total_difficulty":1}},"error":null,"id":1}"#.to_string()
                } else if text.contains("get_version") {
                    r#"{"result":{"Ok":{"node_version":"5.5.2","block_header_version":6}},"error":null,"id":1}"#.to_string()
                } else {
                    format!(
                        r#"{{"result":{{"Ok":{{"chain":"{chain}","sync_status":"no_sync","tip":{{"height":42}},"sync_info":{{"current_height":42,"highest_height":42}},"connections":3,"user_agent":"MW/Grin 5.5.2"}}}},"error":null,"id":1}}"#
                    )
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).expect("stub head");
                stream.write_all(body.as_bytes()).expect("stub body");
            }
        });
        (port, handle)
    }

    #[test]
    fn flapping_node_loses_synced_state() {
        let root = fresh_root("flap");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let (port, stub) = stub_node("test");
        service
            .update_settings(
                None,
                Some(format!("http://127.0.0.1:{port}")),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("pin stub node");
        let up = service.node_info();
        assert!(up.connected, "stub node not connected");
        assert!(up.synced, "ready stub node was not synced");
        stub.join().expect("stub");
        let down = service.node_info();
        assert!(!down.connected, "closed node still connected");
        assert!(!down.synced, "flapping node kept a stale synced state");
    }

    #[test]
    fn a_lookalike_chain_is_not_ready() {
        let root = fresh_root("wrong-chain");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        let (port, stub) = stub_node("main");
        service
            .update_settings(
                None,
                Some(format!("http://127.0.0.1:{port}")),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("pin stub node");
        let info = service.node_info();
        assert!(!info.synced, "a mainnet node was accepted for testnet");
        assert_eq!(info.phase, crate::node::NodePhase::Unknown);
        stub.join().expect("stub");
    }

    #[test]
    fn a_node_error_is_not_treated_as_a_synced_chain() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("stub bind");
        let port = listener.local_addr().expect("stub addr").port();
        let stub = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("stub accept");
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let body = r#"{"result":{"Err":"chain db corrupt"},"error":null,"id":1}"#;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).expect("stub head");
            stream.write_all(body.as_bytes()).expect("stub body");
        });

        let root = fresh_root("broken-chain");
        let mut service = WalletService::open_at(AppPaths::at(&root));
        service
            .update_settings(
                None,
                Some(format!("http://127.0.0.1:{port}")),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("pin broken node");
        let info = service.node_info();
        assert!(
            !info.connected,
            "a node with a chain error reported connected"
        );
        assert!(!info.synced, "a broken chain was reported as synced");
        stub.join().expect("stub");
    }

    fn pending_view(state: &str, created_at: i64) -> PendingView {
        PendingView {
            slate_id: "00000000-0000-0000-0000-000000000001".into(),
            kind: "send".into(),
            amount: "100000000".into(),
            destination: Some("tgrin1qexample".into()),
            created_at,
            state: state.into(),
            attempts: 0,
            last_error: None,
        }
    }

    #[test]
    fn pending_alerts_sort_critical_first_and_warn_only_after_30_minutes() {
        let now = 10_000_000i64;
        let items = vec![
            pending_view("locked", now - 1_000),
            pending_view("locked", now - 45 * 60_000),
            pending_view("finalized_not_posted", now - 1_000),
            pending_view("posted", now - 3 * 3600_000),
            pending_view("response_lost", now - 5 * 60_000),
        ];
        let alerts = pending_alerts(&items, now);
        assert_eq!(alerts.len(), 3, "unexpected alerts: {alerts:?}");
        assert_eq!(alerts[0].severity, PendingSeverity::Critical);
        assert!(alerts[0].message.contains("не опубликована"));
        assert_eq!(alerts[1].severity, PendingSeverity::Critical);
        assert!(alerts[1].message.contains("ответ мог быть потерян"));
        assert_eq!(alerts[2].severity, PendingSeverity::Warning);
        assert!(alerts[2].message.contains("45 мин"));
    }

    #[test]
    fn fresh_or_finished_slates_do_not_alert() {
        let now = 20_000_000i64;
        let items = vec![
            pending_view("prepared", now - 29 * 60_000),
            pending_view("posted", now - 10 * 3600_000),
            pending_view("confirmed", now - 10 * 3600_000),
            pending_view("cancelled", now - 10 * 3600_000),
        ];
        assert!(pending_alerts(&items, now).is_empty());
    }
}
