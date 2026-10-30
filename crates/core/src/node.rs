//! Managing the official Grin node as a child process.
//!
//! The wallet never reimplements consensus: it talks to `grin` (the Rust node
//! from grin.mw) over its JSON-RPC `/v2/foreign` endpoint. This module only
//! starts, stops and locates that binary so the desktop app is self-contained.

use std::fs::{self, File};
use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::AppPaths;
use crate::error::{FlintError, Result};
use grin_wallet_impls::HTTPNodeClient;

/// Testnet and mainnet use different default API ports.
pub fn default_api_port(network: &str) -> u16 {
    if network == "testnet" {
        13413
    } else {
        3413
    }
}

/// True only for a literal loopback endpoint (`localhost`, 127.0.0.0/8, ::1).
///
/// A managed node's `.api_secret` is a local bearer credential; it must never be
/// attached to an endpoint that merely starts with a loopback string such as
/// `localhost.attacker.invalid`.
pub fn is_loopback_node_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default().trim();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// A persisted node URL is usable only over TLS, or over plain HTTP to a literal
/// loopback host. This closes the "hand-edited settings.json" path that skipped
/// the interactive `update_settings` validation.
pub fn node_url_allowed(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw.trim()) else {
        return false;
    };
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => is_loopback_node_url(raw),
        _ => false,
    }
}

pub fn default_node_url(network: &str) -> String {
    format!("http://127.0.0.1:{}", default_api_port(network))
}

/// Everything that must agree between readiness checks and the wallet: which
/// endpoint is asked, with which secret, on which network and with which
/// confirmation threshold.
///
/// Built once per operation from [`crate::WalletService::node_snapshot`]. The
/// child `grin-wallet` gets the same URL and secret written into its config
/// before launch, so "the node is ready" and "the wallet talks to that node"
/// can no longer describe two different endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeSnapshot {
    pub network: String,
    pub url: String,
    pub secret: Option<String>,
    pub min_confirmations: u64,
}

impl NodeSnapshot {
    pub fn new(
        network: impl Into<String>,
        url: impl Into<String>,
        secret: Option<String>,
        min_confirmations: u64,
    ) -> Self {
        Self {
            network: network.into(),
            url: url.into().trim().to_string(),
            secret,
            min_confirmations,
        }
    }

    /// The managed node's defaults, for tests and for callers that only have
    /// paths (no settings loaded).
    pub fn managed(paths: &AppPaths, network: &str) -> Self {
        let secret_path = paths.node_dir(network).join(".api_secret");
        let secret = crate::win_acl::restrict(&secret_path)
            .ok()
            .and_then(|_| fs::read_to_string(secret_path).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self::new(network, default_node_url(network), secret, 10)
    }

    pub fn client(&self) -> Result<HTTPNodeClient> {
        HTTPNodeClient::new(&self.url, self.secret.clone(), Duration::from_secs(10))
            .map_err(FlintError::wallet)
    }
}

pub struct NodeManager {
    node_dir: PathBuf,
    pub api_port: u16,
    /// Internal loopback port the node itself listens on. The public port is the
    /// CORS-stripping proxy.
    internal_port: u16,
    network: String,
    binary: PathBuf,
    child: Option<Child>,
    proxy: Option<crate::node_proxy::NodeCorsProxy>,
    /// `false` runs the node directly on the public port (direct mode).
    proxy_enabled: bool,
    /// We attached to a node we did not spawn (manual node / direct fallback).
    external: bool,
}

impl NodeManager {
    pub fn new(paths: &AppPaths, network: &str, binary_override: Option<&str>) -> Result<Self> {
        let binary = match binary_override.map(str::trim).filter(|s| !s.is_empty()) {
            Some(path) => PathBuf::from(path),
            None => locate_binary().ok_or_else(|| {
                FlintError::Invalid(
                    "бинарь ноды grin не найден: положите grin.exe в resources/node или укажите путь в настройках"
                        .into(),
                )
            })?,
        };

        Ok(Self {
            node_dir: paths.node_dir(network),
            api_port: default_api_port(network),
            internal_port: default_api_port(network) + 2,
            network: network.to_string(),
            binary,
            child: None,
            proxy: None,
            proxy_enabled: true,
            external: false,
        })
    }

    /// Override loopback ports (tests must not depend on an app already using
    /// the defaults). Production code keeps the network defaults.
    pub fn set_api_ports(&mut self, api_port: u16, internal_port: u16) {
        self.api_port = api_port;
        self.internal_port = internal_port;
    }

    /// Turn the CORS-stripping proxy on/off. Direct mode makes the node bind
    /// the public port itself.
    pub fn set_proxy_enabled(&mut self, enabled: bool) {
        self.proxy_enabled = enabled;
    }

    pub fn proxy_enabled(&self) -> bool {
        self.proxy_enabled
    }

    /// True when we did not spawn the node: it was already healthy on our port.
    pub fn is_external(&self) -> bool {
        self.external
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn node_dir(&self) -> &Path {
        &self.node_dir
    }

    pub fn network(&self) -> &str {
        &self.network
    }

    pub fn is_running(&mut self) -> bool {
        if self.external {
            return true;
        }
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// PID of the child we started; `None` for an external node.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    /// Spawn the node (generating its config on first run) and wait until the
    /// RPC port accepts connections.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        fs::create_dir_all(&self.node_dir)?;

        let config = self.node_dir.join("grin-server.toml");
        if !config.exists() {
            self.write_config()?;
        }
        self.ensure_api_secrets()?;
        // A healthy node already on the public port wins over both modes: this
        // is the manual-node / direct fallback path (no second node, no proxy).
        if self.public_port_is_healthy_node() {
            self.external = true;
            self.child = None;
            self.proxy = None;
            return Ok(());
        }
        if std::net::TcpStream::connect(("127.0.0.1", self.api_port)).is_ok() {
            return Err(FlintError::Node(format!(
                "порт {} занят сервисом, который не отвечает как нода этой сети",
                self.api_port
            )));
        }
        let listen_port = if self.proxy_enabled {
            self.internal_port
        } else {
            self.api_port
        };
        self.patch_api_addr(listen_port)?;
        // Fail closed: never run a node whose config does not point at our
        // secret, that is exactly the "200 without Authorization" hole.
        let expected = self.node_dir.join(".api_secret");
        let text = fs::read_to_string(&config)?;
        let configured = config_path_value(&text, "api_secret_path");
        let addr = config_string_value(&text, "api_http_addr").unwrap_or_default();
        let loopback = addr.starts_with("127.0.0.1:");
        if configured.as_deref() != Some(expected.as_path())
            || self.api_secret().is_none()
            || !loopback
        {
            return Err(FlintError::Node(
                "нода не будет запущена: конфиг не указывает на секрет API или слушает не loopback"
                    .into(),
            ));
        }

        let log_path = self.node_dir.join("node.log");
        let log = File::create(&log_path)?;

        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(&self.node_dir);
        if self.network == "testnet" {
            cmd.arg("--testnet");
        }
        cmd.arg("--no-tui")
            .arg("server")
            .arg("-c")
            .arg(&config)
            .arg("run")
            // Do not inherit the GUI process's stdin: the Ctrl-Break
            // AttachConsole/FreeConsole dance can leave that handle invalid,
            // which makes the next CreateProcess fail with ERROR_INVALID_HANDLE.
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        }

        let child = cmd
            .spawn()
            .map_err(|e| FlintError::Node(format!("не удалось запустить ноду: {e}")))?;
        self.child = Some(child);

        self.wait_for_api(Duration::from_secs(40))?;
        if self.proxy_enabled {
            // Public port: CORS-stripping proxy in front of the node.
            let proxy = crate::node_proxy::NodeCorsProxy::start(self.api_port, self.internal_port)
                .map_err(|e| {
                    FlintError::Node(format!(
                        "не удалось занять порт {} для прокси ноды: {e}",
                        self.api_port
                    ))
                })?;
            self.proxy = Some(proxy);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        // Drop the proxy first so it stops accepting connections, then the node.
        self.proxy = None;
        self.external = false;
        if let Some(mut child) = self.child.take() {
            #[cfg(windows)]
            {
                // Grin installs a ctrlc handler and closes the chain DB only on
                // Ctrl-C/Ctrl-Break. CREATE_NEW_PROCESS_GROUP + a hidden console
                // lets us signal that group; a hard kill is the fallback.
                if send_ctrl_break(child.id()) {
                    let deadline = Instant::now() + Duration::from_secs(20);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => return Ok(()),
                            Ok(None) if Instant::now() < deadline => {
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            _ => break,
                        }
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    /// Is a node we did not start answering on the public port, with our secret
    /// and the right chain/agent? That is the direct fallback.
    fn public_port_is_healthy_node(&self) -> bool {
        let addr = format!("127.0.0.1:{}", self.api_port);
        let Ok(sock) = addr.parse() else {
            return false;
        };
        if std::net::TcpStream::connect_timeout(&sock, Duration::from_millis(300)).is_err() {
            return false;
        }
        let url = format!("http://{addr}");
        fetch_native_status(&url, Duration::from_secs(2), self.api_secret().as_deref())
            .is_some_and(|status| identity_matches(&self.network, &status))
    }

    /// Last lines of the node log, for error surfaces in the UI.
    pub fn log_tail(&self, lines: usize) -> String {
        let path = self.node_dir.join("node.log");
        let Ok(mut file) = File::open(path) else {
            return String::new();
        };
        let mut buf = String::new();
        let _ = file.read_to_string(&mut buf);
        let all: Vec<&str> = buf.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    /// Rewrite `api_http_addr` so the node binds the internal loopback port.
    fn patch_api_addr(&self, port: u16) -> Result<()> {
        let path = self.node_dir.join("grin-server.toml");
        let text = fs::read_to_string(&path)?;
        let line = format!("api_http_addr = \"127.0.0.1:{port}\"");
        let mut out = String::new();
        let mut found = false;
        for existing in text.lines() {
            if existing.trim_start().starts_with("api_http_addr") {
                out.push_str(&line);
                found = true;
            } else {
                out.push_str(existing);
            }
            out.push('\n');
        }
        if !found {
            return Err(FlintError::Node(
                "конфиг ноды без api_http_addr — некуда перевести внутренний порт".into(),
            ));
        }
        fs::write(&path, out)?;
        Ok(())
    }

    fn wait_for_api(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let listen_port = if self.proxy_enabled {
            self.internal_port
        } else {
            self.api_port
        };
        let addr = format!("127.0.0.1:{listen_port}");

        while Instant::now() < deadline {
            if let Some(child) = self.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    let tail = self.log_tail(12);
                    return Err(FlintError::Node(format!(
                        "нода завершилась с {status}. Лог:\n{tail}"
                    )));
                }
            }
            if TcpStream::connect_timeout(&addr.parse().expect("addr"), Duration::from_millis(400))
                .is_ok()
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(300));
        }

        Err(FlintError::Node(
            "нода не открыла RPC-порт за 40 секунд — смотрите node.log".into(),
        ))
    }

    /// `grin server config` writes an absolute-path config into the cwd.
    fn write_config(&self) -> Result<()> {
        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(&self.node_dir);
        if self.network == "testnet" {
            cmd.arg("--testnet");
        }
        cmd.arg("server")
            .arg("config")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let output = cmd
            .output()
            .map_err(|e| FlintError::Node(format!("grin server config: {e}")))?;
        if !output.status.success() {
            return Err(FlintError::Node(format!(
                "grin server config вернул {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Keep the console log useful: the generated config defaults to Warning,
        // which hides peer and sync progress we show in the UI.
        let path = self.node_dir.join("grin-server.toml");
        if let Ok(text) = fs::read_to_string(&path) {
            let patched = text.replace(
                r#"stdout_log_level = "Warning""#,
                r#"stdout_log_level = "Info""#,
            );
            if patched != text {
                fs::write(&path, patched)?;
            }
        }
        Ok(())
    }

    /// The node only enforces API auth when the secret file exists. Create it
    /// before the first start and make sure the config points at it — otherwise
    /// `/v2/owner` answers `200` to anything on loopback.
    fn ensure_api_secrets(&self) -> Result<()> {
        let config = self.node_dir.join("grin-server.toml");
        let mut text = fs::read_to_string(&config)?;
        let api = self.node_dir.join(".api_secret");
        let foreign = self.node_dir.join(".foreign_api_secret");

        for (key, path) in [
            ("api_secret_path", &api),
            ("foreign_api_secret_path", &foreign),
        ] {
            if !text.contains(key) {
                let line = format!("{key} = \"{}\"", path.display());
                let Some(pos) = text.find("api_http_addr") else {
                    return Err(FlintError::Node(
                        "конфиг ноды без api_http_addr — некуда вписать секрет API".into(),
                    ));
                };
                let Some(eol) = text[pos..].find('\n') else {
                    return Err(FlintError::Node(
                        "конфиг ноды без перевода строки после api_http_addr".into(),
                    ));
                };
                let insert = pos + eol + 1;
                text.insert_str(insert, &format!("{line}\n"));
            }
        }
        fs::write(&config, text)?;

        for path in [&api, &foreign] {
            if !path.is_file() {
                crate::win_acl::write_private_atomic(path, random_secret()?.as_bytes())?;
            }
            // Existing pre-ACL secrets are migrated before the node can use them.
            crate::win_acl::restrict(path)?;
        }
        Ok(())
    }

    /// Secret the node expects in `Authorization: Basic base64("grin:<secret>")`.
    pub fn api_secret(&self) -> Option<String> {
        let path = self.node_dir.join(".api_secret");
        crate::win_acl::restrict(&path).ok()?;
        fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// Read a quoted string value from a TOML config line (`key = "value"`).
fn config_string_value(text: &str, key: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(key))?;
    Some(line.split('"').nth(1)?.replace("\\\\", "\\"))
}

/// Read a quoted path value from a TOML config line (`key = "value"`).
/// Windows backslashes arrive escaped, so they are normalized back.
fn config_path_value(text: &str, key: &str) -> Option<PathBuf> {
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(key))?;
    let value = line.split('"').nth(1)?.replace("\\\\", "\\");
    Some(PathBuf::from(value))
}

fn random_secret() -> Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| FlintError::Node("rng: не удалось сгенерировать секрет API".into()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
fn send_ctrl_break(pid: u32) -> bool {
    const CTRL_BREAK_EVENT: u32 = 1;
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    extern "system" {
        fn FreeConsole() -> i32;
        fn AttachConsole(dw_process_id: u32) -> i32;
        fn GenerateConsoleCtrlEvent(dw_ctrl_event: u32, dw_process_group_id: u32) -> i32;
    }
    unsafe {
        // A console test runner is already attached to its own console; detach
        // temporarily so we can attach to the node's hidden console. GUI builds
        // have no console and skip this reattach.
        let had_console = FreeConsole() != 0;
        if AttachConsole(pid) == 0 {
            if had_console {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
            return false;
        }
        let sent = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0;
        FreeConsole();
        if had_console {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
        sent
    }
}

impl Drop for NodeManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Look for the bundled binary or one on PATH.
pub fn locate_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("FLINT_NODE_BIN") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    // Walk up from the executable and the working directory: covers
    // `target/debug/flint-slint.exe`, `target/debug/deps/*.exe` (tests) and the
    // workspace root when the binary is bundled next to the sources.
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
            candidates.push(ancestor.join("resources").join("node").join("grin.exe"));
            candidates.push(ancestor.join("node").join("grin.exe"));
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    #[cfg(feature = "embed-node")]
    if let Some(extracted) = extract_embedded() {
        return Some(extracted);
    }

    None
}

/// Single-file portable build: the node ships inside the exe and is written out
/// next to it on first use.
#[cfg(feature = "embed-node")]
const EMBEDDED_NODE: &[u8] = include_bytes!("../../../resources/node/grin.exe");

#[cfg(feature = "embed-node")]
fn extract_embedded() -> Option<PathBuf> {
    // Portable mode keeps everything under data/ so the folder stays clean.
    let dir = match crate::config::portable_root() {
        Some(root) => root.join("node"),
        None => std::env::current_exe().ok()?.parent()?.join("node"),
    };
    let path = dir.join("grin.exe");

    if path.is_file() {
        return Some(path);
    }
    if fs::create_dir_all(&dir).is_ok() && fs::write(&path, EMBEDDED_NODE).is_ok() {
        return Some(path);
    }

    // Read-only install location: fall back to the temp directory.
    let fallback = std::env::temp_dir().join("flint-node");
    fs::create_dir_all(&fallback).ok()?;
    let path = fallback.join("grin.exe");
    if !path.is_file() {
        fs::write(&path, EMBEDDED_NODE).ok()?;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_follow_network() {
        assert_eq!(default_api_port("testnet"), 13413);
        assert_eq!(default_api_port("mainnet"), 3413);
        assert_eq!(default_node_url("testnet"), "http://127.0.0.1:13413");
    }

    #[test]
    fn start_creates_api_secrets_and_config_paths() {
        let root = std::env::temp_dir().join(format!("flint-node-secret-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = AppPaths::at(&root);
        let node_dir = paths.node_dir("testnet");
        fs::create_dir_all(&node_dir).unwrap();
        fs::write(
            node_dir.join("grin-server.toml"),
            "api_http_addr = \"127.0.0.1:13413\"\n",
        )
        .unwrap();

        let manager = NodeManager::new(&paths, "testnet", None).expect("manager");
        manager.ensure_api_secrets().expect("secrets");

        let secret = manager.api_secret().expect("api secret");
        assert_eq!(secret.len(), 64, "32 bytes as hex");
        assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(node_dir.join(".foreign_api_secret").is_file());

        let config = fs::read_to_string(node_dir.join("grin-server.toml")).unwrap();
        assert!(config.contains("api_secret_path"));
        assert!(config.contains("foreign_api_secret_path"));
        let _ = fs::remove_dir_all(&root);
    }

    fn native(sync: NativeSync, tip: u64) -> NativeStatus {
        NativeStatus {
            sync: Some(sync),
            tip_height: tip,
            connections: 1,
            ..Default::default()
        }
    }

    #[test]
    fn phase_comes_from_the_native_status_only() {
        use NodePhase::*;
        assert_eq!(classify_phase(false, false, None), NotRunning);
        assert_eq!(classify_phase(true, false, None), Starting);
        assert_eq!(
            classify_phase(true, true, Some(&native(NativeSync::HeaderSync, 0))),
            Headers
        );
        assert_eq!(
            classify_phase(true, true, Some(&native(NativeSync::BodySync, 0))),
            Blocks
        );
        let mut ready = native(NativeSync::NoSync, 100);
        ready.highest_height = 100;
        assert_eq!(classify_phase(true, true, Some(&ready)), Ready);
        // Genesis only: "no_sync" alone is not enough.
        assert_eq!(
            classify_phase(true, true, Some(&native(NativeSync::NoSync, 0))),
            Unknown
        );
        assert_eq!(
            classify_phase(true, true, Some(&native(NativeSync::AwaitingPeers, 100))),
            Unknown
        );
        assert_eq!(
            classify_phase(true, true, Some(&native(NativeSync::Initial, 100))),
            Starting
        );
    }

    #[test]
    fn a_tip_far_behind_the_peers_is_not_ready() {
        let mut status = native(NativeSync::NoSync, 1_000_000);
        status.highest_height = 1_000_000;
        assert_eq!(
            classify_phase(true, true, Some(&status)),
            NodePhase::Ready,
            "the same height is fresh"
        );

        status.tip_height = 999_999;
        assert_eq!(
            classify_phase(true, true, Some(&status)),
            NodePhase::Ready,
            "one block of propagation delay is fine"
        );

        status.tip_height = 999_990;
        assert_eq!(
            classify_phase(true, true, Some(&status)),
            NodePhase::Unknown,
            "ten blocks behind is a stale tip, not ready"
        );

        // No peer target at all: a connected node reports `no_sync` while it is
        // caught up (Grin omits `sync_info` then), so the node's own state is
        // the proof. Losing every peer still makes the tip untrustworthy.
        status.highest_height = 0;
        status.tip_height = 999_990;
        assert_eq!(
            classify_phase(true, true, Some(&status)),
            NodePhase::Ready,
            "caught-up node with peers but no sync_info"
        );
        status.connections = 0;
        assert_eq!(
            classify_phase(true, true, Some(&status)),
            NodePhase::Unknown,
            "no peers means no proof that the tip is fresh"
        );
    }

    #[test]
    fn node_identity_must_match_network() {
        let mut status = native(NativeSync::NoSync, 100);
        status.chain = Some("test".into());
        status.user_agent = Some("MW/Grin 5.5.2".into());
        assert!(identity_matches("testnet", &status));
        assert!(!identity_matches("mainnet", &status));

        status.chain = Some("main".into());
        assert!(identity_matches("mainnet", &status));

        status.user_agent = Some("Evil/1.0".into());
        assert!(!identity_matches("mainnet", &status));
        status.user_agent = None;
        assert!(!identity_matches("mainnet", &status));
    }

    #[test]
    fn no_peers_means_not_ready() {
        let mut status = native(NativeSync::NoSync, 100);
        status.connections = 0;
        assert_eq!(
            classify_phase(true, true, Some(&status)),
            NodePhase::Unknown
        );
    }

    #[test]
    fn body_sync_family_maps_to_blocks_not_unknown() {
        for status in [
            "txhashsetpibd_download",
            "txhashset_download",
            "txhashset_setup",
            "txhashset_validate",
        ] {
            assert_eq!(
                NativeSync::parse(status),
                NativeSync::BodySync,
                "{status} must be body sync"
            );
        }
        assert_eq!(NativeSync::parse("shutdown"), NativeSync::Other);
    }

    #[test]
    fn quiet_log_with_stale_height_is_not_ready() {
        // The audit case: the node answers RPC and reports a non-zero (possibly
        // lagging) height, but there is no native sync state. A quiet log is not
        // evidence of a synced chain, so this must stay unknown.
        assert_eq!(classify_phase(true, true, None), NodePhase::Unknown);
        let stale = NativeStatus {
            sync: Some(NativeSync::Other),
            tip_height: 500_000,
            ..Default::default()
        };
        assert_eq!(classify_phase(true, true, Some(&stale)), NodePhase::Unknown);
    }

    #[test]
    fn sync_stats_prefers_the_log_we_own() {
        let dir = std::env::temp_dir().join(format!("flint-node-stats-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let now = chrono::Local::now().format("%Y%m%d %H:%M:%S%.3f");
        // node.log is what NodeManager writes; the native log is only a fallback.
        let owned = format!(
            "{now} INFO x - Starting GRIN\n\
             {now} INFO adapter - Received 32 block headers from 1.2.3.4:3414\n\
             {now} INFO chain - Received block 000000 at 12 from 5.6.7.8:3414\n"
        );
        fs::write(dir.join("node.log"), owned).expect("node.log");
        fs::write(dir.join("grin-server.log"), format!("{now} garbage\n")).expect("native log");

        let s = sync_stats(&dir);
        assert_eq!(s.headers, 32, "must read node.log, not grin-server.log");
        assert_eq!(s.blocks, 1);
        assert_eq!(s.peers, 2);
        assert!(s.last_header_age_secs.expect("header age") < 60.0);

        // Without node.log the native log is still usable.
        fs::remove_file(dir.join("node.log")).expect("remove");
        fs::write(
            dir.join("grin-server.log"),
            format!("{now} INFO adapter - Received 32 block headers from 1.2.3.4:3414\n"),
        )
        .expect("native log");
        assert_eq!(sync_stats(&dir).headers, 32);
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Sync telemetry parsed from the node's own log: the v2 RPC exposes only the
/// current tip, so progress during the initial download comes from here.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SyncStats {
    /// Headers downloaded since this node process started.
    pub headers: u64,
    pub headers_per_sec: f64,
    /// Block messages seen in this run (body sync is chatty while it lasts).
    pub blocks: u64,
    /// Seconds since the last header batch / block line in the log.
    pub last_header_age_secs: Option<f64>,
    pub last_block_age_secs: Option<f64>,
    pub peers: u32,
    pub chain_mb: f64,
}

/// What the node is actually doing.
///
/// "Ready" is deliberately conservative: RPC answering or a non-zero height is
/// not proof of a synced chain (a node downloading headers answers `get_tip`
/// with its local, still-low tip).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePhase {
    NotRunning,
    /// Process is up, RPC not answering yet.
    Starting,
    Headers,
    Blocks,
    Ready,
    #[default]
    Unknown,
}

impl NodePhase {
    pub fn label(&self) -> &'static str {
        match self {
            NodePhase::NotRunning => "нода не запущена",
            NodePhase::Starting => "нода запускается",
            NodePhase::Headers => "качает заголовки",
            NodePhase::Blocks => "качает блоки",
            NodePhase::Ready => "готова",
            NodePhase::Unknown => "статус неизвестен",
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self, NodePhase::Ready)
    }
}

/// The node's own sync state, as reported by its v2 owner API.
///
/// This is the authority: log silence and a non-zero tip are not proof of a
/// synced chain, and a stalled node can look quiet for any amount of time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSync {
    Initial,
    NoSync,
    AwaitingPeers,
    HeaderSync,
    BodySync,
    Other,
}

impl NativeSync {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "initial" => NativeSync::Initial,
            "no_sync" => NativeSync::NoSync,
            "awaiting_peers" => NativeSync::AwaitingPeers,
            "header_sync" => NativeSync::HeaderSync,
            "body_sync" => NativeSync::BodySync,
            // The body-sync family: PIBD download, txhashset download/setup/validate.
            // All of them mean "chain body is still being obtained", not "unknown".
            status if status.starts_with("txhashset") => NativeSync::BodySync,
            _ => NativeSync::Other,
        }
    }
}

/// A snapshot of what the node says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeStatus {
    pub sync: Option<NativeSync>,
    pub tip_height: u64,
    /// Chain height reached so far (headers or blocks, depending on the phase).
    pub current_height: u64,
    /// Height the node has heard about from its peers — the sync target.
    pub highest_height: u64,
    pub connections: u32,
    /// `test` / `main`: the node must serve the network we think it does.
    pub chain: Option<String>,
    /// `MW/Grin <version>` — proves it is a Grin node, not a lookalike.
    pub user_agent: Option<String>,
}

/// Ask the node for its native status via the v2 owner API (`get_status`).
///
/// Grin 5.5 does not mount the v1 REST `/status`, so this is the only place the
/// node reports `sync_status` and current/highest heights. The blocking client is
/// deliberate: every caller already runs on a blocking thread, and the timeout is
/// short because this sits on the status-refresh path.
pub fn fetch_native_status(
    node_url: &str,
    timeout: Duration,
    api_secret: Option<&str>,
) -> Option<NativeStatus> {
    fetch_native_status_result(node_url, timeout, api_secret).ok()
}

/// Same request as [`fetch_native_status`], but preserves the reason for a
/// failed call so the UI can distinguish a wrong owner secret from a timeout.
pub fn fetch_native_status_result(
    node_url: &str,
    timeout: Duration,
    api_secret: Option<&str>,
) -> std::result::Result<NativeStatus, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| format!("HTTP-клиент ноды: {error}"))?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "get_status",
        "params": [],
        "id": 1,
    });
    let mut request = client.post(format!("{}/v2/owner", node_url.trim_end_matches('/')));
    if let Some(secret) = api_secret.map(str::trim).filter(|s| !s.is_empty()) {
        // Same convention as grin's own client: Basic base64("grin:<secret>").
        let encoded = base64::encode(format!("grin:{secret}"));
        request = request.header(reqwest::header::AUTHORIZATION, format!("Basic {encoded}"));
    }
    let response = request
        .json(&body)
        .send()
        .map_err(|error| format!("нода не ответила: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 => "нода отклонила API-секрет (401/403)".to_string(),
            code => format!("нода вернула HTTP {code}"),
        });
    }
    let value: serde_json::Value = response
        .json()
        .map_err(|error| format!("ответ статуса ноды не JSON: {error}"))?;
    let payload = value
        .get("result")
        .and_then(|r| r.get("Ok"))
        .or_else(|| value.get("Ok"))
        .ok_or_else(|| "нода вернула статус без result.Ok".to_string())?;
    let sync = payload.get("sync_status").and_then(|v| v.as_str());
    Ok(NativeStatus {
        sync: sync.map(NativeSync::parse),
        tip_height: payload
            .pointer("/tip/height")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        current_height: payload
            .pointer("/sync_info/current_height")
            .or_else(|| payload.pointer("/sync_info/completed_to_height"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        highest_height: payload
            .pointer("/sync_info/highest_height")
            .or_else(|| payload.pointer("/sync_info/required_height"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        connections: payload
            .get("connections")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        chain: payload
            .get("chain")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        user_agent: payload
            .get("user_agent")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// A node is "in sync" only when its tip is not behind what its peers report.
/// A couple of blocks is normal propagation delay; more means the node lost
/// sync while still answering `no_sync`.
pub const TIP_FRESHNESS_BLOCKS: u64 = 2;

/// Classify the node from its own status, never from a single RPC answer.
///
/// Without a native status the phase is `Unknown` on purpose: a quiet log plus a
/// non-zero height is exactly what a stalled node looks like.
/// Gate for a new outgoing spend when the node is not fully caught up.
///
/// `Headers`/`Blocks` may proceed only with an explicit user acknowledgement:
/// the chain will reject a conflicting transaction, but the wallet can still
/// lock change for a payment that never confirms. Unknown/Starting/NotRunning
/// never proceed.
pub fn send_sync_gate(
    phase: NodePhase,
    allow_unsynced: bool,
) -> std::result::Result<Option<String>, String> {
    if phase.ready() {
        return Ok(None);
    }
    if allow_unsynced && matches!(phase, NodePhase::Headers | NodePhase::Blocks) {
        return Ok(Some(
            "нода догоняет цепь: транзакция может быть отвергнута сетью, а входы останутся заблокированы до отмены"
                .to_string(),
        ));
    }
    Err(format!(
        "отправка заблокирована: {}. Дождитесь полной синхронизации ноды — иначе баланс и подтверждения недостоверны",
        phase.label()
    ))
}

pub fn classify_phase(running: bool, connected: bool, native: Option<&NativeStatus>) -> NodePhase {
    if !running {
        return NodePhase::NotRunning;
    }
    if !connected {
        return NodePhase::Starting;
    }
    let Some(status) = native else {
        return NodePhase::Unknown;
    };
    match status.sync {
        Some(NativeSync::HeaderSync) => NodePhase::Headers,
        Some(NativeSync::BodySync) => NodePhase::Blocks,
        // `no_sync` at genesis or without peers is not ready. When peers are
        // connected but the node reports no peer target (Grin omits
        // `sync_info` while caught up), the node's own `no_sync` is the proof;
        // keeping this Unknown made every fully synced node permanently
        // unusable for scans and sends.
        Some(NativeSync::NoSync)
            if status.tip_height > 0
                && status.connections > 0
                && (status.highest_height == 0
                    || status.tip_height.saturating_add(TIP_FRESHNESS_BLOCKS)
                        >= status.highest_height) =>
        {
            NodePhase::Ready
        }
        Some(NativeSync::Initial) => NodePhase::Starting,
        _ => NodePhase::Unknown,
    }
}

/// Is this really our network's Grin node? Chain and user agent must match, so a
/// local lookalike on the same port cannot be taken for the node.
pub fn identity_matches(network: &str, status: &NativeStatus) -> bool {
    let expected = if network == "mainnet" { "main" } else { "test" };
    status.chain.as_deref() == Some(expected)
        && status
            .user_agent
            .as_deref()
            .is_some_and(|agent| agent.starts_with("MW/Grin"))
}

/// The node log we own: `NodeManager` redirects stdout here, and the native
/// `grin-server.log` is only a fallback for nodes started by hand.
fn node_log_path(node_dir: &Path) -> Option<PathBuf> {
    [node_dir.join("node.log"), node_dir.join("grin-server.log")]
        .into_iter()
        .find(|p| p.is_file())
}

/// Age of the node log, used to tell "starting" from "not running".
pub fn log_age_secs(node_dir: &Path) -> Option<f64> {
    let path = node_log_path(node_dir)?;
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified.elapsed().ok().map(|d| d.as_secs_f64())
}

pub fn sync_stats(node_dir: &Path) -> SyncStats {
    let Some(log_path) = node_log_path(node_dir) else {
        return SyncStats::default();
    };
    let Ok(raw) = fs::read_to_string(log_path) else {
        return SyncStats::default();
    };

    // Only the current run counts: everything after the last "Starting GRIN".
    let run_start = raw
        .rfind("Starting GRIN")
        .and_then(|i| raw[i..].find('\n').map(|j| i + j + 1))
        .unwrap_or(0);
    let run = &raw[run_start..];

    let mut batches = 0u64;
    let mut blocks = 0u64;
    let mut peers: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut stamps: Vec<f64> = Vec::new();
    let mut last_header: Option<f64> = None;
    let mut last_block: Option<f64> = None;

    for line in run.lines() {
        let mut parts = line.splitn(3, ' ');
        let (Some(date), Some(clock), Some(rest)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if date.len() != 8 || clock.len() < 8 {
            continue;
        }
        if let Some(pos) = rest.find(" from ") {
            peers.insert(rest[pos + 6..].trim());
        }
        let Some(secs) = seconds_of_day(clock) else {
            continue;
        };
        if rest.contains("Received 32 block headers") {
            batches += 1;
            stamps.push(secs);
            last_header = Some(secs);
            continue;
        }
        // Body sync lines mention blocks; header batches are counted above.
        if rest.contains("block")
            && !rest.contains("block headers")
            && !rest.contains("blockchain")
            && (rest.contains("Received") || rest.contains("Sending") || rest.contains("accepted"))
        {
            blocks += 1;
            last_block = Some(secs);
        }
    }

    // Rate over the last few hundred batches smooths bursts but stays current.
    let window = &stamps[stamps.len().saturating_sub(200)..];
    let rate = match (window.first(), window.last()) {
        (Some(first), Some(last)) if last - first > 0.5 => {
            (window.len() as f64 * 32.0 / (last - first) * 10.0).round() / 10.0
        }
        _ => 0.0,
    };

    let now = seconds_of_day_now();
    SyncStats {
        headers: batches * 32,
        headers_per_sec: rate,
        blocks,
        last_header_age_secs: last_header.map(|t| (now - t).rem_euclid(86_400.0)),
        last_block_age_secs: last_block.map(|t| (now - t).rem_euclid(86_400.0)),
        peers: peers.len().min(64) as u32,
        chain_mb: (dir_size_mb(&node_dir.join("chain_data")) * 10.0).round() / 10.0,
    }
}

fn seconds_of_day_now() -> f64 {
    use chrono::Timelike;
    let t = chrono::Local::now().time();
    t.hour() as f64 * 3600.0
        + t.minute() as f64 * 60.0
        + t.second() as f64
        + t.nanosecond() as f64 / 1e9
}

fn seconds_of_day(clock: &str) -> Option<f64> {
    let mut it = clock.split(':');
    let h: f64 = it.next()?.parse().ok()?;
    let m: f64 = it.next()?.split('.').next()?.parse().ok()?;
    let s: f64 = it.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

fn dir_size_mb(path: &Path) -> f64 {
    fn walk(dir: &Path, total: &mut u64) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&entry.path(), total);
            } else {
                *total += meta.len();
            }
        }
    }
    let mut total = 0u64;
    walk(path, &mut total);
    total as f64 / (1024.0 * 1024.0)
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use std::io::{Read, Write};

    fn stub_node(expected: Option<String>) -> (u16, std::thread::JoinHandle<bool>) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let text = String::from_utf8_lossy(&buffer).to_lowercase();
            let authorized = match &expected {
                Some(value) => text.contains(&format!("authorization: {}", value.to_lowercase())),
                None => true,
            };
            let body = if authorized {
                r#"{"result":{"Ok":{"sync_status":"no_sync","tip":{"height":42},"connections":3}}}"#
            } else {
                r#"{"error":"unauthorized"}"#
            };
            let status = if authorized {
                "200 OK"
            } else {
                "401 Unauthorized"
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
            authorized
        });
        (port, handle)
    }

    #[test]
    fn status_sends_basic_auth_when_a_secret_is_configured() {
        let expected = format!("Basic {}", base64::encode("grin:s3cret"));
        let (port, handle) = stub_node(Some(expected));
        let status = fetch_native_status(
            &format!("http://127.0.0.1:{port}"),
            Duration::from_secs(3),
            Some("s3cret"),
        );
        assert!(handle.join().expect("stub"), "wrong Authorization header");
        let status = status.expect("status must be parsed");
        assert_eq!(status.tip_height, 42);
        assert_eq!(status.connections, 3);
    }

    #[test]
    fn status_without_secret_is_rejected_by_a_secured_node() {
        let expected = format!("Basic {}", base64::encode("grin:s3cret"));
        let (port, handle) = stub_node(Some(expected));
        let status = fetch_native_status(
            &format!("http://127.0.0.1:{port}"),
            Duration::from_secs(3),
            None,
        );
        let _ = handle.join();
        assert!(status.is_none(), "401 must not be parsed as a status");
    }

    #[test]
    fn a_wrong_secret_is_reported_as_an_auth_error() {
        let expected = format!("Basic {}", base64::encode("grin:s3cret"));
        let (port, handle) = stub_node(Some(expected));
        let error = fetch_native_status_result(
            &format!("http://127.0.0.1:{port}"),
            Duration::from_secs(3),
            Some("wrong"),
        )
        .expect_err("wrong secret must fail");
        let _ = handle.join();
        assert!(error.contains("API-секрет"), "unexpected error: {error}");
    }

    #[test]
    fn hanging_node_status_is_bounded_by_the_timeout() {
        // Accept the connection and never answer: a hung node must not hang the
        // caller, it must produce no status within the configured timeout.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                std::thread::sleep(Duration::from_secs(5));
            }
        });

        let started = std::time::Instant::now();
        let status = fetch_native_status(
            &format!("http://127.0.0.1:{port}"),
            Duration::from_millis(200),
            None,
        );
        let elapsed = started.elapsed();
        assert!(status.is_none(), "hanging node produced a status");
        assert!(
            elapsed < Duration::from_secs(3),
            "status call was not bounded: {elapsed:?}"
        );
    }
    #[test]
    fn loopback_node_url_is_strict() {
        assert!(is_loopback_node_url("http://127.0.0.1:13413"));
        assert!(is_loopback_node_url("http://localhost:13413"));
        assert!(is_loopback_node_url("https://[::1]:13413"));
        assert!(is_loopback_node_url("https://127.0.0.2"));
        assert!(!is_loopback_node_url("https://node.example.com"));
        assert!(!is_loopback_node_url("http://localhost.attacker.invalid"));
        assert!(!is_loopback_node_url("http://127.0.0.1.attacker.invalid/"));
        assert!(!is_loopback_node_url("not a url"));
    }

    #[test]
    fn persisted_node_url_policy_rejects_open_transport() {
        assert!(node_url_allowed("https://node.example.com"));
        assert!(node_url_allowed("http://127.0.0.1:13413"));
        assert!(node_url_allowed("http://localhost:13413"));
        assert!(!node_url_allowed("http://node.example.com:13413"));
        assert!(!node_url_allowed("http://localhost.attacker.invalid"));
        assert!(!node_url_allowed("ftp://127.0.0.1"));
        assert!(!node_url_allowed(""));
    }

    #[test]
    fn unsynced_send_gate_requires_explicit_acknowledgement() {
        for phase in [NodePhase::Headers, NodePhase::Blocks] {
            assert!(send_sync_gate(phase, false).is_err());
            assert!(send_sync_gate(phase, true).expect("acknowledged").is_some());
        }
        for phase in [
            NodePhase::Starting,
            NodePhase::Unknown,
            NodePhase::NotRunning,
        ] {
            assert!(send_sync_gate(phase, true).is_err(), "{phase:?}");
        }
        assert!(send_sync_gate(NodePhase::Ready, false)
            .expect("ready")
            .is_none());
    }
}
