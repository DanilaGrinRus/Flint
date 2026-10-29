//! Node lifecycle and Tor listener handlers. This module owns only event
//! wiring; wallet/runtime checks stay in `WalletRuntime`, and the same
//! generation/busy guards as the previous `main.rs` block are preserved.

use std::thread;

use flint_runtime::WalletRuntime;
use slint::ComponentHandle;

use crate::{apply_tor_status, i18n, node_line, tor_bridge_status_text, AppWindow};

pub fn install(app: &AppWindow, wallet: WalletRuntime) {
    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_node_toggle_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let running = wallet.node_running();
            if let Some(app) = weak.upgrade() {
                app.set_node_busy(true);
                app.set_node_note(if running {
                    "останавливаю ноду…".into()
                } else {
                    "запускаю ноду…".into()
                });
            }
            thread::spawn(move || {
                let result = if running {
                    wallet.node_stop()
                } else {
                    wallet.node_start()
                };
                let info = wallet.node_info().unwrap_or_default();
                let running = wallet.node_running();
                let node_text = node_line(&info);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_node_busy(false);
                        app.set_node_running(running);
                        app.set_node_text(node_text.into());
                        match result {
                            Ok(()) => app.set_node_note(
                                if running {
                                    "процесс ноды запущен"
                                } else {
                                    "нода остановлена"
                                }
                                .into(),
                            ),
                            Err(e) => app.set_node_note(e.into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_tor_start_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            if let Some(app) = weak.upgrade() {
                app.set_tor_busy(true);
                app.set_tor_note(i18n::tr("запускаю Tor…").into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.tor().start();
                let status = wallet.tor().status();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_tor_busy(false);
                        match result {
                            Ok(start) => {
                                app.set_tor_running(true);
                                app.set_tor_onion(start.onion.into());
                                app.set_tor_note(
                                    format!("Tor включён; relay: {}", start.relay_url).into(),
                                );
                            }
                            Err(e) => app.set_tor_note(i18n::tr(&format!("Tor: {e}")).into()),
                        }
                        if let Ok(status) = status {
                            apply_tor_status(&app, &status);
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_tor_stop_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            if let Some(app) = weak.upgrade() {
                app.set_tor_busy(true);
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                wallet.tor().stop();
                let status = wallet.tor().status();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_tor_busy(false);
                        app.set_tor_running(false);
                        app.set_tor_onion("".into());
                        match status {
                            Ok(status) => apply_tor_status(&app, &status),
                            Err(e) => app.set_tor_note(e.into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_tor_refresh_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            if let Some(app) = weak.upgrade() {
                app.set_tor_busy(true);
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let status = wallet.tor().status();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_tor_busy(false);
                        match status {
                            Ok(status) => apply_tor_status(&app, &status),
                            Err(e) => app.set_tor_note(e.into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_tor_bridges_save_clicked(move |text| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let text = text.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_tor_busy(true);
                app.set_tor_bridge_status(i18n::tr("сохраняю мосты…").into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.tor().save_bridges(&text);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_tor_busy(false);
                        match result {
                            Ok(bridges) => {
                                app.set_tor_bridges_text(bridges.text.clone().into());
                                app.set_tor_bridge_status(tor_bridge_status_text(&bridges).into());
                            }
                            Err(e) => app.set_tor_bridge_status(e.into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_tor_log_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            if let Some(app) = weak.upgrade() {
                app.set_tor_busy(true);
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.tor().log();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_tor_busy(false);
                        match result {
                            Ok((lines, present, path)) => {
                                let text = if present {
                                    lines.join("\n")
                                } else {
                                    format!("лог ещё не создан: {path}")
                                };
                                app.set_tor_log_text(text.into());
                            }
                            Err(e) => app.set_tor_note(e.into()),
                        }
                    }
                });
            });
        });
    }
}
