//! Settings handlers: open, data-root selection, UI language and save.
use crate::{
    apply_snapshot, apply_ui_language, i18n, pick_data_folder, refresh_tor_status,
    reset_wallet_context, snapshot, tor_bridge_status_text, wallet_context_busy, AppWindow,
    SharedPendingGate, Snapshot,
};
use flint_runtime::WalletRuntime;
use slint::ComponentHandle;
use std::thread;

pub fn install(app: &AppWindow, wallet: WalletRuntime, pending_gate: SharedPendingGate) {
    let wallet_source = wallet;
    let weak = app.as_weak();
    let wallet = wallet_source.clone();
    app.on_settings_open_clicked(move || {
        let view = wallet.settings_view();
        let data_root = wallet.data_root_view();
        let profile = wallet.profile();
        if let Some(app) = weak.upgrade() {
            app.set_show_settings(true);
            app.set_settings_error("".into());
            app.set_settings_warning("".into());
            app.set_settings_profile(profile.into());
            refresh_tor_status(&app, &wallet);
            match wallet.tor().bridges() {
                Ok(bridges) => {
                    app.set_tor_bridges_text(bridges.text.clone().into());
                    app.set_tor_bridge_status(tor_bridge_status_text(&bridges).into());
                }
                Err(e) => app.set_tor_bridge_status(e.into()),
            }
            match data_root {
                Ok(data_root) => {
                    let selected = data_root
                        .override_root
                        .clone()
                        .unwrap_or_else(|| data_root.active.clone());
                    app.set_settings_active_data_dir(data_root.active.display().to_string().into());
                    app.set_settings_data_dir(selected.display().to_string().into());
                    app.set_settings_default_data_dir(
                        data_root.default_root.display().to_string().into(),
                    );
                    app.set_settings_data_dir_forced(data_root.forced_by_environment);
                }
                Err(e) => app.set_settings_error(i18n::tr(&e).into()),
            }
            match view {
                Ok(view) => {
                    app.set_settings_network(view.network.into());
                    app.set_settings_node_url(view.node_url.into());
                    app.set_settings_relay(view.relay_url.into());
                    app.set_settings_nostr_relay(view.nostr_relay_url.into());
                    app.set_settings_min_conf(view.min_confirmations.to_string().into());
                    app.set_settings_note("".into());
                    app.set_settings_warning(view.settings_warning.into());
                }
                Err(e) => app.set_settings_error(i18n::tr(&e).into()),
            }
        }
    });

    {
        let weak = app.as_weak();
        app.on_settings_nostr_save_clicked({
            let weak = weak.clone();
            let wallet = wallet_source.clone();
            move |url| {
                let url = url.to_string();
                if let Some(app) = weak.upgrade() {
                    app.set_settings_error("".into());
                    app.set_settings_note("".into());
                }
                match wallet.update_nostr_relay(Some(url)) {
                    Ok(view) => {
                        if let Some(app) = weak.upgrade() {
                            app.set_settings_nostr_relay(view.nostr_relay_url.into());
                            app.set_settings_note("Nostr relay сохранён".into());
                        }
                    }
                    Err(error) => {
                        if let Some(app) = weak.upgrade() {
                            app.set_settings_error(i18n::tr(&error).into());
                        }
                    }
                }
            }
        });
        app.on_settings_data_dir_browse_clicked(move || {
            let initial = match weak.upgrade() {
                Some(app)
                    if !app.get_settings_data_dir_forced() && !app.get_settings_data_dir_busy() =>
                {
                    app.set_settings_data_dir_busy(true);
                    app.set_settings_error("".into());
                    app.set_settings_note("".into());
                    app.get_settings_data_dir().to_string()
                }
                _ => return,
            };
            let weak = weak.clone();
            thread::spawn(move || {
                let result = pick_data_folder(&initial);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_settings_data_dir_busy(false);
                        match result {
                            Ok(Some(path)) => app.set_settings_data_dir(path.into()),
                            Ok(None) => {}
                            Err(error) => app.set_settings_error(i18n::tr(&error).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_settings_data_dir_save_clicked(move |requested| {
            let requested = requested.to_string();
            if let Some(app) = weak.upgrade() {
                if app.get_settings_data_dir_forced() {
                    app.set_settings_error(
                        "каталог задан через --data или FLINT_DATA_DIR; измените параметр запуска"
                            .into(),
                    );
                    return;
                }
                if wallet_context_busy(&app) || app.get_settings_data_dir_busy() {
                    app.set_settings_error(
                        "дождитесь завершения текущей операции перед сменой каталога".into(),
                    );
                    return;
                }
                app.set_settings_data_dir_busy(true);
                app.set_settings_error("".into());
                app.set_settings_note("".into());
            }
            let weak = weak.clone();
            let wallet = wallet.clone();
            thread::spawn(move || {
                let result = wallet.set_data_root(&requested);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_settings_data_dir_busy(false);
                        match result {
                            Ok(data_root) => {
                                let selected = data_root
                                    .override_root
                                    .clone()
                                    .unwrap_or_else(|| data_root.active.clone());
                                app.set_settings_data_dir(selected.display().to_string().into());
                                app.set_settings_note(
                                    "Каталог сохранён. Закройте и снова запустите Flint; файлы не копировались."
                                        .into(),
                                );
                            }
                            Err(error) => app.set_settings_error(i18n::tr(&error).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_ui_language_changed(move |label| {
            let code = if label.as_str() == "English" {
                "en"
            } else {
                "ru"
            };
            if let Err(error) = wallet.set_ui_language(code) {
                if let Some(app) = weak.upgrade() {
                    app.set_ui_language_note(error.into());
                }
                return;
            }
            if let Some(app) = weak.upgrade() {
                apply_ui_language(&app, code);
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_language_save_requested(move |label| {
            let code = if label.as_str() == "English" {
                "en"
            } else {
                "ru"
            };
            if let Err(error) = wallet.set_ui_language(code) {
                if let Some(app) = weak.upgrade() {
                    app.set_ui_language_note(error.into());
                }
                return;
            }
            if let Some(app) = weak.upgrade() {
                apply_ui_language(&app, code);
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        let pending_gate = pending_gate.clone();
        app.on_settings_save_clicked(move |network, profile, node_url, relay, min_conf| {
            let weak = weak.clone();
            let wallet = wallet_source.clone();
            let pending_gate = pending_gate.clone();
            let network = network.to_string();
            let profile = profile.to_string();
            let node_url = node_url.to_string();
            let relay = relay.to_string();
            let min_conf = min_conf.to_string();
            let context_change = weak
                .upgrade()
                .map(|app| {
                    network != app.get_settings_network().as_str()
                        || profile != app.get_active_wallet().as_str()
                })
                .unwrap_or(false);
            if let Some(app) = weak.upgrade() {
                app.set_settings_error("".into());
                app.set_settings_note("".into());
                app.set_settings_warning("".into());
                if context_change {
                    if wallet_context_busy(&app) {
                        app.set_settings_error(
                            "дождитесь завершения текущей операции перед сменой кошелька".into(),
                        );
                        return;
                    }
                    app.set_wallet_switch_busy(true);
                }
            }
            let before = wallet.context_generation();
            thread::spawn(move || {
                let min_conf: u64 = if min_conf.trim().is_empty() {
                    10
                } else {
                    match min_conf.trim().parse() {
                        Ok(v) => v,
                        Err(_) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = weak.upgrade() {
                                    if context_change {
                                        app.set_wallet_switch_busy(false);
                                    }
                                    app.set_settings_error(
                                        "минимум подтверждений должен быть числом".into(),
                                    );
                                }
                            });
                            return;
                        }
                    }
                };
                let result = (|| -> Result<Snapshot, String> {
                    let current = wallet.settings_view()?;
                    let current_profile = wallet.profile();
                    if current.network != network || current_profile != profile {
                        let profile = if profile == "default" {
                            None
                        } else {
                            Some(profile.clone())
                        };
                        wallet.switch(Some(network.clone()), Some(profile))?;
                    }
                    wallet.update_settings(
                        None,
                        Some(node_url.trim().to_string()).filter(|v| !v.is_empty()),
                        None,
                        None,
                        None,
                        Some(relay.trim().to_string()).filter(|v| !v.is_empty()),
                        Some(min_conf),
                    )?;
                    snapshot(&wallet)
                })();
                let after = wallet.context_generation();
                let recovery = if result.is_err() && after != before {
                    snapshot(&wallet).ok()
                } else {
                    None
                };
                let running = wallet.node_running();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if context_change {
                            app.set_wallet_switch_busy(false);
                        }
                        if after != before {
                            reset_wallet_context(&app, &pending_gate);
                            app.set_wallet_generation(after as i32);
                        }
                        match result {
                            Ok(snapshot) => {
                                apply_snapshot(&app, snapshot, None);
                                app.set_node_running(running);
                                app.set_settings_note(i18n::tr("Настройки сохранены").into());
                            }
                            Err(e) => {
                                if let Some(snapshot) = recovery {
                                    apply_snapshot(&app, snapshot, None);
                                }
                                app.set_settings_error(i18n::tr(&e).into());
                            }
                        }
                    }
                });
            });
        });
    }
}
