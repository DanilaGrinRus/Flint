//! Seed lifecycle, wallet deletion and RPC restore.
//!
//! These operations are destructive or mainnet-gated; they belong to the
//! runtime so Slint share the same checks. The history key and Tor are
//! runtime-owned too; `change_password_with_history` coordinates the seed and
//! the wrapped history key.

use std::fmt;
use std::time::Duration;

use flint_core::seed::{LegacySeed, SeedKind, SeedStore};
use flint_core::{AppPaths, RestoredWallet, WalletProcess, WalletRpc};
use zeroize::Zeroizing;

use crate::{rpc, SeedStatus, WalletRuntime};

#[derive(Clone, Debug)]
pub struct MigrateOutcome {
    pub address: String,
    pub legacy_bytes: usize,
    pub removed_backups: usize,
}

impl fmt::Display for MigrateOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} байт, backups: {})",
            self.address, self.legacy_bytes, self.removed_backups
        )
    }
}

#[derive(Clone, Debug)]
pub struct RestoreOutcome {
    pub address: String,
    pub needs_scan: bool,
    /// Embedded restore leaves the history key creation to the shell; RPC
    /// restore does not create one until the first successful open.
    pub history_key: bool,
}

#[derive(Clone, Debug)]
pub struct PasswordChangeOutcome {
    pub history_rewrapped: bool,
    pub history_error: Option<String>,
}

impl WalletRuntime {
    fn rpc_paths(
        &self,
    ) -> Result<(std::path::PathBuf, String, String, std::path::PathBuf), String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        let network = wallet.settings().network.clone();
        let name = wallet
            .settings()
            .wallet_name
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let dir = flint_core::rpc_backend::wallet_dir(
            wallet.paths(),
            &network,
            wallet.settings().wallet_name.as_deref(),
        );
        Ok((dir, network, name, wallet.paths().root().to_path_buf()))
    }

    /// Seed protection of the active wallet plus whether it is unlocked.
    /// During an RPC scan the backend mutex is held by the scan call, so the
    /// scan itself proves the wallet was unlocked; do not block the UI waiting
    /// for that mutex.
    fn seed_store(&self) -> Result<(SeedStore, String, bool), String> {
        let (network, embedded_unlocked, home) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            let network = wallet.settings().network.clone();
            let name = wallet.settings().wallet_name.clone();
            let home = if self.is_rpc() {
                flint_core::rpc_backend::wallet_dir(wallet.paths(), &network, name.as_deref())
            } else {
                wallet.paths().wallet_home_for(&network, name.as_deref())
            };
            (network, wallet.unlocked(), home)
        };
        let unlocked = if self.is_rpc() {
            if self.scan_runtime().busy() {
                true
            } else {
                self.rpc.unlocked()
            }
        } else {
            embedded_unlocked
        };
        Ok((SeedStore::new(home, &network), network, unlocked))
    }

    pub fn seed_status(&self) -> Result<SeedStatus, String> {
        let (store, network, unlocked) = self.seed_store()?;
        Ok(SeedStatus {
            kind: match store.kind() {
                SeedKind::Missing => "missing",
                SeedKind::Legacy => "legacy",
                SeedKind::Envelope => "envelope",
            }
            .to_string(),
            backups: store.legacy_backups().len(),
            legacy_leftover: store.has_legacy_leftover(),
            network,
            unlocked,
        })
    }

    /// Change the seed/owner password. The caller remains responsible for the
    /// messenger history wrapper around this call.
    pub fn change_password(&self, old: &str, new: &str) -> Result<(), String> {
        // One policy for embedded and both RPC seed kinds. Validate before any
        // history/seed work so a weak password cannot cause a partial rewrap.
        flint_core::service::validate_password(new).map_err(|e| e.to_string())?;
        self.scan_runtime().ensure_idle("смена пароля")?;
        let result = if self.is_rpc() {
            let (dir, network, _, _) = self.rpc_paths()?;
            let store = SeedStore::new(&dir, &network);
            if store.kind() == SeedKind::Envelope {
                // Only the envelope is rewrapped: upstream's password is random
                // and the user's password never reaches it.
                store.change_password(old, new).map_err(|e| e.to_string())
            } else {
                self.rpc
                    .call(|backend| backend.change_password(old, new))
                    .map_err(|e| e.to_string())
            }
        } else {
            self.with_wallet(|wallet| wallet.change_password(old, new))
        };
        if let Err(error) = result {
            if self.is_rpc() {
                if let Ok((dir, network, _, _)) = self.rpc_paths() {
                    if seed_password_matches(&dir, &network, new)
                        || legacy_password_matches(&dir, new)
                    {
                        return Err(format!(
                            "пароль seed изменён, но RPC вернул ошибку: {error}; \
                             откройте кошелёк новым паролем, ключ истории .next сохранён"
                        ));
                    }
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// Change the seed password and rewrap the per-profile history key in one
    /// operation. The next wrapper is written before the seed changes, so a
    /// crash between the two still leaves a recoverable `.history.key.next`.
    pub fn change_password_with_history(
        &self,
        old: &str,
        new: &str,
    ) -> Result<PasswordChangeOutcome, String> {
        flint_core::service::validate_password(new).map_err(|e| e.to_string())?;
        let pending = self.prepare_history_rewrap(old, new)?;
        let changed = self.change_password(old, new);
        // The old cached password can outlive a successful change, or a change
        // whose RPC returned an ambiguous error after committing. Drop the
        // profile cache before propagating either result.
        self.clear_unlock_cache();
        changed?;
        match pending {
            Some(pending) => match self.commit_history_rewrap(&pending) {
                Ok(()) => Ok(PasswordChangeOutcome {
                    history_rewrapped: true,
                    history_error: None,
                }),
                Err(error) => Ok(PasswordChangeOutcome {
                    history_rewrapped: false,
                    history_error: Some(error),
                }),
            },
            None => Ok(PasswordChangeOutcome {
                history_rewrapped: true,
                history_error: None,
            }),
        }
    }

    pub fn migrate_envelope(
        &self,
        password: &str,
        confirmation: &str,
        remove_backups: bool,
    ) -> Result<MigrateOutcome, String> {
        if password != confirmation {
            return Err("пароли не совпадают".into());
        }
        let (store, network, unlocked) = self.seed_store()?;
        if unlocked {
            return Err("заблокируйте кошелёк перед включением защиты seed".into());
        }
        if store.kind() != SeedKind::Legacy {
            return Err("кошелёк уже защищён или seed отсутствует".into());
        }
        let entropy =
            LegacySeed::read_file(&store.legacy_path(), password).map_err(|e| e.to_string())?;
        let expected =
            flint_core::address::address_from_entropy(entropy.as_slice(), network != "mainnet", 0)
                .map_err(|e| e.to_string())?;
        let actual = if self.is_rpc() {
            let pwd = Zeroizing::new(password.to_string());
            self.rpc.call(move |backend| {
                let result = (|| -> flint_core::Result<String> {
                    backend
                        .open_with_timeout(pwd.as_str(), Duration::from_secs(8))
                        .map_err(rpc::open_error)?;
                    backend.address_at(0)
                })();
                if let Err(e) = backend.lock() {
                    return Err(e);
                }
                result
            })?
        } else {
            self.with_wallet(|wallet| wallet.verify_legacy_address(password))?
        };
        if actual != expected {
            return Err(
                "адрес кошелька не совпадает с адресом из seed: миграция остановлена, wallet.seed не тронут"
                    .into(),
            );
        }
        let outcome = store
            .migrate_legacy(password, remove_backups)
            .map_err(|e| e.to_string())?;
        Ok(MigrateOutcome {
            address: outcome.address,
            legacy_bytes: outcome.legacy_bytes,
            removed_backups: outcome.removed_backups.len(),
        })
    }

    pub fn disable_envelope(&self, password: &str) -> Result<(), String> {
        let (store, network, unlocked) = self.seed_store()?;
        if network == "mainnet" {
            return Err(
                "на mainnet откат запрещён: кошелёк без конверта не откроется — используйте новый кошелёк"
                    .into(),
            );
        }
        if unlocked {
            return Err("заблокируйте кошелёк перед отключением Argon2id".into());
        }
        store.disable_envelope(password).map_err(|e| e.to_string())
    }

    pub fn repair_envelope(&self, password: &str, force: bool) -> Result<String, String> {
        let (store, _, unlocked) = self.seed_store()?;
        if unlocked {
            return Err("заблокируйте кошелёк перед восстановлением seed".into());
        }
        let path = if force {
            store.restore_legacy_from_envelope(password)
        } else {
            store.quarantine_unreadable_envelope(password)
        }
        .map_err(|e| e.to_string())?;
        Ok(path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default())
    }

    /// Delete the active wallet: embedded moves the wallet directory, RPC stops
    /// the child and moves the RPC directory into `trash/<network>`. Tor is
    /// stopped by the caller before this method.
    pub fn delete_wallet(&self, confirm_name: &str) -> Result<String, String> {
        self.scan_runtime().ensure_idle("удаление кошелька")?;
        self.tor.stop();
        if !self.is_rpc() {
            return self.with_wallet(|wallet| wallet.delete_wallet(confirm_name));
        }

        // Cancel a late background start before validating the name.
        self.rpc.invalidate();
        let (dir, network, name, root) = self.rpc_paths()?;
        if confirm_name.trim() != name {
            return Err(format!("для удаления введите имя кошелька: {name}"));
        }
        if !dir.exists() {
            return Err("RPC-кошелёк не найден".into());
        }
        self.rpc.stop();
        let target = trash_target(&root, &network, &name)?;
        // The child may release the LMDB lock a moment after terminate().
        let mut last = None;
        for _ in 0..20 {
            match std::fs::rename(&dir, &target) {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        if let Some(e) = last {
            return Err(format!("не удалось убрать RPC-кошелёк: {e}"));
        }
        self.with_wallet(|wallet| wallet.set_wallet_name(None))?;
        self.rpc.stop_settled();
        Ok(target.display().to_string())
    }

    /// Restore a wallet from a mnemonic. Embedded returns a history flag; RPC
    /// restore writes the envelope first and leaves the wallet locked.
    pub fn restore(&self, mnemonic: &str, password: &str) -> Result<RestoreOutcome, String> {
        self.scan_runtime().ensure_idle("восстановление кошелька")?;
        self.tor.stop();
        if !self.is_rpc() {
            let restored: RestoredWallet =
                self.with_wallet(|wallet| wallet.restore_wallet(mnemonic, password))?;
            return Ok(RestoreOutcome {
                address: restored.address,
                needs_scan: restored.needs_scan,
                history_key: true,
            });
        }
        self.restore_rpc(mnemonic, password)
    }

    pub(crate) fn create_rpc(&self, password: &str) -> Result<flint_core::CreatedWallet, String> {
        flint_core::service::validate_password(password).map_err(|e| e.to_string())?;
        let (dir, network, _, _) = self.rpc_paths()?;
        let generated = SeedStore::new(&dir, &network)
            .generate()
            .map_err(|e| e.to_string())?;
        let mnemonic = Zeroizing::new(generated.mnemonic.to_string());
        let address = self.initialize_rpc(mnemonic.as_str(), password, false, true)?;
        Ok(flint_core::CreatedWallet {
            mnemonic: mnemonic.to_string(),
            address,
        })
    }

    fn restore_rpc(&self, mnemonic: &str, password: &str) -> Result<RestoreOutcome, String> {
        let address = self.initialize_rpc(mnemonic, password, true, false)?;
        Ok(RestoreOutcome {
            address,
            needs_scan: true,
            history_key: false,
        })
    }

    /// Shared RPC create/restore transaction. The envelope and the restore scan
    /// flag are durable before the upstream database exists. Creation leaves the
    /// new wallet open; restore verifies the address and locks it again.
    fn initialize_rpc(
        &self,
        mnemonic: &str,
        password: &str,
        needs_scan: bool,
        leave_unlocked: bool,
    ) -> Result<String, String> {
        flint_core::service::validate_password(password).map_err(|e| e.to_string())?;
        let (root, network, profile, testnet, node_view) = {
            let wallet = self
                .wallet
                .lock()
                .map_err(|_| "кошелёк занят".to_string())?;
            (
                wallet.paths().root().to_path_buf(),
                wallet.settings().network.clone(),
                wallet.settings().wallet_name.clone(),
                wallet.settings().is_testnet(),
                wallet.node_snapshot(),
            )
        };
        let paths = AppPaths::at(root);
        let dir = flint_core::rpc_backend::wallet_dir(&paths, &network, profile.as_deref());
        let store = SeedStore::new(&dir, &network);
        if store.is_initialized() || dir.join("wallet_data").join("db").exists() {
            return Err(
                "в выбранном кошельке уже есть seed или база: операция разрешена только в пустой каталог"
                    .into(),
            );
        }
        let entropy =
            flint_core::seed::entropy_from_mnemonic(mnemonic).map_err(|e| e.to_string())?;
        let address = flint_core::address::address_from_entropy(entropy.as_slice(), testnet, 0)
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir.join("wallet_data")).map_err(err)?;
        store
            .write_envelope(password, &address, entropy.as_slice(), None)
            .map_err(err)?;
        if needs_scan {
            self.with_wallet(|wallet| wallet.require_scan())?;
        }

        let mnemonic_cli = Zeroizing::new(mnemonic.trim().to_string());
        let upstream = Zeroizing::new(SeedStore::new_upstream_password().map_err(err)?);
        let process = WalletProcess::start_uninitialized_with_node(
            &paths,
            &network,
            profile.as_deref(),
            &node_view,
        )
        .map_err(err)?;
        {
            let rpc = WalletRpc::connect(&process.url(), process.secret()).map_err(err)?;
            let words = mnemonic_cli.split_whitespace().count() as u32;
            rpc.create_wallet(mnemonic_cli.as_str(), upstream.as_str(), words)
                .map_err(err)?;
        }
        drop(process);
        self.rpc.restart()?;

        let expected = address.clone();
        self.rpc.call(move |backend| {
            backend
                .open_with_timeout(upstream.as_str(), Duration::from_secs(30))
                .map_err(rpc::open_error)?;
            let actual = backend.address_at(0)?;
            if !leave_unlocked {
                backend.lock()?;
            }
            if actual != expected {
                return Err(flint_core::FlintError::Invalid(
                    "адрес кошелька не совпадает с конвертом".into(),
                ));
            }
            Ok(())
        })?;
        store.remove_verified_seed();
        Ok(address)
    }
}

fn seed_password_matches(dir: &std::path::Path, network: &str, password: &str) -> bool {
    let store = SeedStore::new(dir, network);
    if store.kind() != SeedKind::Envelope {
        return false;
    }
    match store.prepare_unlock(password) {
        Ok(prepared) => {
            store.rollback_unlock(&prepared);
            true
        }
        Err(_) => false,
    }
}

fn legacy_password_matches(dir: &std::path::Path, password: &str) -> bool {
    let path = dir.join("wallet_data").join(flint_core::seed::SEED_FILE);
    LegacySeed::read_file(&path, password).is_ok()
}

fn trash_target(
    root: &std::path::Path,
    network: &str,
    name: &str,
) -> Result<std::path::PathBuf, String> {
    let trash = root.join("trash").join(network);
    std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    Ok(trash.join(format!("{name}-{stamp}")))
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
