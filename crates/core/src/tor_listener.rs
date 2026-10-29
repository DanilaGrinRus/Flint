//! Tor listener for the RPC wallet: publishes the address-derived onion with a
//! system `tor.exe`.
//!
//! The hidden service points at a loopback [`Gateway`], not at the wallet. The
//! gateway exposes exactly two things: `POST /v2/foreign` (payments) and the
//! `/flint/v1/msg*` messenger API. The owner API is never reachable through the
//! onion. The same Tor instance also exposes a local SOCKS port used for
//! outgoing sends and direct onion messages.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{FlintError, Result};
use crate::gateway::{wait_ready, Gateway, GatewayConfig};
use crate::onion::{onion_address, tor_hidden_service_keys};

pub fn locate_tor_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("FLINT_TOR_BIN") {
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
                    .join("tor")
                    .join("expert")
                    .join("tor")
                    .join("tor.exe"),
                ancestor.join("tor").join("tor.exe"),
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
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(probe.local_addr()?.port())
}

/// Machine-readable bridge state for the settings UI. It deliberately exposes
/// diagnostics, not bridge secrets beyond the lines the user already saved.
#[derive(Clone, Debug, serde::Serialize)]
pub struct BridgeTransportStatus {
    pub kind: String,
    pub client: Option<String>,
    pub available: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct BridgeDiagnostics {
    pub path: String,
    pub configured: bool,
    pub lines: usize,
    pub transports: Vec<BridgeTransportStatus>,
    pub error: Option<String>,
}

pub fn bridge_diagnostics(base_dir: &Path) -> BridgeDiagnostics {
    let path = base_dir.join("bridges.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let tor = locate_tor_binary();
    let mut kinds = Vec::<String>::new();
    for line in &lines {
        let line = line.strip_prefix("Bridge ").unwrap_or(line).trim();
        let kind = line.split_whitespace().next().unwrap_or("");
        if matches!(kind, "obfs4" | "snowflake" | "webtunnel")
            && !kinds.iter().any(|known| known == kind)
        {
            kinds.push(kind.to_string());
        }
    }
    let transports = kinds
        .into_iter()
        .map(|kind| {
            let client = tor
                .as_deref()
                .and_then(|tor| locate_transport(&kind, tor, base_dir));
            BridgeTransportStatus {
                available: client.is_some(),
                client: client.map(|path| path.display().to_string()),
                kind,
            }
        })
        .collect();
    let error = if !lines.is_empty() && tor.is_none() {
        Some("tor.exe не найден: положите Tor expert bundle в resources/tor".into())
    } else {
        None
    };
    BridgeDiagnostics {
        path: path.display().to_string(),
        configured: !lines.is_empty(),
        lines: lines.len(),
        transports,
        error,
    }
}

/// Optional obfs4/snowflake/webtunnel bridges for networks where plain Tor is
/// blocked. The file lives at `<data>/<network>/tor/bridges.txt`; one bridge
/// line per line, comments with `#`.
fn torrc_quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

/// Tor's Windows build creates its data and hidden-service directories through
/// ANSI APIs, so a Unicode data root makes it fail with "No such file or
/// directory" even when the directory exists. The 8.3 short path is a stable
/// ASCII alias for the same directory when the volume still provides one.
#[cfg(windows)]
fn short_ascii_path(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    unsafe extern "system" {
        fn GetShortPathNameW(
            lpsz_long_path: *const u16,
            lpsz_short_path: *mut u16,
            cch_buffer: u32,
        ) -> u32;
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut buffer = vec![0u16; 32_768];
    let len = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
    if len == 0 || len as usize >= buffer.len() {
        return None;
    }
    let short = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize]));
    short.to_string_lossy().is_ascii().then_some(short)
}

/// Directory for the directories Tor itself creates. It stays equal to the
/// caller's directory for ASCII roots and resolves to the 8.3 alias otherwise.
fn tor_owned_base(base_dir: &Path) -> Result<PathBuf> {
    let text = base_dir.to_string_lossy();
    if text.is_ascii() && !text.contains(' ') {
        return Ok(base_dir.to_path_buf());
    }
    #[cfg(windows)]
    if let Some(short) = short_ascii_path(base_dir) {
        return Ok(short);
    }
    Err(FlintError::Wallet(format!(
        "Tor не поддерживает путь данных с пробелами или не-ASCII символами ({}); \
         выберите каталог без них",
        base_dir.display()
    )))
}

fn bridge_config(base_dir: &Path, tor_exe: &Path) -> Result<Option<String>> {
    let path = base_dir.join("bridges.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(FlintError::Io(error)),
    };
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if lines.is_empty() {
        return Ok(None);
    }

    let mut transports = Vec::<String>::new();
    let mut out = String::from("UseBridges 1\n");
    for line in lines {
        let line = line.strip_prefix("Bridge ").unwrap_or(line).trim();
        if line.is_empty() {
            continue;
        }
        let kind = line.split_whitespace().next().unwrap_or("");
        if matches!(kind, "obfs4" | "snowflake" | "webtunnel") {
            if !transports.iter().any(|known| known == kind) {
                transports.push(kind.to_string());
            }
        }
        out.push_str(&format!("Bridge {line}\n"));
    }

    if !transports.is_empty() {
        for transport in transports {
            let client = locate_transport(&transport, tor_exe, base_dir).ok_or_else(|| {
                FlintError::Wallet(
                    "bridges.txt использует pluggable transport, но lyrebird.exe не найден; \
                     положите Tor expert bundle с pluggable_transports рядом с tor.exe"
                        .into(),
                )
            })?;
            let quoted = client.display().to_string().replace('"', "\\\"");
            out.push_str(&format!(
                "ClientTransportPlugin {transport} exec \"{quoted}\"\n"
            ));
        }
    }
    Ok(Some(out))
}

fn locate_transport(_kind: &str, tor_exe: &Path, base_dir: &Path) -> Option<PathBuf> {
    // Tor's expert bundle ships one unified pluggable-transport client:
    // lyrebird 0.8.x implements obfs4, snowflake and webtunnel. There is no
    // separate snowflake-client/webtunnel-client to find.
    if let Ok(path) = std::env::var("FLINT_LYREBIRD_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Some(parent) = tor_exe.parent() {
        candidates.push(parent.join("pluggable_transports").join("lyrebird.exe"));
        candidates.push(parent.join("lyrebird.exe"));
    }
    candidates.push(base_dir.join("lyrebird.exe"));
    candidates.into_iter().find(|path| path.is_file())
}

/// Tor writes this to its notice log when it has a usable consensus and a
/// circuit. Waiting for it prevents reporting an onion that cannot be reached
/// yet (the hostname file appears earlier).
fn wait_bootstrap(base_dir: &Path, deadline: Instant) -> bool {
    while Instant::now() < deadline {
        if let Ok(log) = std::fs::read_to_string(base_dir.join("tor.log")) {
            if log.contains("Bootstrapped 100% (done)") {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

pub struct TorListener {
    onion: String,
    socks_port: u16,
    child: Child,
    /// Hidden service target: payments + messenger, never the owner API.
    gateway: Gateway,
    dir: PathBuf,
}

/// Gateway behind the published onion. The endpoint is reachable from the
/// network, so acknowledgements must be signed; the plain default is only for
/// local tests.
fn listener_gateway_config(wallet_port: u16, base_dir: &Path) -> GatewayConfig {
    GatewayConfig {
        wallet_foreign_url: format!("http://127.0.0.1:{wallet_port}/v2/foreign"),
        store_dir: Some(base_dir.join("mailbox")),
        require_signed_ack: true,
        require_pow: true,
        ttl_secs: 12 * 60 * 60,
        max_per_recipient: 30,
        max_total: 1_000,
        max_payload: 32 * 1024,
        max_posts_per_minute: 30,
        ..GatewayConfig::default()
    }
}

impl TorListener {
    /// Publish one onion that serves both the foreign API of `wallet_port` and
    /// the messenger gateway. The onion is derived from `seed`, so peers can
    /// compute it from the slatepack address alone.
    pub fn start(seed: &[u8; 32], wallet_port: u16, base_dir: &Path) -> Result<Self> {
        let exe = locate_tor_binary().ok_or_else(|| {
            FlintError::Wallet(
                "tor.exe не найден — положите Tor expert bundle в resources/tor".into(),
            )
        })?;
        let expected = onion_address(seed)?;

        std::fs::create_dir_all(base_dir)?;
        // This Tor build creates its DataDirectory and HiddenServiceDir through
        // ANSI Windows APIs: a non-ASCII path fails with "No such file or
        // directory" even when the directory exists. Use the 8.3 short path for
        // everything Tor itself creates when the data root contains Unicode.
        let tor_base = tor_owned_base(base_dir)?;
        let hs_dir = tor_base.join("hs");
        tor_hidden_service_keys(&hs_dir, seed)?;
        let data_dir = tor_base.join("data");
        std::fs::create_dir_all(&data_dir)?;
        let socks_port = free_port()?;

        let gateway = Gateway::start(listener_gateway_config(wallet_port, base_dir))?;
        if !wait_ready(&gateway, Duration::from_secs(5)) {
            return Err(FlintError::Wallet(
                "gateway не ответил на health-check".into(),
            ));
        }

        let torrc = tor_base.join("torrc");
        let log_level = std::env::var("FLINT_TOR_LOG").unwrap_or_else(|_| "notice".to_string());
        let mut torrc_text = format!(
            "SocksPort 127.0.0.1:{socks_port}\nDataDirectory {}\nHiddenServiceDir {}\nHiddenServicePort 80 127.0.0.1:{}\nLog {log_level} file {}\n",
            torrc_quote(&data_dir),
            torrc_quote(&hs_dir),
            gateway.addr().port(),
            // `Log ... file` keeps quotes as part of the file name in Tor's
            // config parser; tor_base is ASCII and space-free, so it is safe
            // to write unquoted here.
            tor_base.join("tor.log").display()
        );
        if let Some(bridges) = bridge_config(base_dir, &exe)? {
            torrc_text.push_str(&bridges);
        }
        std::fs::write(&torrc, torrc_text)?;

        let _ = std::fs::remove_file(tor_base.join("tor.log"));
        let log = std::fs::File::create(tor_base.join("tor.out"))?;
        let log_err = log.try_clone()?;
        let mut cmd = Command::new(&exe);
        cmd.arg("-f")
            .arg(&torrc)
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
            .map_err(|e| FlintError::Wallet(format!("не удалось запустить tor: {e}")))?;

        let hostname_path = hs_dir.join("hostname");
        let deadline = Instant::now() + Duration::from_secs(150);
        let mut published = None;
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&hostname_path) {
                let onion = text.trim().trim_end_matches(".onion").to_string();
                if !onion.is_empty() {
                    published = Some(onion);
                    break;
                }
            }
            // A dead Tor process will never publish; fail with its log instead
            // of blocking for the full timeout.
            if let Ok(Some(status)) = child.try_wait() {
                let log = std::fs::read_to_string(base_dir.join("tor.log")).unwrap_or_default();
                return Err(FlintError::Wallet(format!(
                    "tor завершился ({status}) до публикации onion:\n{log}"
                )));
            }
            std::thread::sleep(Duration::from_millis(300));
        }

        // A hostname appears before Tor has enough directory information to
        // reach the service; don't report a usable onion until bootstrap is done.
        if published.is_some() && !wait_bootstrap(base_dir, deadline) {
            let _ = child.kill();
            let _ = child.wait();
            let log = std::fs::read_to_string(base_dir.join("tor.log")).unwrap_or_default();
            return Err(FlintError::Wallet(format!(
                "tor не завершил bootstrap за 150 секунд:\n{log}"
            )));
        }

        match published {
            Some(onion) if onion == expected => Ok(Self {
                onion,
                socks_port,
                child,
                gateway,
                dir: base_dir.to_path_buf(),
            }),
            Some(onion) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(FlintError::Wallet(format!(
                    "tor опубликовал чужой onion: {onion}, ожидался {expected}"
                )))
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                Err(FlintError::Wallet(
                    "tor не опубликовал сервис за 150 секунд".into(),
                ))
            }
        }
    }

    pub fn onion(&self) -> &str {
        &self.onion
    }

    /// Local `host:port` of the SOCKS5 proxy for outgoing requests.
    pub fn socks_addr(&self) -> String {
        format!("127.0.0.1:{}", self.socks_port)
    }

    /// Loopback gateway that stores this wallet's messenger inbox.
    pub fn gateway_url(&self) -> String {
        self.gateway.url()
    }

    /// Loopback port the hidden service actually targets.
    pub fn gateway_port(&self) -> u16 {
        self.gateway.addr().port()
    }

    /// Address peers use for direct onion delivery.
    pub fn url(&self) -> String {
        format!("http://{}.onion", self.onion)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TorListener {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_gateway_requires_signed_acks_and_p2p_limits() {
        let config = listener_gateway_config(13413, Path::new("C:/tmp/flint-gateway-test"));
        assert!(
            config.require_signed_ack,
            "onion-reachable gateway must reject unsigned acks"
        );
        assert!(config.require_pow, "P2P gateway must require proof of work");
        assert_eq!(config.max_per_recipient, 30);
        assert_eq!(config.max_total, 1_000);
        assert_eq!(config.max_payload, 32 * 1024);
        assert_eq!(config.max_posts_per_minute, 30);
        assert_eq!(config.ttl_secs, 12 * 60 * 60);
    }

    #[test]
    fn torrc_paths_are_quoted() {
        let quoted = torrc_quote(Path::new(r#"C:\Users\A B\flint "data"\tor"#));
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
        assert!(quoted.contains(r#"\\"#), "{quoted}");
        assert!(quoted.contains(r#"\""#), "{quoted}");
    }

    #[test]
    fn bridges_are_read_from_the_data_dir() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        let tor = root.join("tor").join("tor.exe");
        let lyrebird = root
            .join("tor")
            .join("pluggable_transports")
            .join("lyrebird.exe");
        std::fs::create_dir_all(lyrebird.parent().unwrap()).unwrap();
        std::fs::write(&lyrebird, b"fake").unwrap();
        std::fs::write(
            root.join("bridges.txt"),
            "# bridge list\nobfs4 192.0.2.10:443 ABCD cert=abc iat-mode=0\n",
        )
        .unwrap();

        let config = bridge_config(&root, &tor).unwrap().expect("bridges");
        assert!(config.contains("UseBridges 1"));
        assert!(config.contains("Bridge obfs4 192.0.2.10:443 ABCD"));
        assert!(config.contains("ClientTransportPlugin obfs4"));
        assert!(config.contains("lyrebird.exe"));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn plain_bridge_needs_no_pluggable_transport() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("bridges.txt"), "192.0.2.10:9001\n").unwrap();
        let config = bridge_config(&root, Path::new("C:/tor/tor.exe"))
            .unwrap()
            .expect("bridges");
        assert!(config.contains("Bridge 192.0.2.10:9001"));
        assert!(!config.contains("ClientTransportPlugin"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_lyrebird_is_an_error() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("bridges.txt"),
            "obfs4 192.0.2.10:443 ABCD cert=abc\n",
        )
        .unwrap();
        assert!(bridge_config(&root, Path::new("C:/tor/tor.exe")).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn snowflake_uses_lyrebird() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        let tor = root.join("tor").join("tor.exe");
        let lyrebird = root
            .join("tor")
            .join("pluggable_transports")
            .join("lyrebird.exe");
        std::fs::create_dir_all(lyrebird.parent().unwrap()).unwrap();
        std::fs::write(&lyrebird, b"fake").unwrap();
        std::fs::write(
            root.join("bridges.txt"),
            "snowflake 192.0.2.10:80 ABCD fingerprint=abc\n",
        )
        .unwrap();
        let config = bridge_config(&root, &tor).unwrap().expect("bridges");
        assert!(config.contains("ClientTransportPlugin snowflake"));
        assert!(config.contains("lyrebird.exe"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn snowflake_without_lyrebird_is_an_error() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("bridges.txt"),
            "snowflake 192.0.2.10:80 ABCD fingerprint=abc\n",
        )
        .unwrap();
        let error = bridge_config(&root, Path::new("C:/tor/tor.exe")).unwrap_err();
        assert!(error.to_string().contains("lyrebird.exe"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn torrc_quotes_unicode_and_spaces() {
        let quoted = torrc_quote(Path::new(r"C:\Данные Flint\тор"));
        assert!(quoted.starts_with('"') && quoted.ends_with('"'), "{quoted}");
        assert!(quoted.contains("Данные Flint"), "{quoted}");
        assert!(quoted.contains("тор"), "{quoted}");
        assert!(!quoted.trim_matches('"').contains('"'), "{quoted}");
    }

    #[test]
    fn webtunnel_uses_lyrebird() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        let tor = root.join("tor").join("tor.exe");
        let lyrebird = root
            .join("tor")
            .join("pluggable_transports")
            .join("lyrebird.exe");
        std::fs::create_dir_all(lyrebird.parent().unwrap()).unwrap();
        std::fs::write(&lyrebird, b"fake").unwrap();
        std::fs::write(
            root.join("bridges.txt"),
            "webtunnel 192.0.2.10:443 ABCD url=https://example.invalid/abc\n",
        )
        .unwrap();
        let config = bridge_config(&root, &tor).unwrap().expect("bridges");
        assert!(
            config.contains("ClientTransportPlugin webtunnel"),
            "{config}"
        );
        assert!(config.contains("lyrebird.exe"), "{config}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unreadable_bridge_file_is_an_error() {
        let root = std::env::temp_dir().join(format!("flint-bridges-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("bridges.txt")).unwrap();
        let error = bridge_config(&root, Path::new("C:/tor/tor.exe")).unwrap_err();
        assert!(!error.to_string().is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(windows)]
    #[test]
    fn unicode_data_dir_uses_an_ascii_short_path() {
        let dir = std::env::temp_dir().join(format!("flint-tor проверка {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = tor_owned_base(&dir).expect("short path");
        assert!(
            base.to_string_lossy().is_ascii(),
            "short path is not ASCII: {}",
            base.display()
        );
        assert!(
            base.is_dir(),
            "short path does not resolve: {}",
            base.display()
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
