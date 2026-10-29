//! Onboarding/security handlers: create, restore, seed reveal, password change, delete.
use crate::{apply_snapshot, i18n, seed_status_text, snapshot, AppWindow};
use flint_runtime::WalletRuntime;
use slint::ComponentHandle;
use std::thread;
use zeroize::Zeroizing;

pub fn install(app: &AppWindow, wallet: WalletRuntime) {
    let wallet_source = wallet;
    let weak = app.as_weak();
    let wallet = wallet_source.clone();
    app.on_create_clicked(move |password, confirm| {
        let weak = weak.clone();
        let wallet = wallet.clone();
        let password = Zeroizing::new(password.to_string());
        let confirm = Zeroizing::new(confirm.to_string());
        if password.as_str() != confirm.as_str() {
            if let Some(app) = weak.upgrade() {
                app.set_create_error(i18n::tr("пароли не совпадают").into());
            }
            return;
        }
        if let Some(app) = weak.upgrade() {
            app.set_create_busy(true);
            app.set_create_error("".into());
        }
        let generation = wallet.context_generation();
        thread::spawn(move || {
            let result = wallet
                .create(password.as_str())
                .and_then(|created| snapshot(&wallet).map(|snapshot| (created, snapshot)));
            if result.is_ok() {
                wallet.remember_unlock(password.as_str());
                let _ = wallet.remember_recent_profile(&wallet.profile());
                let _ = wallet.unlock_history_key(password.as_str());
            }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = weak.upgrade() {
                    if app.get_wallet_generation() as u64 != generation {
                        return;
                    }
                    app.set_create_busy(false);
                    match result {
                        Ok((created, snapshot)) => {
                            app.set_initialized(true);
                            app.set_unlocked(true);
                            app.set_create_password("".into());
                            app.set_create_password2("".into());
                            app.set_create_address(created.address.into());
                            app.set_create_mnemonic(created.mnemonic.into());
                            apply_snapshot(&app, snapshot, None);
                        }
                        Err(e) => app.set_create_error(i18n::tr(&e).into()),
                    }
                }
            });
        });
    });

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_restore_clicked(move |mnemonic, password, confirm| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let mnemonic = Zeroizing::new(mnemonic.to_string());
            let password = Zeroizing::new(password.to_string());
            let confirm = Zeroizing::new(confirm.to_string());
            if password.as_str() != confirm.as_str() {
                if let Some(app) = weak.upgrade() {
                    app.set_restore_error(i18n::tr("пароли не совпадают").into());
                }
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.set_restore_busy(true);
                app.set_restore_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.restore(mnemonic.as_str(), password.as_str());
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_restore_busy(false);
                        match result {
                            Ok(_) => {
                                app.set_initialized(true);
                                app.set_restore_mnemonic("".into());
                                app.set_restore_password("".into());
                                app.set_restore_password2("".into());
                                app.set_show_restore(false);
                                app.set_unlock_error(
                                    "кошелёк восстановлен; разблокируйте паролем".into(),
                                );
                            }
                            Err(e) => app.set_restore_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_security_open_clicked(move || {
            if let Some(app) = weak.upgrade() {
                app.set_show_settings(false);
                app.set_show_security(true);
                app.set_sec_old("".into());
                app.set_sec_new("".into());
                app.set_sec_new2("".into());
                app.set_sec_delete("".into());
                app.set_sec_note("".into());
                app.set_sec_error("".into());
                match wallet.seed_status() {
                    Ok(status) => app.set_sec_seed_status(seed_status_text(&status).into()),
                    Err(e) => app.set_sec_seed_status(i18n::tr(&format!("seed: {e}")).into()),
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_seed_reveal_open_clicked(move || {
            if let Some(app) = weak.upgrade() {
                app.set_seed_reveal_password("".into());
                app.set_seed_revealed("".into());
                app.set_seed_reveal_error("".into());
                app.set_show_seed(true);
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_seed_reveal_clicked(move |password| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let password = password.to_string();
            if password.trim().is_empty() {
                app.set_seed_reveal_error(i18n::tr("введите пароль").into());
                return;
            }
            app.set_seed_reveal_busy(true);
            app.set_seed_reveal_error("".into());
            let weak = app.as_weak();
            let wallet = wallet.clone();
            thread::spawn(move || {
                let result = wallet.reveal_mnemonic(&password);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_seed_reveal_busy(false);
                        match result {
                            Ok(seed) => {
                                app.set_seed_revealed(seed.into());
                                app.set_seed_reveal_password("".into());
                            }
                            Err(error) => {
                                app.set_seed_reveal_error(i18n::tr(&error).into());
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_change_password_clicked(move |old, new, confirm| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let old = Zeroizing::new(old.to_string());
            let new = Zeroizing::new(new.to_string());
            let confirm = Zeroizing::new(confirm.to_string());
            if new.as_str() != confirm.as_str() {
                if let Some(app) = weak.upgrade() {
                    app.set_sec_error(i18n::tr("новые пароли не совпадают").into());
                }
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.set_sec_busy(true);
                app.set_sec_error("".into());
                app.set_sec_note("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.change_password_with_history(old.as_str(), new.as_str());
                wallet.clear_unlock_cache();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_sec_busy(false);
                        match result {
                            Ok(outcome) => {
                                app.set_sec_old("".into());
                                app.set_sec_new("".into());
                                app.set_sec_new2("".into());
                                if let Some(error) = outcome.history_error {
                                    app.set_sec_seed_password("".into());
                                    app.set_sec_seed_confirm("".into());
                                    app.set_sec_error(
                                        format!(
                                            "пароль изменён, но ключ истории не перевязан: {error}"
                                        )
                                        .into(),
                                    );
                                } else if outcome.history_rewrapped {
                                    app.set_sec_note(
                                        "пароль изменён, ключ истории перевязан".into(),
                                    );
                                } else {
                                    app.set_sec_note(i18n::tr("пароль изменён").into());
                                }
                            }
                            Err(e) => app.set_sec_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_delete_wallet_clicked(move |confirm| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let confirm = confirm.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_sec_busy(true);
                app.set_sec_error("".into());
                app.set_sec_note("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.delete_wallet(&confirm);
                let snapshot = result.as_ref().ok().and_then(|_| snapshot(&wallet).ok());
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_sec_busy(false);
                        match result {
                            Ok(_) => {
                                app.set_show_security(false);
                                if let Some(snapshot) = snapshot {
                                    apply_snapshot(&app, snapshot, None);
                                }
                                app.set_sec_note(i18n::tr("кошелёк перемещён в корзину").into());
                            }
                            Err(e) => app.set_sec_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }
}
