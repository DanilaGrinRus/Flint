//! Messenger handlers: open/send/inbox, contacts, history, block and recovery.
use crate::format::{amount as format_amount, short_address};
use crate::{
    auto_slatepack_targets, copy_text, i18n, presentation, AppWindow, MessageRow, SharedPendingGate,
};
use flint_runtime::WalletRuntime;
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use zeroize::Zeroizing;

pub fn install(
    app: &AppWindow,
    wallet: WalletRuntime,
    auto_process_flag: Arc<AtomicBool>,
    pending_gate: SharedPendingGate,
) {
    let wallet_source = wallet;
    let flag_source = auto_process_flag;
    let weak = app.as_weak();
    let wallet = wallet_source.clone();
    let auto_process_flag = flag_source.clone();
    app.on_messenger_open_clicked(move || {
        let weak = weak.clone();
        let wallet = wallet.clone();
        let auto_process_flag = auto_process_flag.clone();
        let generation = wallet.context_generation();
        thread::spawn(move || {
            let status = wallet.messenger_status();
            let contacts = wallet.messenger_contacts();
            let blocked = wallet.messenger_blocked();
            let nostr_identity = wallet.messenger_nostr_identity();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = weak.upgrade() {
                    if app.get_wallet_generation() as u64 != generation {
                        return;
                    }
                    app.set_show_messages(true);
                    app.set_show_add_contact(false);
                    app.set_show_recover_history(false);
                    if !app.get_auto_process_slatepacks()
                        && !auto_process_flag.load(Ordering::SeqCst)
                    {
                        app.set_messenger_error("".into());
                        app.set_messenger_note("".into());
                    }
                    app.set_messenger_inbox("".into());
                    app.set_messenger_history("".into());
                    app.set_messenger_selected_address("".into());
                    app.set_messenger_selected_name("".into());
                    app.set_messenger_messages(slint::ModelRc::new(slint::VecModel::from(Vec::<
                        MessageRow,
                    >::new(
                    ))));
                    if let Ok(contacts) = contacts {
                        app.set_messenger_contacts(slint::ModelRc::new(slint::VecModel::from(
                            presentation::contact_rows(&contacts),
                        )));
                    }
                    if let Ok(identity) = &nostr_identity {
                        if let Some(key) = identity["public_key"].as_str() {
                            app.set_messenger_nostr_key(key.into());
                        }
                    }
                    if let Ok(blocked) = blocked {
                        app.set_messenger_blocked(presentation::blocked_text(&blocked).into());
                        app.set_messenger_blocked_addresses(slint::ModelRc::new(
                            slint::VecModel::from(presentation::blocked_rows(&blocked)),
                        ));
                    }
                    match status {
                        Ok(status) => {
                            let messenger_address = status["address"].as_str().unwrap_or("—");
                            app.set_messenger_address(messenger_address.into());
                            app.set_messenger_address_short(
                                short_address(messenger_address).into(),
                            );
                            let transport = status["transport"].as_str().unwrap_or("relay");
                            let transport_label = match transport {
                                "nostr" => "Nostr",
                                "direct" => "прямой onion",
                                _ => "relay",
                            };
                            let key = status["history_key"].as_bool().unwrap_or(false);
                            let mut line = format!(
                                "{} · {} · {}",
                                status["network"].as_str().unwrap_or("?"),
                                transport_label,
                                if key {
                                    "ключ истории открыт"
                                } else {
                                    "ключ истории закрыт"
                                }
                            );
                            if let Some(onion) = status["onion"].as_str() {
                                line.push_str(&format!(" · {onion}"));
                            }
                            if let Some(e) = status["history_error"].as_str() {
                                line.push_str(&format!(" · история: {e}"));
                            }
                            app.set_messenger_status(line.into());
                        }
                        Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                    }
                }
            });
        });
    });

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_copy_nostr_clicked(move || {
            let result = wallet.messenger_nostr_identity().and_then(|value| {
                let key = value["public_key"]
                    .as_str()
                    .ok_or_else(|| "Nostr-ключ недоступен".to_string())?;
                copy_text(key)
            });
            if let Some(app) = weak.upgrade() {
                match result {
                    Ok(()) => app.set_clipboard_note(i18n::tr("ключ скопирован").into()),
                    Err(e) => app.set_clipboard_note(e.into()),
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_send_clicked(move |peer, text| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let peer = peer.to_string();
            let text = text.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_messenger_busy(true);
                app.set_messenger_error("".into());
                app.set_messenger_note("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.messenger_send(&peer, &text);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_messenger_busy(false);
                        match result {
                            Ok(value) => {
                                app.set_messenger_text("".into());
                                let queued = value["queued"].as_bool().unwrap_or(false);
                                app.set_messenger_note(
                                    if queued {
                                        "сообщение сохранено в очереди и будет доставлено повторно"
                                            .to_string()
                                    } else {
                                        format!(
                                            "отправлено: {}",
                                            value["id"].as_str().unwrap_or("?")
                                        )
                                    }
                                    .into(),
                                );
                            }
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        let auto_process_flag = flag_source.clone();
        app.on_messenger_inbox_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let auto_process = if let Some(app) = weak.upgrade() {
                app.set_messenger_busy(true);
                app.set_messenger_error("".into());
                app.set_messenger_note("".into());
                app.get_auto_process_slatepacks() || auto_process_flag.load(Ordering::SeqCst)
            } else {
                auto_process_flag.load(Ordering::SeqCst)
            };
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.messenger_inbox();
                // Fault-injection hook for the queue restart drill. It sits
                // after the durable inbox queue write and before automation.
                if std::env::var("FLINT_CRASH_AFTER_INBOX").ok().as_deref() == Some("1") {
                    std::thread::sleep(Duration::from_secs(30));
                }
                let mut auto_notes: Vec<String> = Vec::new();
                let mut auto_errors: Vec<String> = Vec::new();
                if auto_process {
                    if let Ok(value) = &result {
                        let contacts = wallet.messenger_contacts().unwrap_or_default();
                        for (id, from, text) in auto_slatepack_targets(value, &contacts) {
                            let _ = wallet.messenger_inbox_mark(&id, "processing", None);
                            match wallet.prepare_receive(&text) {
                                Ok(slate) => {
                                    if let Err(error) = wallet.finalize_receive(&slate.slate_id) {
                                        let _ = wallet.messenger_inbox_mark(
                                            &id,
                                            "failed",
                                            Some(&error),
                                        );
                                        auto_errors.push(error);
                                        continue;
                                    }
                                    match wallet.messenger_send(&from, &slate.slatepack) {
                                        Ok(sent) => {
                                            if let Some(reply_id) = sent["id"].as_str() {
                                                let _ = wallet
                                                    .messenger_inbox_attach_reply(&id, reply_id);
                                                if std::env::var("FLINT_CRASH_AFTER_REPLY_ATTACH")
                                                    .ok()
                                                    .as_deref()
                                                    == Some("1")
                                                {
                                                    std::process::abort();
                                                }
                                            }
                                            let _ = wallet.messenger_inbox_mark(&id, "done", None);
                                            auto_notes.push(format!(
                                                "slatepack от контакта автофинализирован · {} GRIN",
                                                format_amount(&slate.amount_nano)
                                            ));
                                        }
                                        Err(error) => {
                                            let _ = wallet.messenger_inbox_mark(
                                                &id,
                                                "failed",
                                                Some(&error),
                                            );
                                            auto_errors.push(error);
                                        }
                                    }
                                }
                                Err(receive_error) => {
                                    let mut finalized = false;
                                    if let Ok(list) = wallet.pending() {
                                        for item in list.items {
                                            if item.kind != "send" {
                                                continue;
                                            }
                                            if wallet.finalize_send(&item.slate_id, &text).is_ok() {
                                                let _ =
                                                    wallet.messenger_inbox_mark(&id, "done", None);
                                                auto_notes.push(
                                                    "ответ на отправку автофинализирован"
                                                        .to_string(),
                                                );
                                                finalized = true;
                                                break;
                                            }
                                        }
                                    }
                                    if !finalized {
                                        let _ = wallet.messenger_inbox_mark(
                                            &id,
                                            "failed",
                                            Some(&receive_error),
                                        );
                                        auto_errors.push(receive_error);
                                    }
                                }
                            }
                        }
                    }
                }
                let auto_note = if auto_notes.is_empty() {
                    None
                } else {
                    Some(auto_notes.join("; "))
                };
                let auto_error = if auto_errors.is_empty() {
                    None
                } else {
                    Some(auto_errors.join("; "))
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_messenger_busy(false);
                        match result {
                            Ok(value) => {
                                let mut lines = Vec::new();
                                if let Some(messages) = value["messages"].as_array() {
                                    for message in messages {
                                        lines.push(format!(
                                            "{}: {}",
                                            message["from"].as_str().unwrap_or("?"),
                                            message["kind"]["text"]
                                                .as_str()
                                                .or_else(|| message["kind"]["kind"].as_str())
                                                .unwrap_or("сообщение")
                                        ));
                                    }
                                }
                                app.set_messenger_inbox(lines.join("\n").into());
                                let inbox_note = format!(
                                    "новых: {} · blocked: {} · failed: {}",
                                    value["messages"].as_array().map(Vec::len).unwrap_or(0),
                                    value["blocked"].as_u64().unwrap_or(0),
                                    value["failed"].as_u64().unwrap_or(0)
                                );
                                app.set_messenger_note(
                                    match &auto_note {
                                        Some(note) => format!("{inbox_note} · {note}"),
                                        None => inbox_note,
                                    }
                                    .into(),
                                );
                                if let Some(error) = auto_error {
                                    app.set_messenger_error(i18n::tr(&error).into());
                                }
                            }
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    // When auto-processing is enabled, poll the messenger even if the
    // conversation pane is not in front. The callback serializes work through
    // `messenger-busy`; the timer only schedules, never blocks the event loop.
    let auto_inbox_timer = slint::Timer::default();
    {
        let weak = app.as_weak();
        let auto_process_flag = flag_source.clone();
        auto_inbox_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(10),
            move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                if app.get_unlocked()
                    && (app.get_auto_process_slatepacks()
                        || auto_process_flag.load(Ordering::SeqCst))
                    && !app.get_messenger_busy()
                {
                    app.invoke_messenger_inbox_clicked();
                }
            },
        );
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_contact_save_clicked(move |address, name, nostr| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let address = address.to_string();
            let name = name.to_string();
            let nostr = nostr.to_string();
            let saved_address = address.clone();
            let saved_name = name.clone();
            if let Some(app) = weak.upgrade() {
                app.set_messenger_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = if nostr.trim().is_empty() {
                    wallet.messenger_contact_save(&address, &name)
                } else {
                    wallet.messenger_contact_save_nostr(&address, &name, Some(nostr.as_str()))
                };
                let contacts = wallet.messenger_contacts().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        if let Some(contacts) = contacts {
                            app.set_messenger_contacts(slint::ModelRc::new(slint::VecModel::from(
                                presentation::contact_rows(&contacts),
                            )));
                        }
                        match result {
                            Ok(_) => {
                                app.set_messenger_contact_name("".into());
                                app.set_messenger_contact_address("".into());
                                app.set_messenger_error("".into());
                                app.set_show_add_contact(false);
                                app.set_messenger_selected_address(saved_address.clone().into());
                                app.set_messenger_selected_name(saved_name.clone().into());
                                app.set_messenger_peer(saved_address.into());
                                app.set_messenger_note(i18n::tr("контакт сохранён").into());
                            }
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_contact_delete_clicked(move |address| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let address = address.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_messenger_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.messenger_contact_delete(&address);
                let contacts = wallet.messenger_contacts().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        if let Some(contacts) = contacts {
                            app.set_messenger_contacts(slint::ModelRc::new(slint::VecModel::from(
                                presentation::contact_rows(&contacts),
                            )));
                        }
                        match result {
                            Ok(_) => app.set_messenger_note(i18n::tr("контакт удалён").into()),
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_history_clicked(move |peer| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let peer = peer.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_messenger_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.messenger_history(&peer);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        if app.get_messenger_selected_address().as_str() != peer
                            && app.get_messenger_peer().as_str() != peer
                        {
                            return;
                        }
                        match result {
                            Ok(value) => {
                                let text = presentation::history_text(&value);
                                app.set_messenger_messages(slint::ModelRc::new(
                                    slint::VecModel::from(presentation::message_rows(&value)),
                                ));
                                app.set_messenger_history(
                                    if text.is_empty() {
                                        "история пустая".to_string()
                                    } else {
                                        text
                                    }
                                    .into(),
                                );
                            }
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_block_clicked(move |address, blocked| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let address = address.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_messenger_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.messenger_block(&address, blocked);
                let blocked_list = wallet.messenger_blocked().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        if let Some(blocked_list) = blocked_list {
                            app.set_messenger_blocked(
                                presentation::blocked_text(&blocked_list).into(),
                            );
                            app.set_messenger_blocked_addresses(slint::ModelRc::new(
                                slint::VecModel::from(presentation::blocked_rows(&blocked_list)),
                            ));
                        }
                        match result {
                            Ok(_) => app.set_messenger_note(
                                if blocked {
                                    "адрес заблокирован"
                                } else {
                                    "адрес разблокирован"
                                }
                                .into(),
                            ),
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet_source.clone();
        app.on_messenger_recover_clicked(move |password| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let password = Zeroizing::new(password.to_string());
            if let Some(app) = weak.upgrade() {
                app.set_messenger_busy(true);
                app.set_messenger_error("".into());
                app.set_messenger_note("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.recover_legacy_history(password.as_str());
                let status = wallet.messenger_status().ok();
                let contacts = wallet.messenger_contacts().ok();
                let blocked = wallet.messenger_blocked().ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_messenger_busy(false);
                        app.set_messenger_recovery_password("".into());
                        if let Some(contacts) = contacts {
                            app.set_messenger_contacts(slint::ModelRc::new(slint::VecModel::from(
                                presentation::contact_rows(&contacts),
                            )));
                        }
                        if let Some(blocked) = blocked {
                            app.set_messenger_blocked(presentation::blocked_text(&blocked).into());
                            app.set_messenger_blocked_addresses(slint::ModelRc::new(
                                slint::VecModel::from(presentation::blocked_rows(&blocked)),
                            ));
                        }
                        if let Some(status) = status {
                            if let Some(address) = status["address"].as_str() {
                                app.set_messenger_address(address.into());
                                app.set_messenger_address_short(short_address(address).into());
                            }
                        }
                        match result {
                            Ok(_) => app.set_messenger_note(
                                i18n::tr("старая история восстановлена").into(),
                            ),
                            Err(e) => app.set_messenger_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        app.on_messenger_close_clicked(move || {
            if let Some(app) = weak.upgrade() {
                app.set_show_messages(false);
                app.set_messenger_peer("".into());
                app.set_messenger_text("".into());
                app.set_messenger_error("".into());
                app.set_messenger_note("".into());
                app.set_messenger_inbox("".into());
                app.set_messenger_history("".into());
                app.set_messenger_recovery_password("".into());
                app.set_messenger_selected_address("".into());
                app.set_messenger_selected_name("".into());
                app.set_messenger_messages(slint::ModelRc::new(slint::VecModel::from(Vec::<
                    MessageRow,
                >::new(
                ))));
                app.set_show_add_contact(false);
                app.set_show_recover_history(false);
            }
        });
    }
}
