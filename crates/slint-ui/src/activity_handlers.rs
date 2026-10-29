//! Activity handlers: swaps, copy, pending/history actions and seed maintenance.
use crate::{
    activity_rows, apply_pending_result, arm_pending_watchdog, copy_text, i18n, load_pending,
    make_qr, pending_allowed, presentation, seed_status_text, AppWindow, PendingAction,
    SharedPendingGate,
};
use flint_runtime::{SwapRuntime, WalletRuntime};
use slint::ComponentHandle;
use std::thread;
use zeroize::Zeroizing;

pub fn install(
    app: &AppWindow,
    wallet: WalletRuntime,
    swaps: SwapRuntime,
    pending_gate: SharedPendingGate,
) {
    let wallet_source = wallet;
    let swaps_source = swaps;
    let weak = app.as_weak();
    let swaps = swaps_source.clone();
    app.on_swaps_open_clicked(move || {
        let weak = weak.clone();
        let swaps = swaps.clone();
        if let Some(app) = weak.upgrade() {
            app.set_show_swaps(true);
            app.set_swap_note(i18n::tr("загружаю борд…").into());
        }
        thread::spawn(move || {
            let result = swaps.board("XMR/GRIN");
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = weak.upgrade() {
                    match result {
                        Ok(board) => {
                            app.set_swap_offers(slint::ModelRc::new(slint::VecModel::from(
                                presentation::swap_rows(&board),
                            )));
                            app.set_swap_note(
                                format!(
                                    "{} · {} офферов · stamps {}",
                                    board.board,
                                    board.offers.len(),
                                    board.stamps_balance
                                )
                                .into(),
                            );
                        }
                        Err(e) => {
                            app.set_swap_note(i18n::tr(&format!("борд: {}", i18n::tr(&e))).into())
                        }
                    }
                }
            });
        });
    });

    {
        let weak = app.as_weak();
        let swaps = swaps_source.clone();
        app.on_swap_take_clicked(move |id| {
            let weak = weak.clone();
            let swaps = swaps.clone();
            let id = id.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_swap_note(i18n::tr("беру оффер…").into());
                app.set_swap_busy(true);
            }
            thread::spawn(move || {
                let result = swaps.take(&id, None);
                let board = swaps.board("XMR/GRIN");
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_swap_busy(false);
                        match result {
                            Ok(swap) => {
                                app.set_last_swap_id(swap.id.clone().into());
                                app.set_swap_note(
                                    format!("взят {} · {}", swap.id, swap.state.label()).into(),
                                );
                                if let Ok(board) = board {
                                    app.set_swap_offers(slint::ModelRc::new(
                                        slint::VecModel::from(presentation::swap_rows(&board)),
                                    ));
                                }
                            }
                            Err(e) => app.set_swap_note(
                                i18n::tr(&format!("оффер: {}", i18n::tr(&e))).into(),
                            ),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let swaps = swaps_source.clone();
        let wallet = wallet_source.clone();
        app.on_swap_simulate_clicked(move |id| {
            let weak = weak.clone();
            let swaps = swaps.clone();
            let id = id.to_string();
            let tip = wallet.node_info().map(|node| node.height).unwrap_or(1_000);
            if let Some(app) = weak.upgrade() {
                app.set_swap_busy(true);
                app.set_swap_note(i18n::tr("симулирую своп…").into());
            }
            thread::spawn(move || {
                let result = swaps.simulate(&id, tip);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_swap_busy(false);
                        match result {
                            Ok(swap) => {
                                app.set_swap_note(
                                    format!("симуляция: {} · {}", swap.state.label(), swap.id)
                                        .into(),
                                );
                            }
                            Err(e) => app.set_swap_note(
                                i18n::tr(&format!("симуляция: {}", i18n::tr(&e))).into(),
                            ),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_copy_address_clicked(move || {
            let result = wallet.address().and_then(|address| copy_text(&address));
            if let Some(app) = weak.upgrade() {
                match result {
                    Ok(()) => app.set_clipboard_note(i18n::tr("адрес скопирован").into()),
                    Err(e) => app.set_clipboard_note(e.into()),
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_copy_send_clicked(move |text| {
            let result = copy_text(text.as_str());
            if let Some(app) = weak.upgrade() {
                match result {
                    Ok(()) => app.set_send_note(i18n::tr("slatepack скопирован").into()),
                    Err(e) => app.set_send_error(i18n::tr(&e).into()),
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_copy_receive_clicked(move |text| {
            let result = copy_text(text.as_str());
            if let Some(app) = weak.upgrade() {
                match result {
                    Ok(()) => app.set_receive_note(i18n::tr("ответ скопирован").into()),
                    Err(e) => app.set_receive_error(i18n::tr(&e).into()),
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        let gate = pending_gate.clone();
        app.on_pending_reload_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let gate = gate.clone();
            let generation = wallet.context_generation();
            if let Some(app) = weak.upgrade() {
                app.set_pending_loading(true);
                app.set_pending_error("".into());
            }
            if let Ok(mut gate) = gate.lock() {
                gate.loaded = false;
                gate.readable.clear();
                gate.retryable.clear();
                gate.cancelable.clear();
            }
            arm_pending_watchdog(weak.clone(), gate.clone(), generation);
            thread::spawn(move || {
                let pending = load_pending(&wallet);
                let gate_pending = gate.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        apply_pending_result(&app, pending, &gate_pending, generation);
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        let gate = pending_gate.clone();
        app.on_history_open_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let gate = gate.clone();
            let generation = wallet.context_generation();
            if let Some(app) = weak.upgrade() {
                app.set_show_history(true);
                app.set_history_note(i18n::tr("загружаю историю…").into());
                app.set_pending_loading(true);
                app.set_pending_error("".into());
            }
            if let Ok(mut gate) = gate.lock() {
                gate.loaded = false;
                gate.readable.clear();
                gate.retryable.clear();
                gate.cancelable.clear();
            }
            arm_pending_watchdog(weak.clone(), gate.clone(), generation);
            thread::spawn(move || {
                // Pending is a fail-closed view of its own: show it even if the
                // activity/RPC call is slow or fails.
                let pending = load_pending(&wallet);
                let weak_pending = weak.clone();
                let gate_pending = gate.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak_pending.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        apply_pending_result(&app, pending, &gate_pending, generation);
                    }
                });
                let rows = activity_rows(&wallet, 200);
                let count = rows.len();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_history(slint::ModelRc::new(slint::VecModel::from(rows)));
                        app.set_history_note(i18n::tr(&format!("операций: {count}")).into());
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        let gate = pending_gate.clone();
        app.on_retry_pending_clicked(move |id| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let gate = gate.clone();
            let id = id.to_string();
            let generation = wallet.context_generation();
            if !pending_allowed(&gate, generation, &id, PendingAction::Retry) {
                if let Some(app) = weak.upgrade() {
                    app.set_history_note(
                        "повтор запрещён: операция не в состоянии finalizing/finalized_not_posted"
                            .into(),
                    );
                }
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.set_pending_loading(true);
                app.set_pending_error("".into());
            }
            if let Ok(mut gate) = gate.lock() {
                gate.loaded = false;
                gate.readable.clear();
                gate.retryable.clear();
                gate.cancelable.clear();
            }
            arm_pending_watchdog(weak.clone(), gate.clone(), generation);
            thread::spawn(move || {
                let result = wallet.retry_post(&id);
                let pending = load_pending(&wallet);
                let history = activity_rows(&wallet, 200);
                let gate_pending = gate.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        apply_pending_result(&app, pending, &gate_pending, generation);
                        app.set_history(slint::ModelRc::new(slint::VecModel::from(history)));
                        match result {
                            Ok(tx) => app.set_history_note(
                                i18n::tr(&format!("повтор отправлен: {tx}")).into(),
                            ),
                            Err(e) => app.set_history_note(
                                i18n::tr(&format!("повтор: {}", i18n::tr(&e))).into(),
                            ),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        let gate = pending_gate.clone();
        app.on_cancel_pending_clicked(move |id| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let gate = gate.clone();
            let id = id.to_string();
            let generation = wallet.context_generation();
            if !pending_allowed(&gate, generation, &id, PendingAction::Cancel) {
                if let Some(app) = weak.upgrade() {
                    app.set_history_note(
                        "отмена запрещена: операция уже подписана или не находится в locked".into(),
                    );
                }
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.set_pending_loading(true);
                app.set_pending_error("".into());
            }
            if let Ok(mut gate) = gate.lock() {
                gate.loaded = false;
                gate.readable.clear();
                gate.retryable.clear();
                gate.cancelable.clear();
            }
            arm_pending_watchdog(weak.clone(), gate.clone(), generation);
            thread::spawn(move || {
                let result = wallet.cancel_pending(&id);
                let pending = load_pending(&wallet);
                let history = activity_rows(&wallet, 200);
                let gate_pending = gate.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        apply_pending_result(&app, pending, &gate_pending, generation);
                        app.set_history(slint::ModelRc::new(slint::VecModel::from(history)));
                        match result {
                            Ok(()) => app.set_history_note(
                                i18n::tr("операция отменена или разблокирована").into(),
                            ),
                            Err(e) => app.set_history_note(
                                i18n::tr(&format!("отмена: {}", i18n::tr(&e))).into(),
                            ),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_export_pending_request_clicked(move |id| {
            let result = wallet.export_pending_request(id.as_str()).and_then(|text| {
                copy_text(&text)?;
                Ok(())
            });
            if let Some(app) = weak.upgrade() {
                app.set_history_note(match result {
                    Ok(()) => i18n::tr("запрос экспортирован в буфер обмена").into(),
                    Err(error) => i18n::tr(&format!("экспорт запроса: {error}")).into(),
                });
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_export_pending_clicked(move |id| {
            let result = wallet
                .export_pending_response(id.as_str())
                .and_then(|text| {
                    copy_text(&text)?;
                    Ok(())
                });
            if let Some(app) = weak.upgrade() {
                app.set_history_note(match result {
                    Ok(()) => i18n::tr("ответ экспортирован в буфер обмена").into(),
                    Err(error) => i18n::tr(&format!("экспорт ответа: {error}")).into(),
                });
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_rotate_address_clicked(move || match wallet.rotate_address() {
            Ok((index, address)) => {
                if let Some(app) = weak.upgrade() {
                    app.set_address_text(address.clone().into());
                    if let Some(qr) = make_qr(&address) {
                        app.set_address_qr(qr);
                    }
                    app.set_clipboard_note(i18n::tr(&format!("новый адрес #{index}")).into());
                }
            }
            Err(e) => {
                if let Some(app) = weak.upgrade() {
                    app.set_clipboard_note(e.into());
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_seed_disable_clicked(move |password| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let password = Zeroizing::new(password.to_string());
            if let Some(app) = weak.upgrade() {
                app.set_sec_busy(true);
                app.set_sec_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.disable_envelope(password.as_str());
                let status = wallet.seed_status().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_sec_busy(false);
                        if let Some(status) = status {
                            app.set_sec_seed_status(seed_status_text(&status).into());
                        }
                        match result {
                            Ok(()) => app.set_sec_note(
                                i18n::tr("Argon2id отключён; wallet.seed восстановлен").into(),
                            ),
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
        app.on_seed_repair_clicked(move |password| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let password = Zeroizing::new(password.to_string());
            if let Some(app) = weak.upgrade() {
                app.set_sec_busy(true);
                app.set_sec_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.repair_envelope(password.as_str(), false);
                let status = wallet.seed_status().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_sec_busy(false);
                        if let Some(status) = status {
                            app.set_sec_seed_status(seed_status_text(&status).into());
                        }
                        match result {
                            Ok(path) => app
                                .set_sec_note(i18n::tr(&format!("конверт отложен: {path}")).into()),
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
        app.on_seed_migrate_clicked(move |password, confirm, remove_backups| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let password = Zeroizing::new(password.to_string());
            let confirm = Zeroizing::new(confirm.to_string());
            if let Some(app) = weak.upgrade() {
                app.set_sec_busy(true);
                app.set_sec_error("".into());
                app.set_sec_note("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result =
                    wallet.migrate_envelope(password.as_str(), confirm.as_str(), remove_backups);
                let status = wallet.seed_status().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_sec_busy(false);
                        if let Some(status) = status {
                            app.set_sec_seed_status(seed_status_text(&status).into());
                        }
                        match result {
                            Ok(outcome) => {
                                app.set_sec_seed_password("".into());
                                app.set_sec_seed_confirm("".into());
                                app.set_sec_note(
                                    i18n::tr(&format!("seed защищён: {outcome}")).into(),
                                );
                            }
                            Err(e) => app.set_sec_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }
}
