#![cfg_attr(windows, windows_subsystem = "windows")]

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use flint_core::WalletStatus;
use flint_runtime::{SwapRuntime, WalletRuntime};
use zeroize::Zeroizing;

mod activity_handlers;
mod format;
mod i18n;
mod messenger_handlers;
mod onboarding_handlers;
mod platform;
mod presentation;
mod sanitize;
mod settings_handlers;
mod tor_node;

use format::amount as format_amount;
use format::short_address;
use platform::{pick_data_folder, show_startup_error};

#[cfg(windows)]
fn system_idle_seconds() -> Option<u64> {
    #[repr(C)]
    struct LastInputInfo {
        cb_size: u32,
        dw_time: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
        fn GetTickCount() -> u32;
    }
    let mut info = LastInputInfo {
        cb_size: std::mem::size_of::<LastInputInfo>() as u32,
        dw_time: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    Some(unsafe { GetTickCount().wrapping_sub(info.dw_time) } as u64 / 1000)
}

#[cfg(not(windows))]
fn system_idle_seconds() -> Option<u64> {
    None
}

slint::include_modules!();

#[cfg(windows)]
fn single_instance_guard() -> bool {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> *mut c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    let name: Vec<u16> = "Local\\FlintWalletSingleInstance\0"
        .encode_utf16()
        .collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return true;
    }
    if unsafe { GetLastError() } == 183 {
        unsafe { CloseHandle(handle) };
        return false;
    }
    std::mem::forget(Box::new(handle));
    true
}

#[cfg(not(windows))]
fn single_instance_guard() -> bool {
    true
}

/// Exclusive claim on the selected data root. The named mutex above only covers
/// one Windows session and one UI process; a second session, a junction alias or
/// a direct helper could otherwise open the same wallet directory concurrently.
/// The handle lives for the whole process and the OS releases it on exit.
#[cfg(windows)]
struct DataRootLock(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for DataRootLock {
    fn drop(&mut self) {
        #[link(name = "kernel32")]
        extern "system" {
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_data_root_lock(root: &std::path::Path) -> Result<DataRootLock, String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *const c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    std::fs::create_dir_all(root).map_err(|error| format!("каталог данных недоступен: {error}"))?;
    let path = root.join("flint.lock");
    let name: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // GENERIC_WRITE, share=0, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0x4000_0000,
            0,
            std::ptr::null(),
            4,
            0x80,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return Err(format!(
            "каталог данных уже занят другим экземпляром Flint (код {})",
            unsafe { GetLastError() }
        ));
    }
    Ok(DataRootLock(handle))
}

#[cfg(not(windows))]
fn acquire_data_root_lock(_root: &std::path::Path) -> Result<(), String> {
    Ok(())
}

fn status_line(status: &WalletStatus) -> String {
    let phase = i18n::tr(if status.unlocked {
        "кошелёк открыт"
    } else if status.initialized {
        "заблокирован"
    } else {
        "кошелёк не создан"
    });
    if status.mode == "grin-wallet" {
        format!("{} · {phase} · grin-wallet", status.network)
    } else {
        format!("{} · {phase}", status.network)
    }
}

pub(crate) fn node_line(info: &flint_core::NodeInfo) -> String {
    if info.connected {
        let english = i18n::is_english();
        let sync = match (info.sync_current, info.sync_target) {
            (Some(current), Some(target)) if target > current => {
                let percent = (current.saturating_mul(100) / target).min(100);
                if english {
                    format!(" · sync {current}/{target} ({percent}%)")
                } else {
                    format!(" · синхронизация {current}/{target} ({percent}%)")
                }
            }
            _ => String::new(),
        };
        let peers = info
            .peers
            .map(|peers| {
                if english {
                    format!(" · {peers} peers")
                } else {
                    format!(" · {peers} пиров")
                }
            })
            .unwrap_or_default();
        let line = if english {
            format!(
                "node: {} · height {}{}{}{sync}",
                i18n::tr(info.phase.label()),
                info.height,
                info.version
                    .as_deref()
                    .map(|v| format!(" · {v}"))
                    .unwrap_or_default(),
                peers
            )
        } else {
            format!(
                "нода: {} · высота {}{}{}{sync}",
                info.phase.label(),
                info.height,
                info.version
                    .as_deref()
                    .map(|v| format!(" · {v}"))
                    .unwrap_or_default(),
                peers
            )
        };
        line
    } else {
        i18n::tr(match info.phase {
            flint_core::node::NodePhase::Starting => {
                "нода: процесс запускается, RPC ещё не отвечает — сумма может быть неактуальной"
            }
            flint_core::node::NodePhase::Headers => {
                "нода: качает заголовки, RPC ещё не отвечает — сумма может быть неактуальной"
            }
            flint_core::node::NodePhase::Blocks => {
                "нода: качает блоки, RPC ещё не отвечает — сумма может быть неактуальной"
            }
            flint_core::node::NodePhase::NotRunning => "нода не запущена",
            flint_core::node::NodePhase::Ready | flint_core::node::NodePhase::Unknown => {
                "нода не подключена — сумма может быть неактуальной"
            }
        })
    }
}

/// Format an integer nanogrin string as GRIN with nine decimals.
///
/// Core money fields are integer nano strings. The old implementation appended
/// `.000000000`, which displayed `1` nano as `1.000000000 GRIN` (a billion-fold
/// error). No float is involved: whole and fractional parts are split with
/// integer division and `u128` parsing.
fn balance_line(summary: &flint_core::WalletSummary) -> String {
    format!("{} GRIN", format_amount(&summary.balance.spendable))
}

pub(crate) fn is_slatepack_armor(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.len() > 40
        && trimmed.starts_with("BEGINSLATEPACK.")
        && trimmed.ends_with("ENDSLATEPACK.")
}

/// The auto-finalizer must see only armored messages from addresses that are
/// still in the saved-contact book. Keep the selection pure and testable so an
/// unknown sender can never be fed into `prepare_receive` by accident.
pub(crate) fn auto_slatepack_targets(
    inbox: &serde_json::Value,
    contacts: &serde_json::Value,
) -> Vec<(String, String, String)> {
    let allowed: HashSet<String> = contacts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["address"].as_str())
        .map(str::to_string)
        .collect();
    inbox["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| {
            let text = message["kind"]["text"].as_str()?;
            let from = message["from"].as_str()?;
            let id = message["id"].as_str()?;
            if message["reply_id"].as_str().is_some() {
                return None;
            }
            if !allowed.contains(from) || !is_slatepack_armor(text) {
                return None;
            }
            Some((id.to_string(), from.to_string(), text.to_string()))
        })
        .collect()
}

/// A full rescan is the repair path for a balance cache polluted by a cloned
/// wallet. Surface the change instead of silently replacing the number.
fn balance_change_warning(before: &str, after: &str) -> String {
    let (Ok(before_nano), Ok(after_nano)) = (parse_nano(before), parse_nano(after)) else {
        return String::new();
    };
    if before_nano == after_nano {
        return String::new();
    }
    i18n::tr(&format!(
        "Полный rescan изменил баланс: было {} GRIN, стало {} GRIN. Проверьте историю и комиссии; это мог быть устаревший кэш.",
        format_amount(before),
        format_amount(after)
    ))
}

fn scan_state_line(state: flint_core::ScanState, error: Option<&str>) -> String {
    match state {
        flint_core::ScanState::Running => i18n::tr("скан идёт"),
        flint_core::ScanState::Completed => i18n::tr("скан завершён"),
        flint_core::ScanState::Failed => error
            .map(i18n::tr)
            .unwrap_or_else(|| i18n::tr("скан не завершён")),
        flint_core::ScanState::NotStarted => String::new(),
    }
}

pub(crate) fn apply_tor_status(app: &AppWindow, status: &flint_runtime::TorStatus) {
    app.set_tor_running(status.running);
    app.set_tor_onion(status.onion.clone().unwrap_or_default().into());
    let note = if status.running {
        let bootstrap = i18n::tr(if status.bootstrap {
            " (bootstrap 100%)"
        } else {
            " (bootstrap идёт)"
        });
        i18n::tr(&format!(
            "Tor включён{}; relay: {}",
            bootstrap,
            status.relay_url.as_deref().unwrap_or("—")
        ))
    } else if status.tor_exe.is_none() {
        i18n::tr("tor.exe не найден — положите Tor expert bundle в resources/tor")
    } else {
        i18n::tr("Tor выключен; включите для прямого приёма и messenger")
    };
    app.set_tor_note(note.into());
}

pub(crate) fn tor_bridge_status_text(bridges: &flint_runtime::TorBridges) -> String {
    let mut text = format!(
        "мосты: {} · строк: {}",
        i18n::tr(if bridges.diagnostics.configured {
            "настроены"
        } else {
            "не заданы"
        }),
        bridges.diagnostics.lines
    );
    for transport in &bridges.diagnostics.transports {
        text.push_str(&format!(
            " · {}: {}",
            transport.kind,
            i18n::tr(if transport.available {
                "клиент найден"
            } else {
                "клиент не найден"
            })
        ));
    }
    if let Some(error) = &bridges.diagnostics.error {
        text.push_str(&format!(" · {error}"));
    }
    i18n::tr(&text)
}

pub(crate) fn refresh_tor_status(app: &AppWindow, wallet: &WalletRuntime) {
    app.set_tor_available(wallet.is_rpc());
    match wallet.tor().status() {
        Ok(status) => apply_tor_status(app, &status),
        Err(e) => app.set_tor_note(e.into()),
    }
}

pub(crate) fn copy_text(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("{}: {e}", i18n::tr("буфер обмена")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("{}: {e}", i18n::tr("буфер обмена")))
}

fn qr_image_size(modules: usize, scale: usize, quiet: usize) -> u32 {
    ((modules + quiet * 2) * scale) as u32
}

pub(crate) fn make_qr(text: &str) -> Option<slint::Image> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let modules = code.width();
    let scale = 6usize;
    let quiet = 4usize;
    let size = qr_image_size(modules, scale, quiet);
    let colors = code.to_colors();
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(size, size);
    let pixels = buffer.make_mut_slice();
    for y in 0..size as usize {
        for x in 0..size as usize {
            let dark = if x < quiet * scale || y < quiet * scale {
                false
            } else {
                let mx = (x - quiet * scale) / scale;
                let my = (y - quiet * scale) / scale;
                if mx >= modules || my >= modules {
                    false
                } else {
                    colors[my * modules + mx] == qrcode::Color::Dark
                }
            };
            pixels[y * size as usize + x] = if dark {
                slint::Rgba8Pixel {
                    r: 9,
                    g: 13,
                    b: 17,
                    a: 255,
                }
            } else {
                slint::Rgba8Pixel {
                    r: 231,
                    g: 238,
                    b: 244,
                    a: 255,
                }
            };
        }
    }
    Some(slint::Image::from_rgba8(buffer))
}

fn parse_nano(raw: &str) -> Result<u64, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("введите сумму".into());
    }
    let (whole, frac) = match raw.split_once('.') {
        Some((w, f)) => (w, f),
        None => (raw, ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return Err("сумма пустая".into());
    }
    if frac.len() > 9 {
        return Err("не больше 9 знаков после точки".into());
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole
            .parse()
            .map_err(|_| "сумма должна быть числом".to_string())?
    };
    let mut frac_digits = frac.to_string();
    while frac_digits.len() < 9 {
        frac_digits.push('0');
    }
    let frac: u64 = if frac_digits.is_empty() {
        0
    } else {
        frac_digits
            .parse()
            .map_err(|_| "сумма должна быть числом".to_string())?
    };
    whole
        .checked_mul(1_000_000_000)
        .and_then(|v| v.checked_add(frac))
        .ok_or_else(|| "слишком большая сумма".to_string())
}

pub(crate) fn activity_rows(wallet: &WalletRuntime, limit: usize) -> Vec<ActivityRow> {
    wallet
        .activity(limit)
        .unwrap_or_default()
        .into_iter()
        .map(|tx| {
            let direction = i18n::tr(if tx.direction == "in" {
                "Получено"
            } else {
                "Отправлено"
            });
            let title = if tx.status == "confirmed" {
                direction.to_string()
            } else {
                format!("{direction} · {}", tx.status)
            };
            let when = chrono::DateTime::from_timestamp(tx.created_at, 0)
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%d.%m %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "—".to_string());
            ActivityRow {
                title: title.into(),
                amount: format_amount(&tx.amount).into(),
                when: when.into(),
            }
        })
        .collect()
}

fn pending_actions(kind: &str, state: &str) -> (bool, bool) {
    use flint_transfer_contract::{Action, Direction, Stage};
    let direction = if kind == "receive" {
        Direction::Receive
    } else {
        Direction::Send
    };
    let stage = Stage::from_wallet_state(kind, state);
    let can_retry =
        kind == "send" && (stage.allowed(Action::Repost) || matches!(stage, Stage::Finalized));
    let can_cancel = direction == Direction::Send
        && matches!(stage, Stage::Prepared)
        && stage.allowed(Action::Cancel);
    (can_retry, can_cancel)
}

pub(crate) struct PendingData {
    rows: Vec<PendingRow>,
    unreadable: Vec<slint::SharedString>,
    diagnostics: String,
    alert: String,
}

/// Commands are allowed only for ids that were in a successfully loaded
/// readable pending list of the same wallet generation. The UI explanation is
/// not the enforcement.
#[derive(Default)]
pub(crate) struct PendingGate {
    generation: Option<u64>,
    loaded: bool,
    readable: HashSet<String>,
    retryable: HashSet<String>,
    cancelable: HashSet<String>,
}

pub(crate) type SharedPendingGate = Arc<Mutex<PendingGate>>;

#[derive(Clone, Copy)]
pub(crate) enum PendingAction {
    Retry,
    Cancel,
}

pub(crate) fn pending_allowed(
    gate: &SharedPendingGate,
    generation: u64,
    id: &str,
    action: PendingAction,
) -> bool {
    gate.lock()
        .map(|gate| {
            if gate.generation != Some(generation) || !gate.loaded || !gate.readable.contains(id) {
                return false;
            }
            match action {
                PendingAction::Retry => gate.retryable.contains(id),
                PendingAction::Cancel => gate.cancelable.contains(id),
            }
        })
        .unwrap_or(false)
}

fn pending_next_action(kind: &str, state: &str, _rpc: bool) -> &'static str {
    use flint_transfer_contract::{Direction, NextAction, Stage};
    let direction = if kind == "receive" {
        Direction::Receive
    } else {
        Direction::Send
    };
    let action = Stage::from_wallet_state(kind, state).next_action(direction);
    match action {
        NextAction::Resend => {
            "Запрос создан; повторная пересылка S1 недоступна — отмените или дождитесь ответа"
        }
        NextAction::Repost => "Повторить публикацию",
        NextAction::Wait => "Проверить подтверждение сети",
        NextAction::ExportResponse => "Экспортировать ответ и завершить приём",
        NextAction::Finalize => "Завершить приём",
        NextAction::RestoreScan => "Проверить состояние операции",
        NextAction::Cancel => "Отменить безопасно",
    }
}

fn node_error_action(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("401") || lower.contains("403") || lower.contains("секрет") {
        "проверьте API-секрет ноды в настройках"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "проверьте подключение и повторите запрос"
    } else if lower.contains("connection")
        || lower.contains("недоступ")
        || lower.contains("sending request")
    {
        "проверьте адрес ноды и подключение"
    } else if lower.contains("сеть") || lower.contains("network") {
        "проверьте выбранную сеть в настройках"
    } else {
        "повторите запрос или откройте диагностику"
    }
}

pub(crate) fn load_pending(wallet: &WalletRuntime) -> Result<PendingData, String> {
    let list = wallet.pending()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let alert = flint_core::pending_alerts(&list.items, now)
        .iter()
        .take(3)
        .map(|item| item.message.clone())
        .collect::<Vec<_>>()
        .join(" · ");
    let rpc = wallet.is_rpc();
    let rows = list
        .items
        .into_iter()
        .map(|p| {
            let (can_retry, can_cancel) = pending_actions(&p.kind, &p.state);
            PendingRow {
                id: p.slate_id.into(),
                amount: format!("{} {}", format_amount(&p.amount), p.kind).into(),
                state: p.state.clone().into(),
                next_action: pending_next_action(&p.kind, &p.state, rpc).into(),
                error: p.last_error.unwrap_or_default().into(),
                can_retry,
                can_cancel,
                can_export: p.kind == "receive" && p.state == "response_received",
                can_export_request: p.kind == "send"
                    && matches!(
                        p.state.as_str(),
                        "prepared" | "locked" | "failed_recoverable"
                    ),
            }
        })
        .collect();
    Ok(PendingData {
        rows,
        alert,
        unreadable: list
            .unreadable
            .into_iter()
            .map(|name| name.into())
            .collect(),
        diagnostics: list.diagnostics,
    })
}

fn apply_pending(app: &AppWindow, data: PendingData, gate: &SharedPendingGate, generation: u64) {
    if app.get_wallet_generation() as u64 != generation {
        return;
    }
    if let Ok(mut gate) = gate.lock() {
        gate.generation = Some(generation);
        gate.loaded = true;
        gate.readable = data.rows.iter().map(|row| row.id.to_string()).collect();
        gate.retryable = data
            .rows
            .iter()
            .filter(|row| row.can_retry)
            .map(|row| row.id.to_string())
            .collect();
        gate.cancelable = data
            .rows
            .iter()
            .filter(|row| row.can_cancel)
            .map(|row| row.id.to_string())
            .collect();
    }
    app.set_pending(slint::ModelRc::new(slint::VecModel::from(data.rows)));
    app.set_pending_alert(data.alert.into());
    app.set_pending_unreadable(slint::ModelRc::new(slint::VecModel::from(data.unreadable)));
    app.set_pending_diagnostics(data.diagnostics.into());
    app.set_pending_error("".into());
    app.set_pending_loading(false);
}

pub(crate) fn apply_pending_result(
    app: &AppWindow,
    result: Result<PendingData, String>,
    gate: &SharedPendingGate,
    generation: u64,
) {
    if app.get_wallet_generation() as u64 != generation {
        return;
    }
    match result {
        Ok(data) => apply_pending(app, data, gate, generation),
        Err(error) => apply_pending_error(app, error, gate),
    }
}

pub(crate) fn arm_pending_watchdog(
    weak: slint::Weak<AppWindow>,
    gate: SharedPendingGate,
    generation: u64,
) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(12));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = weak.upgrade() {
                if app.get_wallet_generation() as u64 == generation && app.get_pending_loading() {
                    apply_pending_error(
                        &app,
                        "не удалось получить очередь за 12 с; состояние неизвестно".into(),
                        &gate,
                    );
                }
            }
        });
    });
}

fn apply_pending_error(app: &AppWindow, error: String, gate: &SharedPendingGate) {
    if let Ok(mut gate) = gate.lock() {
        gate.generation = None;
        gate.loaded = false;
        gate.readable.clear();
        gate.retryable.clear();
        gate.cancelable.clear();
    }
    app.set_pending(slint::ModelRc::new(slint::VecModel::from(
        Vec::<PendingRow>::new(),
    )));
    app.set_pending_unreadable(slint::ModelRc::new(slint::VecModel::from(Vec::<
        slint::SharedString,
    >::new())));
    app.set_pending_diagnostics("".into());
    app.set_pending_error(i18n::tr(&error).into());
    app.set_pending_alert("".into());
    app.set_pending_loading(false);
}

pub(crate) fn seed_status_text(status: &flint_runtime::SeedStatus) -> String {
    let kind = match status.kind.as_str() {
        "envelope" => "Argon2id envelope",
        "legacy" => "legacy PBKDF2",
        "missing" => "seed не найден",
        other => other,
    };
    format!(
        "seed: {kind} · backups: {} · legacy leftover: {} · network: {}",
        status.backups, status.legacy_leftover, status.network
    )
}

pub(crate) struct Snapshot {
    generation: u64,
    profiles: flint_runtime::RuntimeProfiles,
    recent_profiles: Vec<String>,
    status: WalletStatus,
    summary: Option<flint_core::WalletSummary>,
    node: flint_core::NodeInfo,
    activity: Vec<ActivityRow>,
}

pub(crate) fn snapshot(wallet: &WalletRuntime) -> Result<Snapshot, String> {
    // Capture before reading: if a switch wins the race, apply_snapshot drops
    // this entire result instead of mixing balances from two profiles.
    let generation = wallet.context_generation();
    let profiles = wallet.profiles()?;
    let status = wallet.status()?.status;
    let summary = if status.unlocked {
        wallet.summary().ok()
    } else {
        None
    };
    let node = wallet.node_info().unwrap_or_default();
    let activity = if status.unlocked {
        activity_rows(wallet, 20)
    } else {
        Vec::new()
    };
    Ok(Snapshot {
        generation,
        profiles,
        recent_profiles: wallet.recent_profiles(),
        status,
        summary,
        node,
        activity,
    })
}

/// Re-open the just-switched profile from the in-memory grace cache when its
/// password was used recently. The switch itself already succeeded; a failed
/// auto-open just leaves the normal password screen.
fn snapshot_after_switch(wallet: &WalletRuntime) -> Option<Snapshot> {
    let mut current = snapshot(wallet).ok()?;
    if current.status.initialized && !current.status.unlocked {
        let profile = wallet.profile();
        if let Some(password) = wallet.cached_unlock(&profile) {
            if wallet.open(password.as_str()).is_ok() {
                let _ = wallet.unlock_history_key(password.as_str());
                if let Ok(next) = snapshot(wallet) {
                    current = next;
                }
            }
        }
    }
    Some(current)
}

fn apply_profiles(app: &AppWindow, profiles: &flint_runtime::RuntimeProfiles) {
    let mut names = profiles.profiles.clone();
    if !names.contains(&profiles.active) {
        names.push(profiles.active.clone());
    }
    names.sort();
    names.dedup();
    app.set_wallet_profiles(slint::ModelRc::new(slint::VecModel::from(
        names
            .into_iter()
            .map(slint::SharedString::from)
            .collect::<Vec<_>>(),
    )));
    app.set_active_wallet(profiles.active.clone().into());
    app.set_settings_profile(profiles.active.clone().into());
}

/// A profile switch is a security boundary in the UI as well as in the runtime:
/// no password, mnemonic, slate, pending id or messenger draft survives it.
pub(crate) fn reset_wallet_context(app: &AppWindow, pending_gate: &SharedPendingGate) {
    app.set_password("".into());
    app.set_unlock_note("".into());
    app.set_unlock_error("".into());
    app.set_unlock_busy(false);
    app.set_create_password("".into());
    app.set_create_password2("".into());
    app.set_create_mnemonic("".into());
    app.set_create_address("".into());
    app.set_create_error("".into());
    app.set_create_busy(false);
    app.set_restore_mnemonic("".into());
    app.set_restore_password("".into());
    app.set_restore_password2("".into());
    app.set_restore_error("".into());
    app.set_restore_busy(false);
    app.set_send_to("".into());
    app.set_send_amount("".into());
    app.set_send_slatepack("".into());
    app.set_send_slate_id("".into());
    app.set_send_response("".into());
    app.set_send_prepared(false);
    app.set_send_confirmation("".into());
    app.set_send_prepared_to("".into());
    app.set_send_prepared_amount("".into());
    app.set_send_prepared_fee("".into());
    app.set_send_note("".into());
    app.set_send_error("".into());
    app.set_send_busy(false);
    app.set_receive_incoming("".into());
    app.set_receive_slatepack("".into());
    app.set_receive_slate_id("".into());
    app.set_receive_note("".into());
    app.set_receive_error("".into());
    app.set_receive_busy(false);
    app.set_messenger_address("".into());
    app.set_messenger_status("".into());
    app.set_messenger_peer("".into());
    app.set_messenger_text("".into());
    app.set_messenger_note("".into());
    app.set_messenger_error("".into());
    app.set_messenger_inbox("".into());
    app.set_messenger_history("".into());
    app.set_messenger_blocked("".into());
    app.set_messenger_contacts(slint::ModelRc::new(slint::VecModel::from(
        Vec::<ContactRow>::new(),
    )));
    app.set_messenger_contact_address("".into());
    app.set_messenger_contact_name("".into());
    app.set_messenger_recovery_password("".into());
    app.set_messenger_busy(false);
    app.set_clipboard_note("".into());
    app.set_sec_old("".into());
    app.set_sec_new("".into());
    app.set_sec_new2("".into());
    app.set_sec_delete("".into());
    app.set_sec_seed_password("".into());
    app.set_sec_seed_confirm("".into());
    app.set_sec_note("".into());
    app.set_sec_error("".into());
    app.set_sec_seed_status("".into());
    app.set_sec_busy(false);
    app.set_show_seed(false);
    app.set_seed_reveal_password("".into());
    app.set_seed_revealed("".into());
    app.set_seed_reveal_error("".into());
    app.set_seed_reveal_busy(false);
    app.set_settings_note("".into());
    app.set_settings_error("".into());
    app.set_settings_warning("".into());
    app.set_tor_busy(false);
    app.set_tor_running(false);
    app.set_tor_onion("".into());
    app.set_tor_note("".into());
    app.set_tor_bridges_text("".into());
    app.set_tor_bridge_status("".into());
    app.set_tor_log_text("".into());
    app.set_show_create(false);
    app.set_show_restore(false);
    app.set_show_send(false);
    app.set_show_receive(false);
    app.set_show_settings(false);
    app.set_show_security(false);
    app.set_show_history(false);
    app.set_show_messages(false);
    app.set_show_swaps(false);
    app.set_history(slint::ModelRc::new(slint::VecModel::from(
        Vec::<ActivityRow>::new(),
    )));
    apply_pending_error(app, String::new(), pending_gate);
    app.set_balance_warning("".into());
    app.set_scan_note("".into());
}

pub(crate) fn apply_snapshot(app: &AppWindow, snapshot: Snapshot, error: Option<String>) {
    if app.get_wallet_generation() < 0 || app.get_wallet_generation() as u64 != snapshot.generation
    {
        return;
    }
    apply_profiles(app, &snapshot.profiles);
    app.set_recent_profiles(slint::ModelRc::new(slint::VecModel::from(
        snapshot
            .recent_profiles
            .into_iter()
            .map(slint::SharedString::from)
            .collect::<Vec<_>>(),
    )));
    app.set_settings_network(snapshot.status.network.clone().into());
    app.set_network_label(snapshot.status.network.to_uppercase().into());
    app.set_initialized(snapshot.status.initialized);
    app.set_unlocked(snapshot.status.unlocked);
    app.set_status_text(status_line(&snapshot.status).into());
    app.set_scanning(snapshot.status.scan_state == flint_core::ScanState::Running);
    app.set_balance_text(
        snapshot
            .summary
            .as_ref()
            .map(balance_line)
            .unwrap_or_else(|| "—".to_string())
            .into(),
    );
    let (available, awaiting, locked, pending_visible) = snapshot
        .summary
        .as_ref()
        .map(|summary| {
            let awaiting_nano = summary.balance.awaiting.parse::<u64>().unwrap_or(0);
            let locked_nano = summary.balance.locked.parse::<u64>().unwrap_or(0)
                + summary.balance.immature.parse::<u64>().unwrap_or(0);
            (
                format_amount(&summary.balance.spendable),
                format_amount(&summary.balance.awaiting),
                format_amount(&(locked_nano.to_string())),
                awaiting_nano > 0 || locked_nano > 0,
            )
        })
        .unwrap_or_else(|| ("—".to_string(), "—".to_string(), "—".to_string(), false));
    app.set_balance_available(available.into());
    app.set_balance_awaiting(awaiting.into());
    app.set_balance_locked(locked.into());
    app.set_balance_pending_visible(pending_visible);
    let node_syncing = matches!(
        snapshot.node.phase,
        flint_core::node::NodePhase::Headers | flint_core::node::NodePhase::Blocks
    );
    app.set_node_syncing(node_syncing);
    let (balance_state, send_enabled) = if !snapshot.status.unlocked {
        (i18n::tr("войдите, чтобы увидеть баланс").to_string(), false)
    } else {
        match snapshot.summary.as_ref() {
            Some(summary) if summary.needs_scan => (
                i18n::tr("баланс неизвестен до полного скана").to_string(),
                false,
            ),
            Some(summary) if !summary.validated => (
                i18n::tr("баланс не проверен нодой (кеш)").to_string(),
                // While catching up, the user may explicitly accept the warning
                // in the send dialog; a scan is still mandatory.
                node_syncing,
            ),
            Some(_) => (
                i18n::tr("баланс проверен").to_string(),
                snapshot.node.phase.ready() || node_syncing,
            ),
            None => (i18n::tr("баланс недоступен").to_string(), false),
        }
    };
    app.set_balance_state(balance_state.into());
    app.set_send_enabled(send_enabled);
    app.set_scan_state(
        scan_state_line(
            snapshot.status.scan_state,
            snapshot.status.scan_error.as_deref(),
        )
        .into(),
    );
    let scan_running = snapshot.status.scan_state == flint_core::ScanState::Running;
    app.set_scan_running(scan_running);
    if !scan_running {
        app.set_scan_progress(0.0);
    }
    if snapshot.status.scan_state == flint_core::ScanState::NotStarted {
        app.set_scan_note("".into());
    }
    let address = snapshot
        .summary
        .as_ref()
        .map(|s| s.address.clone())
        .or_else(|| snapshot.status.address.clone())
        .unwrap_or_else(|| "—".to_string());
    app.set_address_text(address.clone().into());
    app.set_address_short(short_address(&address).into());
    if app.get_qr_address() != address {
        if let Some(qr) = make_qr(&address) {
            app.set_address_qr(qr);
        }
        app.set_qr_address(address.into());
    }
    app.set_node_text(node_line(&snapshot.node).into());
    let phase_label = match snapshot.node.phase {
        flint_core::node::NodePhase::Unknown => snapshot
            .node
            .status_error
            .as_deref()
            .map(|error| format!("нода: {error} · {}", i18n::tr(node_error_action(error))))
            .unwrap_or_else(|| i18n::tr("статус ноды неизвестен").to_string()),
        flint_core::node::NodePhase::Ready => i18n::tr("нода готова").to_string(),
        _ => i18n::tr(snapshot.node.phase.label()).to_string(),
    };
    app.set_node_phase_label(sanitize::log_line(&phase_label).into());
    app.set_node_height(snapshot.node.height.to_string().into());
    app.set_node_version(snapshot.node.version.clone().unwrap_or_default().into());
    app.set_node_peers(
        snapshot
            .node
            .peers
            .map(|peers| peers.to_string())
            .unwrap_or_else(|| "0".to_string())
            .into(),
    );
    app.set_node_ready(snapshot.node.phase.ready());
    app.set_node_starting(matches!(
        snapshot.node.phase,
        flint_core::node::NodePhase::Starting
            | flint_core::node::NodePhase::Headers
            | flint_core::node::NodePhase::Blocks
    ));
    app.set_unlock_error(i18n::tr(&error.unwrap_or_default()).into());
    app.set_activity(slint::ModelRc::new(slint::VecModel::from(
        snapshot.activity,
    )));
}

/// Fast, deterministic validation before touching the wallet. The runtime does
/// the authoritative name normalization; this only catches the two mistakes a
/// user makes in the profile dialog.
fn profile_request_error(active: &str, requested: &str) -> Option<&'static str> {
    let requested = requested.trim();
    if requested.is_empty() {
        return Some("введите имя профиля");
    }
    if requested == active {
        return Some("этот профиль уже активен");
    }
    None
}

/// A profile switch must not overtake a wallet mutation started from this
/// window. Read-only board/node polling is deliberately excluded.
pub(crate) fn wallet_context_busy(app: &AppWindow) -> bool {
    app.get_wallet_switch_busy()
        || app.get_scan_running()
        || app.get_unlock_busy()
        || app.get_create_busy()
        || app.get_restore_busy()
        || app.get_send_busy()
        || app.get_receive_busy()
        || app.get_pending_loading()
        || app.get_sec_busy()
        || app.get_messenger_busy()
        || app.get_tor_busy()
        || app.get_settings_data_dir_busy()
}

pub(crate) fn apply_ui_language(app: &AppWindow, code: &str) {
    let english = code == "en";
    i18n::set_english(english);
    // Slint keeps the bundled translations in the component and re-evaluates
    // all `@tr` bindings when this selection changes. An empty language is the
    // source (Russian) catalogue; bundled English is `en_US`.
    let selected = if english {
        slint::select_bundled_translation("en_US")
    } else {
        slint::select_bundled_translation("")
    };
    app.set_ui_language(code.into());
    match selected {
        Ok(()) => {
            let note = if english {
                "Language switched to English"
            } else {
                "Язык переключён на русский"
            };
            app.set_ui_language_note(note.into());
        }
        Err(error) => {
            app.set_ui_language_note(format!("не удалось переключить язык: {error}").into());
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    if !single_instance_guard() {
        return Ok(());
    }
    let mut data_dir = None;
    let mut backend =
        flint_runtime::BackendKind::from_name(&std::env::var("FLINT_BACKEND").unwrap_or_default());
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--data" {
            data_dir = args.next();
        } else if arg == "--backend" {
            if let Some(value) = args.next() {
                backend = flint_runtime::BackendKind::from_name(&value);
            }
        } else if let Some(value) = arg.strip_prefix("--backend=") {
            backend = flint_runtime::BackendKind::from_name(value);
        }
    }
    if let Some(dir) = data_dir {
        std::env::set_var("FLINT_DATA_DIR", dir);
    }
    let paths = match flint_core::AppPaths::resolve_checked() {
        Ok(paths) => paths,
        Err(error) => {
            show_startup_error(&error);
            return Ok(());
        }
    };
    let _data_root_lock = match acquire_data_root_lock(paths.root()) {
        Ok(guard) => guard,
        Err(error) => {
            show_startup_error(&error);
            return Ok(());
        }
    };

    let language = std::fs::read_to_string(paths.root().join("ui-language")).unwrap_or_default();

    let app = AppWindow::new()?;
    // The checkbox lives inside the messages view, which can be rebuilt when a
    // profile is switched. Keep the auto-finalizer switch in Rust as well so a
    // background tick never loses the user's choice during that rebuild.
    let auto_process_flag = Arc::new(AtomicBool::new(false));
    // Bundled translations register while the first component is created, so
    // the selection must happen after it (Slint marks @tr bindings dirty).
    if language.trim() == "en" {
        i18n::set_english(true);
        match slint::select_bundled_translation("en_US") {
            Ok(()) => eprintln!("ui language: en_US selected"),
            Err(error) => eprintln!("ui language: {error}"),
        }
    }
    // The Slint window declares min-size only so the WM can resize it; start
    // at the long-standing 1120x760 preview size.
    app.window()
        .set_size(slint::LogicalSize::new(1280.0, 860.0));
    if std::env::args().any(|arg| arg == "--modal") {
        app.set_show_send(true);
    }

    let wallet = WalletRuntime::open_at(paths, backend);
    let initial_auto = wallet.auto_process_slatepacks();
    auto_process_flag.store(initial_auto, Ordering::SeqCst);
    app.set_auto_process_slatepacks(initial_auto);
    {
        let flag = auto_process_flag.clone();
        let wallet = wallet.clone();
        let weak = app.as_weak();
        app.on_auto_process_toggled(move |enabled| {
            flag.store(enabled, Ordering::SeqCst);
            if let Err(error) = wallet.set_auto_process_slatepacks(enabled) {
                if let Some(app) = weak.upgrade() {
                    app.set_ui_language_note(format!("auto-slatepacks: {error}").into());
                }
            }
        });
    }
    app.set_ui_language(wallet.ui_language().into());
    if wallet.is_rpc() {
        // `grin-wallet` takes seconds to start; paint the locked screen first and
        // let unlock wait for readiness, exactly like the Slint shell.
        let background = wallet.clone();
        thread::spawn(move || {
            let _ = background.start_rpc_background();
        });
    }
    let swaps = SwapRuntime::new();
    if let Ok(initial) = snapshot(&wallet) {
        let node_text = node_line(&initial.node);
        apply_snapshot(&app, initial, None);
        app.set_node_text(node_text.into());
    }
    app.set_node_running(wallet.node_running());
    refresh_tor_status(&app, &wallet);
    let pending_gate: SharedPendingGate = Arc::new(Mutex::new(PendingGate::default()));

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        let pending_gate = pending_gate.clone();
        app.on_wallet_switch_clicked(move |requested| {
            let requested = requested.trim().to_string();
            let weak = weak.clone();
            let wallet = wallet.clone();
            let pending_gate = pending_gate.clone();
            if let Some(app) = weak.upgrade() {
                if requested == app.get_active_wallet().as_str() {
                    return;
                }
                if wallet_context_busy(&app) {
                    app.set_wallet_switch_error(
                        "дождитесь завершения текущей операции перед сменой кошелька".into(),
                    );
                    return;
                }
                app.set_wallet_switch_busy(true);
                app.set_wallet_switch_error("".into());
                app.set_unlock_note(i18n::tr("переключаю кошелёк…").into());
            }
            thread::spawn(move || {
                let before = wallet.context_generation();
                let profile = (requested != "default").then_some(requested);
                let result = wallet.switch(None, Some(profile));
                let after = wallet.context_generation();
                if result.is_ok() {
                    let _ = wallet.remember_recent_profile(&wallet.profile());
                }
                let snapshot = if result.is_ok() {
                    snapshot_after_switch(&wallet)
                } else if after != before {
                    snapshot(&wallet).ok()
                } else {
                    None
                };
                let running = wallet.node_running();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_wallet_switch_busy(false);
                        app.set_unlock_note("".into());
                        match result {
                            Ok(_) => {
                                if let Some(snapshot) = snapshot {
                                    reset_wallet_context(&app, &pending_gate);
                                    app.set_wallet_generation(snapshot.generation as i32);
                                    app.set_node_running(running);
                                    let needs_seed = !snapshot.status.initialized;
                                    apply_snapshot(&app, snapshot, None);
                                    refresh_tor_status(&app, &wallet);
                                    if needs_seed {
                                        app.set_show_create(true);
                                        app.set_create_error("".into());
                                    }
                                } else {
                                    app.set_wallet_switch_error(
                                        "кошелёк переключён, но статус не прочитан; обновите окно"
                                            .into(),
                                    );
                                }
                            }
                            Err(error) => {
                                if after != before {
                                    reset_wallet_context(&app, &pending_gate);
                                    if let Some(snapshot) = snapshot {
                                        app.set_wallet_generation(snapshot.generation as i32);
                                        apply_snapshot(&app, snapshot, None);
                                    }
                                }
                                app.set_wallet_switch_error(i18n::tr(&error).into());
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        app.on_profile_new_clicked(move || {
            if let Some(app) = weak.upgrade() {
                app.set_settings_profile("".into());
                app.set_settings_error("".into());
                app.set_settings_note("".into());
                app.set_profile_create_error("".into());
                app.set_profile_create_note(
                    i18n::tr("Введите имя нового профиля и нажмите «Создать профиль».").into(),
                );
                app.set_show_settings(true);
            }
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        let pending_gate = pending_gate.clone();
        app.on_profile_create_clicked(move |requested| {
            let requested = requested.trim().to_string();
            let Some(app) = weak.upgrade() else {
                return;
            };
            app.set_profile_create_error("".into());
            app.set_profile_create_note("".into());
            if let Some(error) = profile_request_error(app.get_active_wallet().as_str(), &requested)
            {
                app.set_profile_create_error(i18n::tr(error).into());
                return;
            }
            if wallet_context_busy(&app) {
                app.set_profile_create_error(
                    i18n::tr("дождитесь завершения текущей операции перед сменой кошелька").into(),
                );
                return;
            }
            app.set_profile_create_busy(true);
            let weak = app.as_weak();
            let wallet = wallet.clone();
            let pending_gate = pending_gate.clone();
            thread::spawn(move || {
                let before = wallet.context_generation();
                let result = wallet.switch(None, Some(Some(requested.clone())));
                let after = wallet.context_generation();
                if result.is_ok() {
                    let _ = wallet.remember_recent_profile(&requested);
                }
                let snapshot = if result.is_ok() {
                    snapshot_after_switch(&wallet)
                } else {
                    snapshot(&wallet).ok()
                };
                let running = wallet.node_running();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        app.set_profile_create_busy(false);
                        match result {
                            Ok(_) => {
                                if let Some(snapshot) = snapshot {
                                    reset_wallet_context(&app, &pending_gate);
                                    app.set_wallet_generation(snapshot.generation as i32);
                                    app.set_node_running(running);
                                    let needs_seed = !snapshot.status.initialized;
                                    apply_snapshot(&app, snapshot, None);
                                    refresh_tor_status(&app, &wallet);
                                    app.set_show_settings(false);
                                    app.set_settings_profile(requested.into());
                                    if needs_seed {
                                        app.set_show_create(true);
                                        app.set_create_error("".into());
                                    } else {
                                        app.set_unlock_note(
                                            i18n::tr("Профиль открыт; введите пароль.").into(),
                                        );
                                    }
                                } else {
                                    app.set_profile_create_error(
                                        i18n::tr(
                                            "кошелёк переключён, но статус не прочитан; обновите окно",
                                        )
                                        .into(),
                                    );
                                }
                            }
                            Err(error) => {
                                if after != before {
                                    reset_wallet_context(&app, &pending_gate);
                                    if let Some(snapshot) = snapshot {
                                        app.set_wallet_generation(snapshot.generation as i32);
                                        apply_snapshot(&app, snapshot, None);
                                    }
                                }
                                app.set_profile_create_error(i18n::tr(&error).into());
                            }
                        }
                    }
                });
            });
        });
    }

    app.on_paste_text(|| {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get_text().ok())
            .unwrap_or_default()
            .into()
    });

    // Keep the node badge and scan state fresh without rebuilding the whole
    // snapshot. The callback starts at most one worker; the timer itself never
    // blocks the event loop.
    let live_timer = slint::Timer::default();
    let live_refreshing = Arc::new(AtomicBool::new(false));
    let live_was_ready = Arc::new(AtomicBool::new(false));
    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        let refreshing = live_refreshing.clone();
        let was_ready = live_was_ready.clone();
        live_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(2),
            move || {
                if refreshing.swap(true, Ordering::SeqCst) {
                    return;
                }
                let weak = weak.clone();
                let wallet = wallet.clone();
                let refreshing = refreshing.clone();
                let was_ready = was_ready.clone();
                let generation = wallet.context_generation();
                thread::spawn(move || {
                    let info = wallet.node_info();
                    let disk = wallet.disk_space().ok();
                    let traffic = info
                        .as_ref()
                        .map(|info| {
                            matches!(
                                info.phase,
                                flint_core::node::NodePhase::Headers
                                    | flint_core::node::NodePhase::Blocks
                            )
                        })
                        .unwrap_or(false);
                    let scan = wallet.scan_runtime().view();
                    let scan_running = scan
                        .as_ref()
                        .map(|s| s.busy || matches!(s.state, flint_core::ScanState::Running))
                        .unwrap_or(false);
                    let scan_progress = scan
                        .as_ref()
                        .ok()
                        .and_then(|s| s.done.zip(s.target))
                        .filter(|(_, target)| *target > 0)
                        .map(|(done, target)| (done as f32 / target as f32).clamp(0.0, 1.0))
                        .unwrap_or(0.0);
                    let scan_paused = scan.as_ref().map(|s| s.paused).unwrap_or(false);
                    let scan_pausable = scan.as_ref().map(|s| s.pausable).unwrap_or(false);
                    let running = wallet.node_running();
                    let ready = info.as_ref().map(|i| i.phase.ready()).unwrap_or(false);
                    let became_ready = ready && !was_ready.swap(true, Ordering::SeqCst);
                    if !ready {
                        was_ready.store(false, Ordering::SeqCst);
                    }
                    let full = if became_ready {
                        snapshot(&wallet).ok()
                    } else {
                        None
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        refreshing.store(false, Ordering::SeqCst);
                        if let Some(app) = weak.upgrade() {
                            if app.get_wallet_generation() < 0
                                || app.get_wallet_generation() as u64 != generation
                            {
                                return;
                            }
                            if let Some(snapshot) = full {
                                apply_snapshot(&app, snapshot, None);
                            }
                            if let Ok(info) = info {
                                app.set_node_text(node_line(&info).into());
                                app.set_node_ready(info.phase.ready());
                                app.set_node_starting(matches!(
                                    info.phase,
                                    flint_core::node::NodePhase::Starting
                                        | flint_core::node::NodePhase::Headers
                                        | flint_core::node::NodePhase::Blocks
                                ));
                            }
                            app.set_node_running(running);
                            app.set_disk_warning(
                                disk.filter(|(free, _)| *free < 2 * 1024 * 1024 * 1024)
                                    .map(|(free, _)| {
                                        format!(
                                            "мало места на диске: свободно {:.1} ГиБ — синхронизация может остановиться",
                                            free as f64 / (1024.0 * 1024.0 * 1024.0)
                                        )
                                    })
                                    .unwrap_or_default()
                                    .into(),
                            );
                            app.set_traffic_warning(
                                if traffic {
                                    "нода синхронизирует заголовки/блоки — возможен большой сетевой трафик"
                                } else {
                                    ""
                                }
                                .into(),
                            );
                            if let Ok(scan) = scan {
                                app.set_scan_state(
                                    scan_state_line(scan.state, scan.error.as_deref()).into(),
                                );
                            }
                            app.set_scan_running(scan_running);
                            app.set_scan_paused(scan_paused);
                            app.set_scan_pausable(scan_pausable);
                            app.set_scan_progress(scan_progress);
                        }
                    });
                });
            },
        );
    }

    // Slate alerts are checked less often than node status: pending() reads the
    // journal (or asks grin-wallet), so a one-minute cadence is enough for an
    // in-app warning and does not compete with the two-second node refresh.
    let alert_timer = slint::Timer::default();
    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        let busy = Arc::new(AtomicBool::new(false));
        alert_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(60),
            move || {
                if busy.swap(true, Ordering::SeqCst) {
                    return;
                }
                let weak = weak.clone();
                let wallet = wallet.clone();
                let busy = busy.clone();
                let generation = wallet.context_generation();
                thread::spawn(move || {
                    let text = wallet
                        .pending_alerts()
                        .map(|alerts| {
                            alerts
                                .iter()
                                .take(3)
                                .map(|alert| alert.message.clone())
                                .collect::<Vec<_>>()
                                .join(" · ")
                        })
                        .unwrap_or_default();
                    let _ = slint::invoke_from_event_loop(move || {
                        busy.store(false, Ordering::SeqCst);
                        if let Some(app) = weak.upgrade() {
                            if app.get_wallet_generation() as u64 == generation {
                                app.set_pending_alert(text.into());
                            }
                        }
                    });
                });
            },
        );
    }

    // Lock an unlocked wallet after five minutes without keyboard/mouse input.
    // The grace cache is cleared at the same time, so the next profile switch
    // asks for the password again.
    let auto_lock_timer = slint::Timer::default();
    let auto_lock_busy = Arc::new(AtomicBool::new(false));
    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        let busy = auto_lock_busy.clone();
        auto_lock_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(15),
            move || {
                if busy.swap(true, Ordering::SeqCst) {
                    return;
                }
                if !wallet.is_unlocked() || system_idle_seconds().unwrap_or(0) < 300 {
                    busy.store(false, Ordering::SeqCst);
                    return;
                }
                let weak = weak.clone();
                let wallet = wallet.clone();
                let busy = busy.clone();
                let generation = wallet.context_generation();
                thread::spawn(move || {
                    let result = wallet.lock();
                    if result.is_ok() {
                        wallet.clear_unlock_cache();
                        wallet.lock_history_key();
                    }
                    let snapshot = snapshot(&wallet).ok();
                    let running = wallet.node_running();
                    let _ = slint::invoke_from_event_loop(move || {
                        busy.store(false, Ordering::SeqCst);
                        if let Some(app) = weak.upgrade() {
                            if app.get_wallet_generation() as u64 == generation {
                                app.set_node_running(running);
                                if let Some(snapshot) = snapshot {
                                    apply_snapshot(&app, snapshot, None);
                                    app.set_unlock_note(
                                        i18n::tr("кошелёк заблокирован после 5 минут бездействия")
                                            .into(),
                                    );
                                }
                            }
                        }
                    });
                });
            },
        );
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_unlock_clicked(move |password| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let password = Zeroizing::new(password.to_string());
            if let Some(app) = weak.upgrade() {
                if app.get_unlock_busy() {
                    return;
                }
                app.set_unlock_busy(true);
                app.set_unlock_error("".into());
                app.set_unlock_note(i18n::tr("проверяю пароль…").into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.open(password.as_str()).map(|_| ());
                if result.is_ok() {
                    wallet.remember_unlock(password.as_str());
                    let _ = wallet.remember_recent_profile(&wallet.profile());
                    let _ = wallet.unlock_history_key(password.as_str());
                }
                let (snapshot, error) = match result {
                    Ok(()) => match snapshot(&wallet) {
                        Ok(snapshot) => (Some(snapshot), None),
                        Err(e) => (None, Some(e)),
                    },
                    Err(e) => (None, Some(e)),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_unlock_busy(false);
                        app.set_unlock_note("".into());
                        if let Some(snapshot) = snapshot {
                            app.set_unlock_error("".into());
                            app.set_password("".into());
                            apply_snapshot(&app, snapshot, error);
                        } else {
                            app.set_password("".into());
                            app.set_unlock_error(error.unwrap_or_else(|| "ошибка".into()).into());
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_lock_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.lock();
                if result.is_ok() {
                    wallet.clear_unlock_cache();
                    wallet.lock_history_key();
                }
                let outcome = result.and_then(|_| snapshot(&wallet));
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_password("".into());
                        match outcome {
                            Ok(snapshot) => apply_snapshot(&app, snapshot, None),
                            Err(e) => app.set_unlock_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_refresh_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = snapshot(&wallet);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        match result {
                            Ok(snapshot) => apply_snapshot(&app, snapshot, None),
                            Err(e) => app.set_unlock_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_scan_clicked(move || {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let scan = wallet.scan_runtime().clone();
            let before_balance = wallet
                .summary()
                .ok()
                .map(|summary| summary.balance.spendable);
            if let Some(app) = weak.upgrade() {
                app.set_scanning(true);
                app.set_scan_paused(false);
                app.set_scan_note(i18n::tr("скан запущен…").into());
                app.set_balance_warning("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let progress_weak = weak.clone();
                let prepared = if wallet.is_rpc() {
                    wallet.ensure_node_ready()
                } else {
                    Ok(())
                };
                let result = prepared.and_then(|_| {
                    scan.scan(None, move |done, total| {
                        let text = format!("скан: {done} / {total}");
                        let weak = progress_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = weak.upgrade() {
                                if app.get_wallet_generation() as u64 != generation {
                                    return;
                                }
                                app.set_scan_note(text.into());
                            }
                        });
                    })
                });
                let snapshot = snapshot(&wallet);
                let warning = snapshot
                    .as_ref()
                    .ok()
                    .and_then(|snapshot| snapshot.summary.as_ref())
                    .and_then(|summary| {
                        before_balance.as_deref().map(|before| {
                            balance_change_warning(before, &summary.balance.spendable)
                        })
                    })
                    .unwrap_or_default();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        match snapshot {
                            Ok(snapshot) => apply_snapshot(&app, snapshot, None),
                            Err(e) => app.set_unlock_error(i18n::tr(&e).into()),
                        }
                        if !warning.is_empty() {
                            app.set_balance_warning(warning.into());
                        }
                        match result {
                            Ok(height) => app.set_scan_note(
                                i18n::tr(&format!("скан завершён: блок {height}")).into(),
                            ),
                            Err(e) => app
                                .set_scan_note(i18n::tr(&format!("скан: {}", i18n::tr(&e))).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_scan_cancel_clicked(move || {
            let weak = weak.clone();
            let scan = wallet.scan_runtime().clone();
            let generation = wallet.context_generation();
            if let Some(app) = weak.upgrade() {
                app.set_scan_note(i18n::tr("отмена скана…").into());
            }
            thread::spawn(move || {
                let error = scan.cancel().err();
                if let Some(error) = error {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = weak.upgrade() {
                            if app.get_wallet_generation() as u64 != generation {
                                return;
                            }
                            app.set_scan_note(
                                i18n::tr(&format!("скан: {}", i18n::tr(&error))).into(),
                            );
                        }
                    });
                }
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        let scan = wallet.scan_runtime().clone();
        app.on_scan_pause_clicked(move || {
            let weak = weak.clone();
            let scan = scan.clone();
            let generation = wallet.context_generation();
            let paused = weak
                .upgrade()
                .map(|app| app.get_scan_paused())
                .unwrap_or(false);
            if let Some(app) = weak.upgrade() {
                app.set_scan_note(
                    if paused {
                        "возобновление скана…"
                    } else {
                        "пауза после текущего батча…"
                    }
                    .into(),
                );
            }
            thread::spawn(move || {
                let result = if paused { scan.resume() } else { scan.pause() };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        match result {
                            Ok(()) => app.set_scan_note(
                                if paused {
                                    "скан продолжен"
                                } else {
                                    "скан приостановлен после батча"
                                }
                                .into(),
                            ),
                            Err(error) => app.set_scan_note(
                                i18n::tr(&format!("скан: {}", i18n::tr(&error))).into(),
                            ),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_send_prepare_clicked(move |to, amount| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let to = to.to_string();
            let amount = amount.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_send_busy(true);
                app.set_send_error("".into());
                app.set_send_note("".into());
                app.set_send_prepared(false);
                app.set_send_slate_id("".into());
                app.set_send_slatepack("".into());
            }
            let generation = wallet.context_generation();
            let allow_unsynced = weak
                .upgrade()
                .map(|app| app.get_send_allow_unsynced())
                .unwrap_or(false);
            thread::spawn(move || {
                let result = parse_nano(&amount).and_then(|nano| {
                    if allow_unsynced {
                        wallet.prepare_send_allow_unsynced(&to, nano)
                    } else {
                        wallet.prepare_send(&to, nano)
                    }
                });
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_send_busy(false);
                        match result {
                            Ok(slate) => {
                                app.set_send_slate_id(slate.slate_id.into());
                                app.set_send_slatepack(slate.slatepack.into());
                                app.set_send_confirmation(slate.confirmation.into());
                                app.set_send_prepared(true);
                                app.set_send_prepared_to(slate.destination.into());
                                app.set_send_prepared_amount(
                                    format_amount(&slate.amount_nano).into(),
                                );
                                app.set_send_prepared_fee(
                                    format!("{} нано", slate.fee_nano).into(),
                                );
                                let note = match slate.warning.as_deref() {
                                    Some(warning) => i18n::tr(warning).to_string(),
                                    None => format!(
                                        "Slatepack готов · комиссия {} нано",
                                        slate.fee_nano
                                    ),
                                };
                                app.set_send_note(note.into());
                            }
                            Err(e) => app.set_send_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_send_finalize_clicked(move |slate_id, response| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let slate_id = slate_id.to_string();
            let response = response.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_send_busy(true);
                app.set_send_error("".into());
            }
            let generation = wallet.context_generation();
            let confirmation = weak
                .upgrade()
                .map(|app| app.get_send_confirmation())
                .unwrap_or_default();
            let confirmation = confirmation.to_string();
            thread::spawn(move || {
                let result = wallet.finalize_send(&slate_id, &response, &confirmation);
                let snapshot = snapshot(&wallet);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_send_busy(false);
                        match result {
                            Ok(tx) => {
                                app.set_send_prepared(false);
                                app.set_send_confirmation("".into());
                                app.set_send_slate_id("".into());
                                app.set_send_slatepack("".into());
                                app.set_send_prepared_to("".into());
                                app.set_send_prepared_amount("".into());
                                app.set_send_prepared_fee("".into());
                                app.set_send_note(i18n::tr(&format!("Отправлено: {tx}")).into());
                                if let Ok(snapshot) = snapshot {
                                    apply_snapshot(&app, snapshot, None);
                                }
                            }
                            Err(e) => app.set_send_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_receive_prepare_clicked(move |incoming| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let incoming = incoming.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_receive_busy(true);
                app.set_receive_error("".into());
                app.set_receive_note("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.prepare_receive(&incoming);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_receive_busy(false);
                        match result {
                            Ok(receive) => {
                                app.set_receive_slate_id(receive.slate_id.into());
                                app.set_receive_slatepack(receive.slatepack.into());
                                app.set_receive_note(
                                    format!("Ответ готов · {} нано", receive.amount_nano).into(),
                                );
                            }
                            Err(e) => app.set_receive_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_receive_finalize_clicked(move |slate_id| {
            let weak = weak.clone();
            let wallet = wallet.clone();
            let slate_id = slate_id.to_string();
            if let Some(app) = weak.upgrade() {
                app.set_receive_busy(true);
                app.set_receive_error("".into());
            }
            let generation = wallet.context_generation();
            thread::spawn(move || {
                let result = wallet.finalize_receive(&slate_id);
                let snapshot = snapshot(&wallet);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = weak.upgrade() {
                        if app.get_wallet_generation() as u64 != generation {
                            return;
                        }
                        app.set_receive_busy(false);
                        match result {
                            Ok(()) => {
                                app.set_receive_note(i18n::tr("Приём завершён").into());
                                if let Ok(snapshot) = snapshot {
                                    apply_snapshot(&app, snapshot, None);
                                }
                            }
                            Err(e) => app.set_receive_error(i18n::tr(&e).into()),
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let wallet = wallet.clone();
        app.on_diagnostics_export_clicked(move || {
            let result = wallet.diagnostics_export().and_then(|text| {
                copy_text(&text)?;
                Ok(())
            });
            if let Some(app) = weak.upgrade() {
                app.set_node_note(match result {
                    Ok(()) => i18n::tr("диагностика скопирована без секретов").into(),
                    Err(error) => i18n::tr(&format!("диагностика: {error}")).into(),
                });
            }
        });
    }

    tor_node::install(&app, wallet.clone());

    settings_handlers::install(&app, wallet.clone(), pending_gate.clone());

    onboarding_handlers::install(&app, wallet.clone());

    messenger_handlers::install(
        &app,
        wallet.clone(),
        auto_process_flag.clone(),
        pending_gate.clone(),
    );

    activity_handlers::install(&app, wallet.clone(), swaps.clone(), pending_gate.clone());

    let result = app.run();
    // Process exit does not run destructors for the cloned runtime handles;
    // stop the RPC child and managed node explicitly.
    wallet.shutdown();
    result
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_data_root_lock, balance_change_warning, format_amount, is_slatepack_armor,
        node_error_action, pending_actions, pending_allowed, pending_next_action,
        profile_request_error, qr_image_size, PendingAction, PendingGate, SharedPendingGate,
    };
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    #[test]
    fn pending_rows_explain_the_next_step_without_claiming_network_confirmation() {
        assert_eq!(
            pending_next_action("send", "finalized_not_posted", false),
            "Повторить публикацию"
        );
        assert_eq!(
            pending_next_action("receive", "response_received", false),
            "Экспортировать ответ и завершить приём"
        );
        assert_eq!(
            pending_next_action("receive", "response_received", true),
            "Экспортировать ответ и завершить приём"
        );
        assert!(node_error_action("401 Unauthorized").contains("API-секрет"));
        assert!(node_error_action("error sending request").contains("подключение"));
    }

    #[test]
    fn a_full_rescan_balance_change_is_reported() {
        let warning = balance_change_warning("1452.885000000", "1451.862000000");
        assert!(warning.contains("1452.885000000"), "{warning}");
        assert!(warning.contains("1451.862000000"), "{warning}");
        assert!(balance_change_warning("1.000000000", "1.000000000").is_empty());
        assert!(balance_change_warning("bad", "1.000000000").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn data_root_lock_is_exclusive() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("flint-data-lock-{}-{stamp}", std::process::id()));
        let first = acquire_data_root_lock(&root).expect("first owner");
        assert!(
            acquire_data_root_lock(&root).is_err(),
            "the same data root must not have two owners"
        );
        drop(first);
        assert!(
            acquire_data_root_lock(&root).is_ok(),
            "lock must be released"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nano_is_formatted_with_nine_decimals() {
        assert_eq!(format_amount("1"), "0.000000001");
        assert_eq!(format_amount("999999999"), "0.999999999");
        assert_eq!(format_amount("1000000000"), "1.000000000");
        assert_eq!(format_amount("1500000000"), "1.500000000");
        assert_eq!(format_amount("123456789012345678"), "123456789.012345678");
        assert_eq!(format_amount("0"), "0.000000000");
        assert_eq!(format_amount(""), "0.000000000");
    }

    #[test]
    fn already_formatted_or_non_numeric_amounts_are_not_reinterpreted() {
        assert_eq!(format_amount("1.5"), "1.5");
        assert_eq!(format_amount("not money"), "not money");
        assert_eq!(format_amount("-1500000000"), "-1.500000000");
    }

    #[test]
    fn slatepack_armor_is_detected_but_plain_text_is_not() {
        let armor = "BEGINSLATEPACK. AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA ENDSLATEPACK.";
        assert!(is_slatepack_armor(armor));
        assert!(is_slatepack_armor(&format!("  \n{armor}\n  ")));
        assert!(!is_slatepack_armor("BEGINSLATEPACK. short ENDSLATEPACK."));
        assert!(!is_slatepack_armor("UI live e2e: default -> alice"));
        assert!(!is_slatepack_armor("BEGINSLATEPACK. no end"));
    }

    #[test]
    fn auto_targets_only_armor_from_saved_contacts() {
        let armor = "BEGINSLATEPACK. AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA ENDSLATEPACK.";
        let inbox = serde_json::json!({
            "messages": [
                {"id": "one", "from": "tgrin1known", "kind": {"text": armor}},
                {"id": "two", "from": "tgrin1unknown", "kind": {"text": armor}},
                {"id": "three", "from": "tgrin1known", "kind": {"text": "plain text"}},
                {"id": "four", "from": "tgrin1known", "kind": {"text": armor}},
                {"id": "five", "from": "tgrin1known", "kind": {"text": armor}, "reply_id": "sealed-reply"},
            ]
        });
        let contacts = serde_json::json!([{"address": "tgrin1known"}]);
        let targets = super::auto_slatepack_targets(&inbox, &contacts);
        assert_eq!(
            targets.len(),
            2,
            "оба armor от контакта должны попасть в батч"
        );
        assert!(targets.iter().all(|(_, from, _)| from == "tgrin1known"));
    }

    #[test]
    fn qr_buffer_includes_the_quiet_zone_on_both_sides() {
        assert_eq!(qr_image_size(33, 6, 4), 246);
        assert_eq!(qr_image_size(1, 1, 4), 9);
    }

    #[test]
    fn pending_commands_require_a_readable_loaded_entry() {
        let gate: SharedPendingGate = Arc::new(Mutex::new(PendingGate::default()));
        assert!(!pending_allowed(&gate, 7, "missing", PendingAction::Retry));
        {
            let mut state = gate.lock().expect("gate");
            state.generation = Some(7);
            state.loaded = true;
            state.readable = HashSet::from(["good".to_string(), "cancel".to_string()]);
            state.retryable = HashSet::from(["good".to_string()]);
            state.cancelable = HashSet::from(["cancel".to_string()]);
        }
        assert!(pending_allowed(&gate, 7, "good", PendingAction::Retry));
        assert!(!pending_allowed(&gate, 8, "good", PendingAction::Retry));
        assert!(!pending_allowed(&gate, 7, "good", PendingAction::Cancel));
        assert!(pending_allowed(&gate, 7, "cancel", PendingAction::Cancel));
        assert!(!pending_allowed(&gate, 7, "cancel", PendingAction::Retry));
        assert!(!pending_allowed(
            &gate,
            7,
            "broken-pending.json",
            PendingAction::Retry
        ));
        assert!(!pending_allowed(
            &gate,
            7,
            "broken-pending.json",
            PendingAction::Cancel
        ));
        {
            let mut state = gate.lock().expect("gate");
            state.loaded = false;
        }
        assert!(!pending_allowed(&gate, 7, "good", PendingAction::Retry));
        assert_eq!(pending_actions("send", "locked"), (false, true));
        assert_eq!(pending_actions("send", "finalizing"), (true, false));
        assert_eq!(
            pending_actions("send", "finalized_not_posted"),
            (true, false)
        );
        assert_eq!(pending_actions("send", "posted"), (false, false));
        assert_eq!(
            pending_actions("send", "failed_recoverable"),
            (false, false)
        );
        assert_eq!(
            pending_actions("receive", "response_received"),
            (false, false)
        );
    }

    #[test]
    fn profile_creation_rejects_empty_and_active() {
        assert_eq!(
            profile_request_error("default", "   "),
            Some("введите имя профиля")
        );
        assert_eq!(
            profile_request_error("default", "default"),
            Some("этот профиль уже активен")
        );
        assert!(profile_request_error("default", " alice ").is_none());
    }

    #[test]
    fn every_tr_literal_has_an_english_translation() {
        let source = include_str!("../ui/spike.slint");
        let catalog = include_str!("../lang/en_US/LC_MESSAGES/flint-slint.po");
        let mut rest = source;
        let mut checked = 0usize;
        while let Some(position) = rest.find("@tr(\"") {
            let tail = &rest[position + 5..];
            let end = tail.find("\")").expect("unterminated @tr literal");
            let msgid = &tail[..end];
            if !msgid.is_empty() {
                assert!(
                    catalog.contains(&format!("msgid \"{msgid}\"")),
                    "missing English translation for: {msgid}"
                );
                checked += 1;
            }
            rest = &tail[end + 2..];
        }
        assert!(checked > 100, "only {checked} static UI strings checked");
    }

    #[test]
    fn english_catalog_is_non_empty() {
        let catalog = include_str!("../lang/en_US/LC_MESSAGES/flint-slint.po");
        assert!(catalog.contains("\"Language: en_US\\n\""));
        assert!(
            catalog.matches("msgid \"").count() > 100,
            "catalog is incomplete"
        );
    }

    #[test]
    fn theme_muted_and_accent_dim_contrast_is_readable() {
        fn channel(hex: &str, index: usize) -> f64 {
            u8::from_str_radix(&hex[index..index + 2], 16).unwrap() as f64 / 255.0
        }
        fn lum(hex: &str) -> f64 {
            let rgb: Vec<f64> = (0..3).map(|i| channel(hex, 1 + i * 2)).collect();
            let c: Vec<f64> = rgb
                .iter()
                .map(|x| {
                    if *x <= 0.04045 {
                        x / 12.92
                    } else {
                        ((x + 0.055) / 1.055).powf(2.4)
                    }
                })
                .collect();
            0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
        }
        fn ratio(a: &str, b: &str) -> f64 {
            let (hi, lo) = if lum(a) > lum(b) {
                (lum(a), lum(b))
            } else {
                (lum(b), lum(a))
            };
            (hi + 0.05) / (lo + 0.05)
        }
        // Values mirror crates/slint-ui/ui/theme.slint.
        assert!(ratio("#8b8b95", "#18181c") >= 4.5);
        assert!(ratio("#0e0e10", "#3fbf98") >= 4.5);
    }
}
