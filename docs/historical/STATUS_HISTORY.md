# Flint 0.1.69 — фактическое состояние

> **HISTORICAL:** журнал версий 0.1.x до удаления Tauri (2026-10-26). Ссылки на `crates/app`, `ui/`, `--selftest`, `verify-ui` и Tauri IPC — исторические. Актуальный статус — [STATUS.md](../status/STATUS.md).


> **DEV BUILD. НЕ АУДИРОВАН. НЕ ДЛЯ РЕАЛЬНЫХ СРЕДСТВ.**
>
> **Tauri удалён 2026-10-26**: единственный UI — `crates/slint-ui`; RPC, Tor и
> messenger живут в `flint-runtime`. Backup: `backup/tauri-removal-20261026-091749/`.
> Исторические разделы ниже могут упоминать Tauri как прежнюю оболочку.
> UIA/DPI smoke 2026-10-26: DPI 96 (100%), имена контролов и Tab-порядок есть,
> offscreen-контролов 0, Unicode-контакт отрисован; 125/150%, NVDA и IME ещё
> не проверены. Экран «Получить GRIN» исправлен: тело в ScrollView, QR 120×120,
> кнопки закреплены снизу, добавлена синхронизация ноды (высота/фаза/пиры);
> исправлен обрез QR: quiet zone входила в размер как 2×quiet вместо
> 2×quiet×scale — теперь QR целиком, тест `qr_buffer_includes_the_quiet_zone`.
> Пароль: Enter/Ctrl+V (латиница и русская раскладка), `проверяю пароль…`,
> ошибка сбрасывается при новом вводе; нода: `stdin=null` при spawn устранил
> `ERROR_INVALID_HANDLE` после stop/start, тест restart-after-stop проходит.
>
> Дата аудита: **2026-10-25**, обновлено для 0.1.69. Проверено:
> `cargo test --workspace` — **225 тестов, 0 падений** (Windows, Rust 1.99.1); ещё 16 наборов помечены `#[ignore]` и требуют живой цепи/сети; отдельным прогоном с фичей `crash-injection` — kill-дрилл миграции M0–M5.
>
> В рабочем дереве после 0.1.68 уже есть Argon2id-конверт seed: см. разделы
> «Фаза 3, срез 1–2», «срез 3», «срез 4» и «срез 5» — RPC-бэкенд на конверте,
> mainnet-гейт, миграция legacy, ревью/крэш-дрилл и Argon2id для ключа истории.
> Артефакты `dist/` 0.1.69 собраны (portable zip/single, source zip, SBOM, build-info,
> SHA256SUMS), но **не подписаны**; релизный статус — dev, не для денег.
>
> Лицензия проекта — **GPL-3.0-only** (`COPYING`, `THIRD_PARTY_NOTICES.md`);
> source archive обязана сопровождать бинарники.
> Живые прогоны на testnet: отправка и приём (включая идемпотентный повтор),
> локальный режим, восстановление из фразы со сканом и подтверждённым балансом,
> отмена RPC-скана с перезапуском backend, кооперативная отмена embedded-скана
> между батчами, смена endpoint с перезапуском RPC backend, graceful stop с
> RPC/нодой и реальным Tor, Tor-прокси (owner API закрыт, foreign
> проходит), прямой обмен сообщениями через два реальных onion. [`tools/verify-ui.mjs`../../tools/verify-ui.mjs) на
> 0.1.69: 5 пунктов меню, CSP-нарушений нет, внешняя навигация заблокирована,
> скриншот `shots/ui-check.png`. `--selftest` 0.1.69 — exit 0, 7,5–10,3 с, кошелёк
> создан/переоткрыт, нода стартовала и остановилась, Janus завершил своп.
> **Скорость первого кадра:** первый `wallet_status` больше не ждёт node RPC.
> На release без запущенной ноды фазы: `state ready +2 мс`, `window created
> +268 мс`, `ui ready +333 мс`. `tools/verify-ui.mjs` с учётом polling:
> **1,22 с** против **6,18 с** до оптимизации. RPC `grin-wallet` теперь
> стартует в фоне после окна; unlock/Tor ждут готовности, а не блокируют first
> paint. `node_info` вызывается после splash и не держит wallet-lock.
>
> **RPC-сессия, скан и команды в runtime (этапы 1–5):** процесс `grin-wallet`,
> соединение, readiness и поколение принадлежат `flint-runtime`
> (`crates/runtime/src/rpc.rs`); управление сканом и отмена — там же
> (`crates/runtime/src/scan.rs`). Tauri `AppState` больше не хранит
> `rpc`/`wallet_proc`/`rpc_ready`/`rpc_error`/`scan`/`scan_generation`.
> Живой прогон `crates/runtime/tests/rpc_lifecycle.rs -- --ignored` — старт,
> open, address, lock, stop на реальном `grin-wallet.exe`;
> `node tools/live-rpc-lifecycle.mjs` — open/lock/delete через Tauri IPC;
> `node tools/live-rpc-scan-cancel.mjs` — кнопка скана, прогресс, отмена,
> `scan_state: failed`, `pending_scan` сохранён, backend перезапущен, повторный
> скан; `node tools/live-scan-cancel.mjs` — embedded-отмена и retry. Побочно
> исправлено: UI `refresh` сохраняет `node` из `node_info`, иначе RPC-кнопка
> скана оставалась disabled. Backend-neutral чтение тоже в runtime
> (`crates/runtime/src/wallet.rs`): живой прогон RPC `open → summary → address →
> activity → pending → profiles → rotate → lock → delete`. Деньги тоже в runtime
> (`wallet.rs`): живой прогон `send_prepare`/`send_local` без средств →
> `NotEnoughFunds`, `receive_prepare` с мусором и `cancel`/`retry_post` с
> неизвестным slate → ошибки. Seed-lifecycle в `seed.rs`: живой прогон
> change-password → migrate → open → disable → delete; отдельный live-test
> restore из фразы. Найден и исправлен дефект `RpcBackend::change_password`
> (лишний token в params). Switch/settings/shutdown в runtime: живой прогон
> `min_confirmations=5`, смена `node_url` с restart RPC и возвратом,
> `wallet_switch` default→alt→default с reopen; Tauri и runtime делят один
> `NodeManager`. Tor-listener теперь в runtime (`crates/runtime/src/tor.rs`):
> start/stop/status/bridges/log, остановка на switch/settings/rotate/delete/
> scan-cancel/shutdown; Tauri-команды только сериализуют результат. Живой
> `#[ignore]`-тест runtime start/stop Tor прошёл; Slint Settings запускает и
> останавливает Tor, UIA видит onion и `bootstrap 100%`.
> Slint RPC-режим: UIA `flint-slint --backend=grin-wallet` — unlock, статус
> `testnet · кошелёк открыт · grin-wallet`, баланс/адрес, scan → cancel →
> locked, после закрытия процессов `grin-wallet`/`flint-slint` нет; embedded UIA:
> unlock, скан с прогрессом `260026 / 3309055`, cancel, кошелёк открыт.
> По ревью Slint: `format_amount` теперь целочисленный nano→GRIN (тесты на
> 1 nano/1 GRIN/дробные), Slint показывает `баланс проверен`/`неизвестен`/`кеш`,
> фазу ноды и состояние скана, `Отправить` блокируется при неизвестном балансе;
> добавлена кнопка «Заблокировать» с очисткой пароля, create обновляет snapshot,
> смена пароля перевязывает ключ истории через runtime; switch/settings
> запрещены во время скана; публикация RPC-сессии атомарна (commit-lock,
> повторная проверка generation/shutting_down); закрытие во время скана
> проверено — дочерних процессов не остаётся. Preview
> `dist/Flint-0.1.69-slint-preview.zip` теперь содержит
> `resources/wallet/grin-wallet.exe`, RPC-режим проверен из распакованного
> пакета, вместе с `resources/tor/expert` (UIA: запуск/остановка Tor, onion,
> `bootstrap 100%`); Slint Settings также сохраняет мосты и показывает `tor.log`
> (UIA: `obfs4: клиент найден`, строки `Bootstrapped`). Messenger тоже в runtime
> (`crates/runtime/src/messenger.rs` + `history.rs`): status, send/inbox,
> history/conversations, contacts, blocked, recovery и ключ истории; Tauri-
> команды стали тонкими. Живой `live-history-recovery.mjs` прошёл; Slint-экран
> «Сообщения» показывает адрес/ключ истории/relay, отправку, входящие,
> контакты, blocked и recovery (UIA: сохранение контакта, блокировка).
> RPC-поля, хелперы, Tor и messenger-команды из Tauri
> удалены; остались финальная приёмка и удаление самого Tauri (нужно явное
> согласие); границы — [RPC_EXTRACTION.md](../architecture/RPC_EXTRACTION.md).

## Как читать этот статус

| Метка | Значение |
|---|---|
| ✅ **проверено** | есть тест или прогон, который падает при поломке |
| ⚠️ **код есть, не проверено** | путь исполняется, но end-to-end никто не проходил |
| ❌ **нет / сломано** | функции нет либо она делает не то, что обещает UI |

Скриншоты, demo-бэкенд и «зелёные тесты» сами по себе не переводят пункт в ✅.

## 1. Проверено запуском

| Что | Чем подтверждено |
|---|---|
| Создание кошелька: 32 байта → 24 слова, пароль ≥ 8 символов | `crates/core/tests/lifecycle.rs::wallet_lifecycle_offline` |
| Переоткрытие с тем же паролем даёт тот же адрес; неверный пароль отвергается | там же |
| Настройки, смена сети, перенос дефолтного порта | `settings_roundtrip`, `network_switch_moves_default_node_port` |
| Порты по сети (testnet 13413 / mainnet 3413) | `node.rs::ports_follow_network` |
| Машина состояний Janus (7 сценариев) | `crates/janus` |
| Запуск и остановка настоящего `grin.exe` | `tests/node_manager.rs` |
| Идентификаторы pending: только канонический UUID, обход каталога отвергается | `service.rs::pending_ids_must_be_canonical_uuids`, `pending_access_rejects_traversal_before_touching_disk` |
| Pending: атомарная запись, перезапись без `.tmp`-мусора, идемпотентное удаление | `pending_survives_an_atomic_round_trip` |
| Входящий slate нельзя «отменить» | `incoming_slates_cannot_be_cancelled` |
| `node_info` не выдаёт ответ RPC за синхронизацию | `node_info_never_claims_sync` |
| `--selftest` не трогает `FLINT_DATA_DIR` и убирает за собой | прогон на каталоге-приманке: 4 файла, хэши до/после совпали, временных каталогов не осталось |
| Окно рендерится под строгим CSP, IPC доступен, внешняя навигация блокируется | `tools/verify-ui.mjs`: 4 пункта меню, `invoke` — функция, CSP-нарушений нет, `location.href` остался `http://tauri.localhost/` |
| Состояние транзакции: старые записи, запрет отмены после подписи, возобновление с публикации | `legacy_records_get_a_state_from_their_kind`, `signed_transactions_cannot_be_cancelled_or_blank_retried`, `finalizing_an_already_signed_transaction_resumes_at_the_broadcast` |
| Прямой обмен сообщениями через реальный Tor: два onion, два gateway, оба направления | `direct_onion::two_wallets_exchange_directly_over_onion` — 40,77 с, включая bootstrap |
| Публикация gateway приложением: `TorListener` → onion → прямой конверт | `tor_listener_gateway::tor_listener_publishes_a_gateway_reachable_by_direct_onion` — 37,54 с |

## 2. Код есть, но end-to-end не проверено

- `prepare_send` → slatepack → `finalize_send` → `post_tx`; `prepare_receive` →
  ответный slatepack → `finalize_receive`; `summary`, `activity`, `list_pending`.
- Отмена: вызов `Owner::cancel_tx` и разблокировка входов проверены только на
  уровне кода и отказа для входящих — на живой ноде не прогонялись.
- Mainnet: проверялся запуск ноды и формат адреса, реальных транзакций не было.
- Поведение при недоступной/несинхронизированной ноде: офлайн-фолбэк есть, на
  живой цепи не проверялся.

Ни один из этих путей не покрыт тестом: 74 юнит-теста ядра не двигают деньги.

## 3. Исправлено в фазе 0

Все пункты — из аудита предыдущей ревизии; каждый закрыт кодом, а где возможно —
тестом.

| # | Что было | Что стало |
|---|---|---|
| 3.1 | «Отмена» удаляла JSON-запись, выходы оставались `locked` навсегда | [`cancel_pending`../../crates/core/src/service.rs#L646) вызывает `Owner::cancel_tx(None, None, Some(uuid))` и удаляет запись только после успеха; для входящих отмена запрещена |
| 3.2 | `tx_lock_outputs` до `save_pending`: крэш оставлял заблокированные деньги без следа | Запись пишется до блокировки ([service.rs:511../../crates/core/src/service.rs#L511)); при ошибке блокировки — `cancel_tx` и удаление записи. На приёме — тот же порядок |
| 3.3 | Битый адрес молча давал незашифрованный slatepack | Невалидный адрес — ошибка; plaintext только по явному `manual`; ответ несёт `encrypted:false`, UI показывает предупреждение ([service.rs:462../../crates/core/src/service.rs#L462)) |
| 3.4 | `slate_id` из IPC склеивался в путь: `../../settings` удалял `settings.json` | [`pending_path`../../crates/core/src/service.rs#L709) принимает только канонический UUID ([`parse_slate_id`../../crates/core/src/service.rs#L805)); запись — tmp + `sync_all` + rename ([service.rs:720../../crates/core/src/service.rs#L720)) |
| 3.5 | `csp: null`, IPC открыт любому скрипту, навигация не ограничена | Строгий CSP ([tauri.conf.json:13../../crates/app/tauri.conf.json#L13)), окно создаётся в коде с `on_navigation` ([main.rs:607../../crates/app/src/main.rs#L607)) |
| 3.6 | `--selftest` брал `FLINT_DATA_DIR` и делал `remove_dir_all` | Свой каталог `flint-selftest-<uuid>` с маркером ([main.rs:421../../crates/app/src/main.rs#L421)); удаляется только помеченный ([main.rs:436../../crates/app/src/main.rs#L436)) |
| 3.7 | `synced = true` при любом ответе `get_tip` | [`synced = false`../../crates/core/src/service.rs#L218) до появления настоящего статуса; индикатор в UI жёлтый с пояснением |
| 3.8 | «Сбросить демо-данные» показывалась в нативном приложении | Кнопка рендерится только в demo-режиме ([settings.js../../ui/js/views/settings.js)) |

### Фаза 1, срез 1 — журнал состояний и повторная публикация

| Что | Как сделано | Чем проверено |
|---|---|---|
| Состояние транзакции переживает крэш | [`TxState`../../crates/core/src/service.rs): `prepared → locked → response_received → finalized_not_posted → posted`, пишется вместе с pending после каждого шага | `legacy_records_get_a_state_from_their_kind` — старые записи без поля восстанавливают состояние из `kind` |
| Подписанная транзакция не подписывается второй раз | [`finalize_send`../../crates/core/src/service.rs) при `finalized_not_posted` уходит в `retry_post`, а не в `finalize_tx` | `finalizing_an_already_signed_transaction_resumes_at_the_broadcast` |
| Повторная публикация без повторной подписи | [`retry_post`../../crates/core/src/service.rs) берёт подписанную сделку из хранилища Grin (`get_stored_tx`), а не из нашего JSON; команда `wallet_retry_post` + кнопка «Опубликовать» в UI | `signed_transactions_cannot_be_cancelled_or_blank_retried` |
| Отмена запрещена после подписи | `cancel_pending` отказывает для `finalized_not_posted` и `posted` | там же |
| Состояние видно в UI | `PendingView.state`, заголовки строк и набор кнопок зависят от состояния | проверено рендером demo-экрана с pending-строкой (`?pending=1`) |

### Фаза 1, срез 2 — исправления по аудиту 0.1.6

| Дефект | Исправление | Чем проверено |
|---|---|---|
| Крэш между `finalize_tx` и записью о подписи оставлял `locked` | [`Finalizing`../../crates/core/src/service.rs) пишется **до** `finalize_tx`; после крэша возобновление идёт в `retry_post`, повторная подпись недостижима | `finalizing_records_are_probed_not_re_signed` |
| При неудачной отмене pending удалялся | Удаление только при успешном `cancel_tx`; иначе запись переходит в `failed_recoverable` и остаётся | `failed_recoverable_records_stay_visible` |
| `retry_post` не проверял, не принята ли транзакция раньше | Перед публикацией читается tx-log кошелька: если сделка уже `confirmed`, запись закрывается без повторной отправки | — (нужен живой узел) |
| Нечитаемые pending-файлы пропускались молча | `pending_problems()` → поле `unreadable` в IPC → тост в UI; файлы остаются на диске | `broken_pending_files_are_reported_not_dropped` |
| Неоднозначное состояние ждало ручного клика | `reconcile_pending()` вызывается при разблокировке (`open_wallet_and_reconcile`) | `reconcile_is_a_noop_while_locked` |
| `posted` удалялся сразу после публикации | Запись остаётся до подтверждения; `reconcile_posted()` снимает её, когда tx-log показывает `confirmed` (дешёвая проверка без запроса к ноде, выполняется при выдаче pending) | `posted_records_stay_until_the_chain_confirms_them` |

Почему `posted` теперь хранится: инвариант «после крэша журнал отвечает, подписана
ли транзакция, опубликована ли и подтверждена ли» нельзя обеспечить, удаляя запись
раньше подтверждения. Журнал — источник истины для незавершённых операций;
tx-log Grin остаётся историей.

### Фаза 2, срез 1 — статус ноды

| Что | Как сделано | Чем проверено |
|---|---|---|
| «Синхронизирован» больше не выводится из RPC | [`NodePhase`../../crates/core/src/node.rs): `not_running / starting / headers / blocks / ready / unknown`; `synced` истинно только для `ready` | `phase_comes_from_the_native_status_only` |
| Фаза берётся из нативного статуса ноды | `POST /v2/owner {"method":"get_status"}` даёт `sync_status`, `current_height`, `highest_height`, `connections`; v1 REST в grin 5.5 не смонтирован | живой прогон: `header_sync`, 2 719 712 / 3 305 183 |
| Тишина лога больше не делает ноду готовой | без `sync_status` фаза всегда `unknown`, даже при ненулевой высоте | `quiet_log_with_stale_height_is_not_ready` |
| Телеметрия читает тот же лог, что пишет нода | `sync_stats`/`log_age_secs` берут 
ode.log` (stdout управляемой ноды), `grin-server.log` — только запасной вариант | `sync_stats_prefers_the_log_we_own` |
| Отправка заблокирована до готовности | [`prepare_send`../../crates/core/src/service.rs) отказывает с человеческой причиной; UI гасит и верхнюю кнопку, и быструю карточку | `sending_is_blocked_until_the_node_reports_ready` + `sendDisabled: true` в живом прогоне |
| Статус в UI привязан только к фазе | заголовок и цвет — по `NodePhase`; прогресс — из `current/highest` | живой прогон: «testnet · качает заголовки 83%», затем «качает блоки 37%» |
| Семейство `txhashset*` (PIBD, download, setup, validate) | относится к `качает блоки`, а не к «неизвестно»; высоты берутся из `completed_to_height`/`required_height` | `body_sync_family_maps_to_blocks_not_unknown` + живой прогон |

Живая проверка на синхронизирующейся testnet-ноде: индикатор показывает
«testnet · качает заголовки 83%», верхняя кнопка «Отправить» имеет `disabled`,
CSP-нарушений нет ([скриншот../assets/shots/ui-check.png)).

### Восстановление из фразы и скан — 0.1.11

| Что | Как сделано | Чем проверено |
|---|---|---|
| Импорт только в пустой каталог кошелька | проверка до любой записи; при отказе ничего не остаётся | `restore_requires_an_empty_wallet_directory`, `restore_rejects_an_invalid_phrase_without_leaving_files` |
| `wallet.seed` никогда не перезаписывается | `recover_from_mnemonic` вызывается только после проверки пустоты и валидации фразы | там же |
| Восстановление даёт тот же адрес | деривация из фразы в другом каталоге и с другим паролем | `restore_recovers_the_same_address_as_the_original_wallet` |
| Нулевой баланс ≠ пусто | флаг `pending_scan` в настройках, `summary.needs_scan`, баннер «Баланс неизвестен» в UI | тот же тест + живой рендер |
| Скан запрещён до готовности ноды | `scan_wallet` отказывает, пока фаза не `ready` | `scan_is_blocked_without_a_ready_node` |

Ограничение: у скана пока нет прогресса и отмены — `Owner::scan` в upstream
блокирующий и не прерывается. Записано в §4.

### Живой сценарий — подготовлен, не запускался

- [`tools/run-live-testnet.ps1`../../tools/run-live-testnet.ps1) — предполётные
  проверки и запуск; [`crates/core/tests/live_testnet.rs`../../crates/core/tests/live_testnet.rs) —
  сам сценарий под `#[ignore]`.
- Предохранители: нода обязана сама сообщить `no_sync` с непустым tip, цепь —
 `test`, оба каталога — не существовать. Иначе скрипт завершается, ничего не
  создавая.
- Проверено на живой ноде: `sync_status=header_sync, current=2 817 632,
  highest=3 305 191` → отказ, exit 1, каталоги не созданы.
- Сценарий: send → receive → finalize → post → ожидание подтверждения, затем
  отмена и проверка возврата в spendable.
- **Финансирование — вне кошелька.** Майнера в приложении нет и не планируется:
  сценарий предполагает уже пополненный кошелёк-отправитель. При нулевом балансе
  он останавливается с явным сообщением, а не имитирует перевод.
- Крэш-точки пока не автоматизированы — отдельный этап после первого прогона.

### Надёжность разблокировки — 0.1.13

- Кнопка «Разблокировать» показывает состояние («Разблокирую…»), блокируется от
  двойного клика и через 4 секунды без ответа пишет «Нода отвечает медленно» —
  «ничего не происходит» больше не выглядит как мёртвый интерфейс.
- Enter работает и в поле пароля, и в любом месте карточки (фокус мог уйти).
- Клик по верхней кнопке при уже открытой карточке только ставит фокус, а не
  перерисовывает экран в тот же вид.
- Необработанные ошибки UI показываются тостом, а не теряются в консоли.
- Разблокировка больше не ждёт данных: экран переключается сразу после `wallet_open`, баланс и история догружаются фоном. Замер с мёртвым узлом: **62 мс** от клика до разблокированного экрана.
- Тесты стали герметичными: три из них зависели от живого узла на порту 13413 и
  упали, как только нода синхронизировалась. Теперь они принудительно смотрят на
  закрытый порт.

### Приём и журнал публикаций — 0.1.15

| Что | Как сделано | Чем проверено |
|---|---|---|
| Входящий перевод закрывается только по цепи | `reconcile_posted` снимает `response_received`-запись, лишь когда tx-log показывает `confirmed`; ответ отправителю сам по себе ничего не подтверждает | `receives_are_reconciled_only_with_an_unlocked_wallet` |
| Неудачная публикация больше не «молчит» | в записи pending появились `attempts` и `last_error`; ошибка узла увеличивает счётчик, сохраняется и показывается в строке («попыток: N») | `failed_publish_attempts_are_visible_in_the_journal` |
| Ошибка публикации ≠ отказ | состояние остаётся `finalized_not_posted`, транзакция уже подписана; текст ошибки прямо говорит, что повтор безопасен, а дубликат отклонит сеть | там же + код `retry_post` |
### Приём slatepack — исправление 0.1.16

- `prepare_receive` больше **не** вызывает `tx_lock_outputs`: он ищет приватный
  контекст, который сохраняет только отправитель, и падал с
 `Wallet store error: DB Not Found Error: Slate id`. На приёме выход создаётся
 `receive_tx` и становится тратимым после подтверждения — блокировка не нужна.
- Приём сделан идемпотентным: если предыдущая попытка уже сохранила входящую
  сделку (но упала позже), повторный «Разобрать» берёт сохранённый ответ из
  pending и возвращает тот же slatepack вместо ошибки
 `TransactionAlreadyReceived`.
- Ограничение: сквозной приём проверен только на уровне кода и сборки — для
  живого прогона нужен реальный отправитель с монетами.
### Клиент grin-wallet owner API — 0.1.17

- [`crates/core/src/wallet_rpc.rs`../../crates/core/src/wallet_rpc.rs): ECDH-хендшейк
  (secp256k1) → общий ключ = X-координата → AES-256-GCM на каждый запрос,
  конверт `encrypted_request_v3`; Basic-аутентификация `grin:<секрет>`.
- Проверено против **реального** `grin-wallet 5.5.1`
  ([`tests/wallet_rpc.rs`../../crates/core/tests/wallet_rpc.rs), `#[ignore]`):
  хендшейк, `open_wallet`, `get_slatepack_address` (`tgrin1…`), `close_wallet` —
  0.7 с; неверный секрет отвергается с 401.
- Обёртки чтения: `summary`, `txs`, `node_height`, `scan`; учтены особенности JSON: кортежи приходят массивами `[bool, value]`, а `u64` — строками.
- Живой прогон: адрес `tgrin1…`, пустой tx-log, `node_height = 3 305 642` — 1.6 с на оба теста.
- Модуль пока не подключён к GUI: это этап 1 миграции, старый бэкенд работает.
### Бэкенд grin-wallet в GUI — 0.1.19

- Флаг `FLINT_BACKEND=grin-wallet` (или `--backend=grin-wallet`): приложение само
  запускает `grin-wallet owner_api` на свободном порту, читает `.owner_api_secret`,
  ждёт порт и подключается по зашифрованному API. Каталог:
 `<data>/<network>/wallet-rpc` (абсолютный путь — дочерний процесс резолвит `-t`
  от своего cwd).
- Через RPC уже работают: `wallet_status` (`mode: "grin-wallet"`), `wallet_open`
  (пароль → `open_wallet`, токен живёт в памяти), `wallet_lock`, `wallet_summary`,
 `wallet_activity`, `wallet_address`.
- Ещё не перенесено и падает явной ошибкой, а не тихим откатом на старый бэкенд:
  создание/восстановление кошелька, скан, отправка, приём, отмена.
- Живая проверка GUI: `mode=grin-wallet, initialized=true, unlocked=false,
  node=ready`; после `wallet_open` — `unlocked=true`, адрес `tgrin1…`,
 `spendable=0`, `txs=0`.
### Ротация адреса — 0.1.20

- Адрес выводится по индексу `m/0/1/<index>`; кнопка «Новый адрес» в окне приёма
  увеличивает индекс и сохраняет его в настройках. Сид-фраза не меняется.
- Старые адреса **продолжают принимать офлайн-slatepack**: расшифровка идёт по
  всем индексам `0..=current` (`decrypt_indices`), а не фиксированным `0..8`.
- Ограничение честно показано в UI (двухшаговое подтверждение): онлайн-приём
  через Tor-onion привязан к текущему адресу, поэтому после ротации старый onion
  перестаёт отвечать; офлайн-приём по старому адресу работает.
- Предел индексов — 63 (`MAX_ADDRESS_INDEX`), защита от бесконечного роста.
- Тест: `address_index_rotates_and_persists` (индекс переживает перезапуск,
  диапазон расшифровки включает старые адреса).
### Адрес → Tor onion — 0.1.21

- [`crates/core/src/address.rs`../../crates/core/src/address.rs): `onion_from_address`
  (`bech32` → ed25519 → base32 onion v3), `onion_key_bytes`, `address_from_key`.
- Проверено на живом `grin-wallet listen`: он поднял hidden service
 `qezlq34gn32544wwh6u23dnnuxp3t2574p3bjuxl3gmu2vmwl5zguvyd` — ровно тот onion,
  что наша функция выводит из адреса
 `tgrin1syetsmuxdm6auukk8756mrdd5h0mn6alu0mpf5htmxv564vktaeq622f2w`.
- Тест-вектор взят из upstream `ov3.rs`: ключ `d03c09…c8bb` даёт
 `2a6at2obto3uvkpkitqp4wxcg6u36qf534eucbskqciturczzc5suyid`.
- Ограничение: автоприём работает только при запущенном `listen` и разблокированном
  кошельке — onion без этого не отвечает.
### Шлюз onion → кошелёк + сообщения — 0.1.22

- [`crates/core/src/gateway.rs`../../crates/core/src/gateway.rs): локальный сервер,
  который ставится за Tor-hidden-service и маршрутизирует:
  - `POST /v2/foreign` → foreign API кошелька (платежи, автоподпись);
  - `POST /flint/v1/msg`, `GET /flint/v1/msg?to=…`, `POST /flint/v1/msg/<id>/ack`
    — сообщения с TTL (по умолчанию 26 ч, ограничен сверху конфигом);
  - `GET /flint/v1/health`.
- Конверт: `from`/`to` обязаны быть валидными slatepack-адресами (проверка через
 `onion_from_address`), payload ≤ 64 КБ, запрос ≤ 128 КБ, TTL не больше
  настроенного. Просроченные сообщения удаляются при чтении, ack удаляет сразу.
- Хранилище: память + файлы `<data>/<network>/gateway/<id>.json`, чтобы очередь
  переживала перезапуск.
- Тесты (4): проксирование в кошелёк, roundtrip+ack, истечение TTL, отказ на
  мусорных адресах.
- Пока **не включён** в приложение: следующий шаг — публикация onion из
  адресного ключа (arti) и запуск шлюза под ним.
### Onion-сервис из адресного ключа — 0.1.23

- [`crates/core/src/onion.rs`../../crates/core/src/onion.rs): `OnionService::start`
  переиспользует arti-интеграцию `grin-wallet` (`start_tor_service`), поднимая
  hidden service ровно на ключе адреса (`onion_address`).
- Живая проверка: сервис стартует (~11 с), onion совпадает с выведенным из
  адреса, в keystore появляется `ks_hs_id` для нужного onion, arti логирует
  успешную загрузку дескриптора в 5 HSDir.
- **Блокер:** клиентский запрос через Tor к этому onion всё ещё отдаёт
 `RESOLVEFAILED` (до 12 попыток за ~3 минуты), хотя публичный onion по тому же
  Tor-клиенту открывается. Значит, Tor и клиент рабочие, проблема в публикации/
  резолве нашего дескриптора; нужен отдельный разбор (логи tor_hsclient).
- Пока `OnionService` **не включён** в приложение: сначала надо закрыть блокер,
  иначе обещание «отправка по адресу через Tor» будет ложным.
### Системный Tor и ключи из адреса — 0.1.24

- `tor_hidden_service_keys` пишет ключи в формате Tor v1: тег 32 байта + ключ
  (96 байт секрет, 64 байта публичный), без контрольной суммы.
- Живой тест с официальным **Tor 0.4.9.14**: Tor принимает наши ключи и выдаёт
  ровно `ot4fzwru…a2yd.onion`, то есть onion детерминированно выводится из
  адреса и для системного Tor (не только для arti).
- Ограничение среды: rendezvous не состоялся — сам Tor пишет
 `Tried for 121 seconds to get a connection … waiting for circuit` и
 `Giving up`; клиентский `curl` через SOCKS получает «host unreachable».
  Это сетевое ограничение (intro-цепочки), а не ошибка ключей/конфига.
- Вывод: для публикации onion нужен системный Tor (arti 0.44 в `grin-wallet`
  не подходит — сам просит обновиться, а HSDir отвечает
 `Invalid HS descriptor`). Финальную проверку rendezvous делать в сети,
  где Tor-цепочки строятся.
### Секрет owner API ноды — 0.1.25

- Исходники `grin 5.5.2` подтвердили формат: `Authorization: Basic base64("grin:" + secret)`
  (`api/src/client.rs`), проверка — постоянное по времени сравнение (`api/src/auth.rs`).
- `fetch_native_status` теперь принимает секрет и отправляет заголовок; вызывающий
  код берёт `settings.node_api_secret`. Без секрета защищённая нода отвечает 401,
  и статус честно становится неизвестным (отправка блокируется).
- Тесты: заголовок уходит именно такой (stub-нода проверяет), и 401 не парсится
  как статус.
- UI-поле для секрета пока не добавлено — задаётся в `settings.json`.

### Нода: owner/foreign API без аутентификации — найдено 0.1.38, исправлено в 0.1.44/0.1.47

- `grin server config` создаёт `grin-server.toml` с путями
 `api_secret_path`/`foreign_api_secret_path`, но сами файлы не создаёт;
 `grin server -c …` минует `initial_setup_server`, где они создаются. Дальше
 `get_first_line` на отсутствующий файл возвращает `None`, и `node_apis` не
  включает Basic-auth. Нода стартует с открытыми `/v2/owner` и `/v2/foreign`,
  без предупреждения в логе.
- Проверено на живой ноде: `get_status` без `Authorization` → `200`;
 `OPTIONS` с чужим `Origin` → `200` и `Access-Control-Allow-Origin: *`;
  в `data/testnet/node` нет `.api_secret`.
- 0.1.25 добавляет только клиентскую часть (заголовок `Authorization`), но
  сервер её не требует. Это не отменяет 0.1.39/0.1.40 (Tor-прокси и ACL
  кошелька), а дополняет: у ноды своего секрета нет.
- Фикс и гейт — [ROADMAP.md, фаза 0.5](../status/ROADMAP.md).

### Исходники ноды в проекте

- `resources/src/grin-5.5.2.zip` + распакованное дерево (gitignored).
- Проверено: в ноде **нет** ни slatepack, ни Tor — автоприём slatepack делает
 `grin-wallet listen`, а не нода.
### Write-path grin-wallet — 0.1.26

- В `RpcBackend`: `send_prepare` (`init_send_tx` → `create_slatepack_message` →
 `tx_lock_outputs`), `send_finalize` (`slate_from_slatepack_message` →
 `finalize_tx` → `post_tx`), `receive_prepare` (расшифровка → foreign
 `receive_tx` → ответный armor), `scan`.
- Foreign API поднят тем же процессом (`owner_api --run_foreign`); клиент ходит
  на `/v2/foreign`. Доступные методы проверены живьём: `receive_tx` (3 аргумента),
 `build_coinbase` (1); `get_version`/`get_tip` там отсутствуют.
- GUI-команды `wallet_send_prepare`, `wallet_send_finalize`,
 `wallet_receive_prepare`, `wallet_receive_finalize`, `wallet_scan` переключены
  на бэкенд при `FLINT_BACKEND=grin-wallet`.
- Живые проверки: скан до высоты 3 305 831; `init_send_tx` на пустом кошельке
  вернул `NotEnoughFunds: needed 1.012 GRIN`; мусорный slatepack —
 `SlatepackDecryption`. То же видно через GUI.
- Не проверено: сквозной перевод (нужен пополненный RPC-кошелёк) и отмена
  (`cancel_tx` ещё не подключена).
### Отмена и CLI для RPC-кошелька — 0.1.27

- `RpcBackend::cancel` → `cancel_tx(token, null, slate_id)`; команда `wallet_cancel`
  переключена на бэкенд. Живая проверка: отмена неизвестного slate id даёт
  структурированную ошибку кошелька, а не «метод не найден».
- `crates/core/examples/wallet_cli.rs` — маленький CLI поверх RPC-бэкенда:
 `init/address/balance/receive/send/finalize/cancel`. Им удобно пополнять
  тестовый кошелёк и прогонять сквозной сценарий.
- Созданы два пустых RPC-кошелька: `.tmp/e2e-a`
  (`tgrin143vhlmauzrymda9f3ezvu6lml43ckh73a8ys907g2yvn9p0x482sjlfpu3`) и
 `.tmp/e2e-b` (`tgrin1nplnhqep6zqzku3y6m39qek9cu854d6ee0usjqrjspncegj6j2ps9a5c5g`).
  Оба с нулевым балансом: сквозной перевод ждёт пополнения.
### Кошельки и сеть на плашке статуса — 0.1.28

- Профили кошельков: настройка `wallet_name`; `None` оставляет старый каталог
 `<network>/wallet`, именованные профили живут в `<network>/wallets/<имя>`.
  Миграция не нужна, существующий кошелёк остаётся «основным».
- Сервис и RPC-супервизор используют выбранный профиль
  (`wallet_home_for` / `wallet_dir(..., profile)`).
- Команды `wallet_profiles` (список + активный) и `wallet_switch` (валидация
  имени: латиница/цифры/-/_ до 32 символов, затем перезапуск).
- Клик по плашке статуса открывает «Сеть и кошелёк»: карточки testnet/mainnet,
  список кошельков с активным, поле «Добавить» для нового профиля.
  Переключение применяется после перезапуска (кнопка «Перезапустить»).
- GUI-проверка через CDP: меню открывается, содержит сети, раздел кошельков и
  активный профиль; `wallet_profiles` отвечает.
- Ограничение: список показывает только кошельки с существующим `wallet.seed`;
  новый профиль появится в списке после создания/восстановления в нём.
### Сквозной сценарий на RPC — 0.1.29

Проведён полный цикл на testnet через официальный `grin-wallet` (RPC-бэкенд),
с реальными деньгами тестовой сети:

| Шаг | Результат |
|---|---|
| Донор (встроенный кошелёк, 1488 GRIN) → A, 2 GRIN | slatepack создан, A принял, донор опубликовал; комиссия 0.023 GRIN |
| A → B, 1.5 GRIN | `send` → приём на B → `finalize` → публикация; подтверждено |
| Балансы после | B: **1.5** spendable; A: **0.477** (сдача); донор: 1485.977 (сдача ждёт 10 подтверждений) |
| Отмена | `send` 0.2 → залокировано (spendable 0) → `cancel_tx` → spendable снова 2 GRIN |

Найдено и исправлено по пути: третий параметр foreign `receive_tx` — это `dest`
(`Option<String>`), а не `use_test_rng`; с `false` он отвечал
`InvalidArgStructure "dest"`. Теперь передаётся 
ull`, и приём работает.

Ограничение теста: для скорости подтверждения понижались до 1 (`FLINT_MIN_CONF`);
в приложении по-прежнему 10.
### Pending из tx-log и Tor-листенер — 0.1.30

- `wallet_pending` для RPC-бэкенда больше не пустой: записи строятся из tx-log
  кошелька (источник правды), не из нашего журнала. Неподтверждённые `TxSent`
  показываются как `posted`, `TxReceived` — как `response_received`.
- Исправлен расчёт суммы отправки: в tx-log `amount_debited` — сумма входов, а
 `amount_credited` — сдача; показываем `debited − credited − fee`. Раньше
  показывалось завышенное значение (0.454 вместо 0.1), теперь 0.1.
- Приём по Tor подключён к UI: `tor_start`/`tor_stop`/`tor_status`, раздел
  «Приём по Tor» в меню плашки. Ключ onion берётся из кошелька
  (`get_slatepack_secret_key`), публикуется системным `tor.exe`, наружу
  выставляется порт owner API с foreign-маршрутом.
- Живая проверка через GUI: адрес `tgrin1syets…` → опубликован onion
 `qezlq34g…`, совпадает с `onion_from_address`.
- Ограничение: rendezvous в этой сети по-прежнему не строится (Tor не может
  собрать intro-цепочки), поэтому приём *извне* проверен только по публикации.
  Кошелёк должен быть разблокирован для автоподписи.
### Отправка через Tor — 0.1.31

- [`crates/core/src/socks.rs`../../crates/core/src/socks.rs): SOCKS5 + HTTP/1.1
  клиент без внешних зависимостей (`http_post_onion`), с разбором
 `Content-Length` и `chunked`. Тесты: POST через фейковый прокси и chunked-ответ.
- `TorListener` теперь держит и `SocksPort` (свободный локальный порт), и hidden
  service — один Tor на приём и отправку.
- `RpcBackend::send_over_tor`: `init_send_tx` → `tx_lock_outputs` → POST
 `receive_tx` на onion получателя через SOCKS → `finalize_tx` → `post_tx`.
  При недоступности получателя транзакция **отменяется**, чтобы входы не
  остались заблокированными.
- UI: в окне отправки появилась кнопка «По Tor» (требует включённого приёма по
  Tor — оттуда берётся SOCKS-порт).
- **Не проверено живьём:** в этой сети Tor не строит intro-цепочки, поэтому
  сквозная отправка на удалённый onion недостижима; кроме того, получатель B
  сейчас не слушает. Проверены: SOCKS-клиент (юнит-тесты), сборка, отмена при
  ошибке (по коду). На нормальной сети сценарий нужно прогнать целиком.
### Локальный режим и правки по ревью — 0.1.32

- **Локальный режим**: `send_local` проводит онлайн-сделку через локальный
  foreign API (init → lock → `receive_tx` → finalize → post) без Tor — кнопка
  «Себе» в окне отправки. Живая проверка на A: **0.477 → 0.454 GRIN** (ушла
  только комиссия 0.023), транзакция подтверждена.
- **Явный бэкенд**: состояние хранит запрошенный бэкенд; неудачный запуск
 `grin-wallet` больше не переключает приложение на встроенный каталог, а даёт
  ошибку с причиной. Статус честно показывает выбранный режим.
- **Гейт ноды для RPC**: `ensure_node_ready` блокирует отправку, финализацию,
  онлайн/локальную отправку и скан, пока `NodePhase::Ready` не подтверждён.
- **Индекс адреса в RPC**: адрес, summary и расшифровка следуют
 `settings.address_index`; ротация обновляет бэкенд; диапазон `0..=index`
  (предел 63) вместо жёсткого `0..=15`.
- Разбор внешних ревью и план — [REVIEW_RESPONSE.md](REVIEW_RESPONSE.md).
### Журнал этапов RPC и идемпотентная отправка — 0.1.33

- `RpcBackend` хранит журнал отправки (`rpc-pending/<slate_id>.json`): этап
 `locked` / `finalized_not_posted` / `posted`, попытки и последняя ошибка.
  Запись создаётся **до** блокировки входов и удаляется только после
  подтверждения по tx-log (или отмены).
- `pending` для RPC отдаёт реальный этап из журнала, а не «posted» для любой
  неподтверждённой сделки; записи tx-log используются только для тех сделок,
  которых нет в журнале.
- `send_finalize` сверяет UUID ответа с ожидаемым (ответ другой сделки
  отвергается), не подписывает повторно уже подписанную сделку и не публикует
  дважды: при этапе `posted` возвращает успех, при `finalized_not_posted`
  публикует сохранённую сделку через `get_stored_tx`.
- `wallet_retry_post` для RPC реализован — повтор публикации без финализации.
- Встроенный приём больше не удаляет запись по «Готово»: запись живёт до
  подтверждения входящей транзакции (её снимает `reconcile_posted`).

Живая проверка на testnet (A → B, 0.05 GRIN):

```
send: journal locked, pending state=locked
finalize в отдельном процессе → posted
finalize повторно → posted (идемпотентно, без повторной подписи)
после подтверждения: A 0.3805, B 1.55, журнал пуст, pending пуст
```
### Идемпотентный приём — 0.1.34

- RPC: ответ входящего сохраняется в `rpc-pending/<slate_id>.recv.json`
  **до** шифрования в armor. Повторный разбор того же slatepack возвращает
  сохранённый ответ, а не `TransactionAlreadyReceived`.
- Если ответ всё же потерян (окно между `receive_tx` и записью), пользователь
  получает явный текст: сделка уже в кошельке, ответ восстановить нельзя,
  попросите отправителя создать новую.
- Записи входящих удаляются лениво при следующем `pending`, когда tx-log
  показывает подтверждение.
- Встроенный бэкенд: pending-запись с ответом сохраняется до armor (раньше после),
  «Готово» больше не удаляет запись — её снимает `reconcile_posted` по цепи.

Живая проверка (A → B, 0.02 GRIN):

```
receive #1 → 1383 символа ответа, .recv.json 2127 байт
receive #2 → те же 1383 символа (без повторного receive_tx)
A finalize → posted; после подтверждения: A 0.3375, B 1.57
pending у B → пусто, .recv.json удалён
```
### Переключение сети и профиля без перезапуска — 0.1.35

- `apply_switch` выполняет переход в одном порядке: стоп Tor → закрытие RPC-
  процесса и бэкенда → стоп ноды → закрытие кошелька → сохранение настроек →
  новый `WalletService` → новая нода (если включён автостарт) → новый RPC-
  процесс и подключение. Приложение не перезапускается; кошелёк остаётся
  заблокированным, пароль нового профиля вводится в обычном окне.
- Команды: `wallet_switch` (профиль) и `network_switch` (сеть) применяют
  изменение сразу. UI-меню больше не показывает «Перезапустить».
- Журналы отправок/входящих теперь привязаны к профилю
  (`<network>/pending/<profile>`; у default — прежний путь), RPC-журнал —
 `<network>/rpc-pending` на профиль.
- Список профилей в меню для RPC-бэкенда читает `wallet-rpc` и `rpc/<name>`,
  а не встроенные каталоги.

Живая проверка (CDP, один процесс):

```
default → адрес A; switch p2 → applied, locked; unlock → адрес B;
switch default → unlock → адрес A снова
testnet → mainnet: applied, network=mainnet, initialized=false, unlocked=false;
mainnet → testnet: initialized=true; unlock → тот же адрес A
```
### Состояние на профиль — 0.1.36

- `address_index` и `pending_scan` больше не живут в общих `settings.json`:
  для каждого кошелька они лежат в `<data>/<network>/state/<profile>.json`.
  У профиля default при первом запуске значения берутся из старых настроек,
  поэтому миграция не нужна.
- `set_wallet_name` при смене профиля загружает его состояние, поэтому
  переключение внутри процесса ведёт себя как свежий запуск.
- RPC-бэкенд получает индекс из выбранного кошелька, а не из общих настроек.
- Тест: два профиля независимо ротируют адрес, у каждого свой индекс; default
  сохраняет свой при возврате.
- Журналы (`pending`, `rpc-pending`) уже были разведены по профилям в 0.1.35.
### RPC: восстановление, скан, достоверность баланса — 0.1.37

- **Восстановление из фразы** (`wallet_restore` при бэкенде grin-wallet):
  каталог должен быть пустым; запускается owner API без кошелька, вызывается
 `create_wallet(name=null, mnemonic, words, password)`, процесс
  перезапускается с повторами (база может быть занята ещё мгновение). Возвращает
  адрес и 
eeds_scan: true`.
- **Скан**: `scan` больше не ограничен 30-секундным таймаутом клиента (час),
  после успеха снимает `pending_scan`.
- **Достоверность баланса**: `WalletSummary.validated` теперь отражает флаг
 `(validated, WalletInfo)` от кошелька; UI показывает «Баланс из кэша», если
  нода не подтвердила состояние. 
eeds_scan` для RPC берётся из состояния
  профиля.
- **Найдено ограничение**: после восстановления официальный кошелёк сам
  запускает скан и держит блокировку кошелька — `wallet_open` в это время ждёт
  до конца скана. Явный `scan` в RPC фактически избыточен, нужен прогресс
  (фаза 2) и неблокирующий доступ.
- Живая проверка restore: пустой профиль `fresh`, фраза A → адрес
 `tgrin143vhl…`, 
eeds_scan: true`; при открытии кошелёк начал автоскан.
### Прогресс скана и неблокирующее открытие — 0.1.38

- `WalletProcess::scan_progress()` читает `owner.log` и отдаёт процент из строк
 `Scanning - NN% complete` (строки старше 20 секунд игнорируются).
- Прогресс выводится в шапке: `testnet · скан 42%`; интерфейс сам обновляется
  каждые 5 секунд, пока скан идёт.
- `wallet_status` больше не дергает owner API для адреса, пока идёт скан.
- `wallet_open`: сначала отказ с процентом, иначе открытие с таймаутом 8 секунд;
  вместо 30-секундного зависания — понятное «кошелёк занят сканированием».
- `wallet_summary` / `activity` / `pending` во время скана отвечают сразу
  («сканирует цепь: N%»), а не висят.
- `pending_scan` снимается только после того, как скан действительно закончился
  и кошелёк открылся; тогда же `validated` отражает ответ ноды.

Живая проверка (профиль fresh3, фраза A):

```
restore → адрес tgrin143vhl…, needs_scan=true
open во время скана → ответ сразу; summary → «сканирует цепь»
scan_progress: 37 → 40 → 43 → 46 → 49 → 52 → null
после скана: needs_scan=false, validated=true, spendable=337500000 (0.3375 GRIN)
```
### Tor публикует только foreign API — 0.1.39

- Новый `foreign_proxy`: loopback-сервер, который пропускает ровно один маршрут
 `POST /v2/foreign` и отвечает `403` на всё остальное (owner API, GET, чужие пути).
  Тело ограничено 2 МБ, таймауты 30 с.
- `TorListener` больше не указывает hidden service на порт кошелька: onion ведёт на
  прокси, а owner API остаётся доступен только с localhost.
- Проверено на живом кошельке (`owner_api --run_foreign`):
 `POST /v3/owner` → 403, `GET /v2/foreign` → 403, `POST /v2/foreign` → ответ
  кошелька (200, «Wallet has not been opened» — запрос дошёл).
- Юнит-тест проверяет и маршрутизацию, и отказ; найденная при этом гонка
  (accepted-сокет наследует nonblocking на Windows) исправлена.
- Tor в этой песочнице по-прежнему не публикует дескриптор (rendezvous недоступен),
  поэтому сквозная доставка через onion не проверена — только прокси и маршруты.

### Секреты и ACL — 0.1.40

- `WalletRpc` зануляет ключ сессии AES-GCM и строку Basic-auth в `Drop`.
- `RpcBackend` зануляет keychain-токен открытого кошелька.
- `WalletProcess` зануляет секрет owner API after kill.
- Новый `win_acl`: `icacls` снимает наследование и оставляет только текущего
  пользователя. Применяется к `.owner_api_secret` (как только кошелёк его создаст)
  и к `hs_ed25519_secret_key` Tor.
- Не сделано: шифрование ответного slatepack получателю (сейчас пустой список
  получателей), зануление паролей/фразы в командах Tauri, PBKDF2-100 upstream.

### Крэш-хуки и честный статус теста — 0.1.40

- В debug-сборке появились хуки `FLINT_CRASH_AFTER_PREPARE`,
 `FLINT_CRASH_AFTER_SEND`, `FLINT_CRASH_AFTER_FINALIZE` — процесс умирает на
  границе этапа, журнал остаётся.
- Добавлен `tools/crash-test-rpc.ps1`, но автоматический прогон **не зелёный**:
  PowerShell ждёт дерево процессов и унаследованные хэндлы, а `wallet_cli`
  зависает на выходе после инъекции. Скрипт оставлен как заготовка, не как
  доказательство.
- Что проверено вручную: после краха на этапе `locked` запись видна в pending;
  подписанная, но не опубликованная сделка (`finalized_not_posted`) отменяется
  через `cancel_tx`, средства возвращаются (A: 0.3375 после отмены, потерь нет).
- Крэш-тест на живой ноде остаётся открытым пунктом.
### Крэш-раннер и найденный баг — 0.1.41

- `wallet_cli` теперь сам гасит дочерний кошелёк перед `exit(9)`; хуки в бэкенде
  возвращают спец-ошибку `CRASH_AFTER_*` вместо `process::exit`, поэтому не
  остаётся унаследованных хэндлов и PowerShell не зависает.
- `tools/crash-test-rpc.ps1` прошёл границы:
  1) крах после `send_prepare` — журнал `locked`, slatepack сохранён;
  2) крах после `finalize_tx` — журнал `finalized_not_posted`, транзакция не
     опубликована;
  3) следующий процесс опубликовал сохранённую сделку (`posted`) без повторной
     подписи.
- На шаге 3 найден и исправлен реальный баг: `post_stored` вызывал
 `get_stored_tx` с двумя аргументами вместо трёх (`token, null, slate_id`).
  До исправления продолжение после краха было невозможно.
- Финального подтверждения в цепи за ~30 минут не дождались: узел принял
  транзакцию в пул (`Pushing transaction 201e212e0a2f`, 05:28), но блок с ней не
  пришёл. Это ограничение тестовой цепи, а не вывод об успехе; средства A пока
  заблокированы этой сделкой.
### Перепубликация и зависшая цепь — 0.1.42

- `RpcBackend::republish` публикует сохранённую сделку заново с `fluff=true`,
  чтобы вывести её из stem-фазы; CLI-команда `republish`.
- Зависшая сделка `794d3c15` (kernel 201e212e0a2f) была принята в пул в 05:28 и
  переопубликована в 05:38. Подтверждения нет потому, что верхушка цепи
  остановилась на высоте **3306042** — блоки не производятся. Это ограничение
  тестовой цепи, а не вывод о коде.
- Следствие: средства A заблокированы этой сделкой (spendable 0), B — 1.57.
  Ничего не потеряно: сделка подписана и лежит в пуле; при возобновлении блоков
  она либо подтвердится, либо её можно отменить/переопубликовать.
### Embedded: хуки и подготовка крэш-теста — 0.1.43

- `embedded_cli` получил `init` (создание кошелька) и `receive` (разбор
  slatepack и ответ), поэтому пару embedded-кошельков можно поднять без GUI.
- В `prepare_send` и `finalize_send` добавлены debug-хуки
 `FLINT_CRASH_AFTER_PREPARE` / `FLINT_CRASH_AFTER_FINALIZE` (спец-ошибка),
 `FLINT_CRASH_AFTER_SEND` в CLI; дочернего процесса у embedded нет, поэтому
  выход чистый.
- Поднят и профинансирован тестовый embedded-кошелёк `tgrin10wn5…` (2 GRIN от
  донора, tx `739c540a`). На момент записи перевод ещё **awaiting**: цепь снова
  встала (3306077). Крэш-тест embedded не прогнан — ждём подтверждения.
### Фаза 0.5: API ноды закрыт — 0.1.44

- `NodeManager::ensure_api_secrets` создаёт `.api_secret` (32 случайных байта
  hex) до первого старта и проверяет путь в конфиге; ACL ставится через
 `win_acl`. `.foreign_api_secret` намеренно не создаётся: `/v2/foreign` у ноды
  read-only и нужен кошелькам, иначе они получают 401. Кошельки получают
 `node_api_secret_path` на owner-секрет (0.1.47).
- **Fail-closed перед запуском:** если конфиг не указывает на наш секрет, секрет
  пуст или `api_http_addr` не `127.0.0.1:<port>` — нода не стартует вовсе.
- `WalletService::node_api_secret` берёт секрет из настроек, а при пустом
  значении — из файла рядом с управляемой нодой, поэтому статус и кошелёк
  ходят с Basic-auth.
- Живая проверка на 13413 с owner-секретом:
 `get_status` без `Authorization` — **401**, с `Basic (grin:<secret>)` — **200**.
- `tests/node_manager.rs` (старт реальной ноды) проходит с новым гейтом; добавлен
  юнит-тест на создание секретов и правку конфига.
- Остаток: `Access-Control-Allow-Origin: *` задаётся самим бинарём ноды 5.5.2.
  Без секрета запросы теперь получают 401, поэтому браузерный вызов admin-API
  через CORS не проходит; для полного закрытия нужен патч ноды или прокси —
  записано как остаток, а не как «закрыто».
### Ответный slatepack шифруется — 0.1.45

- RPC-отправка теперь передаёт `sender_index` в `create_slatepack_message`, то
  есть во входящем armor есть адрес отправителя.
- Embedded-приём разбирает sender из armor (ключ адреса, индексы 0..=current) и
  шифрует ответ на него; если sender нет — прежний открытый fallback.
- Адрес отправителя сохраняется в pending, поэтому повторная выдача ответа после
  сбоя тоже шифруется.
- Проверка `slatepack_probe` по флагу `mode`: новый ответ — **MODE=1 (age)**,
  старый (до правки) — **MODE=0 (plaintext)**. Зашифрованный ответ принят
  отправителем: `finalize` A прошёл, сделка `9d1ae7d3` ушла в сеть.
- Побочный фикс после P0: RPC-кошельку прописывается
 `node_api_secret_path` на `.foreign_api_secret` управляемой ноды, иначе после
  включения auth он не мог достучаться до узла. Проверено: A снова видит баланс,
  отмена и отправка работают.
- Остаток: RPC-приём всё ещё отвечает открытым текстом — у него нет локального
  разбора armor для извлечения sender; это следующий шаг.
### RPC-приём тоже шифрует ответ — 0.1.46

- `RpcBackend::slatepack_sender` достаёт ключ адреса через owner API
  (`get_slatepack_secret_key`, hex) и разбирает armor локально; из armor берётся
  sender.
- Ответ входящего шифруется на отправителя; sender сохраняется в
 `rpc-pending/<id>.recv.json`, поэтому повторная выдача ответа тоже шифруется.
- Живая проверка RPC→RPC: B отправил 0.1 на A с sender-адресом; A принял,
  ответ — **MODE=1 (age)**; B финализировал зашифрованный ответ — `posted`.
- Мелочь для тестового CLI: `wallet_cli` выставляет chain type (без этого
  локальный разбор armor падал на `GLOBAL_CHAIN_TYPE`).
- Итог: оба бэкенда (embedded и RPC) шифруют ответный slatepack. Остаток по
  секретам — только зануление пароля/фразы в аргументах команд и политика
  PBKDF2-100 upstream.
### Embedded: крэш-границы и идемпотентная финализация — 0.1.47

- `embedded_cli` получил `init`, `receive`, `scan`; хуки
 `FLINT_CRASH_AFTER_SEND` / `_FINALIZE` проверены на живом кошельке:
  - крах после `send_prepare` — код 9, журнал `locked`, slate сохранён;
  - крах после `finalize_tx` — код 9, журнал `finalized_not_posted`;
  - повторный запуск — `posted` без повторной подписи; повторная финализация
    идемпотентна (код 0).
- Исправлены три дефекта журнала embedded:
  1. ошибка `finalize_tx` «DB Not Found» больше не откатывает запись в `locked`
     (это означало «уже подписано»), а помечает `finalized_not_posted`;
  2. повторная финализация уже опубликованной сделки — no-op;
  3. если журнал уже удалён после подтверждения, повтор проверяет tx-log и
     отвечает успехом, а не «нет такого slate».
- P0-развязка auth: owner API ноды защищён секретом, foreign API оставлен
  открытым (read-only, нужен кошелькам); кошельки (RPC и embedded) получают
 `node_api_secret_path` на owner-секрет. Иначе после включения auth они не
  могли обновлять состояние. Проверено: embedded снова видит баланс и скан.
### Live-lifecycle и юнит-тесты конфигов — 0.1.48

- `tests/live_testnet.rs` переписан: сценарий работает на **существующих**
  каталогах кошельков (не требует пустых), готовность ноды берётся из
 `node_info()` кошелька (с owner-секретом после P0), таймаут подтверждения
  настраивается (`FLINT_LIVE_TIMEOUT_SECS`, по умолчанию 300 с).
- Сценарий: открыть A/B → проверить funded sender → `prepare_send` →
 `prepare_receive` → `finalize_send` → ждать роста spendable у B → отмена
  второй сделки и проверка возврата входов.
- Добавлены юнит-тесты: строгий разбор hex-секрета и запись
 `node_api_secret_path` в `grin-wallet.toml` (TOML-экранирование).
- Сценарий помечен `#[ignore]`; **прогон выполнен**: нода 3306120, 8 пиров,
  перевод 0.1 GRIN подтверждён у получателя, отмена вернула входы
  (spendable не изменился). Тест завершился за 24 с.
### Сверка сумм с цепью — 0.1.49

- `reconcile_posted` больше не удаляет запись только по факту подтверждения:
  сумма в подтверждённой транзакции сверяется с журналом.
  - приём: `amount_credited` должен точно совпасть с записанной суммой;
  - отправка: `debited − credited − fee` должен совпасть.
- Несовпадение не теряется: запись остаётся в журнале в состоянии
 `failed_recoverable` с текстом «сумма в цепи не совпадает с записью».
- Добавлен юнит-тест на обе формулы (совпадение и расхождение).
### Зануление пароля и фразы — 0.1.50

- `wallet_init`, `wallet_open`, `wallet_restore` и RPC-ветка восстановления
  держат пароль и seed-фразу в `Zeroizing`, поэтому они затираются на любом
  выходе из команды, включая ошибки.
- Остаток по секретам — только политика PBKDF2-100 в upstream (решение до
  реальных средств), кода Flint она не касается.
### IPC, установщик и PBKDF2-100 — 0.1.51

- **IPC:** добавлена capability `main` (`core:default` для окна `main`) и
 `freezePrototype: true`. `withGlobalTauri` остаётся: UI использует
 `window.__TAURI__`, а в Tauri 2 собственные команды нельзя ограничить
  capability-файлом — они доступны окну по определению. Защита: строгий CSP,
  одно окно, отсутствие удалённого контента и навигации.
- **Установщик (историческое, удалён в 0.1.55):** `tools/make-installer.ps1` собирал self-extracting
 `dist/Flint-0.1.51-setup.exe` (IExpress, 10.8 МБ). install.cmd кладёт
 `Flint.exe` в `%LOCALAPPDATA%\Flint`, создаёт ярлык на рабочем столе и
  запускает приложение; есть переменные для тестового режима. Автоматический
  прогон установки **завис** (IExpress-окно), поэтому установка помечена как
  требующая ручной проверки — сам exe собран.
- **PBKDF2-100:** в upstream крейтах KDF пароля — PBKDF2 со 100 итерациями, это
  не настраивается конфигом. Кодового фикса без форка нет; смягчение —
  минимальная длина пароля поднята с 8 до **12** символов для создания и
  восстановления. Argon2id-конверт остаётся фазой 3.
### Пункты 2–9 аудита — 0.1.52

- **2. PBKDF2-100.** KDF upstream не настраивается; политика усилена: минимум
  12 символов, минимум 3 класса символов до 16, запрет `password`/`qwerty`/
 `123456`. Тесты на политику добавлены. Argon2id остаётся фазой 3 — это
  осознанное решение, а не «исправлено».
- **4. CORS ноды.** Принятый остаток: заголовок `*` задаёт бинарь 5.5.2.
  Смягчение: owner API требует Basic-auth (без секрета — 401), окно приложения
  под CSP не ходит на порт ноды. Полное закрытие требует патча ноды или
  локального прокси — записано как решение, не как выполненный фикс.
- **5. Доверие к ноде.** Добавлены `chain`/`user_agent` в разбор `get_status`,
  проверка соответствия сети (`test`/`main`) и агента `MW/Grin…`, `ready`
  требует хотя бы одного пира, а не только `no_sync`. Добавлено подключение к
  уже запущенной ноде (`set_reuse_running` + проверка личности). Свежесть tip
  по пирам и отмена RPC-скана сделаны; embedded-отмена реализована
  кооперативно между батчами ([SPIKE_EMBEDDED_SCAN_CANCEL.md](SPIKE_EMBEDDED_SCAN_CANCEL.md)).
- **6. Журнал.** Терминальные состояния `confirmed`/`cancelled` теперь
  архивируются в `pending/archive/`, а не удаляются; есть юнит-тест.
- **7. IPC.** Capability `main`, `freezePrototype`, devtools выключены в release.
  Собственные команды Tauri 2 capability-файлом не ограничиваются — остаются
  CSP и одно окно (осознанный остаток).
- **8. Live-тесты.** `tools/run-live-suite.ps1` запускает `#[ignore]`-сценарий
  с проверкой окружения; CI-провайдера в проекте нет, поэтому это ручной прогон,
  а не CI.
- **9. UI-долги.** Pending-файлы получают ACL текущего пользователя. Смена
  пароля и удаление кошелька — не реализованы: это продуктовые деструктивные
  операции, требуют отдельного дизайна подтверждения.
### Смена пароля и удаление кошелька — 0.1.53

- `WalletService::change_password` (owner API библиотеки) и RPC
 `change_password` через owner API; новый пароль проходит ту же политику
  (12 символов, классы, запрет простых слов).
- Удаление кошелька: `WalletService::delete_wallet` требует точного имени
  профиля и **перемещает** каталог в `trash/<network>/`, ничего не стирая.
  Встроенный бэкенд поддерживает полностью; для RPC удаление пока ручное
  (остановить приложение и перенести каталог) — записано честно.
- UI: карточка «Безопасность» в настройках — «Сменить пароль» и «Убрать
  кошелёк» с подтверждением именем.
- **CORS не закрыт.** Пересборка ноды из исходников невозможна офлайн (в
 `resources/src/grin-5.5.2` нет vendor-каталога), а прокси — отдельное
  архитектурное изменение (нода на внутреннем порту + срезающий CORS прокси).
  Текущее смягчение: Basic-auth обязателен, loopback, CSP окна. Решение:
  отдельная задача перед фон/мессенджером, не «попутный патч».
### CORS закрыт локальным прокси — 0.1.54

- Нода слушает внутренний loopback-порт `13415/3415`, а публичный
 `13413/3413` занимает `node_proxy`: пропускает только `/v2/owner` и
 `/v2/foreign`, вырезает все `Access-Control-*`, на `OPTIONS` отвечает `403`,
  остальные пути — `404`.
- Заголовки клиента (в первую очередь `Authorization`) и тело передаются как
  есть; `Content-Length` пересчитывается, апстрим принудительно `Connection:
  close`, ответ читается до EOF.
- Живьём: POST без auth → `401` без CORS-заголовков; с Basic-auth → `200 OK`
  и тело JSON-RPC; `OPTIONS` с чужим `Origin` → `403`. Тесты прокси: два.
- Остаток: внешняя нода (`node_url` вне приложения) и ручной запуск
 `tools/run-testnet-node.ps1` идут мимо прокси — там CORS-заголовок остаётся
  от бинаря 5.5.2.
### Переключатель прокси и fallback — 0.1.55

- Настройка `node_proxy` (по умолчанию **вкл**): в UI — чекбокс «Прокси ноды
  (закрывает CORS; выкл — прямой режим)». Применяется после перезапуска ноды.
- **Прямой режим:** нода сама слушает публичный порт (`13413/3413`), прокси не
  поднимается. Живьём: auth `200`, CORS-заголовок присутствует — ожидаемо.
- **Fallback:** если на публичном порту уже отвечает здоровая нода с нашим
  секретом и правильными `chain`/`user_agent`, приложение подключается к ней
  (`is_external() == true`) и не запускает вторую ноду и прокси. Живьём:
  второй экземпляр показал `EXTERNAL=true`, порт остался за первой нодой.
- Старые `settings.json` без поля `node_proxy` получают `true` (юнит-тест), а не
  тихий переход в прямой режим.
### Установщик удалён — 0.1.55

- Решение: установщик даёт только копию exe и ярлык, но создаёт **второй**
  каталог данных (`%LOCALAPPDATA%\Flint`). Первый запуск из-за этого выглядит как
  «создание нового кошелька», что для кошелька опасная путаница.
- `tools/make-installer.ps1` и `dist/*-setup.exe` удалены из проекта и релиза.
  Основной способ — portable-zip: данные лежат рядом с exe.
- Если ранняя версия уже установлена: удалите `%LOCALAPPDATA%\Flint` и ярлык,
  **предварительно сохранив seed**, если в этом каталоге создавался кошелёк.

### Мессенджер M1: конверт и SDK — 0.1.56

- Новый крейт `crates/messenger` (`flint-messenger`): конверт v1, подпись
  ed25519, ECIES ed25519→X25519, HKDF-SHA256, ChaCha20-Poly1305.
- Адрес отправителя и тело лежат **внутри шифра**: relay видит только
  получателя, размер и время. Подпись проверяет получатель после расшифровки.
- Plaintext-slatepack запрещён и при запечатывании, и при открытии.
- TTL 26 ч по умолчанию (максимум 7 дней), квоты mailbox 100 сообщений / 64 КиБ,
  дедупликация по id, replay-guard получателя, ack только после успешного open.
- 23 теста: тест-вектор адреса, roundtrip, wrong recipient, подмена ciphertext/
  подписи, подделка отправителя, TTL, plaintext refusal, квоты, replay.
- **Gateway:** API переписан на конверт (без `from`), ack требует получателя,
  id — UUID (traversal закрыт), квоты 100/10k. Сейчас 11 gateway-тестов, включая
  обмен Alice→Bob через `GatewayTransport`, прямой `DirectOnionTransport` и
  запрос, разбитый на два пакета.
- **Гейт M1 закрыт:** два клиента обменялись подписанным и зашифрованным
  сообщением через тестовый relay; получатель проверил отправителя, ack удалил.
- **На момент 0.1.57 не было:** Tor/onion, локального хранилища и desktop-UI.
  Это закрыто в 0.1.58–0.1.65 (разделы ниже).
### M2: хранилище, история, ключ и живой обмен — 0.1.58–0.1.62

- `FileMailbox` в SDK: постоянное хранилище подписанных/зашифрованных конвертов,
  по одному JSON на сообщение. Плейнтекст на диск не попадает — только ciphertext,
  адрес получателя в имени каталога и UUID в имени файла.
- Запись атомарная (tmp → rename), ack удаляет только UUID-файлы, TTL-записи
  чистятся при приёме, квоты те же (100/64 КиБ), доставка идемпотентна по id.
- 4 теста: переживает переоткрытие, на диске нет тела/отправителя, ack/дубликаты,
  квоты/UUID/TTL.
- `History` (0.1.59): шифрованная история диалогов, ключ передаёт приложение;
  ChaCha20-Poly1305, на диске только ciphertext, 4 теста.
- `keywrap` (0.1.61): случайный history-ключ обёрнут PBKDF2-HMAC-SHA256
  (100k итераций) и ChaCha20-Poly1305; `rewrap` при смене пароля, история
  остаётся читаемой. 4 теста. Messenger-тестов — 35.
- **Живой M2-обмен (0.1.62):** два реальных кошелька через gateway
  (`messenger_cli`): A отправил текст, B расшифровал и проверил подпись, ack
  очистил очередь. Identity взята из ключа адреса кошелька и зануляется.
- Осталось для M2: экран сообщений, Argon2id вместо PBKDF2 для обёртки,
  onion-транспорт и UI-история.
### Защита от зависаний — 0.1.60

- **Single instance:** именованный Windows-mutex; второй Flint показывает
  сообщение и выходит, а не висит рядом с первым.
- **Startup watchdog:** если фронтенд не вызвал `ui_ready` за 45 с, приложение
  останавливает ноду, пишет причину в `flint.log` и выходит с кодом 70 вместо
  невидимого зависшего процесса.
- **Hang watchdog:** `IsHungAppWindow` проверяется каждые 5 с; 60 с подряд
  неответившего окна → остановка ноды, лог и выход с кодом 72.
- **Остановка ноды:** `NodeManager::Drop` и обработчик `ExitRequested` гасят
  дочерний `grin`; selftest 0.1.60 не оставил процесса ноды.
- Поведение ноды (синхронизация в фоне, attach, чего ещё нет) описано в
  [NODE_ARCHITECTURE.md](../architecture/NODE_ARCHITECTURE.md).
### UI сообщений — 0.1.63

- Новый экран «Сообщения»: адрес (копирование), поле relay, получатель, текст,
  «Отправить», «Забрать входящие», история диалогов.
- Identity берётся из ключа slatepack-адреса кошелька (embedded и RPC), seed
  зануляется; для RPC открытость определяется успешным `messenger_seed`, а не
  состоянием встроенного сервиса.
- Ключ истории создаётся/разворачивается при открытии кошелька (PBKDF2-обёртка),
  очищается при lock/switch и переворачивается при смене пароля.
- Relay сохраняется в настройках; для локального теста — `messenger_cli gateway`.
- Команды: `messenger_status`, `messenger_send`, `messenger_inbox`,
  `messenger_history`, `messenger_conversations`.
- Осталось: onion-транспорт, real-time/push, вложения/группы (не в v1).
### Перенос workspace в C:\DHW — 2026-10-25

- Рабочий корень перенесён из OneDrive в `C:\DHW`; старая копия удалена.
- Проверено уже из `C:\DHW`: `cargo test --workspace` — 128 тестов, 0 падений;
  release-сборка; selftest 7,4 с; UI 5 пунктов/CSP; relay health 200 и
  `/v2/foreign` 404.
- Живой direct-onion из нового пути: Alice→Bob и Bob→Alice доставлены
  (162,38 с, две неудачные попытки из-за сети, третья успешна).
- Абсолютные пути в `data\**\*.toml` и `dist\data\**\*.toml` переписаны на
  `C:\DHW`; `target/` не переносился и пересобран.

### Tor-мосты для заблокированных сетей — 0.1.69

- `TorListener` читает `<data>/<network>/tor/bridges.txt`; строки obfs4 /
  snowflake / webtunnel добавляются в `torrc` как `UseBridges 1`,
  `ClientTransportPlugin` и `Bridge`.
- Lyrebird 0.8.x — единый клиент для obfs4, snowflake и webtunnel; отдельные
  `snowflake-client.exe`/`webtunnel-client.exe` не нужны.
- Клиент ищется рядом с `tor.exe`/в `pluggable_transports`; путь можно задать
  `FLINT_LYREBIRD_BIN`.
- Без нужного клиента запуск Tor даёт понятную ошибку с именем файла.
- 5 тестов разбора конфига; в UI «Сеть и кошелёк» есть textarea мостов, статус
  транспортов/клиентов, сохранение, перезапуск Tor и просмотр `tor.log`;
  `verify-ui` сохраняет obfs4-строку через реальный IPC.
- В portable zip входят `tor.exe` и `lyrebird.exe` (obfs4/snowflake/webtunnel);
  проверено `verify-ui` на `dist/Flint-portable/Flint.exe`.
- Автоматического получения мостов нет: строки BridgeDB/письма Tor Project
  вводятся один раз в UI.
  Инструкция: [docs/operations/TOR_BRIDGES.md](../operations/TOR_BRIDGES.md).
- `Flint-0.1.68-single.exe` проверен **вне каталога проекта**: selftest exit 0,
  7,7 с, `grin.exe` распакован в `data\node` (встроенная нода работает).

### Адресная книга — 0.1.67

- `ContactBook` в `flint-messenger`: один ChaCha20-Poly1305-блоб с именем,
  адресом и временем; ключ — тот же history-ключ, что у переписки.
- Файл `messenger/contacts/<network>/<profile>/contacts.enc`; на диске нет ни
  адреса, ни никнейма (тест проверяет байты).
- Команды: `messenger_contacts`, `messenger_contact_save`,
  `messenger_contact_delete`; валидация адреса, имени (64 символа, без
  управляющих символов), лимит 500, атомарная запись.
- UI «Сообщения»: список контактов, добавить/изменить/удалить, «Написать»
  подставляет адрес; в истории вместо голого адреса показывается имя.
- 4 теста крейта; всего 128 тестов.

### Офлайн-relay и подписанный ack — 0.1.66

- `flint-relay` — standalone binary: `Gateway` с `wallet_proxy = false`,
  `require_signed_ack = true`, диском и квотами. `/v2/foreign` в relay-режиме
  отвечает 404; кошелька и seed на relay нет.
- Relay можно опубликовать через Tor hidden service; клиент задаёт
  `relay_url = http://<onion>`. Для `.onion` приложение использует SOCKS своего
  Tor, если «Приём по Tor» включён, иначе `FLINT_TOR_SOCKS`.
- Ack подписан ключом получателя и привязан к `id` сообщения и timestamp;
  публичный relay отклоняет старый `{to}` и подпись, снятую с другого `id`.
  Окно `ts` ±300 с.
- Store переживает перезапуск relay; есть тесты
  `relay_mode_has_no_wallet_and_requires_signed_acks` и
  `relay_store_survives_a_process_restart`.
- Ограничения: нет PoW/распределённого rate limit, публичный инстанс не
  развёрнут, часы relay/клиента должны совпадать в пределах 5 минут.
- Проверка 0.1.66: 121 тест, selftest 7,3 с, UI без CSP-ошибок.
- Живые прогоны 0.1.66 с подписанным ack: два onion/два gateway — 38,52 с;
  путь `TorListener` → gateway — 70,22 с (сеть в этот раз медленнее).

### Прямой onion и публикация gateway — 0.1.65

- `TorListener` теперь публикует onion, который смотрит на loopback `Gateway`,
  а не на `ForeignProxy`: один onion обслуживает `POST /v2/foreign` и
  `/flint/v1/msg*`; owner API наружу не выставлен.
- `TorListener::start` ждёт bootstrap Tor, иначе UI показывал onion, который ещё
  не мог построить цепь.
- `DirectOnionTransport` выводит onion получателя из slatepack-адреса и
  доставляет конверт через SOCKS отправителя; fetch/ack идут в локальный
  gateway. Relay не участвует.
- В приложении прямой режим включается пустым `relay_url` при активном «Приём по
  Tor»; экран «Сообщения» показывает «Прямая доставка по Tor».
- Найден и исправлен баг gateway под Tor: принятый сокет наследовал
  nonblocking-флаг, и запрос, пришедший двумя пакетами, закрывался без ответа.
  Регрессия: `gateway_waits_for_a_request_split_across_packets`.
- Исправлен deadlock экрана «Сообщения»: `history_dir` повторно брал
  `wallet`-mutex внутри уже удерживаемой блокировки, из-за чего отправка/инбокс
  зависали. Теперь путь истории строится из того же снимка настроек.
- Исправлено ложное «Кошелёк заблокирован» в «Сообщениях» на RPC-бэкенде:
  `messenger_seed` ожидал обёрнутый JSON, а owner API отдаёт hex-строку.
  Теперь Tor-ключ и ключ мессенджера читаются одним `secret_hex`; ошибка
  показывается в UI, а не маскируется под блокировку. Тест: 121.
- `--selftest` вынесен **до** single-instance guard: раньше при запущенном окне
  он показывал модальный fatal-dialog и зависал. Проверка 0.1.65: exit 0, 7,3 с.
- GUI-release `--selftest` больше не падает на закрытом stdout: отчёт всегда
  пишется в `selftest.json`, вывод в stdout/stderr не обязателен. Проверка
  `Flint.exe --selftest` (release, корень): exit 0, 7,6 с, `ok:true`,
  временный каталог удалён.
- RPC-кошелёк стал переносимым: перед запуском `grin-wallet` перезаписывает
  `data_file_dir`, `api_secret_path` и `send_config_dir` из фактического каталога
  (раньше `init` оставлял абсолютные пути и папку нельзя было перенести).
- Одноразовые тестовые A/B-сборки удалены: 0.1.68 использует обычный backend
  и не содержит test-only флагов.
- Живые проверки: два кошелька через один реальный Tor (40,77 с, оба
  направления) и путь приложения `TorListener` → gateway (37,54 с).

### Onion-транспорт — 0.1.64

- `GatewayTransport::with_socks` умеет `.onion`: SOCKS5 CONNECT (без auth) к
  Tor-прокси, дальше обычный HTTP к gateway. `is_onion()`.
- Приложение выбирает Tor автоматически, если `relay_url` содержит `.onion`;
  адрес SOCKS — `FLINT_TOR_SOCKS`, по умолчанию `127.0.0.1:9050`.
- Тест: настоящий gateway + фейковый SOCKS5-прокси; send/fetch прошли.
- Публикация gateway и прямой peer-to-peer onion — в 0.1.65.
### Фаза 3, срез 1–2: Argon2id-конверт seed (встроенный бэкенд, без релиза)

- Новый модуль [`crates/core/src/seed/`../../crates/core/src/seed/): формат
  upstream `wallet.seed` (PBKDF2-SHA512-100, типы upstream приватны — кодек
  реализован локально), конверт `wallet.seed.v2` (Argon2id 64 МиБ / t=3 / p=4 →
  ChaCha20-Poly1305, AAD связывает сеть, параметры, соль, nonce, длину и адрес) и
  правила хранения. Дизайн и тест-план — [ARGON2ID.md](ARGON2ID.md).
- Новый кошелёк: энтропия → мнемоника → адрес `m/0/1/0` выводится офлайн
  ([`address_from_entropy`../../crates/core/src/address.rs)) → конверт пишется
  **до** upstream-кошелька → upstream создаётся с паролем из 256 случайных бит →
  адрес сверяется с конвертом → `wallet.seed` удаляется. В покое seed-файла нет.
- Разблокировка: конверт расшифровывается паролем пользователя, временный
  `wallet.seed` пишется с новым случайным паролем, upstream открывается им,
  адрес сверяется, файл удаляется. При ошибке прежние байты возвращаются на
  место — тест `legacy_seed_is_restored_when_the_envelope_does_not_match`.
- Legacy-кошельки не тронуты: без конверта путь открытия прежний, `wallet.seed`
  не перезаписывается и не удаляется; смена пароля идёт через upstream, как
  раньше. Для конверта `change_password` переупаковывает только конверт.
- Восстановление пишет конверт первым и помечает кошелёк несчитанным **до**
  создания базы: обрыв не выглядит как подтверждённый нулевой баланс.
- Кросс-проверки с upstream: upstream открывает legacy-файл, записанный Flint
  (`tests/seed_codec.rs`); кошелёк без базы открывается из конверта и получает
  тот же адрес (`tests/seed_spike.rs`). Вектор Argon2id — из эталонной
  реализации (`vendor/argon2/tests/kat.rs`).
- Зависимости: `argon2 0.5.3` и `blake2-rfc`; `cargo vendor` выполнен, сборка
  снова офлайн (`cargo check --workspace --all-targets` — без ошибок).
- Проверено: `cargo test --workspace` — **154 passed, 8 ignored, 0 failed** (на момент среза 1–2);
  `--selftest` — exit 0, 7,5 с, 24 слова, `reopened_matches: true`, нода
  запустилась и остановилась.
- Не сделано на момент срезов 1–2 (закрыто позже в срезах 3–5): RPC-бэкенд,
  миграция существующих кошельков, mainnet-гейт, UI-предупреждение о legacy,
  живой прогон с нодой на конверте, kill-дрилл, Argon2id для ключа истории.
- Ограничение для пользователя: каталог, созданный этой сборкой, официальный
  `grin-wallet` не откроет — пароль upstream случаен. Выход — фраза; откат
  появится вместе с миграцией (3b).

### Фаза 3, срез 3: RPC-бэкенд на конверте и mainnet-гейт — 0.1.69

- `grin-wallet` больше не требует `wallet.seed` для старта. Спайк S4 подтверждён
  живым тестом `owner_api_starts_without_seed_and_opens_after_the_seed_appears`:
  процесс поднимается в пустом каталоге, `open_wallet` до появления файла
  отклоняется без зависания, после появления — открывает кошелёк **без
  перезапуска** и даёт тот же адрес. Поэтому порядок запуска не менялся: процесс
  стартует при старте приложения, пароль спрашивается позже.
- Открытие RPC-кошелька с конвертом — `RpcBackend::open_with_envelope`:
  `prepare_unlock` пишет временный seed с новым случайным паролем → `open_wallet(K)`
  → сверка адреса индекса 0 с конвертом → `commit_unlock` (файл удаляется).
  Ошибка — `rollback_unlock` возвращает прежние байты; при неверном пароле не
  пишется ничего.
- `lock` убирает временный seed; `change_password` для конверта переупаковывает
  только конверт (upstream не вызывается); `restore` для RPC пишет конверт
  **до** `create_wallet`, upstream создаёт кошелёк под случайным паролем, адрес
  сверяется, seed удаляется.
- `wallet_cli` идёт через `SeedStore`; `embedded_cli` — через сервис, который уже
  был на конверте.
- Mainnet-гейт: `SeedStore::ensure_mainnet_ready` в обоих бэкендах — mainnet без
  конверта не открывается («кошелёк не защищён Argon2id; включите защиту seed
  или создайте новый кошелёк»), testnet legacy открывается как раньше. Тест
  `mainnet_does_not_open_a_legacy_seed`.
- Живые тесты на реальном `grin-wallet.exe` (`#[ignore]`): открытие из конверта,
  неверный пароль, restore-путь — `tests/wallet_rpc_envelope.rs`.

### Фаза 3, срез 4: миграция legacy → конверт (3b) — 0.1.69

- `SeedStore::migrate_legacy`: M0 — старый файл расшифровывается в память
  (неверный пароль ничего не меняет), адрес выводится офлайн из энтропии;
  M1 — конверт с исходными байтами в поле `legacy` (tmp → fsync → rename);
  M2/M3 — независимое чтение и расшифровка конверта; M4 — удаление `wallet.seed`;
  M5 — лог без секретов.
- Слабые копии `wallet.seed.bak*` (наследие upstream-смены пароля): миграция
  отказывается, пока они есть, и удаляет их только с явным согласием.
- Откат `disable_envelope`: `wallet.seed` пишется под паролем пользователя,
  читается обратно, затем конверт удаляется. На mainnet откат запрещён — иначе
  гейт сделал бы кошелёк неоткрываемым.
- Восстановление после краха: обрыв после M1 (оба файла) сходится на следующей
  разблокировке (конверт главнее, временный seed удаляется); битый `.tmp-*`
  удаляется перед чтением конверта. Тест
  `interrupted_migration_recovers_on_the_next_unlock`.
- UI: баннер «Seed защищён старым форматом (PBKDF2-100)» на экране кошелька и на
  карточке блокировки, кнопка «Включить Argon2id» (пароль + повтор, чекбокс на
  удаление копий). В настройках — «Защита seed» и «Отключить Argon2id» (testnet).
  Новые команды: `wallet_seed_status`, `wallet_migrate_seed`,
  `wallet_disable_envelope`.
- Диалоги миграции и восстановления теперь блокируют кошелёк **безусловно**
  перед командой, а не только по флагу `lockFirst`: обе точки входа (экран
  кошелька и карточка блокировки) не могут отправить `wallet_migrate_seed` на
  разблокированном кошельке. Проверено живьём embedded и RPC (legacy → envelope,
  кошелёк остаётся заблокированным, адрес/пароль те же); `tools/verify-ui.mjs` —
  без проблем.
- Прогон в собранном приложении (CDP, встроенный бэкенд): init → envelope; откат
  из настроек → legacy и баннер; миграция из диалога на карточке блокировки и с
  экрана кошелька (диалог сам блокирует кошелёк) → envelope, баннер исчезает,
  кошелёк открывается тем же паролем и адресом; скриншот `shots/seed-banner.png`.
  `tools/verify-ui.mjs` на этой сборке — без проблем.
- Kill процесса во время миграции и ревью добавлены позже в 0.1.69 — см. разделы
  «срез 5» и «ревью миграции и kill-дрилл». Не сделано: обрыв питания на VM,
  живой прогон миграции на RPC с нодой, внешнее человеческое ревью.

### Фаза 3, срез 5: Argon2id для ключа истории — 0.1.69

- `crates/messenger/src/keywrap.rs`: обёртка ключа истории — Argon2id (64 МиБ,
  t=3, p=4) → ChaCha20-Poly1305, версия 2. AAD связывает параметры KDF, соль и
  nonce; параметры при чтении ограничены до выделения памяти.
- Версия 1 (PBKDF2-HMAC-SHA256 100k) читается: фиксированный v1-блоб в тестах
  доказывает обратную совместимость. После успешной разблокировки приложение
  само переупаковывает v1 в v2; при смене пароля `rewrap` сразу пишет v2.
- Обёртка пишется атомарно (tmp → fsync → rename → проверка чтением), поэтому
  обрыв не оставляет полузаписанный файл; при неудачном апгрейде старый блоб
  остаётся, история доступна в текущей сессии, ошибка уходит в лог без секретов.
- Проверено 8 тестами: v2 roundtrip/rewrap, v1-фикстура и апгрейд, неверный
  пароль/подмена, версия, абсурдные параметры Argon2, короткий nonce без паники,
  пустой пароль, свежие salt/nonce. `flint-messenger` — 48 тестов.
- Ограничение: это ключ локальной истории, не seed; forward secrecy/ratchet
  по-прежнему нет, v1-блоб остаётся на диске до первого успешного unlock.

### Фаза 3, ревью миграции и kill-дрилл — 0.1.69

- Ревью: два адверсариальных прохода в изолированном контексте по коду и
  docs/history/ARGON2ID.md; отчёт, сценарии и исправления —
  [REVIEW_MIGRATION.md](REVIEW_MIGRATION.md). Это не внешний человеческий аудит.
- Исправлено по ревью: ошибка после M1/M2 удаляет только свой конверт (сверка
  записанных байт), слабые `wallet.seed.bak*` удаляются до `wallet.seed`, M4 при
  ошибке откатывает конверт, временный seed помечается sidecar-файлом (lock/exit
  не удаляет непомеченный legacy seed), `disable_envelope` отказывает на mainnet
  в самом core и убирает слабый seed, если конверт удалить не удалось, конверт
  другой сети отвергается до записи seed, откат без смены пароля возвращает
  точные байты, `write_envelope`/`change_password` проверяют файл и откатывают
  прежний конверт, RPC-статус и список профилей видят `wallet.seed.v2`, команды
  seed-жизненного цикла сериализованы, RPC-открытие больше не сбрасывает
  `needs_scan` у пересозданной пустой базы, ACL применяется до записи seed.
- Добавлен аварийный ремонт: `wallet_repair_envelope` переименовывает нечитаемый
  конверт в `wallet.seed.v2.broken-…` после проверки пароля legacy и того, что
  конверт действительно не открывается; `wallet.seed` остаётся. Кнопка есть в
  баннере незавершённой миграции. Перед миграцией приложение открывает legacy-
  кошелёк и сверяет адрес индекса 0 с выведенным из seed — при расхождении
  миграция отказывает до удаления seed.
- Kill-дрилл: `crates/core/tests/migration_crash.rs` (фича `crash-injection`,
  в релиз не попадает) запускает миграцию в дочернем процессе и абит его в M0,
  M1, M2, M3, M4, M5. После каждой точки: ожидаемые файлы, отсутствие слабых
  копий, тот же seed/адрес, разблокировка или повтор миграции, откат. 6 точек,
  ~3,5 с. Точки «до/после rename» и битый `.v2.tmp` покрыты отдельным
  `interrupted_migration_recovers_on_the_next_unlock`.
- Не сделано: обрыв питания на VM (kill процесса автоматизирован, потеря
  питания — нет), внешнее человеческое ревью. Межпроцессная блокировка
  `.flint-seed.lock` для миграции/отката между процессами Flint добавлена;
  внешний `grin-wallet` её не читает. Живой прогон миграции с синхронизированной
  нодой выполнен (legacy → envelope, оба бэкенда).
- Остаточные риски (осознанно, в [REVIEW_MIGRATION.md](REVIEW_MIGRATION.md)):
  AAD не связывает строки формата (их проверяет валидатор), ACL зависит от
  `icacls` и NTFS, `wallet.seed.bak*` удаляются после успешной разблокировки без
  отдельного подтверждения.

### Пост-ревью 0.1.68: история, Tor-ack, RPC finalizing — 0.1.69

Полный триаж замечаний: [REVIEW_0.1.68_TRIAGE.md](REVIEW_0.1.68_TRIAGE.md).

- **Ключ истории изолирован по профилю:** `.history.key` лежит в
  `<root>/messenger/history/<network>/<profile>/`; старый общий
  `<root>/messenger/history.key` мигрируется при первом открытии, если пароль
  подходит, и не перезаписывается другим профилем. Если история есть, а ключ
  потерян/чужой, новый ключ не создаётся — UI показывает причину.
- **История больше не срывает кошелёк:** ошибка ключа логируется и попадает в
  `messenger_status.history_error`, но `wallet_open`/`wallet_init`/`wallet_restore`
  возвращают успех; экран сообщений показывает предупреждение.
- **Смена пароля транзакционная:** новая обёртка пишется в `.history.key.next`
  до смены пароля seed; после успеха фиксируется, при ошибке удаляется. Обрыв
  после смены пароля восстанавливается через `.next` на следующем unlock.
- **Входящие: история до ack.** `Messenger::fetch_open` отделён от ack; приложение
  пишет `History` (fsync) и только затем `ack_signed`. `History::contains` —
  durable-дедупликация после перезапуска/relay-replay; битый конверт не обрывает
  пачку, не ack-ается и показывается в `failed`; inbox/send сериализованы.
- **Tor-gateway требует signed ack:** `TorListener` создаёт gateway с
  `require_signed_ack = true`; добавлен unit-тест конфигурации.
- **RPC finalizing:** перед `finalize_tx` в журнал пишется durable
  `stage=finalizing` (файл с `sync_all`); после неопределённого результата
  `reconcile_finalizing` ищет сохранённую сделку и публикует её, не подписывая
  повторно; `retry_post` тоже умеет это состояние. Запись журнала отправки и
  получения получила `fsync`.
- **Онлайн-отправки в журнале:** `send_over_tor` и `send_local` пишут
  `RpcSendRecord` (locking → locked → finalizing → finalized_not_posted → posted),
  пытаются отменить при ошибке до финализации и сохраняют `failed_recoverable`,
  если отмена не удалась. `rpc-pending` теперь профильный
  (`<wallet_dir>/rpc-pending`), как у CLI, а не общий на сеть; старый общий
  `<root>/<network>/rpc-pending` больше не читается (записи из него, если были,
  нужно разобрать вручную); нечитаемые RPC-записи возвращаются в
  `wallet_pending.unreadable`, а не выглядят как «ничего нет».
- Не закрыто из ревью 0.1.68: ограниченный локальный quarantine/удаление
  битых конвертов после TTL; IPC, stop, квоты relay, torrc-кавычки и упаковка —
  как в разборе.

### Явные состояния скана — 0.1.69

- `ScanState` (`not_started` / `running` / `completed` / `failed`) хранится в
  профиле `<root>/<network>/state/<profile>.json` вместе с `pending_scan`,
  ошибкой и отметками времени. Парсинг `owner.log` (`Scanning - NN% complete`)
  удалён полностью: отсутствие строки в логе больше ничего не доказывает, и
  процента в UI тоже нет — только состояние.
- Переходы: восстановление ставит `pending_scan` и `not_started` **до** создания
  базы; `begin_scan` пишет `running` до вызова кошелька; `completed` ставится
  только при успешном возврате скана и только он снимает `pending_scan`;
  ошибка/обрыв оставляют `failed` и неизвестный баланс. `running` в файле на
  старте трактуется как прерванный скан, а не как завершённый.
- RPC: `wallet_open` больше не сбрасывает флаг «баланс неизвестен». Если
  кошелёк занят сканированием (наблюдение по ошибке владельца, не по логу),
  ставится `running`; следующий успешный open завершает скан. Ошибка пароля
  состояние скана не меняет. Пересозданная пустая база по-прежнему помечается
  `pending_scan` до чтения баланса.
- `wallet_scan` отказывает, пока состояние `running`; `wallet_summary`,
  `wallet_activity`, `wallet_pending` отвечают по состоянию, а не по возрасту
  лога. В шапке — «скан идёт» без процента; баннер показывает ошибку прошлого
  скана и кнопку «Повторить».
- Смена `min_confirmations` теперь доходит до живого `RpcBackend`
  (`set_min_confirmations`), а не ждёт перезапуска.
- Тесты: `scan_state_is_explicit_and_survives_restarts`,
  `a_failed_scan_keeps_the_balance_unknown`,
  `old_wallet_state_defaults_to_not_started`,
  `a_running_scan_is_interrupted_after_a_restart`,
  `a_corrupt_state_file_means_unknown_balance`.
- Проверено: `cargo test --workspace` — **212 passed, 0 failed, 13 ignored**;
  `--selftest` — ok; `tools/verify-ui.mjs` — без проблем.
- Осталось: живой прогон с реальным mainnet-кошельком/сетью (см. раздел
  «Snapshot ноды»).

### Отмена скана — 0.1.69

- Отмена есть у обоих бэкендов. RPC: скан идёт в отдельном процессе, Flint
  завершает его (`WalletProcess::terminate` — kill + wait), вызов `scan`
  возвращает ошибку, backend перезапускается. Embedded: `Owner::scan` больше не
  вызывается напрямую; `prepare_scan` отдаёт `EmbeddedScanJob`, который водит
  публичную батчевую `grin_wallet_libwallet::scan` по 10 001 блоку и проверяет
  токен между батчами. Поток приложения не убивается, запись в LMDB не
  обрывается.
- `wallet_scan_cancel` ставит токен под тем же замком, что и `busy`, поэтому
  отмена либо успевает до решения `wallet_scan`, либо видит, что скан уже не
  идёт, и отказывает. Для RPC дополнительно завершается процесс; для embedded
  только выставляется токен, который батчевый цикл увидит на границе.
- `prepare_scan` проверяет готовность ноды **до** `begin_scan` и фиксирует tip;
  `EmbeddedScanJob::run` обновляет известные выходы, идёт по батчам и после
  каждого сохраняет `last_scanned_block`. `finish_scan` переводит `Running` в
  `Completed`, `Failed` или `Cancelled`: отмена даёт `Failed` с причиной «скан
  отменён пользователем», `pending_scan` **не снимается**; поздняя отмена не
  перезаписывает уже завершённый скан.
- RPC после отмены перезапускает backend и возвращает кошелёк
  **заблокированным** (Flint не хранит пароль). Embedded процесс не
  перезапускается и остаётся разблокированным. Для RPC upstream может сам
  возобновить прерванный скан при следующем `open_wallet`: пока он идёт,
  `wallet_summary` получает ошибку транспорта (в одном живом прогоне — не
  отвечал до завершения фонового скана), состояние Флинта остаётся `failed` с
  сохранённым `pending_scan`; после завершения появляется обычный баннер «Скан
  не завершён» с кнопкой «Повторить». Ошибка перезапуска не прячется: пишется в
  лог и возвращается вместе с сообщением об отмене.
- `WalletStatus.scan_cancellable` теперь `scan_busy` для обоих бэкендов:
  кнопка «Отменить скан» в шапке, подтверждение и «Отменяю…». Для embedded
  подтверждение объясняет, что отмена сработает после текущего батча и кошелёк
  останется разблокированным. Во время скана `wallet_status` не ждёт мьютекс
  RPC и не запрашивает адрес (embedded), а `wallet_summary`/`activity`/
  `pending` отвечают ошибкой «скан идёт» — иначе шапка и кнопка зависли бы.
- RPC-отмена — **аварийное прерывание процесса**; embedded — мягкая остановка
  на границе батча. В обоих случаях сохранённые между батчами данные не
  откатываются. Money- и lifecycle-команды (`send_*`, `receive_*`, `cancel`,
  `lock`, `open`, смена сети/профиля, пароль, удаление) во время скана
  отказывают по `scan_busy`, чтобы не конкурировать с батчевым писателем.
- **Авто-скан в summary закрыт:** новый и восстановленный embedded-кошелёк
  помечаются upstream `InitNoScanning`, а после успешного `wallet_scan` —
  `InitComplete`. Поэтому `wallet_summary` не запускает полный скан внутри
  `update_wallet_state`; full scan идёт только через отменяемый
  `EmbeddedScanJob`. В живом прогоне `refresh` сразу после создания кошелька
  занял 206 мс вместо многосекундного скана.
- **Embedded-отмена реализована** через публичный API libwallet, без патча
  vendor и без выноса кошелька в процесс; решение и эскиз —
  [SPIKE_EMBEDDED_SCAN_CANCEL.md](SPIKE_EMBEDDED_SCAN_CANCEL.md). Долгий скан
  выполняется вне мьютекса `WalletService`, поэтому статус и отмена доступны,
  пока батчи идут.
- Тесты: `cancelling_a_running_scan_marks_it_failed_and_keeps_pending_scan`,
  `cancelling_a_completed_scan_does_not_overwrite_it`,
  `finish_scan_applies_cooperative_outcomes`,
  `a_late_batch_cancel_does_not_overwrite_completed`,
  `new_and_restored_wallets_skip_upstream_full_scan`.
- Проверено: `cargo test --workspace` — **212 passed, 0 failed, 13 ignored**;
  `--selftest` — ok; `tools/verify-ui.mjs` — без проблем.
- Живой ignored-тест `live_embedded_scan_cancel::embedded_scan_cancels_between_batches`
  (2026-10-26, testnet 3 308 611) прошёл за **84,14 с**: скан отменён на границе
  батча, `failed`/`pending_scan`; частичный и пустой scan теперь отвергаются,
  поэтому повторный скан был **полным** и завершился `completed`. Запуск:
  `FLINT_LIVE_EMBEDDED_SCAN=1 cargo test -p flint-core --test
  live_embedded_scan_cancel -- --ignored --nocapture`.
- Живой прогон RPC (2026-10-25, testnet, высота 3 308 415, настоящий
  `grin-wallet.exe 5.5.1`) через реальный Tauri IPC на профиле, помеченном как
  после restore (`pending_scan=true`): скан запущен, `scan_cancellable=true`;
  кнопка «Отменить скан» в шапке была видима, подтверждение открылось, клик
  запустил отмену, тост показал ошибку; `wallet_scan_cancel` →
  `{"cancelling":true}`; `wallet_scan` вернул «скан отменён пользователем»;
  состояние `failed`, в файле профиля `pending_scan` остался `true` и до, и
  после повторной разблокировки; `wallet_open` после перезапуска прошёл,
  `wallet_summary` показал `needs_scan=true` и причину; повторный скан
  запустился и снова отменился.
- Живой прогон embedded (2026-10-25, testnet, высота 3 308 493, тот же
  `WalletService` в собранном приложении): `refresh` сразу после создания
  кошелька — 206 мс, без скрытого full scan; полный скан, кнопка «Отменить
  скан» видима, подтверждение содержит текст про батч; отмена из UI дала
  `failed` с причиной за 132–383 мс, `unlocked` остался `true`, `wallet_scan`
  вернул «скан отменён пользователем»; повторный **полный** скан
  завершился `completed` (частичный после отмены теперь запрещён).
  Сохранение `pending_scan` при отмене после restore проверено unit-тестом, не
  живым embedded-прогоном (в нём создавался новый кошелёк). Embedded UI-прогон
  оформлен как [live-scan-cancel.mjs../../tools/live-scan-cancel.mjs)
  (`node tools/live-scan-cancel.mjs`, нода запущена), RPC UI-прогон — как
  [live-rpc-scan-cancel.mjs../../tools/live-rpc-scan-cancel.mjs) (медленнее:
  upstream после reopen может сам запустить скан). RPC-прогон 2026-10-25 прошёл
  за 131 с: отмена на 10%, `failed`/`pending_scan`, повторный скан и повторная
  отмена.

### Snapshot ноды и свежесть tip — 0.1.69

- `NodeSnapshot` (`network`, `url`, `secret`, `min_confirmations`) — один
  снимок настроек на операцию: [node.rs../../crates/core/src/node.rs). Им
  пользуются `node_info`, клиент ноды и запуск `grin-wallet`, поэтому проверка
  готовности и кошелёк больше не могут смотреть на разные endpoint'ы.
- Перед запуском дочернего кошелька `grin-wallet.toml` переписывается:
  `check_node_api_http_addr` = URL из настроек, `node_api_secret_path` = файл
  `.node_api_secret` в каталоге профиля с owner-only ACL. Раньше endpoint
  оставался тем, что записал первый `init`, а секрет всегда брался у
  управляемой ноды.
- `settings_set` при смене `node_url` пересоздаёт RPC backend (процесс
  перезапускается, конфиг патчится заново). Если перезапуск не удался, настройки
  уже сохранены, а UI получает `restart_error` и показывает его; успех —
  `node_restarted`. Смена сети по-прежнему требует перезапуска приложения.
- `min_confirmations` доходит до живого backend сразу, без перезапуска.
- **Живой endpoint-switch (2026-10-25):** RPC-кошелёк, локальный прокси
  `127.0.0.1:13415` → настоящая testnet-нода. `settings_set` вернул
  `node_restarted:true`, `settings_get` сохранил новый URL, прокси получил
  запросы (0 → 4, всего 27), после перезапуска `wallet_open` и `wallet_summary`
  работают. Ошибка перезапуска не имитировалась.
- **Живая смена сети (2026-10-25):** `testnet → mainnet → testnet` в **пустом
  временном** каталоге: mainnet применился (`initialized:false`, `locked:true`,
  нода `unknown`), обратно testnet открылся и summary прочитался. Реальный
  mainnet-кошелёк/цепь не трогались; это проверка переключения и пересборки
  контекста, не работа с реальными средствами.
- **Изолированный mainnet-кошелёк:** в отдельном `.tmp`-каталоге создан кошелёк
  с mainnet-настройками, адрес `grin1…`, 24 слова, без ноды, без средств и без
  существующих mainnet-каталогов. Это проверка mainnet-ветки создания, не
  live-сеть.
- **Graceful stop (2026-10-25):** закрытие окна при живых `grin-wallet` и
  управляемой `grin` больше не оставляет процессов. До фикса RPC-кошелёк
  переживал выход приложения. Теперь `shutdown_children` вызывается на
  `ExitRequested`/`Exit`, из watchdog-ов и `restart_app`, снимает RPC/Tor,
  завершает `WalletProcess` (kill + wait) и останавливает ноду; `shutting_down`
  не даёт пути скана перезапустить уже убитый backend. Повторный запуск на том
  же каталоге открыл кошелёк и прочитал summary. Отдельно проверено закрытие
  **во время RPC-скана**: `grin-wallet` не остался, после рестарта состояние
  `failed` (прерванный скан), `wallet_open` снова работает.
- **Graceful stop Tor (2026-10-25, реальный Tor):** `tor.exe` поднялся до
  `Bootstrapped 100% (done)` примерно за 87 с, опубликовал onion
  `l6mowz...yd.onion`; при закрытии окна с активным listener сняты и `tor.exe`,
  и `grin-wallet.exe` (после закрытия процессов нет), повторный запуск открыл
  кошелёк. Первый прогон в этот день застрял на 55% (сеть), повторный прошёл —
  таймаут 150 с достаточен. Это уже проверка настоящей onion-сети, не
  подменного процесса.
- **Свежесть tip:** `NoSync` считается `ready` только когда tip не отстаёт от
  `highest_height` пиров больше чем на `TIP_FRESHNESS_BLOCKS` (2 блока). Нода
  без пиров или на генезисе по-прежнему не `ready`; отсутствие цели у старых
  пиров не выдумывает ограничение.
- Тесты: `node_snapshot_is_one_view_of_the_settings`,
  `node_config_gets_the_settings_endpoint_and_per_wallet_secret`,
  `a_tip_far_behind_the_peers_is_not_ready`. Живой ignored-тест
  `wallet_rpc_envelope::envelope_only_wallet_starts_and_unlocks` с настоящим
  `grin-wallet.exe` проходит после патча конфига.
- Проверено: `cargo test --workspace` — **212 passed, 0 failed, 13 ignored**;
  `--selftest` — ok; `tools/verify-ui.mjs` — без проблем.
- Осталось: живой прогон с реальным mainnet-кошельком/сетью (не делался, чтобы
  не трогать реальные средства).

### P0 mainnet-readiness: graceful stop, release, backup — 0.1.69

- **Graceful stop ноды.** `NodeManager::start` запускает `grin` с
  `CREATE_NEW_PROCESS_GROUP`; `stop` временно подключается к скрытой консоли,
  посылает `CTRL_BREAK_EVENT`, ждёт до 20 с и только затем делает `kill`.
  Тест `starts_and_stops_bundled_node` проверяет `Shutdown complete` в
  `node.log`; `starts_again_after_a_hard_kill` — рестарт после `taskkill /F`.
- **Release.** `tools/release.ps1` проверяет версию, fmt/tests/build
  `--features embed-node`, запускает release `--selftest`, проверяет
  `grin 5.5.2`, генерирует [SBOM](https://cyclonedx.org/) (`tools/sbom.ps1`,
  924 компонента), `dist/build-info.json` и `dist/SHA256SUMS`; собирает portable
  zip/single. `-RequireSigned` превращает отсутствие валидной подписи в ошибку,
  но сертификата в этой сессии нет — обычный запуск даёт **UNSIGNED**.
  Reproducible build с внешними штампами времени остаётся открытым.
- **CI.** `.github/workflows/windows.yml` — fmt/check/tests/build/selftest;
  в workflow добавлены `cargo vendor` + `fetch-node`/`fetch-wallet`, но
  успешного прогона на GitHub-hosted runner ещё нет. Живой testnet-job вынесен
  на self-hosted runner. Локальный аналог — `tools/ci.ps1` (проходит).
- **Backup drill.** Runbook [BACKUP.md](../operations/BACKUP.md) и автотест restore в другом
  корне. Чистая машина — ручной шаг.
- **Отказоустойчивость.** Кроме kill/restart ноды, добавлен тест
  `a_failed_pending_write_leaves_the_previous_record_intact`: запись pending
  упирается в занятый temp-путь (форма disk-full/I/O-ошибки), старая запись
  остаётся читаемой, `pending_problems` пуст, после освобождения пути запись
  восстанавливается. Это не заменяет реальный disk-full/power-loss стенд.
- **Mainnet-каталог без средств.** Embedded: создание `grin1…`, отклонение
  неверного старого пароля, смена пароля с переупаковкой ключа истории,
  повторное открытие, удаление в `trash/mainnet/`. RPC-удаление из UI также
  останавливает `grin-wallet`, снимает Tor и переносит каталог в `trash/`.
- Осталось: внешний аудит, mainnet dry-run, power-loss/disk-full/битая цепь,
  живой сценарий анти-подмены адреса, подпись сертификатом, reproducible build
  и внешнее ревью Argon2. `.flint-seed.lock` уже
  удерживается на весь unlock/migration/password/repair lifecycle Flint.
  В send-диалоге есть полная строка адреса, QR, лимит по `spendable`,
  подтверждение последних 8 символов и обязательный чекбокс.

### Аудит 2026-10-26: закрытые пункты и остаток

- **Seed lifecycle.** `.flint-seed.lock` теперь удерживается на весь
  `prepare_unlock → upstream open → commit/rollback`, а также на
  `write_envelope`, `change_password`, `disable_envelope`,
  `restore_legacy_from_envelope`, `remove_transient_seed` и
  `remove_verified_seed`. Регрессия `unlock_blocks_disable_until_commit`
  воспроизводит чередование из аудита: disable получает отказ, защищённая
  копия остаётся. Внешний `grin-wallet` lock не читает.
- **RPC journal.** `load_record` возвращает `Result<Option<_>>`: повреждённый
  JSON и ошибка доступа — ошибка, а не «записи нет»; `send_prepare` требует
  настроенный журнал; `send_finalize` без записи отказывает до подписи.
  TxSent без журнала показывается как `unknown`, не `posted`.
  Старый `<root>/<network>/rpc-pending` импортируется при open только для
  записей, чей UUID есть в tx-log этого кошелька; чужие/неоднозначные записи
  остаются вне профиля.
- **RPC open/restore.** При ошибке `open_wallet`/`open_with_timeout` wrapper
  делает best-effort `close_wallet`, чтобы потерянный ответ не оставил
  фактически открытый дочерний кошелёк. RPC-restore теперь отказывает не только
  при seed, но и при существующей `wallet_data/db`. После ошибки смены пароля
  проверяется новый пароль: envelope — через `prepare_unlock`, legacy — через
  чтение `wallet.seed`; если он подходит, ошибка сообщает, что пароль изменён,
  и `.history.key.next` остаётся.
- **Scan.** `prepare_scan` отвергает частичный скан при `pending_scan` и пустой
  диапазон `start >= tip`; после отмены restores требуется полный повтор.
  Живой тест 2026-10-26: отмена, затем полный скан — 84,14 с, `completed`.
  `scan_generation` увеличивается при смене сети/профиля; поздний результат
  скана проверяет его под wallet-lock и не пишется в новый кошелёк.
- **Mainnet legacy migration.** Явная проверка адреса в `wallet_migrate_seed`
  идёт через `verify_legacy_address`, который открывает legacy только внутри
  операции миграции; обычный mainnet legacy-open остаётся запрещён.
- **Настройки/пароль.** `settings_set` больше не принимает смену сети — только
  `network_switch`; смена пароля и RPC-restore используют общий
  `validate_password`. Миграция `seed` при этом не трогает upstream-пароль.
- **Messenger.** `GatewayTransport::fetch` разбирает элементы по одному:
  один битый payload больше не валит пачку, хорошие за первыми 20 плохими
  доставляются. Битые relay-id (не более 32 за fetch) ack-аются как карантин;
  подписанный relay такой ack отвергает, и элемент остаётся до TTL.
  `History::list` отдаёт **последние** N, а не первые. При ошибке
  смены пароля `.history.key.next` больше не удаляется: `resolve_history_key`
  повысит его новым паролем. История и контакты теперь лежат в
  `messenger/<kind>/<network>/<backend>/<wallet_id>/<profile>`; старый каталог
  копируется один раз только если его `.history.key` открывается текущим
  паролем. Если не открылся, в «Сообщениях» показывается путь и кнопка
  «Восстановить старую историю»: ввод старого пароля, backup текущего каталога
  в `trash/`, перенос старого.
- **Версии/упаковка.** `tauri.conf.json` синхронизирован с 0.1.69;
  `package.ps1` берёт версию из Cargo.toml, кладёт в zip `resources/node` и
  `resources/wallet`, а single-exe честно описан как не автономный. CI получил
  шаг `cargo vendor` + `fetch-node`/`fetch-wallet`; живой runner — self-hosted.
- **Смена адреса.** `wallet_rotate_address` останавливает активный Tor listener,
  чтобы новый onion не остался старым.
- **Адрес получателя.** В send-диалоге показывается полный адрес, QR и требуется
  чекбокс + ввод последних 8 символов; QR для адреса получения тоже есть.
  Логика хвоста вынесена в `ui/js/address.js` и проверяется
  `node tools/check-address-tail.mjs`; QR-генератор проверяется
  `node tools/check-qr.mjs`. Живой сценарий подмены буфера ещё не проверен.
- **Slint Phase 2.** `crates/slint-ui` — `flint-slint` на Slint 1.18.1 с
  реальным `WalletService`: lock screen, `open_wallet`, `summary`, фоновый
  `node_info`, `activity(20)`, реальный scan с прогрессом, ручной send/receive
  slatepack, settings (network/profile/node URL/relay/min-conf) и start/stop
  managed node. `SwapRuntime` вынесен и используется Tauri swap-командами;
  Slint показывает борд JANUS, умеет «Взять» и «Симулировать»; UIA видит
  `local-demo-board · 3 офферов`, три `XMR → GRIN` и
  `симуляция: завершён · swap_…`.
  **`flint-selftest`** — отдельный Tauri-free бинарь (`cargo run -p
  flint-runtime --bin flint-selftest`), проходит тот же lifecycle/Janus/node
  сценарий: `ok:true`, ~9,2 с. Slint копирует адрес/slatepack/ответ через
  `arboard` и рисует QR адреса через `qrcode` (`shots/slint-qr.png`).
  Slint создаёт/восстанавливает кошелёк; UIA-проверка создания показала seed и
  адрес (seed в логи не пишется). В Slint также смена пароля и удаление кошелька
  с подтверждением имени. Single-instance в Slint проверен: второй процесс
  завершается, первый остаётся. История — отдельный экран `activity(200)`;
  pending-операции можно повторить или отменить. Скан отменяется на границе
  батча; «Новый адрес» ротирует адрес и QR. В security показывается seed-envelope
  status; доступны миграция legacy → Argon2id, disable и quarantine repair.
  Tauri-free preview: `dist/Flint-0.1.69-slint-preview.zip`, release-бинарь
  30,8 МБ, GUI-subsystem; в пакете есть `flint-selftest.exe` (release, `ok:true`),
  `grin-wallet.exe` и Tor expert bundle; messenger в нём отсутствует.
  Добавлен общий `flint-runtime`; Tauri `wallet_status/summary/activity` и Slint
  используют его. UIA: lock screen содержит только пароль/кнопку, фоновые
  контролы скрыты; после unlock кнопки/список/поля имеют имена. Бинарь —
  GUI-subsystem, консольного окна нет. QR, swaps, messenger, network/profile
  switch, NVDA/IME/DPI и 1000+ строк ещё не проверены; Tor проверен UIA.
  Production — Tauri;
  [docs/ui/SLINT.md../../ui/SLINT.md).
- **Backend/секреты/скан.** Пароли и seed-фразы Tauri-команд принимаются через
  `Secret`-newtype и зануляются на drop; `backend_capabilities` — явный контракт
  возможностей; embedded-скан отдаёт `scan_done`/`scan_target` по батчам,
  RPC-скан прогресса не имеет. Границы бэкендов: [BACKENDS.md](../architecture/BACKENDS.md).
  Живой IPC-прогон `FLINT_VERIFY_SECRET_IPC=1 node tools/verify-ui.mjs`:
  create → lock → open → delete — ok. `ensure_local_ipc` отклоняет команды,
  если webview больше не на локальном origin; per-command allowlist для
  app-команд Tauri 2 не предоставляет — остаточное ограничение.
- **Readiness ноды.** `no_sync` без `sync_info` у подключённой ноды теперь
  считается `Ready`: Grin не отдаёт `highest_height`, когда догнал цепь.
  Раньше полностью синхронизированная нода навсегда оставалась `Unknown` и
  блокировала scan/send. Без пиров тот же статус остаётся `Unknown`.
- **Живые проверки:** `live-rpc-lifecycle` (real grin-wallet:
  open/lock/delete; send → `NotEnoughFunds`), `live-history-recovery`
  (старый ключ под другим паролем → восстановление через IPC),
  `live-scan-cancel` (`scan_done/scan_target`, отмена и полный retry-скан).
- **Anti-spam.** Новый конверт несёт SHA-256 proof-of-work (`pow_nonce`/
  `pow_bits`), P2P-gateway и relay требуют его до записи; release — 20 бит,
  debug — 12. Для P2P снижены лимиты: 30 сообщений, 32 КиБ payload, 12 ч TTL,
  30 POST/мин. Добавлен локальный block/mute: заблокированный отправитель
  ack-ается и не попадает в историю. Квоты и rate limit по-прежнему не
  различают отправителей за Tor — это остаточное ограничение.
- Остаток аудита: per-command allowlist для app-команд (нужен Tauri-плагин)
  и успешный чистый CI-прогон; остальное — внешние блокеры из
  [MAINNET_READINESS.md](../status/MAINNET_READINESS.md#L75).

## 4. Остаётся (P1/P2)

**P0 фазы 0.5 закрыт** (0.1.44/0.1.47): owner API требует Basic-auth, секрет
создаётся до старта, loopback проверяется, запуск fail-closed.

1. **Мессенджер.** M1/M2/M3 закрыты по коду: прямой onion (0.1.65), relay и
   подписанный ack (0.1.66). Осталось: публичный relay/PoW и rate limit,
   forward secrecy/ratchet, push и группы.
2. **Фаза 2 (остаток).** Личность ноды (chain/user_agent), пиры для ready,
   подключение к запущенной ноде, свежесть tip, отмена RPC-скана и
   кооперативная отмена embedded-скана между батчами сделаны. Живой
   endpoint-switch и graceful stop проверены; смена сети проверена в пустом
   временном mainnet-каталоге, mainnet-кошелёк создан изолированно, реальный Tor
   bootstrap/остановка проверены. Осталось: живой прогон с реальным
   mainnet-кошельком/сетью (не делался).
3. **CORS вне прокси.** Управляемая нода по умолчанию закрыта прокси (0.1.54).
   CORS остаётся, если выключен `node_proxy` (прямой режим), используется внешний
   `node_url` или нода запущена скриптом напрямую на 13413.
4. **PBKDF2-100 / Argon2id.** Конверт работает для обоих бэкендов, legacy
   мигрируется по кнопке, mainnet без конверта не открывается, keywrap переведён
   на Argon2id v2, kill-дрилл M0–M5 и адверсариальное ревью — в разделах
   «Фаза 3, срез 5» и «ревью миграции» ([REVIEW_MIGRATION.md](REVIEW_MIGRATION.md)).
   Осталось: внешнее человеческое ревью, обрыв питания. Живой прогон с нодой
   выполнен; `.flint-seed.lock` удерживается на весь lifecycle Flint
   (внешний `grin-wallet` его не читает).
5. **UI-долги (остаток).** Смена пароля и удаление кошелька сделаны для
   встроенного бэкенда (удаление — перенос в trash/, подтверждение именем; для
   RPC — вручную). Pending-файлы получили ACL, но остаются открытым JSON.
6. **Права IPC для собственных команд.** В Tauri 2 они не ограничиваются
   capability-файлом; devtools выключены в release, защита — CSP и одно окно.
7. **Live-тесты вручную.** `tools/run-live-suite.ps1` и CI-workflow готовы,
   сценарий прогнан (0.1 GRIN, подтверждение и отмена); живой job требует
   self-hosted runner с нодой, облачного провайдера в проекте нет.
## 5. Известные ограничения окружения

- **WebView2 не стартует под файловой песочницей**: создание окна падает с
 `HRESULT 0x8000FFFF` («Разрушительный сбой»). Это не баг приложения — под обычной
  сессией окно поднимается (проверено `tools/verify-ui.mjs`). Не отлаживайте GUI
  из-под песочницы.
- Зависший процесс `msedgewebview2.exe` держит каталог профиля; после аварийного
  завершения запускайте проверку со свежим `WEBVIEW2_USER_DATA_FOLDER` (скрипт так
  и делает).
- Тестовая цепь Grin — миллионы блоков: полноценные сценарии требуют отдельного
  синхронизированного узла (см. [ROADMAP.md](../status/ROADMAP.md)).

## 6. Как воспроизвести

```powershell
$env:TMP = "$PWD\.tmp"; $env:TEMP = "$PWD\.tmp"   # иначе MSVC падает на длинных путях
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace                             # ожидается 212 passed, 13 ignored
cargo build -p flint-app
node tools/verify-ui.mjs                           # окно + CSP + navigation guard
pwsh -File tools/ci.ps1 -SkipUi                     # fmt/check/tests/build/selftest
pwsh -File tools/release.ps1 -SkipTests             # release + SBOM + portable zip (unsigned)
node tools/live-scan-cancel.mjs                    # живая отмена embedded-скана (нода запущена)
node tools/live-rpc-scan-cancel.mjs                # живая отмена RPC-скана (нода запущена)
pwsh -File tools/run-testnet-node.ps1 start        # testnet-нода для живых гейтов
pwsh -File tools/run-testnet-node.ps1 status       # высота, размер цепочки, батчи заголовков
```

Изоляция самотеста (каталог-приманка должен остаться побайтово тем же):

```powershell
$fixture = "$PWD\.tmp\fixture"; # ...создать testnet\wallet\wallet_data\wallet.seed и settings.json
$env:FLINT_DATA_DIR = $fixture
.\target\debug\flint-app.exe --selftest
```

Проверки, которые в этот статус **не** входят и обязательны до заявки о готовности:

1. Два одноразовых testnet-кошелька на синхронизированной ноде: send → receive →
   ответ → finalize → post → подтверждение.
2. Отмена на живой ноде: `retrieve_txs` показывает cancelled, выходы снова spendable.
3. Крэш в каждой точке: до/после записи pending, до/после блокировки, до/после
  `finalize_tx`, до/после `post_tx`.
4. Mainnet: только после фазы 6 и вручную.
5. Нода: `get_status` без `Authorization` → `401`; `api_http_addr` только
   loopback; owner-метод нельзя выполнить из браузера.





