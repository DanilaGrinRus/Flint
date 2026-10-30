//! Supervises the official `grin-wallet` process and exposes it as a backend.
//!
//! The wallet runs as a separate process (`owner_api`), which is the whole point
//! of the migration: keys, seed and slatepack logic live in upstream code, and
//! Flint only talks to it over the encrypted owner API.

use std::fs::{self, File};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use grin_core::core::Transaction;
use grin_core::ser::{self, DeserializationMode, ProtocolVersion};
use grin_wallet_libwallet::{NodeClient, TxLogEntry, TxLogEntryType, WalletInfo};
use serde_json::{json, Value};

use crate::config::AppPaths;
use crate::error::{FlintError, Result};
use crate::model::{Balance, TxItem, WalletSummary};
use crate::node::NodeSnapshot;
use crate::seed::{SeedKind, SeedStore};
use crate::wallet_rpc::WalletRpc;

/// `grin-wallet.exe` next to the app, in `resources/wallet`, or from the env.
pub fn locate_wallet_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("FLINT_WALLET_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut anchors: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            anchors.push(dir.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        anchors.push(cwd);
    }
    for anchor in anchors {
        for ancestor in anchor.ancestors().take(6) {
            for candidate in [
                ancestor
                    .join("resources")
                    .join("wallet")
                    .join("grin-wallet.exe"),
                ancestor.join("wallet").join("grin-wallet.exe"),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(FlintError::Io)?;
    Ok(listener.local_addr().map_err(FlintError::Io)?.port())
}

/// A running `grin-wallet owner_api`, killed on drop.
pub struct WalletProcess {
    child: Child,
    port: u16,
    secret: String,
    dir: PathBuf,
}

impl WalletProcess {
    pub fn start(paths: &AppPaths, network: &str, profile: Option<&str>) -> Result<Self> {
        let node = crate::node::NodeSnapshot::managed(paths, network);
        Self::start_with_node(paths, network, profile, &node)
    }

    /// Start against the same endpoint/auth snapshot the readiness check uses.
    pub fn start_with_node(
        paths: &AppPaths,
        network: &str,
        profile: Option<&str>,
        node: &crate::node::NodeSnapshot,
    ) -> Result<Self> {
        Self::launch(paths, network, profile, true, node)
    }

    /// Start the wallet process in a directory that has no seed yet. Used for
    /// recovery: `create_wallet` over the owner API needs a running server but
    /// no existing wallet.
    pub fn start_uninitialized(
        paths: &AppPaths,
        network: &str,
        profile: Option<&str>,
    ) -> Result<Self> {
        let node = crate::node::NodeSnapshot::managed(paths, network);
        Self::start_uninitialized_with_node(paths, network, profile, &node)
    }

    pub fn start_uninitialized_with_node(
        paths: &AppPaths,
        network: &str,
        profile: Option<&str>,
        node: &crate::node::NodeSnapshot,
    ) -> Result<Self> {
        Self::launch(paths, network, profile, false, node)
    }

    fn launch(
        paths: &AppPaths,
        network: &str,
        profile: Option<&str>,
        require_seed: bool,
        node: &crate::node::NodeSnapshot,
    ) -> Result<Self> {
        let exe = locate_wallet_binary().ok_or_else(|| {
            FlintError::Wallet(
                "grin-wallet.exe не найден — запустите tools/fetch-wallet.ps1".into(),
            )
        })?;
        let dir = wallet_dir(paths, network, profile);
        fs::create_dir_all(&dir)?;
        // The app may be started with a relative FLINT_DATA_DIR; the wallet child
        // resolves -t against its own cwd, so pin an absolute path.
        let dir = fs::canonicalize(&dir)?;
        // A wallet is initialized once it has an Argon2id envelope *or* a legacy
        // seed. The envelope alone is enough: the transient `wallet.seed` is
        // written only when the user unlocks (see `open_with_envelope`).
        if require_seed && SeedStore::new(&dir, network).kind() == SeedKind::Missing {
            return Err(FlintError::NotInitialized);
        }

        // The wallet process must use the same node endpoint and auth as the
        // readiness check; `grin-wallet.toml` otherwise keeps whatever the
        // first `init` wrote.
        let wallet_config = dir.join("grin-wallet.toml");
        // Without an explicit secret (external node or the managed node has not
        // written its file yet) keep pointing at the managed secret path: it may
        // appear after the node starts.
        let loopback_node = crate::node::is_loopback_node_url(&node.url);
        if node
            .secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
            && loopback_node
        {
            patch_node_secret(&wallet_config, &paths.node_dir(network).join(".api_secret"))?;
        }
        patch_node_config(
            &wallet_config,
            &node.url,
            node.secret.as_deref(),
            dir.as_path(),
        )?;
        if node
            .secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
            && !loopback_node
        {
            clear_node_secret(&wallet_config)?;
        }
        patch_wallet_paths(&wallet_config, &dir)?;

        let port = free_port()?;
        let log = File::create(dir.join("owner.log"))?;
        let log_err = log.try_clone()?;
        let mut cmd = Command::new(&exe);
        if network == "testnet" {
            cmd.arg("--testnet");
        }
        cmd.arg("-t")
            .arg(&dir)
            .arg("owner_api")
            .arg("--run_foreign")
            .arg("-l")
            .arg(port.to_string())
            .current_dir(&dir)
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| FlintError::Wallet(format!("не удалось запустить grin-wallet: {e}")))?;

        // The secret is written at startup; the port accepts a moment later.
        let secret_path = dir.join(".owner_api_secret");
        let deadline = Instant::now() + Duration::from_secs(40);
        let secret = loop {
            if secret_path.is_file() {
                // The wallet creates this file itself. Migrate/verify its ACL
                // before reading the bearer credential; failure aborts launch.
                if let Err(e) = crate::win_acl::restrict(&secret_path) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(FlintError::Wallet(format!(
                        "не удалось защитить .owner_api_secret: {e}"
                    )));
                }
                if let Ok(text) = fs::read_to_string(&secret_path) {
                    if !text.trim().is_empty() {
                        break text.trim().to_string();
                    }
                }
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FlintError::Wallet(
                    "grin-wallet не создал .owner_api_secret за 40 секунд".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        };

        while Instant::now() < deadline {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Ok(Self {
                    child,
                    port,
                    secret,
                    dir,
                });
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = child.kill();
        let _ = child.wait();
        Err(FlintError::Wallet(
            "grin-wallet не открыл порт owner API за 40 секунд".into(),
        ))
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Stop the child and reap it. Used by the scan-cancel path: the owner API
    /// has no cancel method, so a running scan is interrupted by ending the
    /// process. `Drop` uses the same path, so an explicit terminate followed by
    /// drop cannot leave a zombie.
    pub fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WalletProcess {
    fn drop(&mut self) {
        self.terminate();
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

impl Drop for RpcBackend {
    fn drop(&mut self) {
        // The keychain token is a bearer credential for the open wallet.
        use zeroize::Zeroize;
        if let Some(serde_json::Value::String(token)) = self.token.take() {
            let mut token = token;
            token.zeroize();
        }
    }
}

/// Copy old journal records whose slate id is present in this wallet's tx-log.
/// Ambiguous/foreign records stay in the old directory; they are never assigned
/// to a profile by guesswork.
fn import_matching_records(
    old_dir: &Path,
    new_dir: &Path,
    known: &std::collections::HashSet<String>,
) -> Result<usize> {
    let mut imported = 0;
    for entry in std::fs::read_dir(old_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        crate::win_acl::restrict(&path)?;
        let bytes = std::fs::read(&path)?;
        let Ok(record) = serde_json::from_slice::<RpcSendRecord>(&bytes) else {
            continue;
        };
        if !known.contains(&record.slate_id) {
            continue;
        }
        let safe: String = record
            .slate_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if safe.is_empty() {
            continue;
        }
        let target = new_dir.join(format!("{safe}.json"));
        if target.exists() {
            continue;
        }
        crate::win_acl::write_private_atomic(&target, &bytes)?;
        imported += 1;
    }
    Ok(imported)
}

/// Point `grin-wallet.toml` at the managed node's API secret (TOML-escaped).
/// The owner API unwraps the JSON-RPC envelope, but different versions/shapes
/// may still hand back a bare string, `{"key": "..."}` or a wrapped `Ok`.
/// Both the Tor seed and the messenger identity must accept all of them.
fn secret_hex(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("key").and_then(Value::as_str))
        .or_else(|| value.pointer("/result/Ok").and_then(Value::as_str))
        .or_else(|| value.pointer("/Ok").and_then(Value::as_str))
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn set_toml_string(config: &std::path::Path, key: &str, value: &str) -> Result<()> {
    let text = fs::read_to_string(config)?;
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let line = format!("{key} = \"{escaped}\"");
    let mut out = String::new();
    let mut found = false;
    for existing in text.lines() {
        if existing.trim_start().starts_with(key) {
            out.push_str(&line);
            found = true;
        } else {
            out.push_str(existing);
        }
        out.push('\n');
    }
    if !found {
        if let Some(pos) = out.find("check_node_api_http_addr") {
            if let Some(eol) = out[pos..].find('\n') {
                out.insert_str(pos + eol + 1, &format!("{line}\n"));
            }
        } else {
            // Keep it before any `[section]`: appending would make the key a
            // member of the last section (e.g. `[tor]`).
            out.insert_str(0, &format!("{line}\n"));
        }
    }
    fs::write(config, out)?;
    Ok(())
}

fn set_toml_path(config: &std::path::Path, key: &str, value: &std::path::Path) -> Result<()> {
    set_toml_string(config, key, &value.display().to_string())
}

pub fn patch_node_secret(config: &std::path::Path, secret: &std::path::Path) -> Result<()> {
    set_toml_path(config, "node_api_secret_path", secret)
}

/// Remove `node_api_secret_path` from the wallet config. Used when an external
/// node is configured without an explicit secret: a stale line would otherwise
/// keep sending the managed node's local bearer credential to that endpoint.
fn clear_node_secret(config: &std::path::Path) -> Result<()> {
    let text = fs::read_to_string(config)?;
    let mut out = String::new();
    let mut changed = false;
    for line in text.lines() {
        if line.trim_start().starts_with("node_api_secret_path") {
            changed = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if changed {
        fs::write(config, out)?;
    }
    Ok(())
}

/// The wallet process must talk to the same node the readiness check asks.
/// `grin-wallet init` writes `check_node_api_http_addr` once; without this the
/// GUI could check a new endpoint while the wallet kept the old one.
///
/// The API secret is written to a per-wallet file with an owner-only ACL rather
/// than reused from the managed node: an external node has its own secret.
fn patch_node_config(
    config: &std::path::Path,
    url: &str,
    secret: Option<&str>,
    wallet_dir: &std::path::Path,
) -> Result<()> {
    set_toml_string(config, "check_node_api_http_addr", url)?;
    let Some(secret) = secret.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let path = wallet_dir.join(".node_api_secret");
    // A node API secret is a bearer credential; do not leave it under the
    // inherited profile ACL, including the temporary plaintext.
    crate::win_acl::write_private_atomic(&path, secret.as_bytes())?;
    patch_node_secret(config, &path)?;
    Ok(())
}

/// `grin-wallet init` writes absolute paths into `grin-wallet.toml`. A portable
/// folder may be moved or unzipped elsewhere, so rewrite them from the actual
/// directory before every launch.
pub fn patch_wallet_paths(config: &std::path::Path, dir: &std::path::Path) -> Result<()> {
    set_toml_path(config, "data_file_dir", &dir.join("wallet_data"))?;
    set_toml_path(config, "api_secret_path", &dir.join(".owner_api_secret"))?;
    set_toml_path(config, "send_config_dir", dir)
}

pub fn wallet_dir(paths: &AppPaths, network: &str, profile: Option<&str>) -> PathBuf {
    match profile
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != "default")
    {
        Some(name) => paths.root().join(network).join("rpc").join(name),
        None => paths.root().join(network).join("wallet-rpc"),
    }
}

/// Saved response for an inbound slate: if we crash after the foreign API built
/// it, a retry returns this armor instead of failing with
/// `TransactionAlreadyReceived`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RpcReceiveRecord {
    pub slate_id: String,
    pub amount: u64,
    /// `Null` while the durable receive intent exists but no response was saved.
    pub response: Value,
    /// Canonical S1 returned by `slate_from_slatepack_message` for the first
    /// attempt. A repeated request with the same slate id must match it.
    #[serde(default)]
    pub request: Option<Value>,
    /// Sender address from the incoming slatepack, so a resent answer can be
    /// encrypted to them again.
    #[serde(default)]
    pub sender: Option<String>,
    /// `intent` | `received` | `lost`; old records default to `received`.
    #[serde(default = "default_receive_state")]
    pub state: String,
    pub created_at: i64,
}

fn default_receive_state() -> String {
    "received".to_string()
}

/// A repeated receive must carry the same S1 the journal was built for; a
/// missing fingerprint (legacy record) refuses the replay instead of trusting id.
fn check_receive_request(record: &RpcReceiveRecord, slate: &Value) -> Result<()> {
    let expected = record.request.as_ref().ok_or_else(|| {
        FlintError::Wallet("receive journal без отпечатка исходного S1: повтор запрещён".into())
    })?;
    if expected != slate {
        return Err(FlintError::Wallet(
            "повтор receive с тем же slate id, но другим содержимым отклонён".into(),
        ));
    }
    Ok(())
}

fn receive_recovery_capability(version: &Value) -> Result<()> {
    match version
        .get("receive_response_recovery")
        .and_then(Value::as_u64)
    {
        Some(level) if level >= 1 => Ok(()),
        _ => Err(FlintError::Wallet(
            "RPC grin-wallet не поддерживает восстановление ответа получателя \
             (нужна receive_response_recovery >= 1). Обновите grin-wallet из \
             закреплённого Flint fork; receive_tx не вызывался"
                .into(),
        )),
    }
}

fn mark_receive_failed(record: &mut RpcReceiveRecord, error: &str) {
    record.state = if crate::error::receive_tx_already_mutated(error) {
        "lost"
    } else {
        "intent"
    }
    .to_string();
    record.response = Value::Null;
}

/// Journal record for an RPC send: which stage the transfer reached, so a crash
/// between "signed" and "published" can be resumed without signing again.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RpcSendRecord {
    pub slate_id: String,
    pub destination: String,
    pub amount: u64,
    /// `locked` | `finalizing` | `finalized_not_posted` | `posted`
    pub stage: String,
    /// Exact S1 armor persisted before delivery so the pending center can
    /// re-export/resend the original request without re-signing.
    #[serde(default)]
    pub request: Option<String>,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: i64,
}

/// Thin adapter: process + encrypted RPC + mapping into wire types.
pub struct RpcBackend {
    rpc: WalletRpc,
    token: Option<Value>,
    min_confirmations: u64,
    address_index: u32,
    journal_dir: Option<PathBuf>,
    /// Та же нода, которую использует owner API; нужна для публикации сырой
    /// транзакции из `.grintx` без потери подписи в компактном slate.
    node: Option<NodeSnapshot>,
    /// Balance not verified by a scan yet (per wallet profile).
    needs_scan: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetryAction {
    Noop,
    Reconcile,
    Publish,
}

fn required_journal_path(
    path: Option<std::path::PathBuf>,
    what: &str,
) -> Result<std::path::PathBuf> {
    path.ok_or_else(|| {
        FlintError::Invalid(format!(
            "журнал RPC не настроен — сохранение ({what}) запрещено"
        ))
    })
}

fn slate_amount_nano(slate: &serde_json::Value) -> Result<String> {
    let raw = slate
        .get("amt")
        .and_then(Value::as_str)
        .ok_or_else(|| FlintError::Wallet("slate: отсутствует поле amt".into()))?;
    let amount = raw
        .parse::<u64>()
        .map_err(|_| FlintError::Wallet(format!("slate: amt не число: {raw}")))?;
    if amount == 0 {
        return Err(FlintError::Wallet("slate: amt равен нулю".into()));
    }
    Ok(amount.to_string())
}

fn slate_fee_nano(slate: &serde_json::Value) -> String {
    slate
        .get("fee")
        .or_else(|| slate.pointer("/fee_fields/fee"))
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_u64().map(|n| n.to_string()))
        })
        .unwrap_or_else(|| "0".to_string())
}

fn finalize_allowed(stage: &str) -> bool {
    matches!(stage, "prepared" | "locked")
}

fn mark_finalize_failed(record: &mut RpcSendRecord, error: String) {
    // Only a successful finalize_tx return may create finalized_not_posted.
    record.stage = "failed_recoverable".to_string();
    record.last_error = Some(error);
}

fn retry_action(stage: &str) -> Result<RetryAction> {
    match stage {
        "posted" => Ok(RetryAction::Noop),
        "finalizing" => Ok(RetryAction::Reconcile),
        "finalized_not_posted" => Ok(RetryAction::Publish),
        other => Err(FlintError::Invalid(format!(
            "retry_post: повторная публикация запрещена в состоянии {other}"
        ))),
    }
}

fn validate_stored_transaction(tx: &Transaction) -> Result<()> {
    if tx.body.kernels.is_empty() {
        return Err(FlintError::Wallet(
            "сохранённая сделка не содержит kernel; публикация запрещена".into(),
        ));
    }
    for kernel in &tx.body.kernels {
        if !kernel.excess.0.iter().any(|byte| *byte != 0)
            || !kernel
                .excess_sig
                .to_raw_data()
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(FlintError::Wallet(
                "сохранённая сделка имеет пустую подпись kernel; вероятна потеря sigs в компактном slate"
                    .into(),
            ));
        }
    }
    Ok(())
}

impl RpcBackend {
    pub fn connect(url: &str, secret: &str, min_confirmations: u64) -> Result<Self> {
        Ok(Self {
            rpc: WalletRpc::connect(url, secret)?,
            token: None,
            min_confirmations,
            address_index: 0,
            journal_dir: None,
            node: None,
            needs_scan: false,
        })
    }

    /// Where send-stage records live; without it the backend still works but
    /// cannot resume after a crash between finalize and publish.
    pub fn set_journal_dir(&mut self, dir: PathBuf) {
        self.journal_dir = Some(dir);
    }

    /// Публикация recovery идёт напрямую в ту же ноду, минуя lossy compact slate.
    pub fn set_node_snapshot(&mut self, node: NodeSnapshot) {
        self.node = Some(node);
    }

    fn record_path(&self, slate_id: &str) -> Option<PathBuf> {
        let dir = self.journal_dir.as_ref()?;
        let safe: String = slate_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if safe.is_empty() {
            return None;
        }
        Some(dir.join(format!("{safe}.json")))
    }

    fn save_record(&self, record: &RpcSendRecord) -> Result<()> {
        let path = required_journal_path(self.record_path(&record.slate_id), "операция")?;
        let payload = serde_json::to_vec_pretty(record)
            .map_err(|e| FlintError::Invalid(format!("journal: {e}")))?;
        crate::win_acl::write_private_atomic(&path, &payload)?;
        Ok(())
    }

    fn load_record(&self, slate_id: &str) -> Result<Option<RpcSendRecord>> {
        let Some(path) = self.record_path(slate_id) else {
            return Err(FlintError::Invalid(
                "журнал RPC не настроен — денежная операция запрещена".into(),
            ));
        };
        if !path.exists() {
            return Ok(None);
        }
        crate::win_acl::restrict(&path)?;
        let bytes = std::fs::read(&path)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| FlintError::Invalid(format!("журнал RPC повреждён: {e}")))
    }

    /// Import records from the shared pre-profile directory, but only those
    /// whose UUID is present in this wallet's current upstream tx-log. A record
    /// from another profile has no matching ID and is left untouched; this is
    /// what prevents profile A's transaction from appearing in profile B.
    pub fn migrate_legacy_journal(&self, old_dir: &Path) -> Result<usize> {
        let Some(new_dir) = self.journal_dir.as_ref() else {
            return Err(FlintError::Invalid("журнал RPC не настроен".into()));
        };
        if !old_dir.is_dir() || old_dir == new_dir {
            return Ok(0);
        }
        let raw = self.rpc.txs(self.token()?, true)?;
        let entries: Vec<TxLogEntry> = serde_json::from_value(Value::Array(raw))
            .map_err(|e| FlintError::Wallet(format!("txs: {e}")))?;
        let known: std::collections::HashSet<String> = entries
            .iter()
            .filter_map(|t| t.tx_slate_id.map(|u| u.to_string()))
            .collect();
        import_matching_records(old_dir, new_dir, &known)
    }

    fn drop_record(&self, slate_id: &str) {
        if let Some(path) = self.record_path(slate_id) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Secret key of the address at `index`, as the owner API serialises it.
    pub fn slatepack_secret_key(&self, index: u32) -> Result<Value> {
        let token = self.token()?.clone();
        self.rpc
            .call("get_slatepack_secret_key", json!([token, index]))
    }

    /// 32-byte ed25519 secret for the messenger identity. Callers must zeroize.
    pub fn messenger_seed(&self, index: u32) -> Result<[u8; 32]> {
        let value = self.slatepack_secret_key(index)?;
        let text = secret_hex(&value).ok_or_else(|| {
            FlintError::Wallet("get_slatepack_secret_key: неожиданный ответ".into())
        })?;
        let bytes = decode_hex(text)?;
        bytes
            .try_into()
            .map_err(|_| FlintError::Wallet("get_slatepack_secret_key: ожидались 32 байта".into()))
    }

    /// Rotation index used for addresses, summaries and decryption. The app keeps
    /// it in settings, so the backend follows instead of assuming index 0.
    pub fn set_address_index(&mut self, index: u32) {
        self.address_index = index;
    }

    pub fn set_needs_scan(&mut self, needs_scan: bool) {
        self.needs_scan = needs_scan;
    }

    /// Follow a settings change without restarting the wallet process. The
    /// value is otherwise frozen at `connect` time, so a GUI change would apply
    /// only after a restart.
    pub fn set_min_confirmations(&mut self, min_confirmations: u64) {
        self.min_confirmations = min_confirmations;
    }

    /// True when the database was recreated empty by an unlock and the balance
    /// is unknown until a scan (see `open_with_envelope`).
    pub fn needs_scan(&self) -> bool {
        self.needs_scan
    }

    /// Open with an explicit timeout: after a restore the wallet may hold its
    /// lock while scanning, and waiting 30 seconds only freezes the UI.
    pub fn open_with_timeout(
        &mut self,
        password: &str,
        timeout: std::time::Duration,
    ) -> Result<()> {
        match self
            .rpc
            .call_with_timeout("open_wallet", json!([null, password]), timeout)
        {
            Ok(token) => {
                self.token = Some(token);
                Ok(())
            }
            Err(e) => {
                // The response may have been lost after upstream opened the
                // wallet. Best effort close so a failed UI open cannot leave an
                // untracked, effectively open child wallet.
                let _ = self.rpc.close_wallet();
                Err(e)
            }
        }
    }

    pub fn open(&mut self, password: &str) -> Result<()> {
        match self.rpc.open_wallet(password) {
            Ok(token) => {
                self.token = Some(token);
                Ok(())
            }
            Err(e) => {
                let _ = self.rpc.close_wallet();
                Err(e)
            }
        }
    }

    /// Open a wallet whose seed is stored in the Argon2id envelope.
    ///
    /// `grin-wallet` never sees the user password: a transient `wallet.seed`
    /// under a fresh random password is materialized from the envelope, the
    /// wallet is opened with that password, the index-0 address is checked
    /// against the envelope, and only then is the file removed. Any failure puts
    /// the previous state back ([`SeedStore::rollback_unlock`]).
    pub fn open_with_envelope(
        &mut self,
        store: &SeedStore,
        password: &str,
        timeout: Duration,
    ) -> Result<()> {
        // Wrong password fails here, before anything is written.
        let prepared = store.prepare_unlock(password)?;
        if let Err(e) = self.open_with_timeout(prepared.password(), timeout) {
            store.rollback_unlock(&prepared);
            return Err(e);
        }
        let actual = match self.address_at(0) {
            Ok(address) => address,
            Err(e) => {
                let _ = self.lock();
                store.rollback_unlock(&prepared);
                return Err(e);
            }
        };
        if let Err(e) = store.commit_unlock(&prepared, &actual) {
            let _ = self.lock();
            return Err(e);
        }
        // A missing database is recreated empty by upstream: the balance is
        // unknown until a scan, not zero.
        if prepared.fresh_db() {
            self.needs_scan = true;
        }
        Ok(())
    }

    /// Change the wallet password through the owner API.
    ///
    /// Upstream `change_password` takes `(name, old, new)` and does not include
    /// the session token in params (the HTTP Basic auth already identifies the
    /// process). The token is still required here to refuse a locked wallet.
    pub fn change_password(&self, old: &str, new: &str) -> Result<()> {
        let _token = self.token()?;
        self.rpc.call("change_password", json!([null, old, new]))?;
        Ok(())
    }

    pub fn lock(&mut self) -> Result<()> {
        if self.token.is_some() {
            self.rpc.close_wallet()?;
            self.token = None;
        }
        Ok(())
    }

    pub fn unlocked(&self) -> bool {
        self.token.is_some()
    }

    fn token(&self) -> Result<&Value> {
        self.token.as_ref().ok_or(FlintError::Locked)
    }

    pub fn address(&self) -> Result<String> {
        self.address_at(self.address_index)
    }

    /// Address for an explicit derivation index (`m/0/1/<index>`).
    pub fn address_at(&self, index: u32) -> Result<String> {
        let value = self.rpc.call(
            "get_slatepack_address",
            serde_json::json!([self.token()?, index]),
        )?;
        value
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("address")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .ok_or_else(|| FlintError::Wallet("адрес: неожиданный ответ".into()))
    }

    /// Read-only foreign API call (used for health checks and debugging).
    pub fn foreign(&self, method: &str) -> Result<Value> {
        self.rpc.foreign_call(method, json!([]))
    }

    /// Новый receive разрешён только с atomic recovery в upstream. Этот вызов
    /// не меняет wallet state и потому безопасен до durable intent/подписи.
    fn require_receive_recovery(&self) -> Result<()> {
        let version = self.rpc.foreign_call("check_version", json!([]))?;
        receive_recovery_capability(&version)
    }

    /// Indices tried when decrypting incoming slatepacks (rotation-aware).
    fn decrypt_indices(&self) -> Vec<u32> {
        (0..=self.address_index.min(63)).collect()
    }

    /// Build, armor and lock an outgoing transaction.
    /// Returns `(slatepack, slate_id, fee_nano)`.
    pub fn send_prepare(
        &mut self,
        destination: &str,
        amount: u64,
    ) -> Result<(String, String, String)> {
        use grin_wallet_libwallet::InitTxArgs;

        if self.journal_dir.is_none() {
            return Err(FlintError::Invalid(
                "журнал RPC не настроен — отправка запрещена".into(),
            ));
        }
        let token = self.token()?.clone();
        let args = InitTxArgs {
            amount,
            minimum_confirmations: self.min_confirmations,
            max_outputs: 500,
            num_change_outputs: 1,
            selection_strategy_is_use_all: true,
            ..Default::default()
        };
        let slate = self.rpc.call("init_send_tx", json!([token, args]))?;
        let slate_id = slate
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let fee = slate_fee_nano(&slate);

        let slatepack = self.rpc.call(
            "create_slatepack_message",
            // Include our own address so the receiver can encrypt the answer to
            // us instead of sending it in clear text.
            json!([token, slate.clone(), self.address_index, [destination]]),
        )?;
        let slatepack = slatepack
            .as_str()
            .ok_or_else(|| FlintError::Wallet("slatepack: неожиданный ответ".into()))?
            .to_string();

        // Record the stage before locking, so a crash after the lock still leaves
        // a trace saying "built, not answered yet".
        let record = RpcSendRecord {
            slate_id: slate_id.clone(),
            destination: destination.to_string(),
            amount,
            stage: "locked".to_string(),
            request: Some(slatepack.clone()),
            attempts: 0,
            last_error: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.save_record(&record)?;
        // Lock only after the slatepack exists: a failure before this point
        // leaves no locked inputs behind.
        if let Err(e) = self.rpc.call("tx_lock_outputs", json!([token, slate])) {
            let mut record = record;
            record.last_error = Some(e.to_string());
            let _ = self.save_record(&record);
            return Err(e);
        }
        #[cfg(debug_assertions)]
        if std::env::var("FLINT_CRASH_AFTER_PREPARE").is_ok() {
            // Crash-test hook: journal and locked inputs exist; the caller exits.
            return Err(FlintError::Invalid("CRASH_AFTER_PREPARE".into()));
        }
        Ok((slatepack, slate_id, fee))
    }

    /// Apply the payer's response, finalize and broadcast.
    pub fn send_finalize(&mut self, slate_id: &str, response: &str) -> Result<()> {
        let token = self.token()?.clone();
        let indices = self.decrypt_indices();
        let slate = self.rpc.call(
            "slate_from_slatepack_message",
            json!([token, response, indices]),
        )?;
        let response_id = slate.get("id").and_then(Value::as_str).unwrap_or_default();
        if !slate_id.is_empty() && response_id != slate_id {
            return Err(FlintError::Invalid(format!(
                "ответ от другой сделки: {response_id}, ожидался {slate_id}"
            )));
        }

        let mut record = self.load_record(slate_id)?;
        if record.is_none() {
            return Err(FlintError::Invalid(
                "журнал RPC не содержит запись сделки — финализация запрещена; восстановите журнал"
                    .into(),
            ));
        }
        if record
            .as_ref()
            .map(|r| r.stage == "posted")
            .unwrap_or(false)
        {
            // Already handed to the node: never sign again.
            return Ok(());
        }
        if record
            .as_ref()
            .map(|r| r.stage == "finalized_not_posted")
            .unwrap_or(false)
        {
            // Signed earlier but publishing failed: publish the stored tx.
            let mut record = record.take().expect("checked above");
            return self.post_stored(slate_id, &mut record);
        }
        if record
            .as_ref()
            .map(|r| r.stage == "finalizing")
            .unwrap_or(false)
        {
            // The process died after writing the intent but before the result
            // was known. Never sign again and never publish a possibly partial
            // stored transaction; leave explicit manual recovery.
            let record = record.take().expect("checked above");
            return self.reconcile_finalizing(slate_id, record);
        }

        if let Some(r) = record.as_ref() {
            if !finalize_allowed(&r.stage) {
                return Err(FlintError::Invalid(format!(
                    "финализация запрещена в состоянии {}; требуется ручное восстановление",
                    r.stage
                )));
            }
        }

        // Durable intent BEFORE the call: a crash between upstream signing and
        // the journal update must not leave `locked` (which would invite a
        // second finalize_tx).
        if let Some(r) = record.as_mut() {
            r.stage = "finalizing".to_string();
            r.last_error = None;
            self.save_record(r)?;
        }
        #[cfg(debug_assertions)]
        if std::env::var("FLINT_CRASH_BEFORE_FINALIZE").is_ok() {
            // Crash-test hook: the intent is durable, the transaction is not.
            return Err(FlintError::Invalid("CRASH_BEFORE_FINALIZE".into()));
        }
        let final_slate = match self.rpc.call("finalize_tx", json!([token, slate])) {
            Ok(slate) => slate,
            Err(e) => {
                if let Some(r) = record.as_mut() {
                    mark_finalize_failed(r, e.to_string());
                    self.save_record(r)?;
                }
                return Err(FlintError::Wallet(format!(
                    "результат finalize_tx неизвестен: {e}; повторная подпись и автоматическая публикация запрещены"
                )));
            }
        };
        if let Some(r) = record.as_mut() {
            r.stage = "finalized_not_posted".to_string();
            self.save_record(r)?;
        }
        #[cfg(debug_assertions)]
        if std::env::var("FLINT_CRASH_AFTER_FINALIZE").is_ok() {
            // Crash-test hook: the transaction is signed and stored, never sent.
            return Err(FlintError::Invalid("CRASH_AFTER_FINALIZE".into()));
        }
        if let Err(e) = self.rpc.call("post_tx", json!([token, final_slate, false])) {
            if let Some(r) = record.as_mut() {
                r.attempts += 1;
                r.last_error = Some(e.to_string());
                self.save_record(r)?;
            }
            return Err(e);
        }
        if let Some(mut r) = record {
            r.stage = "posted".to_string();
            r.last_error = None;
            self.save_record(&r)?;
        }
        Ok(())
    }

    /// Recover a `finalizing` record after an ambiguous `finalize_tx` result.
    ///
    /// `init_send_tx` also stores a partial S1 transaction, so `get_stored_tx`
    /// succeeding is NOT proof that the transaction was signed. Publishing that
    /// S1 produced a false `posted` state. Until the journal can prove signing
    /// (e.g. an explicit signed marker written after `finalize_tx`), keep the
    /// record for manual recovery and never publish automatically.
    fn reconcile_finalizing(&mut self, slate_id: &str, mut record: RpcSendRecord) -> Result<()> {
        record.stage = "failed_recoverable".to_string();
        record.last_error = Some(
            "finalizing: результат finalize_tx неизвестен; автоматическая публикация запрещена"
                .to_string(),
        );
        self.save_record(&record)?;
        Err(FlintError::Wallet(format!(
            "сделка {slate_id} в состоянии finalizing: нельзя доказать, что сохранённая сделка подписана; автоматическая публикация запрещена, запись оставлена для ручного восстановления"
        )))
    }

    fn remove_record(&self, slate_id: &str) {
        if let Some(path) = self.record_path(slate_id) {
            let _ = fs::remove_file(path);
        }
    }

    /// Failure after `tx_lock_outputs` but before `finalize_tx`: try to cancel
    /// and unlock the inputs. If cancellation fails, keep the record as
    /// `failed_recoverable` so the UI still offers cancel/publish.
    fn fail_online_before_finalize(
        &mut self,
        token: &Value,
        record: &mut RpcSendRecord,
        error: FlintError,
    ) -> FlintError {
        record.last_error = Some(error.to_string());
        let canceled = self
            .rpc
            .call("cancel_tx", json!([token, null, record.slate_id]))
            .is_ok();
        if canceled {
            self.remove_record(&record.slate_id);
        } else {
            record.stage = "failed_recoverable".to_string();
            let _ = self.save_record(record);
        }
        error
    }

    /// Прочитать именно бинарную транзакцию, которую upstream сохранил после
    /// finalize. RPC `get_stored_tx` возвращает компактный slate без `sigs`;
    /// обратная конвертация такого slate создаёт нулевые excess/signature.
    fn read_stored_transaction(&self, slate_id: &str) -> Result<Transaction> {
        let journal = required_journal_path(self.journal_dir.clone(), "публикация")?;
        let root = journal
            .parent()
            .ok_or_else(|| FlintError::Wallet("журнал RPC не имеет каталога кошелька".into()))?;
        let id = uuid::Uuid::parse_str(slate_id).map_err(|_| {
            FlintError::Invalid("некорректный slate id для сохранённой сделки".into())
        })?;
        let path = root
            .join("wallet_data")
            .join("saved_txs")
            .join(format!("{id}.grintx"));
        let raw = fs::read_to_string(&path).map_err(|e| {
            FlintError::Wallet(format!("не удалось прочитать подписанную сделку {id}: {e}"))
        })?;
        let bytes = grin_util::from_hex(raw.trim())
            .map_err(|e| FlintError::Wallet(format!("сохранённая сделка {id} не hex: {e}")))?;
        let tx: Transaction = ser::deserialize(
            &mut &bytes[..],
            ProtocolVersion(1),
            DeserializationMode::default(),
        )
        .map_err(|e| FlintError::Wallet(format!("сохранённая сделка {id} повреждена: {e}")))?;
        validate_stored_transaction(&tx)?;
        Ok(tx)
    }

    fn publish_stored(&self, slate_id: &str, fluff: bool) -> Result<()> {
        let tx = self.read_stored_transaction(slate_id)?;
        let node = self.node.as_ref().ok_or_else(|| {
            FlintError::Invalid("для повторной публикации не настроена нода".into())
        })?;
        node.client()?
            .post_tx(&tx, fluff)
            .map_err(FlintError::wallet)
    }

    /// Publish a transaction that was already signed. Used by retry and by
    /// `send_finalize` when a previous attempt stopped between signing and
    /// broadcasting.
    fn post_stored(&mut self, slate_id: &str, record: &mut RpcSendRecord) -> Result<()> {
        match self.publish_stored(slate_id, false) {
            Ok(_) => {
                record.stage = "posted".to_string();
                record.last_error = None;
                self.save_record(record)?;
                Ok(())
            }
            Err(e) => {
                record.attempts += 1;
                record.last_error = Some(e.to_string());
                self.save_record(record)?;
                Err(e)
            }
        }
    }

    /// Journal entries that cannot be parsed. The UI must not present them as
    /// "no pending transactions": an unreadable file may still be a locked
    /// transaction.
    pub fn pending_unreadable(&self) -> Result<Vec<String>> {
        let Some(dir) = &self.journal_dir else {
            return Ok(Vec::new());
        };
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(dir).map_err(|e| {
            FlintError::Wallet(format!(
                "pending: не удалось прочитать каталог {}: {e}",
                dir.display()
            ))
        })?;
        let mut unreadable = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                FlintError::Wallet(format!("pending: не удалось прочитать запись: {e}"))
            })?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name.ends_with(".recv.json") {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                unreadable.push(name);
                continue;
            };
            if serde_json::from_slice::<RpcSendRecord>(&bytes).is_err() {
                unreadable.push(name);
            }
        }
        unreadable.sort();
        Ok(unreadable)
    }

    pub fn journal_dir_path(&self) -> Option<std::path::PathBuf> {
        self.journal_dir.clone()
    }

    /// Re-broadcast a stored transaction with `fluff`, so the node relays it
    /// immediately instead of leaving it in the stem phase.
    pub fn republish(&mut self, slate_id: &str) -> Result<()> {
        self.publish_stored(slate_id, true)
    }

    /// Re-broadcast a signed transaction without signing it again.
    /// Export the exact stored S1 request for resend. Only a signed request
    /// that has not been finalized may be exported; a posted transfer is not
    /// reversible by resending the request.
    pub fn export_send_request(&self, slate_id: &str) -> Result<String> {
        let record = self
            .load_record(slate_id)?
            .ok_or_else(|| FlintError::Invalid(format!("нет записи о сделке {slate_id}")))?;
        if !matches!(
            record.stage.as_str(),
            "prepared" | "locked" | "failed_recoverable"
        ) {
            return Err(FlintError::Invalid(format!(
                "экспорт запроса запрещён в состоянии {}",
                record.stage
            )));
        }
        record
            .request
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                FlintError::Invalid("S1 не сохранён в журнале; повторная подпись запрещена".into())
            })
    }

    pub fn retry_post(&mut self, slate_id: &str) -> Result<()> {
        let mut record = self
            .load_record(slate_id)?
            .ok_or_else(|| FlintError::Invalid(format!("нет записи о сделке {slate_id}")))?;
        match retry_action(&record.stage)? {
            RetryAction::Noop => Ok(()),
            RetryAction::Reconcile => self.reconcile_finalizing(slate_id, record),
            RetryAction::Publish => self.post_stored(slate_id, &mut record),
        }
    }

    /// Online send over Tor: build the slate, post it to the recipient's onion
    /// (foreign `receive_tx`), then finalize and broadcast.
    ///
    /// On failure after locking, the transaction is cancelled so no inputs stay
    /// locked for a transfer that never happened.
    pub fn send_over_tor(&mut self, destination: &str, amount: u64, socks: &str) -> Result<String> {
        use grin_wallet_libwallet::InitTxArgs;

        let token = self.token()?.clone();
        let onion = crate::address::onion_from_address(destination)?;
        let args = InitTxArgs {
            amount,
            minimum_confirmations: self.min_confirmations,
            max_outputs: 500,
            num_change_outputs: 1,
            selection_strategy_is_use_all: true,
            ..Default::default()
        };
        let slate = self.rpc.call("init_send_tx", json!([token, args]))?;
        let slate_id = slate
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut record = RpcSendRecord {
            slate_id: slate_id.clone(),
            destination: destination.to_string(),
            amount,
            stage: "locking".to_string(),
            request: None,
            attempts: 0,
            last_error: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.save_record(&record)?;
        if let Err(e) = self
            .rpc
            .call("tx_lock_outputs", json!([token, slate.clone()]))
        {
            return Err(self.fail_online_before_finalize(&token, &mut record, e));
        }
        record.stage = "locked".to_string();
        if let Err(e) = self.save_record(&record) {
            return Err(self.fail_online_before_finalize(&token, &mut record, e));
        }

        let body = json!({
            "jsonrpc": "2.0",
            "method": "receive_tx",
            "params": [slate, null, null],
            "id": 1,
        })
        .to_string();
        let posted = crate::socks::http_post_onion(
            socks,
            &onion,
            "/v2/foreign",
            &body,
            std::time::Duration::from_secs(180),
        );

        let (status, text) = match posted {
            Ok(response) => response,
            Err(e) => {
                return Err(self.fail_online_before_finalize(
                    &token,
                    &mut record,
                    FlintError::Wallet(format!("получатель недоступен по Tor: {e}")),
                ));
            }
        };
        if status != 200 {
            return Err(self.fail_online_before_finalize(
                &token,
                &mut record,
                FlintError::Wallet(format!("получатель ответил {status}: {text}")),
            ));
        }
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(e) => {
                return Err(self.fail_online_before_finalize(
                    &token,
                    &mut record,
                    FlintError::Wallet(format!("ответ получателя: {e}")),
                ));
            }
        };
        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            return Err(self.fail_online_before_finalize(
                &token,
                &mut record,
                FlintError::Wallet(format!("получатель: {err}")),
            ));
        }
        let Some(response) = value.pointer("/result/Ok").cloned() else {
            return Err(self.fail_online_before_finalize(
                &token,
                &mut record,
                FlintError::Wallet("ответ получателя без Ok".into()),
            ));
        };

        record.stage = "finalizing".to_string();
        if let Err(e) = self.save_record(&record) {
            return Err(self.fail_online_before_finalize(&token, &mut record, e));
        }
        let final_slate = match self.rpc.call("finalize_tx", json!([token, response])) {
            Ok(slate) => slate,
            Err(e) => {
                mark_finalize_failed(&mut record, e.to_string());
                self.save_record(&record)?;
                return Err(FlintError::Wallet(format!(
                    "результат finalize_tx неизвестен: {e}; повторная подпись и автоматическая публикация запрещены"
                )));
            }
        };
        record.stage = "finalized_not_posted".to_string();
        self.save_record(&record)?;
        match self.rpc.call("post_tx", json!([token, final_slate, false])) {
            Ok(_) => {
                record.stage = "posted".to_string();
                record.last_error = None;
                self.save_record(&record)?;
                Ok(slate_id)
            }
            Err(e) => {
                record.attempts += 1;
                record.last_error = Some(e.to_string());
                self.save_record(&record)?;
                Err(e)
            }
        }
    }

    fn receive_path(&self, slate_id: &str) -> Option<PathBuf> {
        let dir = self.journal_dir.as_ref()?;
        let safe: String = slate_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if safe.is_empty() {
            return None;
        }
        Some(dir.join(format!("{safe}.recv.json")))
    }

    fn save_receive(&self, record: &RpcReceiveRecord) -> Result<()> {
        let path = required_journal_path(self.receive_path(&record.slate_id), "приём")?;
        let payload = serde_json::to_vec_pretty(record)
            .map_err(|e| FlintError::Invalid(format!("receive journal: {e}")))?;
        crate::win_acl::write_private_atomic(&path, &payload)?;
        Ok(())
    }

    fn load_receive(&self, slate_id: &str) -> Result<Option<RpcReceiveRecord>> {
        let Some(path) = self.receive_path(slate_id) else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }
        crate::win_acl::restrict(&path)?;
        let bytes = std::fs::read(&path)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| FlintError::Invalid(format!("журнал приёма RPC повреждён: {e}")))
    }

    /// Sender address of an incoming slatepack, decrypted with our address key.
    pub fn slatepack_sender(&self, armor: &str) -> Option<String> {
        let token = self.token().ok()?;
        for index in self.decrypt_indices() {
            let value = self
                .rpc
                .call("get_slatepack_secret_key", json!([token, index]))
                .ok()?;
            let hex = value.as_str()?;
            let bytes = hex_bytes(hex)?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            let signing = ed25519_dalek::SigningKey::from_bytes(&key);
            let packer = grin_wallet_libwallet::slatepack::Slatepacker::new(
                grin_wallet_libwallet::slatepack::SlatepackerArgs {
                    sender: None,
                    recipients: vec![],
                    dec_key: Some(&signing),
                },
            );
            if let Ok(slatepack) = packer.deser_slatepack(armor.as_bytes(), true) {
                return slatepack.sender.map(|a| a.to_string());
            }
        }
        None
    }

    /// Read the exact stored transaction for diagnostics. Это полный `Transaction`,
    /// а не компактный slate: kernel и подпись здесь не реконструируются.
    /// Это не замена receive recovery: upstream не восстанавливает здесь S2.
    pub fn stored_tx(&self, slate_id: &str) -> Result<Value> {
        serde_json::to_value(self.read_stored_transaction(slate_id)?)
            .map_err(|e| FlintError::Wallet(format!("stored tx json: {e}")))
    }

    /// Local online mode: send to this wallet's own address through the local
    /// foreign API, without Tor. Exercises the exact online pipeline
    /// (init → lock → receive_tx → finalize → post) on a single wallet, which is
    /// what the Tor path uses minus the transport.
    pub fn send_local(&mut self, amount: u64) -> Result<String> {
        use grin_wallet_libwallet::InitTxArgs;

        // Проверка до init_send_tx: старый внешний RPC не должен оставить
        // заблокированные inputs или пройти receive без recovery.
        self.require_receive_recovery()?;
        let token = self.token()?.clone();
        let args = InitTxArgs {
            amount,
            minimum_confirmations: self.min_confirmations,
            max_outputs: 500,
            num_change_outputs: 1,
            selection_strategy_is_use_all: true,
            ..Default::default()
        };
        let slate = self.rpc.call("init_send_tx", json!([token, args]))?;
        let slate_id = slate
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut record = RpcSendRecord {
            slate_id: slate_id.clone(),
            destination: "self".to_string(),
            amount,
            stage: "locking".to_string(),
            request: None,
            attempts: 0,
            last_error: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.save_record(&record)?;
        if let Err(e) = self
            .rpc
            .call("tx_lock_outputs", json!([token, slate.clone()]))
        {
            return Err(self.fail_online_before_finalize(&token, &mut record, e));
        }
        record.stage = "locked".to_string();
        if let Err(e) = self.save_record(&record) {
            return Err(self.fail_online_before_finalize(&token, &mut record, e));
        }

        let body = json!({
            "jsonrpc": "2.0",
            "method": "receive_tx",
            "params": [slate, null, null],
            "id": 1,
        })
        .to_string();
        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                return Err(self.fail_online_before_finalize(
                    &token,
                    &mut record,
                    FlintError::Wallet(format!("local send: {e}")),
                ));
            }
        };
        let posted = client
            .post(format!("{}/v2/foreign", self.rpc.url()))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send();

        let text = match posted.and_then(|r| r.error_for_status()) {
            Ok(response) => response.text().unwrap_or_default(),
            Err(e) => {
                return Err(self.fail_online_before_finalize(
                    &token,
                    &mut record,
                    FlintError::Wallet(format!("локальный приём не ответил: {e}")),
                ));
            }
        };
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(e) => {
                return Err(self.fail_online_before_finalize(
                    &token,
                    &mut record,
                    FlintError::Wallet(format!("локальный ответ: {e}")),
                ));
            }
        };
        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            return Err(self.fail_online_before_finalize(
                &token,
                &mut record,
                FlintError::Wallet(format!("локальный приём: {err}")),
            ));
        }
        let Some(response) = value.pointer("/result/Ok").cloned() else {
            return Err(self.fail_online_before_finalize(
                &token,
                &mut record,
                FlintError::Wallet("локальный ответ без Ok".into()),
            ));
        };

        record.stage = "finalizing".to_string();
        if let Err(e) = self.save_record(&record) {
            return Err(self.fail_online_before_finalize(&token, &mut record, e));
        }
        let final_slate = match self.rpc.call("finalize_tx", json!([token, response])) {
            Ok(slate) => slate,
            Err(e) => {
                mark_finalize_failed(&mut record, e.to_string());
                self.save_record(&record)?;
                return Err(FlintError::Wallet(format!(
                    "результат finalize_tx неизвестен: {e}; повторная подпись и автоматическая публикация запрещены"
                )));
            }
        };
        record.stage = "finalized_not_posted".to_string();
        self.save_record(&record)?;
        match self.rpc.call("post_tx", json!([token, final_slate, false])) {
            Ok(_) => {
                record.stage = "posted".to_string();
                record.last_error = None;
                self.save_record(&record)?;
                Ok(slate_id)
            }
            Err(e) => {
                record.attempts += 1;
                record.last_error = Some(e.to_string());
                self.save_record(&record)?;
                Err(e)
            }
        }
    }

    /// Decrypt an incoming slatepack, let the foreign API build the response and
    /// armor it back. Returns `(response_slatepack, slate_id, amount_nano)`.
    pub fn receive_prepare(&mut self, slatepack: &str) -> Result<(String, String, String)> {
        let token = self.token()?.clone();
        let indices = self.decrypt_indices();
        let slate = self.rpc.call(
            "slate_from_slatepack_message",
            json!([token, slatepack, indices]),
        )?;
        let slate_id = slate
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let amount = slate_amount_nano(&slate)?;
        let request = Some(slate.clone());

        // Encrypt the answer to the sender when their address is present.
        let sender = self.slatepack_sender(slatepack);
        let recipients: Vec<grin_wallet_libwallet::SlatepackAddress> = sender
            .as_deref()
            .and_then(|s| grin_wallet_libwallet::SlatepackAddress::try_from(s).ok())
            .into_iter()
            .collect();

        // A previous attempt may already have built (and saved) the response:
        // return it instead of asking the wallet to receive the slate twice.
        if let Some(record) = self.load_receive(&slate_id)? {
            if matches!(record.state.as_str(), "received" | "intent") {
                check_receive_request(&record, &slate)?;
            }
            match record.state.as_str() {
                "lost" => {
                    return Err(FlintError::Wallet(
                        "входящая сделка уже принята, но ответ был потерян; повторный receive запрещён"
                            .into(),
                    ));
                }
                "received" => {
                    let old: Vec<grin_wallet_libwallet::SlatepackAddress> = record
                        .sender
                        .as_deref()
                        .and_then(|s| grin_wallet_libwallet::SlatepackAddress::try_from(s).ok())
                        .into_iter()
                        .collect();
                    let armored = self.rpc.call(
                        "create_slatepack_message",
                        json!([token, record.response, null, old]),
                    )?;
                    let armored = armored
                        .as_str()
                        .ok_or_else(|| FlintError::Wallet("ответ: неожиданный формат".into()))?
                        .to_string();
                    return Ok((armored, slate_id, record.amount.to_string()));
                }
                "intent" => {}
                other => {
                    return Err(FlintError::Wallet(format!(
                        "receive journal: неизвестное состояние {other}"
                    )));
                }
            }
        } else {
            // Durable intent before the foreign call. A crash after this point
            // leaves a record that says receive_tx may have mutated the wallet.
            let intent = RpcReceiveRecord {
                slate_id: slate_id.clone(),
                amount: amount.parse().unwrap_or(0),
                response: Value::Null,
                request: request.clone(),
                sender: sender.clone(),
                state: "intent".to_string(),
                created_at: chrono::Utc::now().timestamp_millis(),
            };
            self.save_receive(&intent)?;
        }

        // Только intent доходит сюда: received выше armorится без capability,
        // а legacy lost остаётся ручным сценарием и автоматически не оживает.
        self.require_receive_recovery()?;
        // DF-J preparation: durable intent, capability checked, no wallet mutation.
        crate::crash::receive_boundary("intent");
        let response = match self
            .rpc
            .foreign_call("receive_tx", json!([slate, null, null]))
        {
            Ok(response) => response,
            Err(error) => {
                let text = error.to_string();
                let response_unavailable = crate::error::receive_tx_already_mutated(&text);
                let mut record = self.load_receive(&slate_id)?.ok_or_else(|| {
                    FlintError::Wallet("receive_tx завершился ошибкой без журнала приёма".into())
                })?;
                mark_receive_failed(&mut record, &text);
                self.save_receive(&record)?;
                if response_unavailable {
                    return Err(FlintError::Wallet(
                        "входящая сделка уже записана в кошелёк, но сохранённый ответ \
                         недоступен или повреждён. Повторный receive запрещён; \
                         следуйте runbook ручного восстановления"
                            .into(),
                    ));
                }
                return Err(FlintError::Wallet(format!(
                    "receive_tx не выполнен ({text}); запись сохранена как intent, повтор допустим"
                )));
            }
        };

        // OS-level F-04/C failpoint: upstream committed S2, Flint has intent only.
        crate::crash::receive_boundary("upstream");

        // Save before armoring: upstream recovery является второй копией S2,
        // но не отменяет durable журнал Flint и его ACL/atomic write.
        let record = RpcReceiveRecord {
            slate_id: slate_id.clone(),
            amount: amount.parse().unwrap_or(0),
            response: response.clone(),
            request: request.clone(),
            sender: sender.clone(),
            state: "received".to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        crate::crash::receive_journal_write()?;
        self.save_receive(&record)?;
        crate::crash::receive_boundary("journal");

        let armored = self.rpc.call(
            "create_slatepack_message",
            json!([token, response, null, recipients]),
        )?;
        let armored = armored
            .as_str()
            .ok_or_else(|| FlintError::Wallet("ответ: неожиданный формат".into()))?
            .to_string();
        Ok((armored, slate_id, amount))
    }

    /// Rebuild the saved receiver response for a pending RPC receive without
    /// calling receive_tx again. Used by the desktop pending center.
    pub fn export_receive_response(&self, slate_id: &str) -> Result<(String, u64)> {
        let token = self.token()?;
        let record = self
            .load_receive(slate_id)?
            .ok_or_else(|| FlintError::UnknownSlate(slate_id.to_string()))?;
        if record.state != "received" || record.response.is_null() {
            return Err(FlintError::Wallet(
                "RPC: сохранённый ответ отсутствует; повторный receive запрещён".into(),
            ));
        }
        let recipients: Vec<grin_wallet_libwallet::SlatepackAddress> = record
            .sender
            .as_deref()
            .and_then(|s| grin_wallet_libwallet::SlatepackAddress::try_from(s).ok())
            .into_iter()
            .collect();
        let armored = self.rpc.call(
            "create_slatepack_message",
            json!([token, record.response, null, recipients]),
        )?;
        let armored = armored
            .as_str()
            .ok_or_else(|| FlintError::Wallet("ответ: неожиданный формат".into()))?
            .to_string();
        Ok((armored, record.amount))
    }

    /// Ed25519 seed of the address key (`m/0/1/<index>`), used to publish the
    /// onion service. Returned as raw bytes so no seed ever leaves the process.
    pub fn slatepack_seed(&self, index: u32) -> Result<[u8; 32]> {
        let value = self.rpc.call(
            "get_slatepack_secret_key",
            serde_json::json!([self.token()?, index]),
        )?;
        let text = secret_hex(&value)
            .ok_or_else(|| FlintError::Wallet("ключ адреса: неожиданный ответ".into()))?;
        let bytes = grin_util::from_hex(text.trim())
            .map_err(|e| FlintError::Wallet(format!("ключ адреса: {e}")))?;
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| FlintError::Wallet("ключ адреса: ожидались 32 байта".into()))
    }

    /// Pending items derived from the wallet's own tx-log (the source of truth):
    /// every unconfirmed sent or received transaction.
    pub fn pending(&self) -> Result<Vec<crate::service::PendingView>> {
        let raw = self.rpc.txs(self.token()?, true)?;
        let entries: Vec<TxLogEntry> = serde_json::from_value(Value::Array(raw))
            .map_err(|e| FlintError::Wallet(format!("txs: {e}")))?;
        let confirmed: std::collections::HashSet<String> = entries
            .iter()
            .filter(|t| t.confirmed)
            .filter_map(|t| t.tx_slate_id.map(|u| u.to_string()))
            .collect();

        let mut items = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Journal records carry the real stage (locked / signed / published);
        // the tx-log only knows "unconfirmed".
        // Saved receive responses are dropped once the chain confirms them.
        if let Some(dir) = &self.journal_dir {
            if dir.exists() {
                let read = std::fs::read_dir(dir).map_err(|e| {
                    FlintError::Wallet(format!(
                        "pending: не удалось прочитать каталог {}: {e}",
                        dir.display()
                    ))
                })?;
                for entry in read {
                    let entry = entry.map_err(|e| {
                        FlintError::Wallet(format!("pending: не удалось прочитать запись: {e}"))
                    })?;
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.ends_with(".recv.json") {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(entry.path()) else {
                        continue;
                    };
                    if let Ok(record) = serde_json::from_slice::<RpcReceiveRecord>(&bytes) {
                        if confirmed.contains(&record.slate_id) {
                            let _ = std::fs::remove_file(entry.path());
                        } else if record.state != "received" {
                            seen.insert(record.slate_id.clone());
                            items.push(crate::service::PendingView {
                                slate_id: record.slate_id.clone(),
                                kind: "receive".to_string(),
                                amount: record.amount.to_string(),
                                destination: record.sender.clone(),
                                created_at: record.created_at,
                                state: record.state.clone(),
                                attempts: 0,
                                last_error: None,
                            });
                        }
                    }
                }
            }
        }
        if let Some(dir) = &self.journal_dir {
            if dir.exists() {
                let read = std::fs::read_dir(dir).map_err(|e| {
                    FlintError::Wallet(format!(
                        "pending: не удалось прочитать каталог {}: {e}",
                        dir.display()
                    ))
                })?;
                for entry in read {
                    let entry = entry.map_err(|e| {
                        FlintError::Wallet(format!("pending: не удалось прочитать запись: {e}"))
                    })?;
                    if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(entry.path()) else {
                        continue;
                    };
                    let Ok(record) = serde_json::from_slice::<RpcSendRecord>(&bytes) else {
                        continue;
                    };
                    if confirmed.contains(&record.slate_id) {
                        let _ = std::fs::remove_file(entry.path());
                        continue;
                    }
                    seen.insert(record.slate_id.clone());
                    items.push(crate::service::PendingView {
                        slate_id: record.slate_id.clone(),
                        kind: "send".to_string(),
                        amount: record.amount.to_string(),
                        destination: Some(record.destination.clone()),
                        created_at: record.created_at,
                        state: record.stage.clone(),
                        attempts: record.attempts,
                        last_error: record.last_error.clone(),
                    });
                }
            }
        }

        for t in entries.into_iter().filter(|t| !t.confirmed) {
            let fee = t.fee.map(|f| f.fee()).unwrap_or(0);
            let (kind, amount, state) = match t.tx_type {
                TxLogEntryType::TxSent => (
                    "send",
                    // Sent: debited = sum of inputs, credited = change.
                    t.amount_debited
                        .saturating_sub(t.amount_credited)
                        .saturating_sub(fee),
                    // No journal means we cannot prove finalize/post progress.
                    // Never label this "posted": old catalogues only have the
                    // upstream tx-log, and a missing journal must stay unknown.
                    "unknown",
                ),
                TxLogEntryType::TxReceived => ("receive", t.amount_credited, "response_received"),
                TxLogEntryType::TxSentCancelled | TxLogEntryType::TxReceivedCancelled => continue,
                _ => continue,
            };
            let slate_id = t
                .tx_slate_id
                .map(|u| u.to_string())
                .unwrap_or_else(|| format!("tx-{}", t.id));
            if seen.contains(&slate_id) {
                continue;
            }
            items.push(crate::service::PendingView {
                slate_id,
                kind: kind.to_string(),
                amount: amount.to_string(),
                destination: None,
                created_at: t.creation_ts.timestamp_millis(),
                state: state.to_string(),
                attempts: 0,
                last_error: None,
            });
        }
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    /// Cancel a pending transaction by its slate id, releasing locked outputs.
    pub fn cancel(&mut self, slate_id: &str) -> Result<()> {
        let token = self.token()?.clone();
        self.rpc.call("cancel_tx", json!([token, null, slate_id]))?;
        self.drop_record(slate_id);
        Ok(())
    }

    /// Full output scan from `start_height`; returns the node height afterwards.
    /// Scans can take many minutes, so this call is not bound by the default
    /// request timeout.
    pub fn scan(&mut self, start_height: Option<u64>) -> Result<u64> {
        let token = self.token()?.clone();
        self.rpc.call_with_timeout(
            "scan",
            json!([token, start_height, false]),
            std::time::Duration::from_secs(3600),
        )?;
        self.needs_scan = false;
        self.rpc.node_height(&token)
    }

    pub fn summary(&self) -> Result<WalletSummary> {
        let (validated, raw) = self
            .rpc
            .summary(self.token()?, true, self.min_confirmations)?;
        let info: WalletInfo =
            serde_json::from_value(raw).map_err(|e| FlintError::Wallet(format!("summary: {e}")))?;
        Ok(WalletSummary {
            validated,
            address: self.address().unwrap_or_default(),
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
            needs_scan: self.needs_scan,
            // The backend knows only the session-local flag; the app overlays
            // the persisted lifecycle from the wallet profile.
            scan_state: crate::model::ScanState::NotStarted,
            scan_error: None,
        })
    }

    pub fn activity(&self, limit: usize) -> Result<Vec<TxItem>> {
        let raw = self.rpc.txs(self.token()?, true)?;
        let entries: Vec<TxLogEntry> = serde_json::from_value(Value::Array(raw))
            .map_err(|e| FlintError::Wallet(format!("txs: {e}")))?;
        let mut items: Vec<TxItem> = entries
            .into_iter()
            .map(|t| {
                let is_in = matches!(
                    t.tx_type,
                    TxLogEntryType::TxReceived | TxLogEntryType::ConfirmedCoinbase
                );
                let canceled = matches!(
                    t.tx_type,
                    TxLogEntryType::TxSentCancelled | TxLogEntryType::TxReceivedCancelled
                );
                let fee = t.fee.map(|f| f.fee()).unwrap_or(0);
                let amount = if is_in {
                    t.amount_credited
                } else {
                    // Sent: debited = sum of inputs, credited = change.
                    t.amount_debited
                        .saturating_sub(t.amount_credited)
                        .saturating_sub(fee)
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
                    counterparty: None,
                    note: None,
                    tx_type: format!("{:?}", t.tx_type),
                }
            })
            .collect();
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        items.truncate(limit);
        Ok(items)
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return Err(FlintError::Wallet("hex: нечётная длина".into()));
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for pair in bytes.chunks(2) {
        let high = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| FlintError::Wallet("hex: неверный символ".into()))?;
        let low = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| FlintError::Wallet("hex: неверный символ".into()))?;
        out.push(((high as u8) << 4) | low as u8);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_receive_record_defaults_to_received() {
        let record: RpcReceiveRecord = serde_json::from_value(serde_json::json!({
            "slate_id": "00000000-0000-0000-0000-000000000000",
            "amount": 1,
            "response": null,
            "created_at": 0
        }))
        .expect("legacy record");
        assert_eq!(record.state, "received");
    }

    #[test]
    fn old_lost_receive_record_stays_lost() {
        let record: RpcReceiveRecord = serde_json::from_value(serde_json::json!({
            "slate_id": "00000000-0000-0000-0000-000000000000",
            "amount": 1,
            "response": null,
            "state": "lost",
            "created_at": 0
        }))
        .expect("legacy lost record");
        assert_eq!(record.state, "lost");
    }

    #[test]
    fn receive_recovery_capability_is_explicit_and_fail_closed() {
        for unsupported in [
            json!({}),
            json!({"foreign_api_version": 2}),
            json!({"receive_response_recovery": 0}),
            json!({"receive_response_recovery": "1"}),
            json!({"receive_response_recovery": null}),
        ] {
            let error = receive_recovery_capability(&unsupported).expect_err("must reject");
            assert!(error.to_string().contains("receive_tx не вызывался"));
        }
        for supported in [
            json!({"receive_response_recovery": 1}),
            json!({"receive_response_recovery": 2}),
        ] {
            receive_recovery_capability(&supported).expect("supported");
        }
    }

    #[test]
    fn receive_errors_preserve_intent_unless_mutation_is_proven() {
        let mut record = RpcReceiveRecord {
            slate_id: "00000000-0000-0000-0000-000000000000".into(),
            amount: 1,
            response: json!({"old": true}),
            request: None,
            sender: None,
            state: "intent".into(),
            created_at: 0,
        };
        for error in ["timeout", "invalid slate", "DB Not Found"] {
            record.state = "received".into();
            mark_receive_failed(&mut record, error);
            assert_eq!(record.state, "intent", "{error}");
            assert_eq!(record.response, Value::Null);
        }
        for error in [
            "TransactionAlreadyReceived",
            "ReceiveRecoveryUnavailable: slate-id",
            "ReceiveRecoveryIntegrity: mismatch",
        ] {
            record.state = "intent".into();
            mark_receive_failed(&mut record, error);
            assert_eq!(record.state, "lost", "{error}");
        }
    }

    #[test]
    fn slate_amount_requires_nonzero_amt() {
        assert_eq!(
            slate_amount_nano(&serde_json::json!({"amt": "123456789"})).unwrap(),
            "123456789"
        );
        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"amount": "123"}),
            serde_json::json!({"amt": "0"}),
            serde_json::json!({"amt": "abc"}),
        ] {
            assert!(slate_amount_nano(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn slate_fee_accepts_string_and_numeric_json() {
        assert_eq!(
            slate_fee_nano(&serde_json::json!({"fee": "23000000"})),
            "23000000"
        );
        assert_eq!(
            slate_fee_nano(&serde_json::json!({"fee": 23000000})),
            "23000000"
        );
        assert_eq!(
            slate_fee_nano(&serde_json::json!({"fee_fields": {"fee": "23000000"}})),
            "23000000"
        );
        assert_eq!(slate_fee_nano(&serde_json::json!({})), "0");
    }

    #[test]
    fn missing_journal_path_is_an_error_not_a_noop() {
        let error = required_journal_path(None, "операция").expect_err("must fail");
        assert!(error.to_string().contains("журнал RPC не настроен"));
        assert!(required_journal_path(Some(std::path::PathBuf::from("x")), "приём").is_ok());
    }

    #[test]
    fn finalize_is_whitelisted_before_signing() {
        assert!(finalize_allowed("prepared"));
        assert!(finalize_allowed("locked"));
        for forbidden in [
            "finalizing",
            "finalized_not_posted",
            "failed_recoverable",
            "posted",
            "unknown",
        ] {
            assert!(!finalize_allowed(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn every_finalize_error_is_recoverable_and_not_retryable() {
        let mut record = RpcSendRecord {
            slate_id: "00000000-0000-0000-0000-000000000000".into(),
            destination: "tgrin1dest".into(),
            amount: 7,
            stage: "finalizing".into(),
            request: None,
            attempts: 0,
            last_error: None,
            created_at: 1,
        };
        for error in ["DB Not Found", "Slate id", "timeout", "invalid slate"] {
            record.stage = "finalizing".into();
            mark_finalize_failed(&mut record, error.to_string());
            assert_eq!(record.stage, "failed_recoverable", "{error}");
            assert_eq!(record.last_error.as_deref(), Some(error));
            assert!(retry_action(&record.stage).is_err(), "{error}");
        }
    }

    #[test]
    fn retry_post_is_whitelisted_before_any_network_call() {
        assert_eq!(retry_action("posted").unwrap(), RetryAction::Noop);
        assert_eq!(retry_action("finalizing").unwrap(), RetryAction::Reconcile);
        assert_eq!(
            retry_action("finalized_not_posted").unwrap(),
            RetryAction::Publish
        );
        for forbidden in [
            "prepared",
            "locked",
            "failed_recoverable",
            "response_received",
            "unknown",
            "",
        ] {
            assert!(retry_action(forbidden).is_err(), "stage {forbidden}");
        }

        // An invalid stage must not reach the publish arm. This mirrors the
        // dispatch in retry_post(): the whitelist is evaluated first.
        let mut published = 0;
        let mut reconciled = 0;
        for stage in ["locked", "failed_recoverable", "unknown"] {
            if let Ok(action) = retry_action(stage) {
                match action {
                    RetryAction::Noop => {}
                    RetryAction::Reconcile => reconciled += 1,
                    RetryAction::Publish => published += 1,
                }
            }
        }
        assert_eq!(published, 0);
        assert_eq!(reconciled, 0);
    }

    #[test]
    fn stored_transaction_rejects_compact_slate_signature_loss() {
        use grin_core::core::TxKernel;
        use grin_util::secp::{pedersen::Commitment, Signature};

        validate_stored_transaction(&Transaction::empty()).expect_err("kernel required");
        let zero = Transaction::empty().with_kernel(TxKernel::empty());
        let error = validate_stored_transaction(&zero).expect_err("zero signature required");
        assert!(error.to_string().contains("пустую подпись kernel"));

        let mut signed = TxKernel::empty();
        signed.excess = Commitment::from_vec(vec![1; 33]);
        signed.excess_sig = Signature::from_raw_data(&[1; 64]).expect("signature bytes");
        validate_stored_transaction(&Transaction::empty().with_kernel(signed))
            .expect("nonzero stored kernel survives the guard");
    }

    #[test]
    fn legacy_journal_import_only_matches_tx_log() {
        let root = std::env::temp_dir().join(format!("flint-journal-{}", uuid::Uuid::new_v4()));
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(&old).unwrap();
        let record = |id: &str| RpcSendRecord {
            slate_id: id.to_string(),
            destination: "tgrin1dest".into(),
            amount: 7,
            stage: "locked".into(),
            request: None,
            attempts: 0,
            last_error: None,
            created_at: 1,
        };
        for id in [
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        ] {
            std::fs::write(
                old.join(format!("{id}.json")),
                serde_json::to_vec(&record(id)).unwrap(),
            )
            .unwrap();
        }
        std::fs::write(old.join("broken.json"), b"{not json").unwrap();
        let known =
            std::collections::HashSet::from(["aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string()]);
        assert_eq!(import_matching_records(&old, &new, &known).unwrap(), 1);
        assert!(new
            .join("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.json")
            .is_file());
        assert!(!new
            .join("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.json")
            .exists());
        assert!(!new.join("broken.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hex_secret_is_parsed_strictly() {
        let raw = "0d605fea78922543024f7bae6df6c43b051c05a3cad7f39be10e366a77ecf23a";
        let bytes = hex_bytes(raw).expect("hex");
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 0x0d);
        assert_eq!(bytes[31], 0x3a);
        assert!(hex_bytes("abc").is_none(), "odd length is rejected");
        assert!(hex_bytes("zz").is_none(), "non-hex is rejected");
    }

    #[test]
    fn secret_hex_accepts_all_owner_api_shapes() {
        let raw = "0d605fea78922543024f7bae6df6c43b051c05a3cad7f39be10e366a77ecf23a";
        assert_eq!(secret_hex(&serde_json::json!(raw)), Some(raw));
        assert_eq!(secret_hex(&serde_json::json!({"key": raw})), Some(raw));
        assert_eq!(
            secret_hex(&serde_json::json!({"result": {"Ok": raw}})),
            Some(raw)
        );
        assert_eq!(secret_hex(&serde_json::json!({"unexpected": true})), None);
    }

    #[test]
    fn node_config_gets_the_settings_endpoint_and_per_wallet_secret() {
        let dir = std::env::temp_dir().join(format!("flint-nodecfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("grin-wallet.toml");
        std::fs::write(
            &config,
            "data_file_dir = \"C:\\\\old\\\\wallet_data\"\n\
             check_node_api_http_addr = \"http://127.0.0.1:13413\"\n\
             [tor]\n\
             send_config_dir = \"C:\\\\old\"\n",
        )
        .unwrap();

        patch_node_config(&config, "http://10.0.0.5:9999", Some("node-secret"), &dir)
            .expect("patch node config");
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(
            text.contains("check_node_api_http_addr = \"http://10.0.0.5:9999\""),
            "the endpoint from settings must win: {text}"
        );
        assert!(text.contains(".node_api_secret"), "{text}");
        assert!(text.contains("[tor]"), "sections must survive: {text}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".node_api_secret")).unwrap(),
            "node-secret"
        );
        assert!(
            !dir.join(".node_api_secret.tmp").exists(),
            "the secret is renamed into place"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wallet_config_paths_are_rewritten_for_a_moved_folder() {
        let dir = std::env::temp_dir().join(format!("flint-wpaths-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("grin-wallet.toml");
        std::fs::write(
            &config,
            "data_file_dir = \"C:\\\\old\\\\wallet_data\"\n\
             api_secret_path = \"C:\\\\old\\\\.owner_api_secret\"\n\
             check_node_api_http_addr = \"http://127.0.0.1:13413\"\n\
             [tor]\n\
             send_config_dir = \"C:\\\\old\"\n",
        )
        .unwrap();
        let target = std::path::PathBuf::from("C:\\new\\wallet-rpc");
        patch_wallet_paths(&config, &target);
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("data_file_dir = \"C:\\\\new\\\\wallet-rpc\\\\wallet_data\""));
        assert!(text.contains("api_secret_path = \"C:\\\\new\\\\wallet-rpc\\\\.owner_api_secret\""));
        assert!(text.contains("send_config_dir = \"C:\\\\new\\\\wallet-rpc\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wallet_config_gets_the_node_secret_path() {
        let dir = std::env::temp_dir().join(format!("flint-wcfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("grin-wallet.toml");
        std::fs::write(
            &config,
            "check_node_api_http_addr = \"http://127.0.0.1:13413\"\n",
        )
        .unwrap();
        let secret = std::path::PathBuf::from("C:\\data\\node\\.api_secret");
        patch_node_secret(&config, &secret).expect("patch secret");
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("node_api_secret_path"));
        assert!(
            text.contains("data\\\\node\\\\.api_secret"),
            "TOML-escaped: {text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
