# Вынос RPC-бэкенда в `flint-runtime`

> **IMPLEMENTATION JOURNAL:** этапы и старые ссылки на Tauri; актуальные
> границы — в [BACKENDS.md](BACKENDS.md).

**Статус:** этапы 1–5 выполнены; Tor и messenger перенесены в runtime
(`tor.rs`, `messenger.rs`, `history.rs`). **Tauri удалён 2026-10-26**; ссылки на
`crates/app` ниже исторические, backup —
`archive/archV/old-builds/tauri/tauri-removal-20261026-091749/`. Production- и
единственный UI — Slint.
Embedded не удаляется: гейт из [BACKENDS.md](BACKENDS.md#L32-L41) остаётся в
силе. Актуальные границы — в [TAURI_REMOVAL.md](../historical/TAURI_REMOVAL.md).

## 1. Граница задачи

Выносится не `crates/core/src/rpc_backend.rs` — этот адаптер уже не зависит от
UI, — а **владение** RPC-бэкендом:

- запуск/остановка/перезапуск `grin-wallet.exe`;
- соединение `RpcBackend` и его токен;
- готовность/ошибка старта и поколения;
- RPC-скан и его отмена (kill процесса);
- выбор каталога профиля и `rpc-pending`;
- диспетчеризация команд «embedded или RPC».

Цель — один lifecycle для Tauri и Slint. Tauri-команды становятся тонкими
обёртками, JSON-контракт UI не меняется. Tauri при этом не удаляется: Tor,
messenger, история и `--selftest`-ветка остаются в `crates/app`
([TAURI_REMOVAL.md](../historical/TAURI_REMOVAL.md#L33-L40), [SLINT.md../../ui/SLINT.md#L124-L129)).

## 2. Что известно по коду

- `WalletRuntime` — `Arc<Mutex<WalletService>>` + `Arc<Mutex<Option<NodeManager>>>`,
  RPC/scan/Tor явно оставлены шеллу ([runtime/src/lib.rs../../crates/runtime/src/lib.rs#L1-L6),
  [строки 86-91../../crates/runtime/src/lib.rs#L86-L91)).
- В `AppState` RPC-состояние: `backend`, `rpc`, `rpc_ready`, `rpc_error`,
  `rpc_generation`, `wallet_proc`, `scan`, `scan_generation`, `shutting_down`
  ([app/main.rs../../crates/app/src/main.rs#L82-L123)).
- RPC-ветки есть примерно у 25 команд. `wallet_init` — единственная
  неподдерживаемая (`unsupported`, [main.rs:924../../crates/app/src/main.rs#L924-L926)).
- `RpcBackend` блокирующий, без внутренней синхронизации; `&mut self` нужен для
  open/lock/send/receive/scan. Скан держит мьютекс бэкенда до 3600 с; отмена —
  только убийством процесса через отдельный `WalletProcess`
  ([rpc_backend.rs:455-463../../crates/core/src/rpc_backend.rs#L455-L463),
  [main.rs:1408-1446../../crates/app/src/main.rs#L1408-L1446)).
- `WalletProcess::launch` ждёт секрет и порт до 40 с; `scan` — до 3600 с;
  `send_over_tor` — до 180 с; `send_local` — до 60 с.
- `summary()` RPC жёстко отдаёт `scan_state: NotStarted` и `scan_error: None`;
  приложение перекрывает их состоянием `WalletService`
  ([rpc_backend.rs:1615-1640../../crates/core/src/rpc_backend.rs#L1615-L1640),
  [main.rs:1475-1483../../crates/app/src/main.rs#L1475-L1483)).
- `pending_scan` — поле `WalletService`, не `RpcBackend`; при старте RPC оно
  передаётся через `set_needs_scan`.
- В `crates/app` и `crates/runtime` нет RPC-тестов. Живые проверки —
  `#[ignore]`-тесты `crates/core/tests/wallet_rpc*.rs` и
  `tools/live-rpc-scan-cancel.mjs`.

## 3. Целевая граница

| Слой | Сейчас | После |
|---|---|---|
| `flint-core` | `WalletProcess`, `RpcBackend`, `WalletRpc` | без изменений |
| `flint-runtime` | embedded wallet/node | owner RPC-процесса, соединения, readiness, scan control, диспетчеризация |
| `flint-app` | RPC-состояние + ветки команд | IPC, события, `Secret`, Tor, messenger, история |
| `flint-slint` | embedded | тот же runtime, RPC-режим через него |

Runtime-методы возвращают общие модели `flint-core` (`WalletStatus`,
`WalletSummary`, `SendSlate`, `ReceiveSlate`, `RestoredWallet`, `ScanState`).
Оболочки добавляют только UI-специфику: `history_error`, события, JSON-обёртки.

### API-набросок

```rust
pub enum BackendKind { Embedded, GrinWallet }

pub enum RpcPhase {
    Disabled,
    Starting,
    Ready,
    Failed(String),
}

impl WalletRuntime {
    pub fn new() -> Self;                       // embedded
    pub fn new_with_backend(kind: BackendKind) -> Self;
    pub fn from_shared(wallet: Arc<Mutex<WalletService>>, kind: BackendKind) -> Self;
    pub fn backend_kind(&self) -> BackendKind;
    pub fn rpc(&self) -> &RpcRuntime;
    pub fn shutdown(&self);
}

impl RpcRuntime {
    pub fn start(&self) -> Result<(), String>;             // блокирующий, до 40 с
    pub fn wait_ready(&self, timeout: Duration) -> Result<(), String>;
    pub fn restart(&self) -> Result<(), String>;
    pub fn restart_if_dead(&self) -> Result<(), String>;
    pub fn call<T>(&self, f: impl FnOnce(&mut RpcBackend) -> Result<T>) -> Result<T, String>;
    pub fn stop(&self);
    pub fn terminate_process(&self) -> bool;
    pub fn port(&self) -> Option<u16>;
    pub fn phase(&self) -> RpcPhase;
}
```

## 4. Этапы

### Этап 0. Каркас

- `runtime/src/backend.rs` — `BackendKind`, capabilities.
- `runtime/src/rpc.rs` — `RpcRuntime` и состояние.
- `runtime/src/scan.rs` — координатор скана (этап 2).
- Гейт: `cargo check --workspace --all-targets`, `flint-selftest`,
  `cargo tree -p flint-runtime` без `tauri`.

### Этап 1. Процесс и соединение

Переносится:

- `start_rpc_once` ([main.rs:236-274../../crates/app/src/main.rs#L236-L274));
- `wait_rpc_ready` ([main.rs:276-298../../crates/app/src/main.rs#L276-L298));
- `with_rpc` ([main.rs:315-331../../crates/app/src/main.rs#L315-L331));
- `restart_rpc_backend` ([main.rs:586-672../../crates/app/src/main.rs#L586-L672));
- `restart_rpc_if_dead` ([main.rs:1385-1403../../crates/app/src/main.rs#L1385-L1403));
- RPC-часть `shutdown_children` ([main.rs:409-435../../crates/app/src/main.rs#L409-L435));
- `rpc_wallet_dir` / `rpc_journal_dir`
  ([main.rs:207-232../../crates/app/src/main.rs#L207-L232));
- `rpc_open_error` ([main.rs:302-309../../crates/app/src/main.rs#L302-L309)).

**Не объединять** `rpc: Mutex<Option<RpcBackend>>` и
`proc: Mutex<Option<WalletProcess>>` в один мьютекс: скан держит `rpc`
минутами, а отмена должна успеть взять `proc` и убить процесс.

Tauri `setup` по-прежнему запускает `spawn_blocking`, но вызывает
`runtime.rpc().start()` и после возврата шлёт прежнее событие `wallet` с
`rpc_ready`/`rpc_error` ([main.rs:2907-2945../../crates/app/src/main.rs#L2907-L2945)).
Поля `rpc`, `rpc_ready`, `rpc_error`, `rpc_generation`, `wallet_proc` из
`AppState` удаляются.

Гейт: живой `open/lock/status` через RPC; `wallet_rpc` и
`wallet_rpc_envelope` с `--ignored`; ошибка старта не переключает каталог на
embedded.

**Сделано (0.1.69):**

- новый `crates/runtime/src/rpc.rs`: `BackendKind`, `RpcPhase`, `RpcRuntime`
  (process + backend + readiness + generation, раздельные слоты для kill скана);
- `WalletRuntime` хранит backend и RPC-сессию; `new_with_backend`/`open_at`;
- Tauri `AppState` больше не хранит `rpc`, `rpc_ready`, `rpc_error`,
  `rpc_generation`, `wallet_proc`; `start_rpc_once`, `wait_rpc_ready`,
  `restart_rpc_backend`, `restart_rpc_if_dead`, `with_rpc`,
  `shutdown_children` и Tor-порт делегируют в runtime;
- проверки: `cargo test --workspace` (256 passed, 19 ignored), живой
  `runtime_starts_opens_locks_and_stops_the_rpc_child` (реальный
  `grin-wallet.exe`), `node tools/live-rpc-lifecycle.mjs` — open/lock/delete
  через Tauri IPC, `node tools/verify-ui.mjs` — без problems,
  `flint-selftest` и `flint-app --selftest` — `ok:true`.

### Этап 2. Скан

Переносится `ScanControl` ([main.rs:130-184../../crates/app/src/main.rs#L130-L184)),
`scan_snapshot`/`scan_busy_error`
([main.rs:356-384../../crates/app/src/main.rs#L356-L384)),
RPC-ветка `wallet_scan` ([main.rs:1181-1379../../crates/app/src/main.rs#L1181-L1379)),
`wallet_scan_cancel` ([main.rs:1408-1446../../crates/app/src/main.rs#L1408-L1446)).

Сохранить гонку: `busy` снимается и флаг отмены читается под одним локом;
отмена после снятия `busy` получает отказ. Для RPC — `terminate()` процесса,
затем `restart`, `ScanState::Failed`, `pending_scan` остаётся. Для embedded —
кооперативная отмена между батчами. Прогресса у RPC нет:
`scan_progress: false`.

Гейт: `node tools/live-rpc-scan-cancel.mjs`,
`node tools/live-scan-cancel.mjs`, unit-тест машины состояний
«отмена до/после settle».

**Сделано (0.1.69):**

- новый `crates/runtime/src/scan.rs`: `ScanRuntime`, `ScanView`, слот
  `busy`/`cancel`/`done`/`target`, поколение скана; embedded-скан и RPC-скан
  (kill процесса) в одном месте;
- Tauri `AppState` больше не хранит `scan`/`scan_generation`;
  `wallet_scan`/`wallet_scan_cancel` — тонкие обёртки, `wallet_status` берёт
  `done`/`target` и `scan_cancellable` из runtime;
- живой `node tools/live-rpc-scan-cancel.mjs`: кнопка скана в UI, прогресс,
  подтверждение отмены, `scan_state: failed`, `pending_scan` сохранён, backend
  перезапущен и кошелёк снова открылся, повторный скан/отмена прошли;
- живой `node tools/live-scan-cancel.mjs` для embedded: отмена на границе
  батча, `failed`, retry-скан до `completed`;
- побочно: UI `refresh` сохраняет последний `node` из `node_info`; без этого
  `wallet_status` RPC (намеренно без node RPC) отключал кнопку скана;
- `cargo test --workspace` — 256 passed, 19 ignored; новый
  `scan_runtime_starts_idle_and_refuses_a_stray_cancel`.

### Этап 3. Backend-neutral команды

- **3a. Чтение/lifecycle:** status, create/open, lock, summary, address, rotate,
  profiles, activity, pending; `backend_capabilities` остаётся в app.

**Сделано (3a, 0.1.69):**

- новый `crates/runtime/src/wallet.rs`: `status`, `create`, `open`, `lock`,
  `summary`, `address`, `rotate_address`, `profiles`, `activity`, `pending`;
  `RuntimeStatus`, `OpenOutcome`, `RuntimeProfiles`, `PendingList`;
- Tauri-команды `wallet_status/init/open/lock/summary/address/
  rotate_address/profiles/activity/pending` больше не содержат RPC-веток —
  только IPC-обвязку, `history_error` и `Secret`;
- RPC-open перенёс mainnet-гейт, конверт seed, `needs_scan`, импорт старого
  `rpc-pending`; embedded-open — `open_wallet_and_reconcile`;
- живой прогон на testnet: RPC `open` → `wallet_summary`/`wallet_address`/
  `wallet_activity`/`wallet_pending`/`wallet_profiles` → `rotate_address` →
  `lock` → `delete`;
- `cargo test --workspace` — 256 passed, 19 ignored; живой
  `runtime_starts_opens_locks_and_stops_the_rpc_child` теперь проверяет и
  `open/status/address/profiles/activity/pending/lock` через runtime.

- **3b. Деньги:** send_prepare/finalize/retry, send_online/local (только RPC),
  receive_prepare/finalize, cancel. RPC-кортежи маппить в `SendSlate` /
  `ReceiveSlate`; для `receive_finalize` RPC сохранить no-op.

**Сделано (3b, 0.1.69):**

- `crates/runtime/src/wallet.rs` получил `prepare_send`/`finalize_send`/
  `prepare_receive`/`finalize_receive`/`retry_post`/`cancel_pending`,
  `send_online`/`send_local` и `ensure_node_ready`; RPC-кортежи маппятся в
  `SendSlate`/`ReceiveSlate`, `receive_finalize` RPC остаётся no-op;
- Tauri-команды денег — тонкие обёртки; `has_rpc`/`with_rpc` в них больше нет;
  `send_online` получает SOCKS-адрес лениво, после проверок backend/scan/node;
- живой testnet-прогон: `send_prepare` и `send_local` без средств дают
  `NotEnoughFunds`, `receive_prepare` с мусором — ошибку расшифровки,
  `cancel`/`retry_post` с неизвестным slate — ошибки; открытие/замок/удаление
  после этого работают;
- `cargo test --workspace` — 256 passed, 19 ignored; `flint-selftest` и
  `flint-app --selftest` — `ok:true`.

- **3c. Seed/envelope/delete/restore:** `seed_status`, `change_password`,
  `migrate_seed`, `delete_wallet`, `restore_rpc`. Здесь mainnet-гейт
  `ensure_mainnet_ready`, запись конверта до создания БД,
  `remove_verified_seed`, перенос каталога в `trash/<network>`.

**Сделано (3c, 0.1.69):**

- новый `crates/runtime/src/seed.rs`: `seed_status`, `change_password`,
  `migrate_envelope`, `disable_envelope`, `repair_envelope`, `delete_wallet`,
  `restore`; `MigrateOutcome`, `RestoreOutcome`;
- RPC-restore пишет конверт до `create_wallet`, запускает пустой процесс,
  проверяет адрес и удаляет транзитный seed; embedded-restore отдаёт
  `history_key: true`, RPC — `false`, как раньше;
- Tauri-команды seed/restore/delete — обёртки; `wallet_delete_wallet` в app
  останавливает Tor перед RPC-удалением, остальное делает runtime;
- найден и исправлен живой дефект: `RpcBackend::change_password` отправлял
  `token` в params, upstream ждёт 3 аргумента `(name, old, new)`; смена пароля
  RPC теперь проверена живьём;
- живой прогон: legacy seed → смена пароля → миграция в envelope → open →
  lock → disable envelope → удаление; отдельный live-тест restore из
  сгенерированной фразы, envelope-status и удаление;
- `cargo test --workspace` — 256 passed, 19 ignored; `flint-selftest` и
  `flint-app --selftest` — `ok:true`.

Tauri-обёртки сохраняют имена команд и JSON: `history_error` добавляет app,
`wallet_pending` — `count/unreadable`, `wallet_open` — `reconciled`.
Messenger-идентичность ([messenger.rs:605-626../../crates/app/src/messenger.rs#L605-L626))
переводится на runtime-метод `identity_seed()`, чтобы убрать `with_rpc` из
`messenger.rs`.

Гейт: полный `cargo test --workspace`; `verify-ui` create/open/lock/delete;
живые RPC send/receive на testnet; `wallet_rpc_envelope` и
`wallet_rpc_seedless`.

### Этап 4. Switch, settings, shutdown, Tor

- `apply_switch` ([main.rs:437-584../../crates/app/src/main.rs#L437-L584)):
  остановка RPC → смена настроек → новый `WalletService` → новый
  `NodeManager` → старт RPC. Чтобы Tauri не имел второго `NodeManager`,
  добавить `WalletRuntime::from_shared(wallet, node, shutting_down)`.
- `settings_set`: live `set_min_confirmations` и перезапуск RPC при смене
  `node_url` ([main.rs:2487-2537../../crates/app/src/main.rs#L2487-L2537))
  переносятся в runtime.
- `shutdown`: один флаг `shutting_down` в runtime.
- Tor остаётся в app, но получает `rpc_port()` и `rpc_slatepack_seed(index)`.
  Любой `restart_rpc` меняет порт: `TorListener` нельзя оставлять на старом
  порту. Это проверяется отдельно.

Гейт: живой endpoint-switch, graceful stop без процессов, смена сети/профиля.

**Сделано (4, 0.1.69):**

- `WalletRuntime::from_shared(wallet, node, backend)` — Tauri и runtime делят один
  `NodeManager`;
- `switch` в runtime: остановка RPC/скана/ноды, смена сети/профиля, новый
  `WalletService` поверх того же data-root, rebuild ноды, автостарт и запуск RPC;
  app `apply_switch` — тонкая обёртка (Tor и ключ истории остаются в app);
- `update_settings` в runtime: live `min_confirmations`, rebuild менеджера ноды,
  перезапуск RPC при смене `node_url` с `node_restarted`/`restart_error`;
- Tor: `rpc_slatepack_seed`/`messenger_seed` в runtime, app больше не использует
  `with_rpc`; Tor гасится перед switch/restore/delete/scan-cancel и при смене
  endpoint; Slint вызывает `wallet.shutdown()` после `app.run()`;
- живой прогон: `settings_set` min_conf=5, смена `node_url` (restart RPC и
  возврат), `wallet_switch` default→alt→default с reopen; live RPC lifecycle;
- `cargo test --workspace` — 256 passed, 19 ignored, включая
  `runtime_owns_embedded_switch_and_settings_rebuild`.

### Этап 5. Slint RPC

- `flint-slint --backend=grin-wallet` и `FLINT_BACKEND`.
- `WalletRuntime::new_with_backend`.
- В статусе — `mode`, `scan_state`/`scan_error`, `needs_scan`,
  `scan_cancellable`; сейчас они не читаются
  ([slint main.rs:44-53../../crates/slint-ui/src/main.rs#L44-L53)).
- Unlock RPC через runtime, summary/activity/address, скан и отмена, pending
  retry/cancel, send/receive slatepack, seed-envelope, delete.
- messenger уже в runtime, но online-send и `send_local` в Slint ещё не
  портированы; capabilities должны говорить `tor: true`, `scan_progress:
  false`.
- `package-slint.ps1` кладёт `resources/wallet/grin-wallet.exe`; RPC-режим
  проверен из распакованного preview.

Гейт: UIA-прогон `flint-slint --backend=grin-wallet`: unlock → summary → lock →
scan cancel; отдельно — отсутствие регрессии embedded.

**Сделано (5, 0.1.69):**

- `flint-slint` понимает `--backend=grin-wallet` и `FLINT_BACKEND`; RPC-процесс
  стартует в фоне после окна, unlock ждёт готовности;
- статус показывает `grin-wallet`, `ScanState` управляет кнопкой скана;
  `snapshot` использует `WalletRuntime::status`, create/open/scan идут через
  runtime (embedded create для RPC отказывает), scan cancel вызывает
  `ScanRuntime::cancel`;
- UIA RPC-прогон: окно → пароль → статус `testnet · кошелёк открыт · grin-wallet`,
  баланс/адрес, «Сканировать» → «Отменить скан» → кошелёк заблокирован
  (RPC restart); после закрытия `grin-wallet` не остался;
- UIA embedded-регрессия: unlock, адрес, скан с прогрессом
  `260026 / 3309055`, «Отменить скан», кошелёк остаётся открытым;
- `cargo test --workspace` — 256 passed, 19 ignored; `flint-selftest` — ok.

### Этап 6. Тонкий Tauri и уборка

- Удалить из `AppState` RPC-поля и хелперы; в `crates/app` не должно остаться
  `RpcBackend`, `WalletProcess`, `has_rpc`, `with_rpc`, `start_rpc_once`,
  `restart_rpc_backend`.
- Обновить [TAURI_REMOVAL.md](../historical/TAURI_REMOVAL.md#L33-L40),
  [SLINT.md../../ui/SLINT.md#L124-L129), `STATUS.md`.
- Гейт: `rg` по app без RPC-символов; `cargo test --workspace`;
  `flint-selftest`; `verify-ui`; release preview Slint.

**Сделано (6, 0.1.69):**

- `AppState` не содержит `rpc`/`wallet_proc`/`rpc_ready`/`rpc_error`/
  `scan`/`scan_generation`; в `crates/app` нет `RpcBackend`, `WalletProcess`,
  `with_rpc`, `has_rpc`, `start_rpc_once`, `restart_rpc_backend`;
- Tor вынесен в `TorRuntime`: listener, start/stop/status/bridges/log,
  остановка на switch/settings/rotate/delete/scan-cancel/shutdown; Tauri-
  команды стали тонкими;
- messenger и ключ истории вынесены в `messenger.rs`/`history.rs`; Tauri-
  команды только `spawn_blocking`, живой `live-history-recovery.mjs` прошёл;
- messenger-экран Slint покрывает status/send/inbox/history/contacts/blocked;
  удаление Tauri ждёт финальной приёмки и явного согласия, см.
  [TAURI_REMOVAL.md](../historical/TAURI_REMOVAL.md);
- `cargo test --workspace` — 256 passed, 19 ignored; `flint-selftest`,
  `flint-app --selftest`, `verify-ui` — без проблем; Slint RPC UIA — см. этап 5.

## 4.1. Постревью 2026-10-26

Закрыты приоритеты из `FLINT_SLINT_TRANSITION_REVIEW`:

- `format_amount` — целочисленный nano→GRIN (`u128`, без float), тесты на 1
  nano/1 GRIN/дробные/отрицательные; это исправляет прежний показ
  `1 nano = 1.000000000 GRIN`;
- Slint показывает состояние баланса (`проверен`/`неизвестен до скана`/`кеш`),
  фазу ноды и `scan_state`/`scan_error`; `Отправить` блокируется, пока баланс
  неизвестен/не проверен или нода не готова;
- в Slint добавлена кнопка «Заблокировать», очистка пароля после unlock и
  секретных полей при закрытии модалок; create обновляет snapshot;
- смена пароля в Slint идёт через `change_password_with_history`: ключ истории
  перевязывается до смены seed-пароля, `.next` сохраняется при сбое;
- `switch`/`update_settings` запрещены во время скана; публикация RPC-сессии
  сериализована commit-lock и повторно проверяет generation/shutting_down;
- живой сценарий закрытия Slint во время RPC-скана: после закрытия нет
  `grin-wallet`/`flint-slint`;
- `package-slint.ps1` кладёт `grin-wallet.exe`; RPC-режим проверен UIA из
  распакованного preview; `flint-selftest` из пакета — `ok:true`.

## 5. Риски

| Риск | Мера |
|---|---|
| Блокировка UI на старте RPC (до 40 с) | runtime-методы блокирующие; Tauri — `spawn_blocking`, Slint — `thread::spawn`; `status` не ждёт старт |
| Скан держит `rpc` минутами | status читает `scan_busy` и состояние из `WalletService`, не беря `rpc`; отмена берёт только `proc` |
| Поздний результат после switch | `scan_generation`/`rpc_generation` переезжают вместе с кодом |
| Ошибка старта не должна открывать embedded-каталог | `BackendKind` фиксируется на процесс; `mode: "grin-wallet"` даже при `Failed` |
| Два владельца процесса | миграция атомарная: слоты только в runtime, app-поля удаляются в том же срезе |
| Секреты: токен, `wallet.seed`, пароль | `RpcBackend`/`WalletProcess` зануляют на Drop; runtime не логирует секреты |
| Строковые ошибки (`"занят сканированием"`, `"timed out"`) | сохранить маппинг в runtime; строки — только на IPC-границе |
| Tor после restart RPC | остановить/перезапустить listener при смене порта; проверить живым тестом |
| Нет RPC-тестов в app/runtime | unit-тесты состояний + `#[ignore]` live-тесты runtime; скрипты остаются |

## 6. Открытые решения

1. **Выбор бэкенда:** env/CLI на этот срез или persisted-настройка?
   Рекомендация: env/CLI, потому что смена бэкенда меняет каталог.
2. **Tor:** решено перенести в runtime; `TorRuntime` владеет listener и
   гасит его на restart RPC/switch/rotate/delete/scan-cancel.
3. **Trait `WalletBackend`:** не вводить в этом выносе; по
   [BACKENDS.md](BACKENDS.md#L32-L41) это отдельный гейт 0.2.0.
4. **Ошибки:** typed `RuntimeError` внутри, строки на границе.
5. **Порядок:** этап 1 без изменения поведения Tauri; этап 2 — скан; затем
   деньги и seed.

## 7. Критерий готовности

`flint-runtime` умеет полный RPC-lifecycle без единого типа Tauri:
старт/ожидание/restart/stop процесса, open/lock/status/summary/address/activity/
pending/send/receive/scan/scan-cancel/delete, переключение сети и профиля,
shutdown. Tauri-команды — тонкие обёртки с прежним JSON; Slint открывает
RPC-кошелёк и отменяет RPC-скан через тот же runtime. Tauri ещё не удалён:
Tor, messenger и `--selftest`-ветка остаются.
