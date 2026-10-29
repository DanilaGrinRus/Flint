//! Backend-neutral wallet operations.
//!
//! These methods dispatch between the embedded `WalletService` and the RPC
//! `grin-wallet` child. Shells should not grow `if has_rpc` branches for the
//! ordinary read/lifecycle paths: the runtime returns the same wire types from
//! both backends, and only UI-specific details (history key, Tor, events) stay
//! in the shell.

use std::time::Duration;

use flint_core::seed::{SeedKind, SeedStore};
use flint_core::service::PendingView;
use flint_core::{
    CreatedWallet, ReceiveSlate, ScanState, SendSlate, TxItem, WalletService, WalletStatus,
    WalletSummary,
};

use crate::rpc;
use crate::scan::ScanView;
use crate::WalletRuntime;

/// Result of `open`: the embedded backend can reconcile pending entries while
/// opening; the RPC backend leaves that to its journal.
#[derive(Clone, Debug)]
pub struct OpenOutcome {
    pub reconciled: usize,
}

fn pending_action_allowed(state: &str, retry: bool) -> bool {
    if retry {
        matches!(state, "finalizing" | "finalized_not_posted")
    } else {
        matches!(state, "prepared" | "locked")
    }
}

#[derive(Clone, Debug)]
pub struct PendingList {
    pub items: Vec<PendingView>,
    pub unreadable: Vec<String>,
    /// Directory the user can inspect when an entry is unreadable.
    pub diagnostics: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeStatus {
    pub status: WalletStatus,
    pub scan: ScanView,
}

/// Profile list plus the active profile name.
#[derive(Clone, Debug)]
pub struct RuntimeProfiles {
    pub profiles: Vec<String>,
    pub active: String,
}

impl WalletRuntime {
    fn wallet_dir(&self) -> Result<(std::path::PathBuf, String), String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        let network = wallet.settings().network.clone();
        let dir = flint_core::rpc_backend::wallet_dir(
            wallet.paths(),
            &network,
            wallet.settings().wallet_name.as_deref(),
        );
        Ok((dir, network))
    }

    pub(crate) fn with_wallet<T>(
        &self,
        f: impl FnOnce(&mut WalletService) -> flint_core::Result<T>,
    ) -> Result<T, String> {
        let mut wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        f(&mut wallet).map_err(|e| e.to_string())
    }

    fn scan_busy_error(&self, what: &str) -> Result<Option<String>, String> {
        self.scan.busy_error(what)
    }

    /// Status for the UI. RPC status deliberately leaves `node` empty: the
    /// shell polls `node_info` separately so an offline node cannot hold the
    /// first paint.
    pub fn status(&self) -> Result<RuntimeStatus, String> {
        let scan = self.scan.view()?;
        if self.is_rpc() {
            Ok(RuntimeStatus {
                status: self.status_rpc(&scan)?,
                scan,
            })
        } else {
            let status = {
                let wallet = self
                    .wallet
                    .lock()
                    .map_err(|_| "кошелёк занят".to_string())?;
                if scan.busy {
                    wallet.status_without_address()
                } else {
                    wallet.status_fast()
                }
            };
            Ok(RuntimeStatus { status, scan })
        }
    }

    fn status_rpc(&self, scan: &ScanView) -> Result<WalletStatus, String> {
        let (network, initialized, data_dir, scan_state, scan_error, pending_scan) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            let network = wallet.settings().network.clone();
            let dir = flint_core::rpc_backend::wallet_dir(
                wallet.paths(),
                &network,
                wallet.settings().wallet_name.as_deref(),
            );
            (
                network,
                flint_core::config::wallet_data_has_seed(&dir),
                wallet.paths().root().display().to_string(),
                wallet.scan_state(),
                wallet.scan_error().map(str::to_string),
                wallet.pending_scan(),
            )
        };
        let scan_cancellable = scan.busy;
        let (unlocked, address) = if scan.state == ScanState::Running || scan_cancellable {
            (scan_cancellable, None)
        } else {
            self.rpc
                .status_snapshot(!(pending_scan && scan.state == ScanState::Failed))
        };
        Ok(WalletStatus {
            initialized,
            unlocked,
            network,
            mode: "grin-wallet".to_string(),
            address,
            node: Default::default(),
            data_dir,
            scan_state,
            scan_error,
            scan_cancellable,
        })
    }

    /// Show the active profile's seed after checking the password. Display-only:
    /// no unlock, no disk write; the caller must clear the returned string.
    pub fn reveal_mnemonic(&self, password: &str) -> Result<String, String> {
        self.with_wallet(|wallet| wallet.reveal_mnemonic(password))
    }

    /// Create a wallet in the selected profile. Both backends write the same
    /// Argon2id envelope before upstream creates its database.
    pub fn create(&self, password: &str) -> Result<CreatedWallet, String> {
        self.scan.ensure_idle("создание кошелька")?;
        if self.is_rpc() {
            return self.create_rpc(password);
        }
        self.with_wallet(|wallet| wallet.create_wallet(password))
    }

    /// Open the active wallet. RPC handles the seed envelope and the journal
    /// migration here; the shell only adds the messenger history key.
    pub fn open(&self, password: &str) -> Result<OpenOutcome, String> {
        self.scan.ensure_idle("открытие кошелька")?;
        if !self.is_rpc() {
            return self
                .with_wallet(|wallet| wallet.open_wallet_and_reconcile(password))
                .map(|reconciled| OpenOutcome { reconciled });
        }
        self.open_rpc(password)
    }

    fn open_rpc(&self, password: &str) -> Result<OpenOutcome, String> {
        let scan = self.scan.view()?;
        if scan.state == ScanState::Running && scan.busy {
            return Err("кошелёк сканирует цепь — открытие станет доступно после окончания".into());
        }
        let (dir, network) = self.wallet_dir()?;
        let store = SeedStore::new(&dir, &network);
        store.ensure_mainnet_ready().map_err(|e| e.to_string())?;
        let opened = match store.kind() {
            SeedKind::Missing => return Err("кошелёк не найден".into()),
            SeedKind::Legacy => self.rpc.call(|backend| {
                backend
                    .open_with_timeout(password, Duration::from_secs(8))
                    .map_err(rpc::open_error)
            }),
            SeedKind::Envelope => self.rpc.call(|backend| {
                backend
                    .open_with_envelope(&store, password, Duration::from_secs(8))
                    .map_err(rpc::open_error)
            }),
        };
        if let Err(error) = opened {
            // A busy wallet is an observation, not a completed scan: keep
            // `pending_scan`, record `running`, and let the next open finish it.
            let busy = error.contains("занят сканированием")
                || error.contains("timed out")
                || error.contains("error sending request");
            if busy && scan.pending {
                self.with_wallet(|wallet| wallet.note_scan_running())?;
            }
            return Err(error);
        }

        let fresh_db = self.rpc.call(|backend| Ok(backend.needs_scan()))?;
        if fresh_db {
            self.rpc.call(|backend| {
                backend.set_needs_scan(false);
                Ok(())
            })?;
            if scan.state != ScanState::Failed {
                self.with_wallet(|wallet| wallet.require_scan())?;
            }
        } else if scan.state == ScanState::Running {
            self.with_wallet(|wallet| wallet.complete_scan())?;
        }

        let (old_journal, new_journal) = self.with_wallet(|wallet| {
            Ok((
                wallet
                    .paths()
                    .root()
                    .join(&wallet.settings().network)
                    .join("rpc-pending"),
                rpc::journal_dir(wallet),
            ))
        })?;
        if old_journal != new_journal {
            match self
                .rpc
                .call(move |backend| backend.migrate_legacy_journal(&old_journal))
            {
                Ok(0) => {}
                Ok(imported) => {
                    eprintln!("rpc journal: импортировано старых записей: {imported}");
                }
                Err(error) => {
                    eprintln!("rpc journal: старый каталог не импортирован: {error}");
                }
            }
        }
        Ok(OpenOutcome { reconciled: 0 })
    }

    pub fn lock(&self) -> Result<(), String> {
        self.scan.ensure_idle("блокировка кошелька")?;
        if self.is_rpc() {
            self.rpc.call(|backend| backend.lock())?;
            let (dir, network) = self.wallet_dir()?;
            SeedStore::new(&dir, &network).remove_transient_seed();
            return Ok(());
        }
        self.with_wallet(|wallet| {
            wallet.lock();
            Ok(())
        })
    }

    pub fn summary(&self) -> Result<WalletSummary, String> {
        if let Some(message) = self.scan_busy_error("баланс")? {
            return Err(message);
        }
        if self.is_rpc() {
            let mut summary = self.rpc.call(|backend| backend.summary())?;
            let scan = self.scan.view()?;
            summary.needs_scan = scan.pending || summary.needs_scan;
            summary.scan_state = scan.state;
            summary.scan_error = scan.error;
            return Ok(summary);
        }
        self.with_wallet(|wallet| wallet.summary())
    }

    pub fn address(&self) -> Result<String, String> {
        if let Some(message) = self.scan_busy_error("адрес")? {
            return Err(message);
        }
        if self.is_rpc() {
            return self.rpc.call(|backend| backend.address());
        }
        self.with_wallet(|wallet| wallet.address())
    }

    /// Rotate the address index. Tor is owned by the shell and must be stopped
    /// by the caller after this returns.
    pub fn rotate_address(&self) -> Result<(u32, String), String> {
        if let Some(message) = self.scan_busy_error("смена адреса")? {
            return Err(message);
        }
        if self.is_rpc() {
            let index = self.with_wallet(|wallet| wallet.next_address_index())?;
            let address = self.rpc.call(move |backend| {
                backend.set_address_index(index);
                backend.address_at(index)
            })?;
            self.tor.stop();
            return Ok((index, address));
        }
        self.with_wallet(|wallet| {
            let index = wallet.next_address_index()?;
            let address = wallet.address()?;
            Ok((index, address))
        })
    }

    pub fn profiles(&self) -> Result<RuntimeProfiles, String> {
        let (profiles, active) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            let active = wallet.wallet_name().unwrap_or("default").to_string();
            let profiles = if self.is_rpc() {
                wallet.paths().list_rpc_wallets(&wallet.settings().network)
            } else {
                wallet.wallet_profiles()
            };
            (profiles, active)
        };
        Ok(RuntimeProfiles { profiles, active })
    }

    pub fn activity(&self, limit: usize) -> Result<Vec<TxItem>, String> {
        if let Some(message) = self.scan_busy_error("история")? {
            return Err(message);
        }
        if self.is_rpc() {
            return self.rpc.call(|backend| backend.activity(limit));
        }
        self.with_wallet(|wallet| {
            let _ = wallet.reconcile_posted();
            wallet.activity(limit)
        })
    }

    pub fn pending(&self) -> Result<PendingList, String> {
        if let Some(message) = self.scan_busy_error("список")? {
            return Err(message);
        }
        if self.is_rpc() {
            let items = self.rpc.call(|backend| backend.pending())?;
            let unreadable = self.rpc.call(|backend| backend.pending_unreadable())?;
            let diagnostics = self.rpc.call(|backend| Ok(backend.journal_dir_path()))?;
            return Ok(PendingList {
                items,
                unreadable,
                diagnostics: diagnostics
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            });
        }
        let (items, unreadable, diagnostics) = self.with_wallet(|wallet| {
            let _ = wallet.reconcile_posted();
            Ok((
                wallet.list_pending()?,
                wallet.pending_problems()?,
                wallet.pending_dir_path().display().to_string(),
            ))
        })?;
        Ok(PendingList {
            items,
            unreadable,
            diagnostics,
        })
    }

    /// Slates that need a human: unfinished for too long or stuck in a
    /// recoverable/failed step. Read-only; safe while the wallet is unlocked.
    pub fn pending_alerts(&self) -> Result<Vec<flint_core::PendingAlert>, String> {
        let list = self.pending()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(flint_core::pending_alerts(&list.items, now))
    }

    /// Refuse money operations while the node is not ready. The embedded path
    /// has its own guard inside `WalletService`; the RPC path must not rely on
    /// the UI alone.
    pub fn ensure_node_ready(&self) -> Result<(), String> {
        let (snapshot, node_dir) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            let network = wallet.settings().network.clone();
            (wallet.node_snapshot(), wallet.paths().node_dir(&network))
        };
        let info = WalletService::node_info_for(&snapshot, &node_dir);
        if info.phase.ready() {
            Ok(())
        } else {
            Err(format!(
                "нода не готова: {} (высота {}) — отправка и скан заблокированы",
                format!("{:?}", info.phase).to_lowercase(),
                info.height
            ))
        }
    }

    pub fn prepare_send(&self, destination: &str, amount_nano: u64) -> Result<SendSlate, String> {
        if let Some(message) = self.scan_busy_error("отправка")? {
            return Err(message);
        }
        if self.is_rpc() {
            self.ensure_node_ready()?;
            let (slatepack, slate_id, fee_nano) = self
                .rpc
                .call(|backend| backend.send_prepare(destination, amount_nano))?;
            return Ok(SendSlate {
                slate_id,
                slatepack,
                amount_nano: amount_nano.to_string(),
                fee_nano,
                destination: destination.to_string(),
                encrypted: true,
            });
        }
        self.with_wallet(|wallet| wallet.prepare_send(destination, amount_nano))
    }

    pub fn finalize_send(&self, slate_id: &str, response: &str) -> Result<String, String> {
        if let Some(message) = self.scan_busy_error("финализация отправки")? {
            return Err(message);
        }
        if self.is_rpc() {
            self.ensure_node_ready()?;
            self.rpc
                .call(|backend| backend.send_finalize(slate_id, response))?;
            return Ok(slate_id.to_string());
        }
        self.with_wallet(|wallet| wallet.finalize_send(slate_id, response))
    }

    pub fn prepare_receive(&self, slatepack: &str) -> Result<ReceiveSlate, String> {
        if let Some(message) = self.scan_busy_error("приём")? {
            return Err(message);
        }
        if self.is_rpc() {
            let (slatepack, slate_id, amount_nano) = self
                .rpc
                .call(|backend| backend.receive_prepare(slatepack))?;
            return Ok(ReceiveSlate {
                slate_id,
                slatepack,
                amount_nano,
            });
        }
        self.with_wallet(|wallet| wallet.prepare_receive(slatepack))
    }

    pub fn finalize_receive(&self, slate_id: &str) -> Result<(), String> {
        if let Some(message) = self.scan_busy_error("финализация приёма")? {
            return Err(message);
        }
        if self.is_rpc() {
            // The response was already handed back by the foreign API; there is
            // nothing left to sign or broadcast on the receiver side.
            return Ok(());
        }
        self.with_wallet(|wallet| wallet.finalize_receive(slate_id))
    }

    /// Export the exact S1 request for resend. It never signs a new slate.
    pub fn export_pending_request(&self, slate_id: &str) -> Result<String, String> {
        if let Some(message) = self.scan_busy_error("экспорт запроса")? {
            return Err(message);
        }
        if self.is_rpc() {
            return self
                .rpc
                .call(|backend| backend.export_send_request(slate_id));
        }
        self.with_wallet(|wallet| wallet.export_send_request(slate_id))
    }

    /// Export the saved receiver response without signing or mutating state.
    /// RPC journals do not expose the saved response through the current
    /// backend, so this action is explicit rather than guessed.
    pub fn export_pending_response(&self, slate_id: &str) -> Result<String, String> {
        if let Some(message) = self.scan_busy_error("экспорт ответа")? {
            return Err(message);
        }
        if self.is_rpc() {
            return self
                .rpc
                .call(|backend| backend.export_receive_response(slate_id))
                .map(|(armored, _amount)| armored);
        }
        self.with_wallet(|wallet| wallet.export_receive_response(slate_id))
            .map(|slate| slate.slatepack)
    }

    fn ensure_pending_action(&self, slate_id: &str, retry: bool) -> Result<(), String> {
        let list = self.pending()?;
        let item = list
            .items
            .iter()
            .find(|item| item.slate_id == slate_id)
            .ok_or_else(|| {
                "операция не входит в загруженную читаемую очередь; действие запрещено".to_string()
            })?;
        if !pending_action_allowed(&item.state, retry) {
            return Err(format!("действие запрещено в состоянии {}", item.state));
        }
        Ok(())
    }

    pub fn retry_post(&self, slate_id: &str) -> Result<String, String> {
        if let Some(message) = self.scan_busy_error("публикация")? {
            return Err(message);
        }
        self.ensure_pending_action(slate_id, true)?;
        if self.is_rpc() {
            self.ensure_node_ready()?;
            self.rpc.call(|backend| backend.retry_post(slate_id))?;
            return Ok(slate_id.to_string());
        }
        self.with_wallet(|wallet| wallet.retry_post(slate_id))
    }

    pub fn cancel_pending(&self, slate_id: &str) -> Result<(), String> {
        if let Some(message) = self.scan_busy_error("отмена транзакции")? {
            return Err(message);
        }
        self.ensure_pending_action(slate_id, false)?;
        if self.is_rpc() {
            return self.rpc.call(|backend| backend.cancel(slate_id));
        }
        self.with_wallet(|wallet| wallet.cancel_pending(slate_id))
    }

    /// Online send through the Tor SOCKS proxy owned by the shell. `socks` is
    /// called only after the backend/scan/node checks, so an embedded backend
    /// reports the backend error instead of "Tor is not running".
    pub fn send_online<F>(
        &self,
        destination: &str,
        amount_nano: u64,
        socks: F,
    ) -> Result<String, String>
    where
        F: FnOnce() -> Result<String, String>,
    {
        if let Some(message) = self.scan_busy_error("онлайн-отправка")? {
            return Err(message);
        }
        if !self.is_rpc() {
            return Err("онлайн-отправка доступна с бэкендом grin-wallet".into());
        }
        self.ensure_node_ready()?;
        let socks = socks()?;
        self.rpc
            .call(|backend| backend.send_over_tor(destination, amount_nano, &socks))
    }

    pub fn send_local(&self, amount_nano: u64) -> Result<String, String> {
        if let Some(message) = self.scan_busy_error("локальная отправка")? {
            return Err(message);
        }
        if !self.is_rpc() {
            return Err("локальный режим доступен с бэкендом grin-wallet".into());
        }
        self.ensure_node_ready()?;
        self.rpc.call(|backend| backend.send_local(amount_nano))
    }

    /// Ed25519 seed for the messenger identity. Both backends derive it from
    /// the address key at the active index; callers must zeroize the result.
    pub fn messenger_seed(&self) -> Result<[u8; 32], String> {
        if self.is_rpc() {
            let index = self.with_wallet(|wallet| Ok(wallet.address_index()))?;
            return self.rpc.call(move |backend| backend.messenger_seed(index));
        }
        self.with_wallet(|wallet| wallet.messenger_seed())
            .map(|seed| *seed)
    }

    /// Address-key seed used by the Tor listener. RPC-only: the embedded shell
    /// owns its own listener path.
    pub fn rpc_slatepack_seed(&self, index: u32) -> Result<[u8; 32], String> {
        if !self.is_rpc() {
            return Err("Tor-листенер доступен с бэкендом grin-wallet".into());
        }
        self.rpc.call(move |backend| backend.slatepack_seed(index))
    }
}

#[cfg(test)]
mod tests {
    use super::pending_action_allowed;

    #[test]
    fn pending_actions_match_the_state_machine() {
        assert!(pending_action_allowed("locked", false));
        assert!(!pending_action_allowed("locked", true));
        assert!(pending_action_allowed("prepared", false));
        assert!(!pending_action_allowed("prepared", true));
        assert!(pending_action_allowed("finalizing", true));
        assert!(!pending_action_allowed("finalizing", false));
        assert!(pending_action_allowed("finalized_not_posted", true));
        assert!(!pending_action_allowed("finalized_not_posted", false));
        assert!(!pending_action_allowed("posted", true));
        assert!(!pending_action_allowed("posted", false));
        assert!(!pending_action_allowed("failed_recoverable", false));
        assert!(!pending_action_allowed("failed_recoverable", true));
        assert!(!pending_action_allowed("response_received", true));
        assert!(!pending_action_allowed("unknown", false));
    }
}
