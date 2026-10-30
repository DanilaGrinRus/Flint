//! UI-independent wallet session.
//!
//! This crate owns the shared `WalletService` handle, the managed node, the RPC
//! (`grin-wallet`) backend session, Tor listener and messenger/history state.
//! All methods are blocking on purpose: the Slint shell calls them from
//! `std::thread::spawn`, never from the UI thread.

mod history;
mod messenger;
mod rpc;
mod scan;
mod seed;
mod tor;
mod wallet;

pub use history::PendingHistoryRewrap;
pub use rpc::{open_error, BackendKind, RpcPhase, RpcRuntime};
pub use scan::{ScanRuntime, ScanView};
pub use seed::{MigrateOutcome, PasswordChangeOutcome, RestoreOutcome};
pub use tor::{TorBridges, TorRuntime, TorStart, TorStatus};
pub use wallet::{OpenOutcome, PendingList, RuntimeProfiles, RuntimeStatus};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use flint_core::{CreatedWallet, NodeInfo, NodeManager, SettingsView, WalletService, WalletStatus};
use flint_janus::{JanusService, Offer, Swap};

#[derive(Clone)]
pub struct SwapBoard {
    pub board: String,
    pub stamps_balance: u64,
    pub protocol: String,
    pub offers: Vec<Offer>,
}

#[derive(Clone)]
pub struct SwapRuntime {
    janus: Arc<Mutex<JanusService>>,
}

impl SwapRuntime {
    pub fn new() -> Self {
        Self::from_arc(Arc::new(Mutex::new(JanusService::demo(None))))
    }

    pub fn from_arc(janus: Arc<Mutex<JanusService>>) -> Self {
        Self { janus }
    }

    pub fn board(&self, pair: &str) -> Result<SwapBoard, String> {
        let svc = self.janus.lock().map_err(|_| "janus занят".to_string())?;
        Ok(SwapBoard {
            board: svc.board_name().to_string(),
            stamps_balance: svc.stamps_balance(),
            protocol: flint_janus::PROTOCOL.to_string(),
            offers: svc.offers(pair)?,
        })
    }

    pub fn take(&self, id: &str, amount: Option<u64>) -> Result<Swap, String> {
        let mut svc = self.janus.lock().map_err(|_| "janus занят".to_string())?;
        svc.take(id, amount)
    }

    pub fn get(&self, id: &str) -> Option<Swap> {
        self.janus.lock().ok().and_then(|svc| svc.get(id))
    }

    pub fn list(&self) -> Vec<Swap> {
        self.janus.lock().map(|svc| svc.list()).unwrap_or_default()
    }

    pub fn simulate(&self, id: &str, tip: u64) -> Result<Swap, String> {
        let mut svc = self.janus.lock().map_err(|_| "janus занят".to_string())?;
        svc.simulate(id, tip)
    }
}

impl Default for SwapRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct SeedStatus {
    pub kind: String,
    pub backups: usize,
    pub legacy_leftover: bool,
    pub network: String,
    pub unlocked: bool,
}

#[derive(Clone, Debug)]
pub struct SwitchOutcome {
    pub network: String,
    pub profile: String,
}

#[derive(Clone, Debug)]
pub struct SettingsUpdateOutcome {
    pub view: SettingsView,
    pub node_restarted: bool,
    pub restart_error: Option<String>,
}

#[derive(Clone)]
pub struct WalletRuntime {
    wallet: Arc<Mutex<WalletService>>,
    /// Embedded managed node. `new` (Slint) owns it here; `from_shared` can
    /// reuse a handle for tests or embedders that manage their own node.
    node: Arc<Mutex<Option<NodeManager>>>,
    backend: BackendKind,
    rpc: RpcRuntime,
    scan: ScanRuntime,
    tor: TorRuntime,
    history_key: Arc<Mutex<Option<Zeroizing<[u8; 32]>>>>,
    history_error: Arc<Mutex<Option<String>>>,
    messenger_op: Arc<Mutex<()>>,
    unlock_cache: Arc<Mutex<HashMap<String, CachedUnlock>>>,
}

struct CachedUnlock {
    password: Zeroizing<String>,
    at: Instant,
}

/// Switching profiles within this window re-opens the target profile without
/// asking for its password again. The password stays only in memory.
pub const UNLOCK_GRACE: Duration = Duration::from_secs(5 * 60);

fn build_node(wallet: &Arc<Mutex<WalletService>>) -> Option<NodeManager> {
    let w = wallet.lock().ok()?;
    let mut manager = NodeManager::new(
        w.paths(),
        &w.settings().network,
        w.settings().node_bin.as_deref(),
    )
    .ok()?;
    manager.set_proxy_enabled(w.settings().node_proxy);
    Some(manager)
}

fn redact_url(input: &str) -> String {
    let trimmed = input.trim();
    let Some(scheme_end) = trimmed.find("://") else {
        return redact_text(trimmed);
    };
    let scheme = &trimmed[..scheme_end];
    let rest = &trimmed[scheme_end + 3..];
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    format!("{scheme}://{host}{}", &rest[authority_end..])
}

fn redact_text(input: &str) -> String {
    input
        .split_whitespace()
        .map(|part| {
            if part.starts_with("http://") || part.starts_with("https://") {
                redact_url(part)
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = redact_text(text),
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_json),
        serde_json::Value::Object(map) => map.values_mut().for_each(redact_json),
        _ => {}
    }
}

impl WalletRuntime {
    /// Create a session over the default data directory (`FLINT_DATA_DIR` wins).
    pub fn new() -> Self {
        Self::new_with_backend(BackendKind::Embedded)
    }

    /// Create a session with an explicit wallet backend.
    pub fn new_with_backend(backend: BackendKind) -> Self {
        Self::open_at(flint_core::AppPaths::resolve(), backend)
    }

    /// Create a session over an explicit data root. Used by tests and by shells
    /// that pass `--data` without mutating the process environment.
    pub fn open_at(paths: flint_core::AppPaths, backend: BackendKind) -> Self {
        let wallet = Arc::new(Mutex::new(WalletService::open_at(paths)));
        let node = Arc::new(Mutex::new(build_node(&wallet)));
        let rpc = RpcRuntime::new(wallet.clone(), backend.is_rpc());
        let scan = ScanRuntime::new(wallet.clone(), rpc.clone());
        let tor = TorRuntime::new(wallet.clone(), rpc.clone());
        Self {
            wallet,
            node,
            backend,
            rpc,
            scan,
            tor,
            history_key: Arc::new(Mutex::new(None)),
            history_error: Arc::new(Mutex::new(None)),
            messenger_op: Arc::new(Mutex::new(())),
            unlock_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Reuse an existing handle; used while the Slint shell still owns its
    /// `AppState`. The default is embedded, matching the previous behaviour.
    pub fn from_arc(wallet: Arc<Mutex<WalletService>>) -> Self {
        Self::from_arc_with_backend(wallet, BackendKind::Embedded)
    }

    /// Reuse an existing wallet handle and select the backend explicitly.
    pub fn from_arc_with_backend(wallet: Arc<Mutex<WalletService>>, backend: BackendKind) -> Self {
        Self::from_shared(wallet, Arc::new(Mutex::new(None)), backend)
    }

    /// Reuse both the wallet and the managed-node handle. Sharing the `Arc`
    /// lets a runtime switch stop/rebuild the same manager instead of a second
    /// ghost one.
    pub fn from_shared(
        wallet: Arc<Mutex<WalletService>>,
        node: Arc<Mutex<Option<NodeManager>>>,
        backend: BackendKind,
    ) -> Self {
        let rpc = RpcRuntime::new(wallet.clone(), backend.is_rpc());
        let scan = ScanRuntime::new(wallet.clone(), rpc.clone());
        let tor = TorRuntime::new(wallet.clone(), rpc.clone());
        Self {
            wallet,
            node,
            backend,
            rpc,
            scan,
            tor,
            history_key: Arc::new(Mutex::new(None)),
            history_error: Arc::new(Mutex::new(None)),
            messenger_op: Arc::new(Mutex::new(())),
            unlock_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// UI language persisted next to the data root (`ui-language`). The file is
    /// not secret and survives restarts; unknown or missing values fall back to
    /// Russian, which is also the gettext msgid language.
    pub fn ui_language(&self) -> String {
        let Ok(wallet) = self.wallet.lock() else {
            return "ru".into();
        };
        let path = wallet.paths().root().join("ui-language");
        match std::fs::read_to_string(path) {
            Ok(value) if value.trim() == "en" => "en".into(),
            _ => "ru".into(),
        }
    }

    pub fn set_ui_language(&self, language: &str) -> Result<(), String> {
        if !matches!(language, "ru" | "en") {
            return Err(format!("неизвестный язык интерфейса: {language}"));
        }
        let root = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?
            .paths()
            .root()
            .to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| format!("каталог данных: {e}"))?;
        std::fs::write(root.join("ui-language"), format!("{language}\n"))
            .map_err(|e| format!("ui-language: {e}"))
    }

    /// Auto-finalization is persisted next to `ui-language`, so choosing it once
    /// survives a restart. Missing/invalid values are the safe default: off.
    pub fn auto_process_slatepacks(&self) -> bool {
        let Ok(wallet) = self.wallet.lock() else {
            return false;
        };
        wallet.paths().root().join("auto-slatepacks").is_file()
            && std::fs::read_to_string(wallet.paths().root().join("auto-slatepacks"))
                .map(|value| value.trim() == "1")
                .unwrap_or(false)
    }

    pub fn set_auto_process_slatepacks(&self, enabled: bool) -> Result<(), String> {
        let root = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?
            .paths()
            .root()
            .to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| format!("каталог данных: {e}"))?;
        std::fs::write(
            root.join("auto-slatepacks"),
            if enabled { "1\n" } else { "0\n" },
        )
        .map_err(|e| format!("auto-slatepacks: {e}"))
    }

    pub fn handle(&self) -> Arc<Mutex<WalletService>> {
        self.wallet.clone()
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.backend
    }

    pub fn is_rpc(&self) -> bool {
        self.backend.is_rpc()
    }

    /// Free/total bytes on the volume that owns the active data root.
    /// Cheap enough to poll; the UI only needs to warn before the volume fills.
    pub fn disk_space(&self) -> Result<(u64, u64), String> {
        let root = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?
            .paths()
            .root()
            .to_path_buf();
        disk_space(&root)
    }

    pub fn rpc(&self) -> &RpcRuntime {
        &self.rpc
    }

    /// Start the RPC child from UI code without exposing the whole RPC handle.
    pub fn start_rpc_background(&self) -> Result<(), String> {
        self.rpc.start().map_err(|error| error.to_string())
    }

    pub fn scan_runtime(&self) -> &ScanRuntime {
        &self.scan
    }

    /// Monotonic network/profile identity for asynchronous UI result gating.
    pub fn context_generation(&self) -> u64 {
        self.scan.generation()
    }

    pub fn tor(&self) -> &TorRuntime {
        &self.tor
    }

    /// Cancel a scan and drop the Tor listener, whose gateway is bound to the
    /// RPC port that a cancel may restart.
    pub fn cancel_scan(&self) -> Result<(), String> {
        self.tor.stop();
        self.scan.cancel()
    }

    /// Stop every child process this runtime owns. Process exit does not run
    /// destructors for managed state, so shells call this explicitly.
    pub fn shutdown(&self) {
        self.lock_history_key();
        // A worker may still hold a runtime clone; cancel its scan before the
        // RPC child/node disappears so the worker cannot finish against a
        // replaced profile.
        let _ = self.scan.cancel();
        self.tor.shutdown();
        self.rpc.shutdown();
        let _ = self.node_stop();
    }

    pub fn status_fast(&self) -> Result<WalletStatus, String> {
        let w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok(w.status_fast())
    }

    /// Status without the owner-API address call, for a running embedded scan.
    pub fn status_without_address(&self) -> Result<WalletStatus, String> {
        let w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok(w.status_without_address())
    }

    pub fn open_wallet(&self, password: &str) -> Result<(), String> {
        let mut w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        w.open_wallet(password).map_err(|e| e.to_string())
    }

    pub fn node_info(&self) -> Result<NodeInfo, String> {
        let w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok(w.node_info())
    }

    pub fn create_wallet(&self, password: &str) -> Result<CreatedWallet, String> {
        let mut w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        w.create_wallet(password).map_err(|e| e.to_string())
    }

    pub fn settings_view(&self) -> Result<SettingsView, String> {
        let w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok(w.settings_view())
    }

    /// Set or clear the Nostr relay. Separate from `relay_url` and validated by
    /// the transport itself; an empty value disables the transport.
    pub fn update_nostr_relay(
        &self,
        url: Option<String>,
    ) -> Result<flint_core::model::SettingsView, String> {
        let mut w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        w.update_nostr_relay(url).map_err(|e| e.to_string())
    }

    pub fn data_root_view(&self) -> Result<flint_core::config::DataRootView, String> {
        let w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        w.paths().data_root_view()
    }

    /// Select the root for the next process without moving or reopening any
    /// wallet in the current session.
    pub fn set_data_root(
        &self,
        requested: &str,
    ) -> Result<flint_core::config::DataRootView, String> {
        self.scan.ensure_idle("изменение каталога данных")?;
        let w = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        w.paths().set_data_root_override(requested)
    }

    pub fn profile(&self) -> String {
        self.wallet
            .lock()
            .map(|w| {
                w.settings()
                    .wallet_name
                    .clone()
                    .unwrap_or_else(|| "default".to_string())
            })
            .unwrap_or_else(|_| "default".to_string())
    }

    /// Remember a successful password for this profile for [`UNLOCK_GRACE`].
    /// Only the in-process memory holds it; switching profiles within the grace
    /// period re-opens without asking again.
    pub fn remember_unlock(&self, password: &str) {
        if password.is_empty() {
            return;
        }
        let profile = self.profile();
        if let Ok(mut cache) = self.unlock_cache.lock() {
            cache.retain(|_, entry| entry.at.elapsed() < UNLOCK_GRACE);
            cache.insert(
                profile,
                CachedUnlock {
                    password: Zeroizing::new(password.to_string()),
                    at: Instant::now(),
                },
            );
        }
    }

    pub fn cached_unlock(&self, profile: &str) -> Option<Zeroizing<String>> {
        let mut cache = self.unlock_cache.lock().ok()?;
        cache.retain(|_, entry| entry.at.elapsed() < UNLOCK_GRACE);
        cache.get(profile).map(|entry| entry.password.clone())
    }

    pub fn clear_unlock_cache(&self) {
        if let Ok(mut cache) = self.unlock_cache.lock() {
            cache.clear();
        }
    }

    /// Redacted diagnostics for support: never contains passwords, seed
    /// phrases, private keys or API secrets. Node URL and profile name are
    /// operational metadata and are intentionally included.
    pub fn diagnostics_export(&self) -> Result<String, String> {
        let (network, node_url, wallet_name) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                wallet.settings().network.clone(),
                wallet.settings().node_url.clone(),
                wallet.settings().wallet_name.clone(),
            )
        };
        let node = self.node_info().ok();
        let status = self.status_without_address().ok();
        let mut node = serde_json::to_value(node).unwrap_or(serde_json::Value::Null);
        let mut status = serde_json::to_value(status).unwrap_or(serde_json::Value::Null);
        redact_json(&mut node);
        redact_json(&mut status);
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": 1,
            "generated_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            "backend": if self.is_rpc() { "grin-wallet" } else { "embedded" },
            "network": network,
            "node_url": redact_url(&node_url),
            "wallet_name": wallet_name,
            "node": node,
            "status": status,
        }))
        .map_err(|e| format!("diagnostics: {e}"))
    }

    fn recent_profiles_path(&self) -> Option<std::path::PathBuf> {
        self.wallet
            .lock()
            .ok()
            .map(|wallet| wallet.paths().root().join("ui-recent-profiles.json"))
    }

    /// Up to three recently opened profiles, newest first, filtered to profiles
    /// that still exist.
    pub fn recent_profiles(&self) -> Vec<String> {
        let Some(path) = self.recent_profiles_path() else {
            return Vec::new();
        };
        let stored: Vec<String> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let known = self.profiles().map(|p| p.profiles).unwrap_or_default();
        let mut result = Vec::new();
        for name in stored {
            if known.contains(&name) && !result.contains(&name) {
                result.push(name);
            }
            if result.len() == 3 {
                break;
            }
        }
        let active = self.profile();
        if !result.contains(&active) {
            result.insert(0, active);
            result.truncate(3);
        }
        result
    }

    pub fn remember_recent_profile(&self, name: &str) -> Result<(), String> {
        let path = self
            .recent_profiles_path()
            .ok_or_else(|| "кошелёк занят".to_string())?;
        let mut recent = self.recent_profiles();
        recent.retain(|item| item != name);
        recent.insert(0, name.to_string());
        recent.truncate(3);
        let bytes = serde_json::to_vec(&recent).map_err(|e| format!("ui-recent-profiles: {e}"))?;
        std::fs::write(&path, bytes).map_err(|e| format!("ui-recent-profiles: {e}"))
    }

    /// Replace the active network/profile. Stops the managed node, persists the
    /// settings, then installs a fresh WalletService (the same lifecycle the
    /// Slint shell uses). The caller must re-read status/unlock afterwards.
    pub fn switch(
        &self,
        network: Option<String>,
        profile: Option<Option<String>>,
    ) -> Result<SwitchOutcome, String> {
        // Validate before stopping Tor/RPC or locking the old wallet: a typo in
        // a profile name must not tear down the current session.
        let profile = profile
            .map(flint_core::service::normalize_wallet_name)
            .transpose()
            .map_err(|e| e.to_string())?;
        if network
            .as_deref()
            .is_some_and(|name| !flint_core::config::NETWORKS.contains(&name))
        {
            return Err("неизвестная сеть: допустимы testnet и mainnet".into());
        }
        let _switch = self.scan.begin_switch()?;
        self.lock_history_key();
        self.tor.stop();
        // The RPC child holds the old wallet database; stop it before the
        // WalletService is replaced. Any in-flight scan result now belongs to
        // the old context.
        self.rpc.stop();
        self.scan.invalidate();
        self.node_stop()?;
        {
            let mut w = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            w.lock();
            if let Some(net) = network {
                w.update_settings(Some(net), None, None, None, None, None, None)
                    .map_err(|e| e.to_string())?;
            }
            if let Some(name) = profile {
                w.set_wallet_name(name).map_err(|e| e.to_string())?;
            }
            let root = w.paths().root().to_path_buf();
            *w = WalletService::open_at(flint_core::AppPaths::at(root));
        }
        let (auto_start, network, profile) = {
            let w = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                w.settings().auto_start_node,
                w.settings().network.clone(),
                w.settings()
                    .wallet_name
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
            )
        };
        {
            let mut guard = self.node.lock().map_err(|_| "нода занята".to_string())?;
            *guard = build_node(&self.wallet);
            if auto_start {
                if let Some(manager) = guard.as_mut() {
                    let _ = manager.start();
                }
            }
        }
        if self.is_rpc() {
            // A failed start is recorded in the RPC session; the switch itself
            // has already been applied and must not be rolled back.
            let _ = self.rpc.start();
        }
        Ok(SwitchOutcome { network, profile })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_settings(
        &self,
        network: Option<String>,
        node_url: Option<String>,
        node_bin: Option<String>,
        auto_start_node: Option<bool>,
        node_proxy: Option<bool>,
        relay_url: Option<String>,
        min_confirmations: Option<u64>,
    ) -> Result<SettingsUpdateOutcome, String> {
        self.scan.ensure_idle("изменение настроек")?;
        let (before_url, before_network) = {
            let w = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (w.settings().node_url.clone(), w.settings().network.clone())
        };
        if let Some(requested) = network.as_deref() {
            if requested != before_network {
                return Err(
                    "смена сети через settings_set запрещена: используйте network_switch, чтобы не оставить RPC-кошелёк на старой сети"
                        .into(),
                );
            }
        }
        let view = {
            let mut w = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            w.update_settings(
                network,
                node_url,
                node_bin,
                auto_start_node,
                node_proxy,
                relay_url,
                min_confirmations,
            )
            .map_err(|e| e.to_string())?
        };
        if let Some(value) = min_confirmations {
            self.rpc.set_min_confirmations(value.clamp(1, 1000));
        }
        {
            let mut guard = self.node.lock().map_err(|_| "нода занята".to_string())?;
            if let Some(manager) = guard.as_mut() {
                let _ = manager.stop();
            }
            *guard = build_node(&self.wallet);
        }

        let node_changed = view.node_url != before_url;
        let network_changed = view.network != before_network;
        let mut node_restarted = false;
        let mut restart_error = None;
        if node_changed && !network_changed && self.is_rpc() {
            let initialized = {
                let w = self
                    .wallet
                    .lock()
                    .map_err(|_| "кошелёк занят".to_string())?;
                let dir = flint_core::rpc_backend::wallet_dir(
                    w.paths(),
                    &w.settings().network,
                    w.settings().wallet_name.as_deref(),
                );
                flint_core::config::wallet_data_has_seed(&dir)
            };
            if initialized {
                self.tor.stop();
                match self.rpc.restart() {
                    Ok(()) => node_restarted = true,
                    Err(error) => restart_error = Some(error),
                }
            }
        }
        Ok(SettingsUpdateOutcome {
            view,
            node_restarted,
            restart_error,
        })
    }

    pub fn node_running(&self) -> bool {
        let mut guard = match self.node.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        if guard.is_none() {
            *guard = build_node(&self.wallet);
        }
        guard
            .as_mut()
            .map(|node| node.is_running())
            .unwrap_or(false)
    }

    pub fn node_start(&self) -> Result<(), String> {
        let mut guard = self.node.lock().map_err(|_| "нода занята".to_string())?;
        if guard.is_none() {
            *guard = build_node(&self.wallet);
        }
        guard
            .as_mut()
            .ok_or_else(|| "нода недоступна: не найден grin.exe".to_string())?
            .start()
            .map_err(|e| e.to_string())
    }

    pub fn node_stop(&self) -> Result<(), String> {
        let mut guard = self.node.lock().map_err(|_| "нода занята".to_string())?;
        if let Some(node) = guard.as_mut() {
            node.stop().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn node_log(&self, lines: usize) -> String {
        let guard = match self.node.lock() {
            Ok(guard) => guard,
            Err(_) => return String::new(),
        };
        guard
            .as_ref()
            .map(|node| node.log_tail(lines))
            .unwrap_or_default()
    }

    pub fn scan_state(&self) -> flint_core::ScanState {
        self.wallet
            .lock()
            .map(|w| w.scan_state())
            .unwrap_or(flint_core::ScanState::NotStarted)
    }

    pub fn is_unlocked(&self) -> bool {
        self.wallet.lock().map(|w| w.unlocked()).unwrap_or(false)
    }
}

#[cfg(windows)]
fn disk_space(path: &std::path::Path) -> Result<(u64, u64), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_available: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) };
    if ok == 0 {
        return Err("не удалось получить свободное место на диске".into());
    }
    Ok((free, total))
}

#[cfg(test)]
mod redaction_tests {
    use super::{redact_json, redact_text, redact_url};

    #[test]
    fn diagnostics_url_redaction_removes_credentials_and_query() {
        let url = "http://user:pass@127.0.0.1:13413/v2/owner?token=secret#frag";
        let redacted = redact_url(url);
        assert!(!redacted.contains("user"));
        assert!(!redacted.contains("pass"));
        assert!(!redacted.contains("token"));
        assert!(!redacted.contains("frag"));
        assert!(redacted.contains("127.0.0.1:13413"));
        let text = redact_text("failed http://u:p@host:1/path?x=secret");
        assert!(!text.contains("u:p"));
        assert!(!text.contains("secret"));
        let mut value = serde_json::json!({"error": "http://u:p@host:1/x?token=s"});
        redact_json(&mut value);
        let rendered = value.to_string();
        assert!(!rendered.contains("u:p"));
        assert!(!rendered.contains("token"));
    }
}

#[cfg(not(windows))]
fn disk_space(_path: &std::path::Path) -> Result<(u64, u64), String> {
    Err("свободное место на диске доступно только на Windows".into())
}

impl Default for WalletRuntime {
    fn default() -> Self {
        Self::new()
    }
}
