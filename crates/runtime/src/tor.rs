//! Tor listener ownership shared by the Slint shells.
//!
//! The listener publishes one onion for the RPC wallet's foreign API and the
//! messenger gateway. It must be stopped before the RPC child changes port or
//! the slatepack identity rotates.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flint_core::tor_listener::BridgeDiagnostics;
use flint_core::{TorListener, WalletService};
use zeroize::Zeroizing;

use crate::rpc::RpcRuntime;

#[derive(Clone, Debug)]
pub struct TorStart {
    pub onion: String,
    pub relay_url: String,
}

#[derive(Clone, Debug)]
pub struct TorStatus {
    pub onion: Option<String>,
    pub relay_url: Option<String>,
    pub gateway_url: Option<String>,
    pub running: bool,
    pub bootstrap: bool,
    pub bridges: BridgeDiagnostics,
    pub log: Vec<String>,
    pub log_present: bool,
    pub log_path: String,
    pub tor_exe: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TorBridges {
    pub text: String,
    pub path: String,
    pub diagnostics: BridgeDiagnostics,
}

#[derive(Clone)]
pub struct TorRuntime {
    wallet: Arc<Mutex<WalletService>>,
    rpc: RpcRuntime,
    listener: Arc<Mutex<Option<TorListener>>>,
    starting: Arc<AtomicBool>,
}

impl TorRuntime {
    pub fn new(wallet: Arc<Mutex<WalletService>>, rpc: RpcRuntime) -> Self {
        Self {
            wallet,
            rpc,
            listener: Arc::new(Mutex::new(None)),
            starting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn enabled(&self) -> bool {
        self.rpc.enabled()
    }

    pub fn is_running(&self) -> bool {
        self.listener
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub fn socks_addr(&self) -> Option<String> {
        self.listener
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(TorListener::socks_addr))
    }

    pub fn onion(&self) -> Option<String> {
        self.listener
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|t| t.onion().to_string()))
    }

    pub fn gateway_url(&self) -> Option<String> {
        self.listener
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(TorListener::gateway_url))
    }

    fn dir(&self) -> Result<PathBuf, String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok(wallet
            .paths()
            .root()
            .join(wallet.settings().network.clone())
            .join("tor"))
    }

    /// Blocking start. The caller runs it on a worker thread.
    pub fn start(&self) -> Result<TorStart, String> {
        if !self.enabled() {
            return Err("приём по Tor доступен с бэкендом grin-wallet".into());
        }
        if self.starting.swap(true, Ordering::SeqCst) {
            return Err("Tor уже запускается — подождите".into());
        }
        let result = self.start_inner();
        self.starting.store(false, Ordering::SeqCst);
        result
    }

    fn start_inner(&self) -> Result<TorStart, String> {
        if let Some(listener) = self
            .listener
            .lock()
            .map_err(|_| "tor занят".to_string())?
            .as_ref()
        {
            return Ok(TorStart {
                onion: listener.onion().to_string(),
                relay_url: listener.url(),
            });
        }
        self.rpc.wait_ready(Duration::from_secs(20))?;
        let port = self
            .rpc
            .port()
            .ok_or_else(|| "кошелёк не запущен: перезапустите приложение".to_string())?;
        let (index, dir) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                wallet.address_index(),
                wallet
                    .paths()
                    .root()
                    .join(wallet.settings().network.clone())
                    .join("tor"),
            )
        };
        let seed = Zeroizing::new(
            self.rpc
                .call(move |backend| backend.slatepack_seed(index))?,
        );
        let listener = TorListener::start(&seed, port, &dir).map_err(|e| e.to_string())?;
        let start = TorStart {
            onion: listener.onion().to_string(),
            relay_url: listener.url(),
        };
        *self.listener.lock().map_err(|_| "tor занят".to_string())? = Some(listener);
        Ok(start)
    }

    pub fn stop(&self) -> bool {
        self.listener
            .lock()
            .map(|mut guard| guard.take().is_some())
            .unwrap_or(false)
    }

    pub fn status(&self) -> Result<TorStatus, String> {
        let dir = self.dir()?;
        let diagnostics = flint_core::tor_listener::bridge_diagnostics(&dir);
        let (log, log_present) = tail_lines(&dir.join("tor.log"), 80);
        let bootstrap = log.iter().any(|line| line.contains("Bootstrapped 100%"));
        let guard = self.listener.lock().map_err(|_| "tor занят".to_string())?;
        let listener = guard.as_ref();
        Ok(TorStatus {
            onion: listener.map(|t| t.onion().to_string()),
            relay_url: listener.map(TorListener::url),
            gateway_url: listener.map(TorListener::gateway_url),
            running: listener.is_some(),
            bootstrap,
            bridges: diagnostics,
            log,
            log_present,
            log_path: dir.join("tor.log").display().to_string(),
            tor_exe: flint_core::tor_listener::locate_tor_binary().map(|p| p.display().to_string()),
        })
    }

    pub fn bridges(&self) -> Result<TorBridges, String> {
        let dir = self.dir()?;
        let path = dir.join("bridges.txt");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Ok(TorBridges {
            text,
            path: path.display().to_string(),
            diagnostics: flint_core::tor_listener::bridge_diagnostics(&dir),
        })
    }

    pub fn save_bridges(&self, text: &str) -> Result<TorBridges, String> {
        let dir = self.dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("bridges.txt");
        if text.trim().is_empty() {
            let _ = std::fs::remove_file(&path);
        } else {
            let tmp = dir.join("bridges.txt.tmp");
            std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        }
        Ok(TorBridges {
            text: std::fs::read_to_string(&path).unwrap_or_default(),
            path: path.display().to_string(),
            diagnostics: flint_core::tor_listener::bridge_diagnostics(&dir),
        })
    }

    pub fn log(&self) -> Result<(Vec<String>, bool, String), String> {
        let dir = self.dir()?;
        let path = dir.join("tor.log");
        let (lines, present) = tail_lines(&path, 200);
        Ok((lines, present, path.display().to_string()))
    }

    /// Stop the listener before the RPC port or slatepack identity changes.
    pub fn shutdown(&self) {
        self.stop();
    }
}

fn tail_lines(path: &Path, max: usize) -> (Vec<String>, bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Vec::new(), false);
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max);
    (
        lines[start..].iter().map(|line| line.to_string()).collect(),
        true,
    )
}
