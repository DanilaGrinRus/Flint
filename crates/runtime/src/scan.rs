//! Scan coordination for both wallet backends.
//!
//! The embedded scanner is a cooperative batch job: it polls a token every
//! 10 001 blocks and keeps the `WalletService` lock free between batches. The
//! RPC scanner is a single owner-API call that can run for an hour; it is
//! cancelled by terminating the separate `grin-wallet` process.
//!
//! The call-scoped slot (`busy` + cancellation token) is deliberately small and
//! lives under one mutex: `start` and `cancel` must decide against the same
//! `busy` value, otherwise a cancellation can land after the scan result was
//! already applied and kill a process that belongs to the next operation.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use flint_core::{ScanState, WalletService, SCAN_CANCELLED};

use crate::rpc::RpcRuntime;

/// Result of one scan call, without the wallet lock held.
#[derive(Clone, Debug)]
pub struct ScanView {
    pub busy: bool,
    pub state: ScanState,
    pub pending: bool,
    pub error: Option<String>,
    pub done: Option<u64>,
    pub target: Option<u64>,
    /// Embedded scan is paused between batches.
    pub paused: bool,
    /// RPC scans cannot pause without killing and restarting the child.
    pub pausable: bool,
}

impl ScanView {
    /// A scan owned by this process is cancellable on both backends: RPC by
    /// killing the child, embedded at the next batch boundary.
    pub fn cancellable(&self) -> bool {
        self.busy
    }
}

#[derive(Default)]
struct ScanControl {
    busy: bool,
    /// Set while a network/profile switch is installing a new WalletService.
    transitioning: bool,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    done: Option<u64>,
    target: Option<u64>,
}

/// Holds the scan transition slot for the duration of `WalletRuntime::switch`.
pub struct ScanSwitch {
    control: Arc<Mutex<ScanControl>>,
}

impl Drop for ScanSwitch {
    fn drop(&mut self) {
        if let Ok(mut control) = self.control.lock() {
            control.transitioning = false;
        }
    }
}

#[derive(Clone)]
pub struct ScanRuntime {
    wallet: Arc<Mutex<WalletService>>,
    rpc: RpcRuntime,
    control: Arc<Mutex<ScanControl>>,
    /// Incremented on every network/profile switch. A result that settles after
    /// the switch must not be written into the new wallet.
    generation: Arc<AtomicU64>,
}

fn stale_scan_result() -> String {
    "контекст кошелька изменился во время скана — результат не применён".into()
}

impl ScanRuntime {
    pub fn new(wallet: Arc<Mutex<WalletService>>, rpc: RpcRuntime) -> Self {
        Self {
            wallet,
            rpc,
            control: Arc::new(Mutex::new(ScanControl::default())),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Invalidate any result still in flight after a network/profile switch.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Identity of the active wallet context. UI workers capture this before an
    /// operation and discard their result if a switch increments it meanwhile.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn busy(&self) -> bool {
        self.control.lock().map(|guard| guard.busy).unwrap_or(false)
    }

    pub fn ensure_idle(&self, what: &str) -> Result<(), String> {
        if self.busy() {
            return Err(format!(
                "скан идёт — «{what}» станет доступно после окончания или отмены"
            ));
        }
        Ok(())
    }

    /// Reserve the transition slot so a scan cannot start while the runtime
    /// replaces WalletService. Dropping the guard releases it.
    pub fn begin_switch(&self) -> Result<ScanSwitch, String> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| "состояние скана занято".to_string())?;
        if control.busy {
            return Err("скан идёт — смена сети/кошелька запрещена".into());
        }
        if control.transitioning {
            return Err("смена кошелька уже выполняется".into());
        }
        control.transitioning = true;
        Ok(ScanSwitch {
            control: self.control.clone(),
        })
    }

    /// The wording used by summary/activity/pending/send while a scan holds the
    /// wallet. The in-process flag closes the window before `running` is
    /// observed; the persisted state covers a scan started by upstream.
    pub fn busy_error(&self, what: &str) -> Result<Option<String>, String> {
        if self.busy() {
            return Ok(Some(format!(
                "кошелёк сканирует цепь — {what} появится после окончания"
            )));
        }
        let view = self.view()?;
        Ok((view.state == ScanState::Running)
            .then(|| format!("кошелёк сканирует цепь — {what} появится после окончания")))
    }

    pub fn view(&self) -> Result<ScanView, String> {
        let (state, pending, error) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                wallet.scan_state(),
                wallet.pending_scan(),
                wallet.scan_error().map(str::to_string),
            )
        };
        let control = self
            .control
            .lock()
            .map_err(|_| "состояние скана занято".to_string())?;
        Ok(ScanView {
            busy: control.busy,
            state,
            pending,
            error,
            done: control.done,
            target: control.target,
            paused: control.pause.load(Ordering::SeqCst),
            pausable: control.busy && !self.rpc.enabled(),
        })
    }

    pub fn state(&self) -> ScanState {
        self.wallet
            .lock()
            .map(|wallet| wallet.scan_state())
            .unwrap_or(ScanState::NotStarted)
    }

    /// Run one scan. Blocking: the embedded job polls the token between
    /// batches, the RPC call can wait for the owner API for up to an hour.
    pub fn scan<F>(&self, start_height: Option<u64>, progress: F) -> Result<u64, String>
    where
        F: Fn(u64, u64),
    {
        if self.state() == ScanState::Running {
            return Err("скан уже идёт — дождитесь окончания".into());
        }
        // A restored wallet must not be scanned from a partial height: that
        // would mark it complete while old outputs were never visited.
        if start_height.is_some() && self.view().map(|view| view.pending).unwrap_or(false) {
            return Err(
                "частичный скан запрещён до полного скана восстановленного кошелька".into(),
            );
        }
        if self.rpc.enabled() {
            self.scan_rpc(start_height)
        } else {
            self.scan_embedded(start_height, progress)
        }
    }

    fn scan_rpc(&self, start_height: Option<u64>) -> Result<u64, String> {
        let _cancel = self.begin_call()?;
        let info = match self.with_wallet(|wallet| Ok(wallet.node_info())) {
            Ok(info) => info,
            Err(error) => {
                self.finish_call();
                return Err(error);
            }
        };
        if !info.phase.ready() {
            self.finish_call();
            return Err(format!(
                "нода не готова: {:?} (высота {}) — скан не начат",
                info.phase, info.height
            ));
        }
        if let Err(e) = self.with_wallet(|wallet| wallet.begin_scan()) {
            self.finish_call();
            return Err(e);
        }
        let generation = self.generation.load(Ordering::SeqCst);
        let result = self.rpc.call(|backend| backend.scan(start_height));

        let cancel_requested = self.finish_call();
        if !cancel_requested {
            return match result {
                Ok(height) => {
                    self.with_wallet(|wallet| {
                        if self.generation.load(Ordering::SeqCst) != generation {
                            return Err(flint_core::FlintError::Invalid(stale_scan_result()));
                        }
                        wallet.complete_scan()
                    })?;
                    Ok(height)
                }
                Err(error) => {
                    let text = error.clone();
                    self.with_wallet(|wallet| {
                        if self.generation.load(Ordering::SeqCst) != generation {
                            return Err(flint_core::FlintError::Invalid(stale_scan_result()));
                        }
                        wallet.fail_scan(&text)
                    })?;
                    Err(error)
                }
            };
        }

        match result {
            Ok(height) => {
                self.with_wallet(|wallet| {
                    if self.generation.load(Ordering::SeqCst) != generation {
                        return Err(flint_core::FlintError::Invalid(stale_scan_result()));
                    }
                    wallet.complete_scan()
                })?;
                if let Err(e) = self.rpc.restart_if_dead() {
                    return Err(format!("скан завершён, но backend не перезапущен: {e}"));
                }
                Ok(height)
            }
            Err(_error) => {
                let marked = self.with_wallet(|wallet| {
                    if self.generation.load(Ordering::SeqCst) != generation {
                        return Err(flint_core::FlintError::Invalid(stale_scan_result()));
                    }
                    if wallet.scan_state() == ScanState::Running {
                        wallet.cancel_scan(SCAN_CANCELLED)?;
                    }
                    Ok(())
                });
                let restarted = self.rpc.restart();
                match (marked, restarted) {
                    (Ok(()), Ok(())) => Err(SCAN_CANCELLED.to_string()),
                    (Err(mark), Ok(())) => Err(format!(
                        "{SCAN_CANCELLED}; состояние не сохранилось: {mark}"
                    )),
                    (Ok(()), Err(restart)) => Err(format!(
                        "{SCAN_CANCELLED}; backend не перезапущен: {restart}"
                    )),
                    (Err(mark), Err(restart)) => Err(format!(
                        "{SCAN_CANCELLED}; состояние не сохранилось: {mark}; \
                         backend не перезапущен: {restart}"
                    )),
                }
            }
        }
    }

    fn scan_embedded<F>(&self, start_height: Option<u64>, progress: F) -> Result<u64, String>
    where
        F: Fn(u64, u64),
    {
        let cancel = self.begin_call()?;
        let pause = match self.control.lock() {
            Ok(control) => control.pause.clone(),
            Err(_) => {
                self.finish_call();
                return Err("состояние скана занято".into());
            }
        };
        let generation = self.generation.load(Ordering::SeqCst);
        let job = match self.with_wallet(|wallet| wallet.prepare_scan(start_height)) {
            Ok(job) => job,
            Err(e) => {
                self.finish_call();
                return Err(e);
            }
        };
        let control = self.control.clone();
        let outcome = job.run_with_progress_pausable(
            cancel.as_ref(),
            Some(pause.as_ref()),
            move |done, target| {
                if let Ok(mut control) = control.lock() {
                    control.done = Some(done);
                    control.target = Some(target);
                }
                progress(done, target);
            },
        );
        let result = self.with_wallet(|wallet| {
            if self.generation.load(Ordering::SeqCst) != generation {
                return Err(flint_core::FlintError::Invalid(stale_scan_result()));
            }
            wallet.finish_scan(outcome)
        });
        self.finish_call();
        result
    }

    /// Abort the active scan. RPC scans end their separate child; embedded scans
    /// set the token and finish at the next batch boundary.
    /// Pause an embedded scan at the next batch boundary. RPC scans are
    /// implemented as one upstream call and can only be cancelled.
    pub fn pause(&self) -> Result<(), String> {
        let control = self
            .control
            .lock()
            .map_err(|_| "состояние скана занято".to_string())?;
        if !control.busy {
            return Err("скан не идёт".into());
        }
        if self.rpc.enabled() {
            return Err("RPC-скан нельзя приостановить; доступна только отмена".into());
        }
        control.pause.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn resume(&self) -> Result<(), String> {
        let control = self
            .control
            .lock()
            .map_err(|_| "состояние скана занято".to_string())?;
        if !control.busy {
            return Err("скан не идёт".into());
        }
        control.pause.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn cancel(&self) -> Result<(), String> {
        let control = self
            .control
            .lock()
            .map_err(|_| "состояние скана занято".to_string())?;
        if !control.busy {
            return Err("скан не идёт".into());
        }
        control.cancel.store(true, Ordering::SeqCst);
        if !self.rpc.enabled() {
            return Ok(());
        }
        if self.rpc.terminate_process() {
            Ok(())
        } else {
            control.cancel.store(false, Ordering::SeqCst);
            Err("процесс grin-wallet не запущен — скан нельзя прервать".into())
        }
    }

    fn begin_call(&self) -> Result<Arc<AtomicBool>, String> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| "состояние скана занято".to_string())?;
        if control.busy {
            return Err("скан уже идёт — дождитесь окончания".into());
        }
        if control.transitioning {
            return Err("смена кошелька выполняется — скан не начинается".into());
        }
        control.busy = true;
        control.cancel.store(false, Ordering::SeqCst);
        control.pause.store(false, Ordering::SeqCst);
        control.done = None;
        control.target = None;
        Ok(control.cancel.clone())
    }

    fn finish_call(&self) -> bool {
        match self.control.lock() {
            Ok(mut control) => {
                control.busy = false;
                let requested = control.cancel.load(Ordering::SeqCst);
                control.cancel.store(false, Ordering::SeqCst);
                control.pause.store(false, Ordering::SeqCst);
                requested
            }
            Err(_) => false,
        }
    }

    fn with_wallet<T>(
        &self,
        f: impl FnOnce(&mut WalletService) -> flint_core::Result<T>,
    ) -> Result<T, String> {
        let mut wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        f(&mut wallet).map_err(|e| e.to_string())
    }
}
