//! Paths and user settings. Everything lives under one data root so a backup is
//! a single directory copy (`%LOCALAPPDATA%\Flint` on Windows by default).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{FlintError, Result};
use crate::model::ScanState;

pub const NETWORKS: [&str; 2] = ["testnet", "mainnet"];

/// A wallet directory is initialized when it holds either upstream's seed file
/// or Flint's Argon2id envelope (`wallet.seed.v2`, see `docs/history/ARGON2ID.md`).
/// A transient `wallet.seed` next to an envelope is not a separate wallet.
pub fn wallet_data_has_seed(dir: &Path) -> bool {
    let data = dir.join("wallet_data");
    data.join("wallet.seed").is_file() || data.join("wallet.seed.v2").is_file()
}

/// User-editable configuration, persisted as `settings.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub network: String,
    pub node_url: String,
    pub node_api_secret: Option<String>,
    /// Optional override for the bundled `grin.exe`.
    #[serde(default)]
    pub node_bin: Option<String>,
    /// Start the bundled node together with the app.
    #[serde(default)]
    pub auto_start_node: bool,
    /// Put the CORS-stripping proxy in front of the managed node. Off means
    /// direct mode (node on the public port); a healthy node already listening
    /// there wins over both modes and is attached directly.
    #[serde(default = "default_true")]
    pub node_proxy: bool,
    /// Optional messenger relay/gateway base URL (empty = not configured).
    #[serde(default)]
    pub relay_url: String,
    /// Optional Nostr relay (`wss://`) for the Nostr transport; empty = disabled.
    /// Separate from `relay_url`: that one is the Flint HTTP gateway.
    #[serde(default)]
    pub nostr_relay_url: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Confirmations required before an output counts as spendable.
    pub min_confirmations: u64,
    /// A wallet was restored from a phrase and the chain has not been scanned
    /// yet: its balance is unknown, not zero.
    #[serde(default)]
    pub pending_scan: bool,
    /// Active wallet profile. `None` keeps the legacy `<network>/wallet` dir so
    /// existing installations need no migration.
    #[serde(default)]
    pub wallet_name: Option<String>,
    /// Current slatepack address derivation index (m/0/1/<index>).
    /// Rotating it gives a new address from the same seed phrase.
    #[serde(default)]
    pub address_index: u32,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            network: "testnet".to_string(),
            node_url: crate::node::default_node_url("testnet"),
            node_api_secret: None,
            node_bin: None,
            auto_start_node: true,
            node_proxy: true,
            relay_url: String::new(),
            nostr_relay_url: String::new(),
            model: "deepseek-chat".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            api_key: None,
            min_confirmations: 10,
            pending_scan: false,
            wallet_name: None,
            address_index: 0,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        if !NETWORKS.contains(&self.network.as_str()) {
            self.network = "testnet".to_string();
        }
        let url = self.node_url.trim().to_string();
        self.node_url = if url.is_empty() {
            crate::node::default_node_url(&self.network)
        } else {
            url
        };
        if self.model.trim().is_empty() {
            self.model = "deepseek-chat".to_string();
        }
        if self.base_url.trim().is_empty() {
            self.base_url = "https://api.deepseek.com".to_string();
        }
        self
    }

    pub fn is_testnet(&self) -> bool {
        self.network == "testnet"
    }
}

/// Portable layout: everything next to the executable when a `portable` marker
/// file sits beside it (or `FLINT_PORTABLE=1` is set).
pub fn portable_root() -> Option<PathBuf> {
    let forced = cfg!(feature = "portable")
        || std::env::var("FLINT_PORTABLE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()));

    match exe_dir {
        Some(dir) => {
            if forced || dir.join("portable").exists() {
                Some(dir.join("data"))
            } else {
                None
            }
        }
        None if forced => Some(PathBuf::from("data")),
        None => None,
    }
}

const DATA_ROOT_CONFIG_VERSION: u32 = 1;
const DATA_ROOT_CONFIG_NAME: &str = "data-root.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DataRootConfig {
    version: u32,
    root: Option<PathBuf>,
}

/// Paths shown by the data-root setting. The active root does not change until
/// the process is restarted; `override_root` is the selection for the next run.
#[derive(Clone, Debug)]
pub struct DataRootView {
    pub active: PathBuf,
    pub default_root: PathBuf,
    pub override_root: Option<PathBuf>,
    pub forced_by_environment: bool,
}

fn default_data_root() -> PathBuf {
    if let Some(root) = portable_root() {
        return root;
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.trim().is_empty() {
            return PathBuf::from(local).join("Flint");
        }
    }
    PathBuf::from(".flint")
}

fn data_root_config_file() -> PathBuf {
    if let Some(root) = portable_root() {
        // Keep the selector outside `data`: moving or deleting the old data
        // directory must not silently send the next run back to the old root.
        return root
            .parent()
            .map(|dir| dir.join(DATA_ROOT_CONFIG_NAME))
            .unwrap_or_else(|| root.join(DATA_ROOT_CONFIG_NAME));
    }
    default_data_root().join(DATA_ROOT_CONFIG_NAME)
}

fn forced_data_root() -> Option<PathBuf> {
    std::env::var("FLINT_DATA_DIR")
        .ok()
        .filter(|dir| !dir.trim().is_empty())
        .map(PathBuf::from)
}

fn read_data_root_override_from(path: &Path) -> std::result::Result<Option<PathBuf>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "{}: не удалось прочитать выбор каталога данных: {error}",
                path.display()
            ))
        }
    };
    let config: DataRootConfig = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "{}: выбор каталога данных повреждён: {error}; исправьте или удалите этот файл, пока приложение закрыто",
            path.display()
        )
    })?;
    if config.version != DATA_ROOT_CONFIG_VERSION {
        return Err(format!(
            "{}: неподдерживаемая версия выбора каталога данных {}",
            path.display(),
            config.version
        ));
    }
    match config.root {
        Some(root) if root.is_absolute() => Ok(Some(root)),
        Some(_) => Err(format!(
            "{}: каталог данных должен быть непустым абсолютным путём",
            path.display()
        )),
        None => Ok(None),
    }
}

fn write_data_root_override_from(path: &Path, requested: &str) -> std::result::Result<(), String> {
    let requested = requested.trim();
    let root = if requested.is_empty() {
        None
    } else {
        let root = PathBuf::from(requested);
        if !root.is_absolute() {
            return Err(
                "каталог данных должен быть абсолютным путём, например D:\\Flint-data".into(),
            );
        }
        if root.exists() && !root.is_dir() {
            return Err(format!("{} не является каталогом", root.display()));
        }
        fs::create_dir_all(&root)
            .map_err(|error| format!("не удалось создать каталог {}: {error}", root.display()))?;
        Some(root)
    };
    let config = DataRootConfig {
        version: DATA_ROOT_CONFIG_VERSION,
        root,
    };
    let raw = serde_json::to_vec_pretty(&config)
        .map_err(|error| format!("не удалось сериализовать выбор каталога данных: {error}"))?;
    crate::win_acl::write_private_atomic(path, &raw)
        .map_err(|error| format!("не удалось сохранить {}: {error}", path.display()))
}

/// Per-wallet state that must not be shared between profiles of one network:
/// the address rotation index, the "balance unknown until scanned" flag and the
/// explicit scan lifecycle.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WalletState {
    #[serde(default)]
    pub address_index: u32,
    #[serde(default)]
    pub pending_scan: bool,
    #[serde(default)]
    pub scan_state: ScanState,
    #[serde(default)]
    pub scan_error: Option<String>,
    #[serde(default)]
    pub scan_started_at: Option<i64>,
    #[serde(default)]
    pub scan_finished_at: Option<i64>,
}

impl WalletState {
    /// A persisted `running` scan cannot outlive the process that owned it. On
    /// load it means the scan was interrupted (crash, power loss, app restart);
    /// treating it as finished would clear the unknown-balance flag without a
    /// scan. `completed` is authoritative for `pending_scan`.
    pub fn normalized(mut self) -> Self {
        if self.scan_state == ScanState::Running {
            self.scan_state = ScanState::Failed;
            self.scan_error = Some("скан прерван: приложение было перезапущено".to_string());
        }
        if self.scan_state == ScanState::Completed {
            self.pending_scan = false;
        }
        if self.scan_state == ScanState::Failed && self.scan_error.is_none() {
            self.scan_error = Some("скан не завершён".to_string());
        }
        self
    }
}

pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// `FLINT_DATA_DIR`/`--data` wins, then the explicitly selected data root,
    /// then portable mode or `%LOCALAPPDATA%\Flint`. A damaged selector must not
    /// silently open a different wallet.
    pub fn resolve_checked() -> std::result::Result<Self, String> {
        if let Some(root) = forced_data_root() {
            return Ok(Self { root });
        }
        let root = read_data_root_override_from(&data_root_config_file())?
            .unwrap_or_else(default_data_root);
        Ok(Self { root })
    }

    /// Infallible compatibility entry point for headless callers. The Slint
    /// shell uses `resolve_checked` so it can show the configuration error.
    pub fn resolve() -> Self {
        Self::resolve_checked().unwrap_or_else(|error| panic!("{error}"))
    }

    /// Explicit data root (tests, portable profiles).
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data_root_view(&self) -> std::result::Result<DataRootView, String> {
        let forced_by_environment = forced_data_root().is_some();
        let override_root = if forced_by_environment {
            None
        } else {
            read_data_root_override_from(&data_root_config_file())?
        };
        Ok(DataRootView {
            active: self.root.clone(),
            default_root: default_data_root(),
            override_root,
            forced_by_environment,
        })
    }

    /// Persist the root for the next process. No wallet or chain data is moved.
    pub fn set_data_root_override(
        &self,
        requested: &str,
    ) -> std::result::Result<DataRootView, String> {
        if forced_data_root().is_some() {
            return Err(
                "каталог задан через --data или FLINT_DATA_DIR; измените параметр запуска"
                    .to_string(),
            );
        }
        write_data_root_override_from(&data_root_config_file(), requested)?;
        self.data_root_view()
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root).map_err(FlintError::Io)
    }

    /// Wallet home for a given network, isolated per network so a floonet run can
    /// never touch mainnet keys.
    pub fn wallet_home(&self, network: &str) -> PathBuf {
        self.root.join(network).join("wallet")
    }

    /// Directory that holds named wallet profiles of this network.
    pub fn wallets_dir(&self, network: &str) -> PathBuf {
        self.root.join(network).join("wallets")
    }

    /// Wallet directory for a profile: `None` keeps the legacy path so existing
    /// installations need no migration.
    pub fn wallet_home_for(&self, network: &str, name: Option<&str>) -> PathBuf {
        match name
            .map(str::trim)
            .filter(|n| !n.is_empty() && *n != "default")
        {
            Some(name) => self.wallets_dir(network).join(name),
            None => self.wallet_home(network),
        }
    }

    /// Existing profiles: `default` (the legacy dir) plus named directories that
    /// actually contain a seed file or an Argon2id envelope.
    pub fn list_wallets(&self, network: &str) -> Vec<String> {
        let mut names = Vec::new();
        if wallet_data_has_seed(&self.wallet_home(network)) {
            names.push("default".to_string());
        }
        if let Ok(entries) = std::fs::read_dir(self.wallets_dir(network)) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                    continue;
                }
                if wallet_data_has_seed(&entry.path()) {
                    names.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        names.sort();
        names
    }

    /// RPC wallet profiles of this network (`wallet-rpc` plus `rpc/<name>`).
    pub fn list_rpc_wallets(&self, network: &str) -> Vec<String> {
        let mut names = Vec::new();
        let legacy = self.root.join(network).join("wallet-rpc");
        if wallet_data_has_seed(&legacy) {
            names.push("default".to_string());
        }
        if let Ok(entries) = std::fs::read_dir(self.root.join(network).join("rpc")) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                    continue;
                }
                if wallet_data_has_seed(&entry.path()) {
                    names.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        names.sort();
        names
    }

    /// Chain data and config for the managed node of this network.
    pub fn node_dir(&self, network: &str) -> PathBuf {
        self.root.join(network).join("node")
    }

    pub fn pending_dir(&self, network: &str) -> PathBuf {
        self.root.join(network).join("pending")
    }

    /// Journal directory for a specific wallet profile. The default profile keeps
    /// the legacy path so existing records survive the upgrade.
    pub fn pending_dir_for(&self, network: &str, name: Option<&str>) -> PathBuf {
        let base = self.pending_dir(network);
        match name
            .map(str::trim)
            .filter(|n| !n.is_empty() && *n != "default")
        {
            Some(name) => base.join(name),
            None => base,
        }
    }

    /// Per-wallet state file: `<network>/state/<profile>.json`.
    pub fn wallet_state_file(&self, network: &str, name: Option<&str>) -> PathBuf {
        let label = match name
            .map(str::trim)
            .filter(|n| !n.is_empty() && *n != "default")
        {
            Some(name) => name.to_string(),
            None => "default".to_string(),
        };
        self.root
            .join(network)
            .join("state")
            .join(format!("{label}.json"))
    }

    /// Load state for a profile, falling back to values from `settings.json` so
    /// an existing default wallet keeps its rotation index after the upgrade.
    pub fn load_wallet_state(
        &self,
        network: &str,
        name: Option<&str>,
        fallback: (u32, bool),
    ) -> WalletState {
        let path = self.wallet_state_file(network, name);
        match fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<WalletState>(&raw) {
                Ok(state) => state.normalized(),
                // A damaged state file must not look like a scanned wallet with
                // a confirmed zero balance.
                Err(_) => WalletState {
                    address_index: fallback.0,
                    pending_scan: true,
                    scan_state: ScanState::Failed,
                    scan_error: Some(
                        "файл состояния профиля повреждён — баланс считается неизвестным"
                            .to_string(),
                    ),
                    ..Default::default()
                },
            },
            Err(_) => WalletState {
                address_index: fallback.0,
                pending_scan: fallback.1,
                ..Default::default()
            },
        }
    }

    pub fn save_wallet_state(
        &self,
        network: &str,
        name: Option<&str>,
        state: &WalletState,
    ) -> Result<()> {
        let path = self.wallet_state_file(network, name);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let raw = serde_json::to_vec_pretty(state)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &raw)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn load_settings_checked(&self) -> std::result::Result<Settings, String> {
        self.load_settings_with_warning()
            .map(|(settings, _)| settings)
    }

    /// Like [`load_settings_checked`], but a successful recovery from
    /// `settings.json.bak` carries a user-visible warning instead of being
    /// silent. A corrupt primary is always moved aside as `.corrupt-*`.
    pub fn load_settings_with_warning(
        &self,
    ) -> std::result::Result<(Settings, Option<String>), String> {
        let path = self.settings_file();
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Settings::default(), None));
            }
            Err(e) => return Err(format!("settings.json: не удалось прочитать: {e}")),
        };
        match serde_json::from_str::<Settings>(&raw) {
            Ok(settings) => {
                // Migrate pre-ACL settings files (and their backup) on open.
                let mut acl_ok = crate::win_acl::restrict(&path).is_ok();
                let backup = path.with_extension("json.bak");
                if backup.exists() && crate::win_acl::restrict(&backup).is_err() {
                    acl_ok = false;
                }
                let warning = (!acl_ok).then(|| {
                    "не удалось ограничить ACL settings.json/backup; проверьте права вручную"
                        .to_string()
                });
                Ok((settings.sanitized(), warning))
            }
            Err(primary) => {
                let backup = path.with_extension("json.bak");
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or_default();
                let corrupt = path.with_extension(format!("json.corrupt-{stamp}"));
                let moved = fs::rename(&path, &corrupt).is_ok();
                let corrupt_acl_ok = moved && crate::win_acl::restrict(&corrupt).is_ok();
                let corrupt_path = if moved {
                    corrupt.display().to_string()
                } else {
                    path.display().to_string()
                };

                if backup.exists() {
                    crate::win_acl::restrict(&backup).map_err(|e| {
                        format!(
                            "settings.json повреждён: {primary}; не удалось защитить резервную копию {} перед чтением: {e}",
                            backup.display()
                        )
                    })?;
                }
                if let Ok(raw_backup) = fs::read_to_string(&backup) {
                    if let Ok(settings) = serde_json::from_str::<Settings>(&raw_backup) {
                        let restored =
                            crate::win_acl::write_private_atomic(&path, raw_backup.as_bytes())
                                .is_ok();
                        let warning = format!(
                            "settings.json повреждён: {primary}; повреждённый файл сохранён как {corrupt_path}; настройки восстановлены из {}{}{}",
                            backup.display(),
                            if restored {
                                ""
                            } else {
                                "; защищённо записать восстановленный settings.json не удалось — сохраните настройки вручную"
                            },
                            if moved && !corrupt_acl_ok {
                                "; не удалось ограничить ACL повреждённой копии"
                            } else {
                                ""
                            }
                        );
                        tracing::warn!("{warning}");
                        return Ok((settings.sanitized(), Some(warning)));
                    }
                }
                Err(format!(
                    "settings.json повреждён: {primary}; резервная копия {}; повреждённый файл {}",
                    backup.display(),
                    corrupt_path
                ))
            }
        }
    }

    pub fn load_settings(&self) -> Settings {
        match self.load_settings_checked() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::error!("{error}; используются настройки по умолчанию");
                Settings::default()
            }
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.ensure()?;
        let path = self.settings_file();
        let raw = serde_json::to_vec_pretty(settings)?;
        let tmp = path.with_extension("json.tmp");
        {
            // Create an empty file and close it first: the ACL helper opens the
            // file with its own handle and cannot change the DACL while our
            // write handle is still open.
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
        }
        // settings.json can contain node_api_secret; restrict the temp file
        // before any secret bytes are written, not after the write.
        crate::win_acl::restrict(&tmp).map_err(FlintError::Io)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        use std::io::Write;
        file.write_all(&raw)?;
        file.sync_all()?;
        drop(file);

        // Keep the last known-good file. A crash after this point leaves either
        // the old settings or the backup, never an empty/truncated settings.json.
        if path.exists() {
            let backup = path.with_extension("json.bak");
            let previous = fs::read(&path)?;
            crate::win_acl::write_private_atomic(&backup, &previous).map_err(FlintError::Io)?;
        }
        fs::rename(&tmp, &path).map_err(FlintError::Io)?;
        crate::win_acl::restrict(&path).map_err(FlintError::Io)?;
        Ok(())
    }
}

impl std::fmt::Debug for AppPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppPaths")
            .field("root", &self.root)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_default_to_proxy_on() {
        // Settings files written before 0.1.55 have no `node_proxy` field: they
        // must keep the safe default (proxy), not silently switch to direct.
        let raw = r#"{
            "network": "testnet",
            "node_url": "http://127.0.0.1:13413",
            "node_api_secret": null,
            "model": "deepseek-chat",
            "base_url": "https://api.deepseek.com",
            "api_key": null,
            "min_confirmations": 10
        }"#;
        let settings: Settings = serde_json::from_str(raw).expect("old settings parse");
        assert!(
            settings.node_proxy,
            "missing field must default to proxy on"
        );
        assert!(Settings::default().node_proxy);
    }

    #[test]
    fn corrupt_settings_are_recovered_from_backup() {
        let paths = temp_paths("settings-backup");
        paths.ensure().expect("ensure");
        let mut settings = Settings::default();
        settings.network = "mainnet".to_string();
        paths.save_settings(&settings).expect("first save");
        paths
            .save_settings(&settings)
            .expect("second save keeps backup");
        let backup = paths.settings_file().with_extension("json.bak");
        assert!(backup.is_file(), "backup must exist after overwrite");

        std::fs::write(paths.settings_file(), "{ not json").expect("corrupt settings");
        let (recovered, warning) = paths
            .load_settings_with_warning()
            .expect("valid backup must recover");
        assert_eq!(recovered.network, "mainnet");
        let warning = warning.expect("backup recovery must be visible");
        assert!(warning.contains("settings.json"), "unexpected: {warning}");
        assert!(warning.contains(".corrupt-"), "unexpected: {warning}");
        assert!(
            paths.settings_file().is_file(),
            "recovered settings.json must be restored"
        );
        assert!(
            paths.settings_file().with_extension("json.bak").is_file(),
            "backup remains"
        );
        let parents = std::fs::read_dir(paths.root())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(parents, "corrupt settings must be preserved as .corrupt-*");
    }

    #[test]
    fn corrupt_settings_without_backup_are_preserved_and_reported() {
        let paths = temp_paths("settings-corrupt-alone");
        paths.ensure().expect("ensure");
        let mut settings = Settings::default();
        settings.network = "testnet".to_string();
        paths.save_settings(&settings).expect("save");
        std::fs::remove_file(paths.settings_file().with_extension("json.bak")).ok();
        std::fs::write(paths.settings_file(), "{ not json").expect("corrupt settings");

        let error = paths
            .load_settings_with_warning()
            .expect_err("no backup means the error must be reported");
        assert!(error.contains("повреждён"), "unexpected: {error}");
        assert!(
            !paths.settings_file().exists(),
            "corrupt primary must be moved aside"
        );
        let preserved = std::fs::read_dir(paths.root())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(preserved, "corrupt file must survive as .corrupt-*");
    }

    #[test]
    fn failed_settings_save_keeps_the_previous_file() {
        let paths = temp_paths("settings-atomic");
        paths.ensure().expect("ensure");
        let mut settings = Settings::default();
        settings.network = "testnet".to_string();
        paths.save_settings(&settings).expect("first save");
        let before = std::fs::read(paths.settings_file()).expect("settings bytes");

        // A directory at the temporary path makes the atomic write fail after
        // the read, before the rename.
        let tmp = paths.settings_file().with_extension("json.tmp");
        std::fs::create_dir_all(&tmp).expect("tmp blocker");
        let mut changed = settings.clone();
        changed.network = "mainnet".to_string();
        assert!(paths.save_settings(&changed).is_err(), "save must fail");
        assert_eq!(
            std::fs::read(paths.settings_file()).expect("settings bytes"),
            before,
            "failed save must not replace the previous settings"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn old_wallet_state_defaults_to_not_started() {
        let paths = temp_paths("state-old");
        let file = paths.wallet_state_file("testnet", Some("alice"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"address_index":2,"pending_scan":true}"#).unwrap();

        let state = paths.load_wallet_state("testnet", Some("alice"), (0, false));
        assert_eq!(state.address_index, 2);
        assert!(state.pending_scan);
        assert_eq!(
            state.scan_state,
            ScanState::NotStarted,
            "an old file has no scan evidence"
        );
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn a_running_scan_is_interrupted_after_a_restart() {
        let paths = temp_paths("state-running");
        let file = paths.wallet_state_file("testnet", None);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"address_index":0,"pending_scan":true,"scan_state":"running"}"#,
        )
        .unwrap();

        let state = paths.load_wallet_state("testnet", None, (0, false));
        assert_eq!(state.scan_state, ScanState::Failed);
        assert!(state.pending_scan, "the balance stays unknown");
        assert!(state.scan_error.is_some());

        // `completed` is authoritative: the flag must be false even if a stale
        // file still says otherwise.
        std::fs::write(
            &file,
            r#"{"address_index":0,"pending_scan":true,"scan_state":"completed"}"#,
        )
        .unwrap();
        let state = paths.load_wallet_state("testnet", None, (0, false));
        assert!(!state.pending_scan);
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn a_corrupt_state_file_means_unknown_balance() {
        let paths = temp_paths("state-corrupt");
        let file = paths.wallet_state_file("testnet", None);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"{not json").unwrap();

        let state = paths.load_wallet_state("testnet", None, (0, false));
        assert!(state.pending_scan, "a damaged file is not a confirmed zero");
        assert_eq!(state.scan_state, ScanState::Failed);
        assert!(state.scan_error.is_some());
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn portable_root_uses_data_dir_next_to_exe() {
        std::env::set_var("FLINT_PORTABLE", "1");
        let root = portable_root().expect("portable root when FLINT_PORTABLE=1");
        assert!(root.ends_with("data"), "got {}", root.display());
        std::env::remove_var("FLINT_PORTABLE");
    }

    #[test]
    fn data_root_override_round_trip_and_reset() {
        let base = std::env::temp_dir().join(format!("flint-data-root-{}", uuid::Uuid::new_v4()));
        let config = base.join("data-root.json");
        let selected = base.join("новый каталог");
        write_data_root_override_from(&config, selected.to_str().unwrap()).expect("save override");
        assert_eq!(
            read_data_root_override_from(&config).expect("read override"),
            Some(selected.clone())
        );
        assert!(selected.is_dir(), "the selected data root must be usable");

        write_data_root_override_from(&config, "").expect("reset override");
        assert_eq!(
            read_data_root_override_from(&config).expect("read reset"),
            None
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn data_root_override_rejects_relative_paths_and_files() {
        let base = std::env::temp_dir().join(format!("flint-data-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let config = base.join("data-root.json");
        let error = write_data_root_override_from(&config, "relative/data")
            .expect_err("relative data roots are ambiguous");
        assert!(error.contains("абсолют"), "unexpected: {error}");
        assert!(
            !config.exists(),
            "a rejected path must not replace the config"
        );

        let file = base.join("not-a-directory");
        std::fs::write(&file, b"file").unwrap();
        let error = write_data_root_override_from(&config, file.to_str().unwrap())
            .expect_err("a regular file cannot be a data root");
        assert!(error.contains("каталог"), "unexpected: {error}");
        assert!(!config.exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn corrupt_data_root_override_is_not_silently_ignored() {
        let base = std::env::temp_dir().join(format!("flint-data-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let config = base.join("data-root.json");
        std::fs::write(&config, b"{not json").unwrap();
        let error =
            read_data_root_override_from(&config).expect_err("a corrupt selector must fail closed");
        assert!(error.contains("data-root.json"), "unexpected: {error}");
        let _ = std::fs::remove_dir_all(base);
    }

    fn temp_paths(tag: &str) -> AppPaths {
        AppPaths::at(
            std::env::temp_dir().join(format!("flint-paths-{tag}-{}", uuid::Uuid::new_v4())),
        )
    }

    #[test]
    fn legacy_dir_stays_the_default_profile() {
        let paths = temp_paths("legacy");
        assert_eq!(
            paths.wallet_home_for("testnet", None),
            paths.wallet_home("testnet")
        );
        assert_eq!(
            paths.wallet_home_for("testnet", Some("default")),
            paths.wallet_home("testnet")
        );
        assert_eq!(
            paths.wallet_home_for("testnet", Some("alice")),
            paths.wallets_dir("testnet").join("alice")
        );
    }

    #[test]
    fn listing_sees_only_dirs_with_a_seed() {
        let paths = temp_paths("list");
        std::fs::create_dir_all(paths.wallet_home("testnet").join("wallet_data")).unwrap();
        std::fs::write(
            paths
                .wallet_home("testnet")
                .join("wallet_data")
                .join("wallet.seed"),
            b"seed",
        )
        .unwrap();
        let alice = paths
            .wallets_dir("testnet")
            .join("alice")
            .join("wallet_data");
        std::fs::create_dir_all(&alice).unwrap();
        std::fs::write(alice.join("wallet.seed"), b"seed").unwrap();
        let bob = paths.wallets_dir("testnet").join("bob").join("wallet_data");
        std::fs::create_dir_all(&bob).unwrap();
        std::fs::write(bob.join("wallet.seed.v2"), b"envelope").unwrap();
        std::fs::create_dir_all(paths.wallets_dir("testnet").join("broken")).unwrap();

        assert_eq!(
            paths.list_wallets("testnet"),
            vec!["alice", "bob", "default"]
        );
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn rpc_profiles_are_listed_from_their_own_directories() {
        let paths = temp_paths("rpc-list");
        let legacy = paths.root().join("testnet").join("wallet-rpc");
        std::fs::create_dir_all(legacy.join("wallet_data")).unwrap();
        std::fs::write(legacy.join("wallet_data").join("wallet.seed"), b"seed").unwrap();
        let p2 = paths.root().join("testnet").join("rpc").join("p2");
        std::fs::create_dir_all(p2.join("wallet_data")).unwrap();
        std::fs::write(p2.join("wallet_data").join("wallet.seed"), b"seed").unwrap();
        let p3 = paths.root().join("testnet").join("rpc").join("p3");
        std::fs::create_dir_all(p3.join("wallet_data")).unwrap();
        std::fs::write(p3.join("wallet_data").join("wallet.seed.v2"), b"envelope").unwrap();
        std::fs::create_dir_all(paths.root().join("testnet").join("rpc").join("empty")).unwrap();

        assert_eq!(
            paths.list_rpc_wallets("testnet"),
            vec!["default", "p2", "p3"],
            "both wallet.seed and wallet.seed.v2 mark a profile"
        );
        let _ = std::fs::remove_dir_all(paths.root());
    }
}
