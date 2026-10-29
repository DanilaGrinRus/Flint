//! RPC wallet backend (`grin-wallet`) owned by the shared runtime.
//!
//! The core adapter ([`flint_core::RpcBackend`] / [`flint_core::WalletProcess`])
//! is deliberately blocking and has no internal synchronization: every owner-API
//! call waits on a local child process. This module owns the child, the bearer
//! token, readiness and the generation that invalidates a late start after a
//! network/profile switch. The Slint shell must call these methods from
//! `std::thread::spawn`, never from the UI event loop.
//!
//! Lock order:
//!   wallet snapshot (short) -> release -> `backend` (long owner call)
//!   `process` may be taken while `backend` is held by a scan; the reverse order
//!   is forbidden. That is why the process and the backend live in two slots
//!   instead of one `RpcSession` mutex: a scan can hold the backend for an hour,
//!   while cancellation must still be able to kill the child.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flint_core::rpc_backend::{self, RpcBackend, WalletProcess};
use flint_core::{AppPaths, FlintError, WalletService};

/// Which wallet engine this process was started with. It is fixed for the
/// lifetime of the process: a failed RPC start must never silently fall back to
/// the embedded wallet directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Embedded,
    GrinWallet,
}

impl BackendKind {
    pub fn is_rpc(self) -> bool {
        matches!(self, Self::GrinWallet)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::GrinWallet => "grin-wallet",
        }
    }

    pub fn from_name(name: &str) -> Self {
        if name == "grin-wallet" {
            Self::GrinWallet
        } else {
            Self::Embedded
        }
    }
}

/// Observable state of the RPC session. `Starting` is not an error: the child
/// process can take up to forty seconds before it writes its secret and opens
/// the owner API port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpcPhase {
    Disabled,
    Starting,
    Ready,
    Failed(String),
}

#[derive(Clone)]
pub struct RpcRuntime {
    wallet: Arc<Mutex<WalletService>>,
    /// `Some` only while the owner API is connected and the wallet is usable.
    backend: Arc<Mutex<Option<RpcBackend>>>,
    /// Kept separate from `backend` so scan cancellation can terminate the child
    /// while the scan call holds the backend mutex.
    process: Arc<Mutex<Option<WalletProcess>>>,
    /// "Startup finished", not necessarily successfully; `error` carries the
    /// failure. This mirrors the old shell state and keeps `wait_ready`
    /// non-blocking on the polling path.
    ready: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    /// Incremented on every switch/restart. A start that finishes after a switch
    /// must not publish its backend into the new wallet context.
    generation: Arc<AtomicU64>,
    shutting_down: Arc<AtomicBool>,
    /// Serializes the final publish of `backend`/`process` with stop/shutdown.
    /// The generation is re-checked under this lock, so a start that raced a
    /// switch cannot publish its child after the context changed.
    commit: Arc<Mutex<()>>,
    enabled: bool,
}

/// The values needed to start `grin-wallet`, snapshotted without the wallet lock
/// held during the (up to forty seconds) process start.
struct StartConfig {
    root: PathBuf,
    network: String,
    profile: Option<String>,
    journal: PathBuf,
    min_confirmations: u64,
    address_index: u32,
    needs_scan: bool,
    node: flint_core::NodeSnapshot,
}

impl RpcRuntime {
    pub fn new(wallet: Arc<Mutex<WalletService>>, enabled: bool) -> Self {
        Self {
            wallet,
            backend: Arc::new(Mutex::new(None)),
            process: Arc::new(Mutex::new(None)),
            ready: Arc::new(AtomicBool::new(false)),
            error: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            commit: Arc::new(Mutex::new(())),
            enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn phase(&self) -> RpcPhase {
        if !self.enabled {
            return RpcPhase::Disabled;
        }
        if !self.ready.load(Ordering::SeqCst) {
            return RpcPhase::Starting;
        }
        if let Some(error) = self.error.lock().ok().and_then(|guard| guard.clone()) {
            return RpcPhase::Failed(error);
        }
        match self.backend.lock() {
            Ok(guard) if guard.is_some() => RpcPhase::Ready,
            _ => RpcPhase::Failed("grin-wallet backend недоступен".into()),
        }
    }

    pub fn last_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|guard| guard.clone())
    }

    /// Start the child and connect the owner API. Blocking; call from a worker.
    /// A failure is recorded in `error` and `ready` is set so `wait_ready` and
    /// `phase` can report it without polling forever.
    pub fn start(&self) -> Result<(), String> {
        if !self.enabled {
            return Err("RPC-бэкенд не выбран".into());
        }
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("приложение закрывается — backend не запускается".into());
        }
        let expected = self.generation.load(Ordering::SeqCst);
        self.ready.store(false, Ordering::SeqCst);
        self.set_error(None);
        let result = self.start_generation(expected);
        self.set_error(result.as_ref().err().cloned());
        self.ready.store(true, Ordering::SeqCst);
        result
    }

    fn start_generation(&self, expected: u64) -> Result<(), String> {
        if self.generation.load(Ordering::SeqCst) != expected {
            return Err("контекст кошелька изменился во время запуска RPC".into());
        }
        let config = self.snapshot()?;
        let paths = AppPaths::at(config.root);
        let process = WalletProcess::start_with_node(
            &paths,
            &config.network,
            config.profile.as_deref(),
            &config.node,
        )
        .map_err(|e| e.to_string())?;
        let mut backend =
            RpcBackend::connect(&process.url(), process.secret(), config.min_confirmations)
                .map_err(|e| e.to_string())?;
        backend.set_address_index(config.address_index);
        backend.set_needs_scan(config.needs_scan);
        backend.set_node_snapshot(config.node.clone());
        backend.set_journal_dir(config.journal);
        let _commit = self
            .commit
            .lock()
            .map_err(|_| "состояние RPC занято".to_string())?;
        if self.shutting_down.load(Ordering::SeqCst)
            || self.generation.load(Ordering::SeqCst) != expected
        {
            return Err("контекст кошелька изменился во время запуска RPC".into());
        }
        *self
            .backend
            .lock()
            .map_err(|_| "состояние RPC занято".to_string())? = Some(backend);
        *self
            .process
            .lock()
            .map_err(|_| "процесс grin-wallet занят".to_string())? = Some(process);
        Ok(())
    }

    /// Wait until a background start has finished. Mirrors the old twenty-second
    /// safety net; the process itself may still be inside its forty-second
    /// startup on a very slow machine.
    pub fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.ready.load(Ordering::SeqCst) {
                if let Some(error) = self.last_error() {
                    return Err(format!("grin-wallet не запустился: {error}"));
                }
                return if self
                    .backend
                    .lock()
                    .map(|guard| guard.is_some())
                    .unwrap_or(false)
                {
                    Ok(())
                } else {
                    Err("grin-wallet backend недоступен".into())
                };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!(
            "grin-wallet не запустился за {} секунд",
            timeout.as_secs()
        ))
    }

    /// Run one owner-API operation under the backend mutex. The call is blocking:
    /// a scan can keep this mutex for its whole duration by design.
    pub fn call<T, F>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&mut RpcBackend) -> flint_core::Result<T>,
    {
        self.wait_ready(Duration::from_secs(20))?;
        let mut guard = self
            .backend
            .lock()
            .map_err(|_| "состояние RPC занято".to_string())?;
        let backend = guard
            .as_mut()
            .ok_or_else(|| "grin-wallet backend недоступен".to_string())?;
        f(backend).map_err(|e| e.to_string())
    }

    /// Stop the child and reconnect from the current settings. Used after a
    /// killed scan and after `node_url` changes. Retries because LMDB may still
    /// hold the old database for a moment after the child is terminated.
    pub fn restart(&self) -> Result<(), String> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("приложение закрывается — backend не перезапускается".into());
        }
        let expected = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.ready.store(false, Ordering::SeqCst);
        self.set_error(None);
        self.drop_slots();

        let result = self.restart_attempts(expected);
        self.set_error(result.as_ref().err().cloned());
        self.ready.store(true, Ordering::SeqCst);
        result
    }

    fn restart_attempts(&self, expected: u64) -> Result<(), String> {
        let config = self.snapshot()?;
        let paths = AppPaths::at(config.root);
        let mut last_error = String::new();
        for attempt in 0..20 {
            if self.generation.load(Ordering::SeqCst) != expected {
                return Err("контекст кошелька изменился во время перезапуска RPC".into());
            }
            match WalletProcess::start_with_node(
                &paths,
                &config.network,
                config.profile.as_deref(),
                &config.node,
            ) {
                Ok(process) => {
                    match RpcBackend::connect(
                        &process.url(),
                        process.secret(),
                        config.min_confirmations,
                    ) {
                        Ok(mut backend) => {
                            backend.set_address_index(config.address_index);
                            backend.set_needs_scan(config.needs_scan);
                            backend.set_node_snapshot(config.node.clone());
                            backend.set_journal_dir(config.journal.clone());
                            let _commit = self
                                .commit
                                .lock()
                                .map_err(|_| "состояние RPC занято".to_string())?;
                            if self.shutting_down.load(Ordering::SeqCst)
                                || self.generation.load(Ordering::SeqCst) != expected
                            {
                                return Err(
                                    "контекст кошелька изменился во время перезапуска RPC".into()
                                );
                            }
                            *self
                                .backend
                                .lock()
                                .map_err(|_| "состояние RPC занято".to_string())? = Some(backend);
                            *self
                                .process
                                .lock()
                                .map_err(|_| "процесс grin-wallet занят".to_string())? =
                                Some(process);
                            return Ok(());
                        }
                        Err(e) => last_error = e.to_string(),
                    }
                }
                Err(e) => last_error = e.to_string(),
            }
            if attempt < 19 {
                std::thread::sleep(Duration::from_millis(400));
            }
        }
        Err(format!(
            "не удалось перезапустить grin-wallet: {last_error}"
        ))
    }

    /// Restart only when the child process has died. A live process is left
    /// alone: it may be scanning and restarting it would lose that progress.
    pub fn restart_if_dead(&self) -> Result<(), String> {
        if self.is_process_running() {
            return Ok(());
        }
        self.restart()
    }

    pub fn is_process_running(&self) -> bool {
        self.process
            .lock()
            .map(|mut guard| {
                guard
                    .as_mut()
                    .map(WalletProcess::is_running)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Terminate the child without touching the backend slot. The scan path
    /// calls this while its own `call` still holds the backend mutex.
    pub fn terminate_process(&self) -> bool {
        match self.process.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(process) => {
                    process.terminate();
                    true
                }
                None => false,
            },
            Err(_) => false,
        }
    }

    pub fn port(&self) -> Option<u16> {
        self.process
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(WalletProcess::port))
    }

    pub fn set_min_confirmations(&self, value: u64) {
        if let Ok(mut guard) = self.backend.lock() {
            if let Some(backend) = guard.as_mut() {
                backend.set_min_confirmations(value);
            }
        }
    }

    pub fn unlocked(&self) -> bool {
        self.backend
            .lock()
            .map(|guard| guard.as_ref().map(RpcBackend::unlocked).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Cheap status read for `wallet_status`. The caller must not invoke this
    /// while an RPC scan owns the backend mutex, unless it wants to wait; the
    /// scan path reports `scan_cancellable` separately and skips the address.
    pub fn status_snapshot(&self, with_address: bool) -> (bool, Option<String>) {
        let mut guard = match self.backend.lock() {
            Ok(guard) => guard,
            Err(_) => return (false, None),
        };
        match guard.as_mut() {
            Some(backend) => {
                let unlocked = backend.unlocked();
                let address = if with_address && unlocked {
                    backend.address().ok()
                } else {
                    None
                };
                (unlocked, address)
            }
            None => (false, None),
        }
    }

    /// Invalidate a start that is still in flight without touching a live
    /// backend. Used by the delete path before it validates the confirmation.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Drop the backend and child. `ready = false` so a following `wait_ready`
    /// waits for an explicit restart instead of returning a stale success.
    pub fn stop(&self) {
        {
            // Serialize the generation bump with the final publish in `start`.
            // Do not hold this lock while waiting for the (possibly scan-held)
            // backend mutex: shutdown must still be able to terminate the child.
            let _commit = match self.commit.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.ready.store(false, Ordering::SeqCst);
            self.set_error(None);
        }
        self.drop_slots();
    }

    /// Stop and leave the session in a settled "no backend" state. Used after a
    /// wallet deletion: commands must fail immediately instead of polling for a
    /// restart that will never come.
    pub fn stop_settled(&self) {
        self.stop();
        self.ready.store(true, Ordering::SeqCst);
    }

    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    /// Best-effort process exit path. A scan can hold the backend mutex for
    /// minutes; do not block the window close on it. Terminating the child is
    /// what matters, and `Drop` of `WalletProcess` is synchronous.
    pub fn shutdown(&self) {
        self.begin_shutdown();
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.ready.store(false, Ordering::SeqCst);
        self.set_error(None);
        // Wait only for the short commit section, never for the scan-held
        // backend mutex. A start that already passed its generation check sees
        // `shutting_down` and refuses to publish.
        let _commit = match self.commit.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if let Ok(mut guard) = self.backend.try_lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.process.lock() {
            if let Some(mut process) = guard.take() {
                process.terminate();
            }
        }
    }

    fn drop_slots(&self) {
        if let Ok(mut guard) = self.backend.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.process.lock() {
            if let Some(mut process) = guard.take() {
                process.terminate();
            }
        }
    }

    fn set_error(&self, error: Option<String>) {
        if let Ok(mut guard) = self.error.lock() {
            *guard = error;
        }
    }

    fn snapshot(&self) -> Result<StartConfig, String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        let network = wallet.settings().network.clone();
        let dir = rpc_backend::wallet_dir(
            wallet.paths(),
            &network,
            wallet.settings().wallet_name.as_deref(),
        );
        Ok(StartConfig {
            root: wallet.paths().root().to_path_buf(),
            network,
            profile: wallet.settings().wallet_name.clone(),
            journal: dir.join("rpc-pending"),
            min_confirmations: wallet.settings().min_confirmations,
            address_index: wallet.address_index(),
            needs_scan: wallet.pending_scan(),
            node: wallet.node_snapshot(),
        })
    }
}

/// Per-profile journal directory used by the RPC send/receive records.
pub fn journal_dir(wallet: &WalletService) -> PathBuf {
    rpc_backend::wallet_dir(
        wallet.paths(),
        &wallet.settings().network,
        wallet.settings().wallet_name.as_deref(),
    )
    .join("rpc-pending")
}

/// Opening can wait on a scan that holds the wallet lock; say so instead of
/// showing a transport timeout.
pub fn open_error(error: FlintError) -> FlintError {
    let text = error.to_string();
    if text.contains("error sending request") || text.contains("timed out") {
        FlintError::Wallet("кошелёк занят сканированием цепи — попробуйте через минуту".into())
    } else {
        error
    }
}
