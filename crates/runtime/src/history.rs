//! Encrypted messenger-history key lifecycle.
//!
//! The key is separate from the seed: it must survive a password change, a
//! backend switch and a crash between the two writes. The Slint shell and the
//! runtime messenger use the same per-profile key file.

use std::path::{Path, PathBuf};

use flint_core::WalletService;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::WalletRuntime;

pub struct PendingHistoryRewrap {
    pub(crate) primary: PathBuf,
    pub(crate) next: PathBuf,
}

impl WalletRuntime {
    fn history_backend(&self) -> &'static str {
        if self.is_rpc() {
            "grin-wallet"
        } else {
            "embedded"
        }
    }

    fn history_profile_dir(&self, wallet: &WalletService, kind: &str) -> PathBuf {
        let profile = wallet
            .settings()
            .wallet_name
            .clone()
            .unwrap_or_else(|| "default".to_string());
        wallet
            .paths()
            .root()
            .join("messenger")
            .join(kind)
            .join(&wallet.settings().network)
            .join(self.history_backend())
            .join(wallet.wallet_identity())
            .join(profile)
    }

    fn history_legacy_dir(&self, wallet: &WalletService, kind: &str) -> PathBuf {
        let profile = wallet
            .settings()
            .wallet_name
            .clone()
            .unwrap_or_else(|| "default".to_string());
        wallet
            .paths()
            .root()
            .join("messenger")
            .join(kind)
            .join(&wallet.settings().network)
            .join(profile)
    }

    fn history_key_path(&self, wallet: &WalletService) -> PathBuf {
        self.history_profile_dir(wallet, "history")
            .join(".history.key")
    }

    fn history_legacy_key_path(&self, wallet: &WalletService) -> PathBuf {
        wallet.paths().root().join("messenger").join("history.key")
    }

    fn history_has_data(&self, wallet: &WalletService) -> bool {
        let has_history = std::fs::read_dir(self.history_profile_dir(wallet, "history"))
            .map(|entries| entries.flatten().any(|entry| entry.path().is_dir()))
            .unwrap_or(false);
        has_history
            || self
                .history_profile_dir(wallet, "contacts")
                .join("contacts.enc")
                .is_file()
    }

    fn history_key_locations(&self) -> Result<(PathBuf, PathBuf, bool), String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok((
            self.history_key_path(&wallet),
            self.history_legacy_key_path(&wallet),
            self.history_has_data(&wallet),
        ))
    }

    /// Unwrap (or create) the per-profile history key after a wallet unlock.
    /// A failure is recorded, not returned through the wallet-open result: a
    /// broken history key must not make the wallet itself unusable.
    pub fn unlock_history_key(&self, password: &str) -> Result<(), String> {
        self.migrate_legacy_history_scope(password)?;
        let (path, legacy, has_history) = self.history_key_locations()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        match resolve_history_key(&path, &legacy, has_history, password) {
            Ok(key) => {
                *self
                    .history_key
                    .lock()
                    .map_err(|_| "history key busy".to_string())? = Some(key);
                if let Ok(mut guard) = self.history_error.lock() {
                    *guard = None;
                }
                Ok(())
            }
            Err(error) => {
                if let Ok(mut guard) = self.history_error.lock() {
                    *guard = Some(error.clone());
                }
                Err(error)
            }
        }
    }

    pub fn lock_history_key(&self) {
        if let Ok(mut guard) = self.history_key.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.history_error.lock() {
            *guard = None;
        }
    }

    pub fn history_key_ready(&self) -> bool {
        self.history_key
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub fn history_error(&self) -> Option<String> {
        self.history_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub fn cached_history_key(&self) -> Result<[u8; 32], String> {
        let guard = self
            .history_key
            .lock()
            .map_err(|_| "history key busy".to_string())?;
        guard
            .as_ref()
            .map(|key| **key)
            .ok_or_else(|| "кошелёк заблокирован: история недоступна".to_string())
    }

    pub fn legacy_history_view(&self) -> Result<Value, String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        Ok(legacy_history_view(&wallet, self))
    }

    /// Prepare the new wrapper under the new password **before** the seed
    /// password changes. The primary file is untouched; `.next` is promoted by
    /// `unlock_history_key` after a crash.
    pub fn prepare_history_rewrap(
        &self,
        old: &str,
        new: &str,
    ) -> Result<Option<PendingHistoryRewrap>, String> {
        let (path, legacy, has_history) = self.history_key_locations()?;
        let next = path.with_file_name(".history.key.next");
        let wrapped = if path.exists() {
            let bytes = read_history_key(&path)?;
            flint_messenger::rewrap_history_key(old, new, &bytes).map_err(err)?
        } else if legacy.exists() {
            let bytes = read_history_key(&legacy)?;
            match flint_messenger::unwrap_history_key(old, &bytes) {
                Ok(key) => flint_messenger::keywrap::wrap(new, &key).map_err(err)?,
                Err(error) if has_history => {
                    return Err(format!(
                        "история профиля использует общий ключ, который не открывается старым паролем ({error}); смена пароля отменена"
                    ));
                }
                Err(_) => match self.cached_history_key() {
                    Ok(key) => flint_messenger::keywrap::wrap(new, &key).map_err(err)?,
                    Err(_) => return Ok(None),
                },
            }
        } else if let Ok(key) = self.cached_history_key() {
            flint_messenger::keywrap::wrap(new, &key).map_err(err)?
        } else if has_history {
            return Err(
                "история профиля есть, но ключ истории не найден; смена пароля отменена".into(),
            );
        } else {
            return Ok(None);
        };
        save_history_key(&next, &wrapped)?;
        Ok(Some(PendingHistoryRewrap {
            primary: path,
            next,
        }))
    }

    pub fn commit_history_rewrap(&self, pending: &PendingHistoryRewrap) -> Result<(), String> {
        let bytes = read_history_key(&pending.next)?;
        save_history_key(&pending.primary, &bytes)?;
        let _ = std::fs::remove_file(&pending.next);
        Ok(())
    }

    /// Explicit recovery when the old profile-scoped `.history.key` was made
    /// with a different password. The current directory is backed up, not
    /// overwritten in place.
    pub fn recover_legacy_history(&self, password: &str) -> Result<Value, String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        let old = self.history_legacy_dir(&wallet, "history");
        let new = self.history_profile_dir(&wallet, "history");
        let old_key = old.join(".history.key");
        if !old.is_dir() || !old_key.is_file() {
            return Err("старый каталог истории не найден".into());
        }
        if self.history_has_data(&wallet) {
            return Err(
                "в новом каталоге уже есть сообщения/контакты: восстановление не перезаписывает их"
                    .into(),
            );
        }
        let bytes = read_history_key(&old_key)?;
        let key = flint_messenger::unwrap_history_key(password, &bytes)
            .map_err(|e| format!("старый пароль истории не подходит: {e}"))?;
        let root = wallet.paths().root().to_path_buf();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        if new.exists() {
            let backup = root
                .join("trash")
                .join("history")
                .join(format!("current-{stamp}"));
            std::fs::create_dir_all(backup.parent().unwrap_or(&root)).map_err(err)?;
            std::fs::rename(&new, &backup).map_err(err)?;
        }
        copy_dir_all(&old, &new)?;
        let contacts_old = self.history_legacy_dir(&wallet, "contacts");
        let contacts_new = self.history_profile_dir(&wallet, "contacts");
        if contacts_old.is_dir() && !contacts_new.exists() {
            copy_dir_all(&contacts_old, &contacts_new)?;
        }
        *self
            .history_key
            .lock()
            .map_err(|_| "history key busy".to_string())? = Some(key);
        if let Ok(mut guard) = self.history_error.lock() {
            *guard = None;
        }
        Ok(json!({
            "restored": true,
            "path": new.display().to_string(),
        }))
    }

    fn migrate_legacy_history_scope(&self, password: &str) -> Result<(), String> {
        let wallet = self
            .wallet
            .lock()
            .map_err(|_| "кошелёк занят".to_string())?;
        let history_new = self.history_profile_dir(&wallet, "history");
        let history_old = self.history_legacy_dir(&wallet, "history");
        if !history_old.is_dir() || history_new.exists() {
            return Ok(());
        }
        let old_key = history_old.join(".history.key");
        if !old_key.is_file() {
            return Ok(());
        }
        let bytes = read_history_key(&old_key)?;
        if flint_messenger::unwrap_history_key(password, &bytes).is_err() {
            return Ok(());
        }
        copy_dir_all(&history_old, &history_new)?;
        let contacts_old = self.history_legacy_dir(&wallet, "contacts");
        let contacts_new = self.history_profile_dir(&wallet, "contacts");
        if contacts_old.is_dir() && !contacts_new.exists() {
            copy_dir_all(&contacts_old, &contacts_new)?;
        }
        Ok(())
    }
}

fn legacy_history_view(wallet: &WalletService, runtime: &WalletRuntime) -> Value {
    let old = runtime.history_legacy_dir(wallet, "history");
    let new = runtime.history_profile_dir(wallet, "history");
    let old_key = old.join(".history.key");
    json!({
        "present": old.is_dir() && old_key.is_file(),
        "old_path": old.display().to_string(),
        "new_path": new.display().to_string(),
        "new_has_data": runtime.history_has_data(wallet),
    })
}

fn resolve_history_key(
    path: &Path,
    legacy: &Path,
    has_history: bool,
    password: &str,
) -> Result<Zeroizing<[u8; 32]>, String> {
    let next = path.with_file_name(".history.key.next");
    if !path.exists() {
        if next.exists() {
            let bytes = read_history_key(&next)?;
            if let Ok(key) = flint_messenger::unwrap_history_key(password, &bytes) {
                save_history_key(path, &bytes)?;
                let _ = std::fs::remove_file(&next);
                return Ok(key);
            }
        }
        if legacy.exists() {
            let legacy_bytes = read_history_key(legacy)?;
            match flint_messenger::unwrap_history_key(password, &legacy_bytes) {
                Ok(key) => {
                    let wrapped = flint_messenger::keywrap::wrap(password, &key).map_err(err)?;
                    save_history_key(path, &wrapped)?;
                    return Ok(key);
                }
                Err(e) if has_history => {
                    return Err(format!(
                        "история профиля зашифрована общим ключом, который не открывается этим паролем ({e}); новый ключ не создан"
                    ));
                }
                Err(_) => {}
            }
        }
        if has_history {
            return Err(
                "история профиля есть, но ключ истории не найден: восстановите ключ или очистите историю".into(),
            );
        }
        let (key, wrapped) = flint_messenger::create_history_key(password).map_err(err)?;
        save_history_key(path, &wrapped)?;
        return Ok(key);
    }

    let bytes = read_history_key(path)?;
    match flint_messenger::unwrap_history_key(password, &bytes) {
        Ok(key) => {
            let _ = std::fs::remove_file(&next);
            if flint_messenger::keywrap::needs_upgrade(&bytes).map_err(err)? {
                upgrade_history_key(path, password, &key, &bytes);
            }
            Ok(key)
        }
        Err(primary_error) => {
            if next.exists() {
                let next_bytes = read_history_key(&next)?;
                if let Ok(key) = flint_messenger::unwrap_history_key(password, &next_bytes) {
                    save_history_key(path, &next_bytes)?;
                    let _ = std::fs::remove_file(&next);
                    return Ok(key);
                }
            }
            Err(format!(
                "ключ истории этого профиля не открывается паролем ({primary_error}); файл не изменён"
            ))
        }
    }
}

fn read_history_key(path: &Path) -> Result<Vec<u8>, String> {
    // Migrate legacy keys before use; an ACL failure must not be silently ignored.
    flint_core::win_acl::restrict(path).map_err(err)?;
    let meta = std::fs::metadata(path).map_err(err)?;
    if meta.len() > flint_messenger::keywrap::MAX_WRAPPED_BYTES as u64 {
        return Err("ключ истории: файл подозрительно большой".into());
    }
    std::fs::read(path).map_err(err)
}

fn save_history_key(path: &Path, bytes: &[u8]) -> Result<(), String> {
    flint_core::win_acl::write_private_atomic(path, bytes).map_err(err)?;
    if std::fs::read(path).map_err(err)? != bytes {
        return Err("ключ истории: запись не прошла проверку чтением".into());
    }
    Ok(())
}

fn upgrade_history_key(path: &Path, password: &str, key: &[u8; 32], old: &[u8]) {
    let result = (|| -> Result<(), String> {
        let updated = flint_messenger::keywrap::wrap(password, key).map_err(err)?;
        let check = flint_messenger::unwrap_history_key(password, &updated).map_err(err)?;
        if &check[..] != key {
            return Err("ключ истории: проверка после переупаковки не совпала".into());
        }
        save_history_key(path, &updated)
    })();
    if result.is_err() {
        let _ = save_history_key(path, old);
    }
}

fn copy_dir_all(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(err)?;
    for entry in std::fs::read_dir(from).map_err(err)?.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir_all(&source, &target)?;
        } else if !target.exists() {
            std::fs::copy(&source, &target).map_err(err)?;
        }
    }
    Ok(())
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flint-runtime-history-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_key(path: &Path, password: &str) -> Zeroizing<[u8; 32]> {
        let (key, wrapped) = flint_messenger::create_history_key(password).unwrap();
        save_history_key(path, &wrapped).unwrap();
        key
    }

    #[test]
    fn each_profile_gets_its_own_key() {
        let dir = temp_dir("profiles");
        let legacy = dir.join("legacy.key");
        let first = dir.join("a").join(".history.key");
        let second = dir.join("b").join(".history.key");
        let key_a = resolve_history_key(&first, &legacy, false, "password A").unwrap();
        let key_b = resolve_history_key(&second, &legacy, false, "password B").unwrap();
        assert_ne!(&key_a[..], &key_b[..]);
        assert!(first.is_file());
        assert!(second.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_shared_key_is_migrated_once() {
        let dir = temp_dir("legacy");
        let legacy = dir.join("history.key");
        let key = write_key(&legacy, "shared password");
        let profile = dir.join("profile").join(".history.key");
        let opened = resolve_history_key(&profile, &legacy, true, "shared password").unwrap();
        assert_eq!(&opened[..], &key[..]);
        assert!(profile.is_file());
        assert!(legacy.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pending_next_wrapper_is_promoted() {
        let dir = temp_dir("next");
        let path = dir.join("profile").join(".history.key");
        let key = write_key(&path, "old password");
        let next = path.with_file_name(".history.key.next");
        let wrapped = flint_messenger::keywrap::wrap("new password", &key).unwrap();
        save_history_key(&next, &wrapped).unwrap();
        let opened = resolve_history_key(&path, &dir.join("legacy.key"), false, "new password")
            .expect("next wrapper must open");
        assert_eq!(&opened[..], &key[..]);
        assert!(!next.exists());
        assert!(flint_messenger::unwrap_history_key(
            "old password",
            &std::fs::read(&path).unwrap()
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn history_without_a_key_is_refused() {
        let dir = temp_dir("missing");
        let err = resolve_history_key(
            &dir.join("profile").join(".history.key"),
            &dir.join("legacy.key"),
            true,
            "password",
        )
        .unwrap_err();
        assert!(err.contains("ключ"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
