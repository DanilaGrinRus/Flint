# Журнал изменений и проверок Flint

Новые записи добавляются сверху после этого блока. Правила — [README.md](README.md).
Прежние записи 01–22 — [historical/WORK_LOG.md](historical/WORK_LOG.md).

---

## 81 — 2026-10-29 — RPC pending export/resend live

- Добавлен ignored live-test `crates/runtime/tests/rpc_pending_export_live.rs`.
- На подготовленных RPC testnet-кошельках:
  - S1 экспортирован и повторён без новой подписи, armor byte-identical;
  - S2 экспортирован из RPC-журнала, slate_id совпал;
  - транзакция финализирована, получатель увидел +1000 нано;
  - **PASS** за 115.20 с.
- Тест требует `FLINT_LIVE_TESTNET=1` и оба RPC-пароля; без них — FAIL
  конфигурации, не ранний PASS.

**Проверки:** полный CI после правок — **PASS**.
**Остаток:** replay fault/restart, GitHub live jobs, security-диалоги.

---

## 80 — 2026-10-29 — manual 150% and remaining dialogs

- Масштаб 150% проверен на чистом testnet-каталоге:
  `SLINT_SCALE_FACTOR=1.5`, окно 1920×1290, onboarding отрисован без
  обрезаний — **PASS**.
- На 150% проверены диалоги создания и восстановления кошелька — поля
  пароля/seed доступны, обрезаний нет.
- На обычном масштабе открыт QR-диалог (адрес, «Закрыть») — **PASS**.
- Tor live: `direct_onion` 1, `tor_hidden_service` 2,
  `tor_listener_gateway` 1 — **PASS**; `relay_live` — PASS.
- RPC S1 export и response export реализованы; live-evidence pending-действий
  ещё не получено.
- Матрица/DEVICE_ACCEPTANCE обновлены; полный статус остаётся
  **NOT READY_FOR_EXTERNAL_DEVICE_TESTS** из-за replay/live-evidence/GitHub
  и не всех модальных состояний.

---

## 79 — 2026-10-29 — transfer-contract v1 и RPC export

- Добавлен `crates/transfer-contract` v1: типизированные NanoGrin, Network,
  Stage/Action/NextAction, DTO schema 1, контрактные тесты и fixtures.
- Desktop реально использует контракт в pending-гейтах
  (`pending_actions`/`pending_next_action`).
- `RpcBackend::export_receive_response` пересобирает сохранённый response
  из RPC-журнала без повторного `receive_tx`; runtime/UI больше не скрывают
  RPC-экспорт.
- Версионированный source package:
  `lastver/flint-transfer-contract-1.0.0.zip`, SHA-256
  `ca7ebbc51a73fd1275422b9e5b697028879264d16ec5501e92d0c4395ac6d377`.
- Portable RC: `dist/Flint-0.1.69-slint-preview.zip`, exe
  `bd4fceef…`; добавлены BUILD-INFO, SBOM, источники, лицензии, матрица,
  STAGEREADY и DEVICE_ACCEPTANCE. Запуск распакованного пакета вне дерева —
  PASS.
- Полный CI после изменений: **323 passed, 0 failed, 31 ignored**; UI 21.

**Не закрыто до READY:** RPC resend S1, RPC export live-evidence,
messenger replay fault path, Tor live (нет `tor.exe`), GitHub live jobs,
150% scaling. Это перечислено в `RC_ACCEPTANCE_MATRIX.md`.

---

## 78 — 2026-10-29 — activity_handlers

- Swaps/copy, pending/history reload, retry/cancel/export, rotate/seed
  maintenance вынесены в `activity_handlers.rs` (506 строк).
- `main.rs` — **2098 строк**.
- Сохранены pending gate/watchdog, generation guards, clipboard и seed
  подтверждения; порядок установки handlers восстановлен.
- После выноса: полный CI **320 passed, 0 failed, 31 ignored**, UI 21.

Остались wallet/unlock/scan/send/receive/profile handlers.

---

## 77 — 2026-10-29 — messenger_handlers вынос

- Messenger open/send/inbox, contacts, history, block/recovery/close
  вынесены в `messenger_handlers.rs` (562 строки).
- `main.rs` — **2580 строк**.
- Сохранены auto-process flag, generation guards, gate, allow-list,
  durable inbox/outbox и санитизация.
- Release-проверка: unlock alice → «Сообщения» открываются; полный CI —
  **320 passed, 0 failed, 31 ignored**, UI 21.

Остались wallet/profile/scan и pending/history/copy handlers.

---

## 76 — 2026-10-29 — messenger_handlers

- Messenger open/send/inbox, contacts, history, block/recovery и close
  вынесены в `messenger_handlers.rs` (562 строки).
- `main.rs` — **2580 строк**.
- Сохранены auto-process flag, generation guards, pending gate, контактный
  allow-list и outbox/durable queue пути.
- Release-проверка: unlock alice → «Сообщения» открываются, контакт и
  заголовок переписки на месте.

**Проверки:** полный CI **320 passed, 0 failed, 31 ignored**, UI 21.

Остались wallet/profile/scan handlers в `main.rs`.

---

## 75 — 2026-10-29 — onboarding_handlers

- Вынесены create, restore, seed reveal, смена пароля и удаление кошелька
  в `onboarding_handlers.rs` (266 строк).
- `main.rs` — **3115 строк**.
- Сохранены Zeroizing паролей/мнемоники, generation guard, busy-state,
  post-create история/recent-profile, очистка unlock-кэша после смены пароля.
- В release-сборке после выноса UI-тесты 21 passed; полный CI — 320 passed,
  0 failed, 31 ignored.

Остались в `main.rs`: wallet/profile/scan и messenger handlers.

---

## 74 — 2026-10-29 — settings_handlers и дальнейшее разделение UI

- Добавлен `settings_handlers.rs`: settings open, data-root browse/save,
  language save и полный settings save с generation/pending gate.
- `presentation.rs` и `tor_node.rs` сохраняют прежние обязанности.
- `main.rs` сокращён до **3372 строк**; всего вне main: format, presentation,
  sanitize, platform, tor_node, settings_handlers, i18n(+generated).
- Ручной launch release после разделения: unlock alice → «Настройки»
  открываются, поля сети/профиля на месте.
- `Snapshot`, `SharedPendingGate` и helper-функции помечены `pub(crate)`;
  поведение gate не менялось.

**Проверки:** полный CI **320 passed, 0 failed, 31 ignored**, UI 21.
Остались крупные messenger/wallet handlers — разделение продолжается.

---

## 73 — 2026-10-29 — presentation/tor-node модули и единый i18n source

- Из `main.rs` дополнительно выделены:
  - `presentation.rs` — contact/message/history/blocked/swap rows;
  - `tor_node.rs` — обработчики node start/stop и Tor start/stop/refresh/
    bridges/log.
- `main.rs` сокращён примерно с 3970 до 3750 строк; gate-проверки,
  generation guards и порядок регистрации сохранены.
- `lang/catalog.json` — единый источник; `.po` и `i18n_catalog.rs`
  генерируются `tools/i18n-gen.ps1`; `-Check` в CI.
- Release-сборка после разделения запущена вручную: обзор alice открылся,
  UI без регрессии (скрин `.tmp/manual-ui-split.png`).

**Проверки:** полный CI **320 passed, 0 failed, 31 ignored**, UI 21;
`i18n-gen -Check` — current.

---

## 72 — 2026-10-29 — единый источник локализации и закрытие ручных остатков

- Добавлен `lang/catalog.json` — единый авторский источник:
  - `static` (msgctxt/key/en) для `.po`/Slint `@tr`;
  - `dynamic` (key/en) для Rust `CATALOG`.
- `tools/i18n-gen.ps1` генерирует
  `crates/slint-ui/lang/en_US/LC_MESSAGES/flint-slint.po` и
  `crates/slint-ui/src/i18n_catalog.rs`; `-Check` встроен в `tools/ci.ps1`.
- `i18n.rs` больше не хранит CATALOG вручную, а включает сгенерированный файл;
  проверки дублей/placeholders сохранены.
- Исправлена потерянная при извлечении строка
  «Полный rescan изменил баланс…»; восстановлена в source и
  сгенерирована.

**Проверки:** полный CI **320 passed, 0 failed, 31 ignored**, UI 21.
`i18n-gen -Check` — current, `.po` — 215 entries, generator check — pass.

**Live Tor:** `resources/tor/tor.exe` отсутствует — live-набор не запускался,
**NOT VERIFIED**. 150% scale в текущем окружении окно не создало —
**NOT VERIFIED**. GitHub live jobs не запускались.

---

## 71 — 2026-10-29 — ручная проверка release UI

- Release-пакет `lastver/Flint-0.1.69-keyboard`, данные
  `C:\DHW\.tmp\ui-rustsec-live`, профиль alice.
- Проверены вживую:
  - обзор кошелька: балансы «Доступно / Ожидает / Заблокировано»,
    активный профиль, нода, последние операции;
  - история: «Незавершённые переводы», `response_received`, кнопка
    «Экспорт ответа»;
  - экспорт: clipboard 1902 символа, `BEGINSLATEPACK.`/`ENDSLATEPACK.`,
    подтверждение в UI, pending остался `response_received`;
  - сообщения: контакты, поле получателя, переключатель автофинализации.
- Визуально muted-подписи читаются на графитовой поверхности; новых
  обрезаний/наложений не обнаружено.
- Дополнительно проверены настройки (сеть, язык, профиль, каталог, Node/Relay)
  и экран отправки (адрес, сумма, slatepack, ответ получателя) — **PASS**.
- Масштаб 125% (`SLINT_SCALE_FACTOR=1.25`): окно 1600×1075, обзор без
  обрезаний — **PASS**.
- Масштаб 150%: в этом окружении окно не появилось (процесс завершался до
  создания окна) — **NOT VERIFIED**, причину не выдаю за дефект UI.
- Кадры: `.tmp/manual-ui-wallet.png`, `manual-ui-history.png`,
  `manual-ui-messages.png`, `manual-ui-settings.png`, `manual-ui-send.png`,
  `manual-ui-scale125.png`.

**PASS:** ручная проверка перечисленных экранов, экспорта, настроек,
отправки и масштаба 125%.
**Не проверено:** масштаб 150%, все остальные модальные состояния.

---

## 70 — 2026-10-29 — CI/live, lifecycle guards, contrast, modules, i18n, sanitizing

- **CI:** `tools/ci.ps1` теперь включает `cargo test --workspace --doc` и
  `tools/i18n-check.ps1`; desktop workflow проверяет doctests и rustdoc,
  сборочный/headless-гейт сохранён. Live-задания вынесены в отдельные
  `workflow_dispatch` jobs `live-tor` и `live-funded-testnet` с timeout 25/90
  минут; при выборе без условий preflight падает, невыбранные job — skipped.
- **transaction_lifecycle:** явный запуск без `FLINT_LIVE_TESTNET=1` теперь
  FAIL, а не PASS. Full и scan-only разведены отдельными тестами; full
  отказывается при `FLINT_LIVE_SCAN_ONLY=1`. Добавлен
  `tools/run-transaction-lifecycle.ps1` (маркеры, node auth, timeout, без
  вывода паролей).
- **Контраст:** `Theme.muted` `#707078` → `#8b8b95`; расчёт 5.25:1 на
  surface `#18181c`; `accent-dim` пересчитан 8.37:1 и не менялся. Добавлен
  регрессионный тест контраста.
- **Модули:** из `main.rs` выделены `format.rs`, `sanitize.rs`,
  `platform.rs`; границы описаны в doc-комментариях. Поколение/busy/gate
  не менялись.
- **Локализация:** добавлен проверочный тест CATALOG (дубли, placeholders)
  и `tools/i18n-check.ps1` для `.po`; устранены фактические дубли
  `история пустая`, `Tor…`, `заблокированы…`, `кошелёк занят`,
  `каталог задан…`.
- **Внешний текст:** UI-представление контактов, сообщений, истории и
  node-ошибок проходит через `sanitize` (control/bidi, лимиты); оригиналы
  адресов/slatepack/подписей не меняются.
- **seed_spike:** обычный тест сохранён, временный корень удаляется Drop
  только для своего уникального каталога.

**Проверки:** полный CI/workspace **320 passed, 0 failed, 31 ignored**;
UI **21 passed**; i18n-check `.po` 215 entries — ok. Live embedded full cycle
PASS (send/receive/finalize/post/confirm/cancel/restart); RPC live отдельно
на подготовленных RPC-кошельках — **PASS** (309.62 с, тот же цикл).
Windows workflow/live jobs на GitHub не запускались.

---

## 69 — 2026-10-29 — пять остаточных замечаний

- **Порядок очередь/история:** входящий armor пишется в durable queue и
  `save_inbox` до `history.record`. Если сообщение уже в истории, но задачи
  нет, она восстанавливается из armor до ack. Падение между записями больше
  не оставляет «известное сообщение без задания».
- **Шифрование outbox:** `MessageKind` больше не пишется в JSON открытым
  текстом. Kind шифруется ChaCha20-Poly1305 ключом истории с nonce и AAD
  envelope id; `OutboxEntry` хранит только ciphertext/nonce, конверт остаётся
  для повторной доставки. Тест проверяет отсутствие plaintext в файле.
- **Дубли ответа:** если доставка/история ответа не завершились, `messenger_send`
  возвращает `queued: true` с тем же envelope id, а не ошибку, провоцирующую
  повторную обработку входного задания. UI помечает задачу `done`; outbox
  повторяет доставку и историю с тем же id.
- **Диагностика:** URL очищается от userinfo, query и fragment; строки
  `node`/`status` проходят рекурсивную очистку URL. Добавлен unit-тест
  credentials/query.
- **Pending actions:** для `prepared/locked` текст больше не обещает
  несуществующую пересылку S1; для RPC экспорт не предлагается, а текст
  заранее объясняет ограничение. Кнопка экспорта показывается только для
  embedded `receive/response_received`.

**Проверки:** targeted tests 9 runtime lib, 19 rpc_lifecycle, 15 UI; full
workspace прогоняется.

---

## 68 — 2026-10-29 — funded testnet restore в отдельном каталоге

- Отдельный каталог `C:\DHW\.tmp\restore-funded`; использован выбранный
  одноразовый funded testnet seed (24 слова) из проектных testnet-реквизитов.
  Существующие кошельки/fixtures не изменялись.
- После restore приложение показало
  `кошелёк восстановлен; разблокируйте паролем`.
- Полный скан: `scan_state=completed`, `pending_scan=false`;
  **баланс проверен**, доступно **1397.390000000 GRIN**, адрес совпал с
  прежним (`tgrin1flemga…4svprm53`).
- UI-процесс остановлен; каталог оставлен для проверки.

**PASS:** funded testnet restore и обнаружение прежних средств после полного
скана.

---

## 67 — 2026-10-29 — ручная проверка pending/export

- Стенд `C:\DHW\.tmp\ui-rustsec-live`, alice, pending
  `e4359fe8-…` в `response_received`.
- В «История» → «Незавершённые переводы» видны:
  «Экспортировать ответ и завершить приём», `response_received`,
  «Получено · pending».
- Нажата кнопка «Экспорт ответа»: UI показал
  «ответ экспортирован в буфер обмена», clipboard содержит
  1843 символа, начинается `BEGINSLATEPACK.` и заканчивается
  `ENDSLATEPACK.`.
- Pending после экспорта остался `response_received`; это ожидаемо —
  экспорт не подписывает и не меняет перевод.

**PASS:** ручной UI-экспорт и pending-состояние.
**Не проверено:** экспорт для RPC-журнала (там он явно недоступен).

---

## 66 — 2026-10-29 — crash/restart drill for durable inbox queue

- Стенд `C:\DHW\.tmp\ui-rustsec-live`, отдельный queue-файл с real slatepack
  armor; приложение запущено с `FLINT_CRASH_AFTER_INBOX=1`.
- После записи входящей очереди и до автообработки процесс убит
  `Stop-Process -Force`; файл `inbox.json` остался с `state=received`.
- Повторный запуск без failpoint, unlock alice: автоцикл вернул запись,
  обработал её и удалил `inbox.json` (`queue cleared`).
- Это подтверждает именно crash/restart между записью очереди и автоответом,
  хотя исходное сообщение было подготовлено в queue-файле тестом, а не
  получено по сети в том же процессе.

**Проверки:** queue до убийства — есть; после убийства — есть; после рестарта
и auto-inbox — удалена. UI-процесс после прогона остановлен.

---

## 65 — 2026-10-29 — durable outbox, ack-error fix, cache, balances, diagnostics

- **Slatepack ack failure:** `messenger_inbox` now returns opened messages even
  when `ack_signed` fails (`ack_error` in JSON). The next poll retries the ack
  and skips the known history record; auto-finalization is no longer skipped
  after a failed delivery confirmation.
- **Durable outbox:** `messenger_send` seals/persists an `OutboxEntry` before
  delivery, reuses the same envelope id on retry, records history only after
  delivery, and removes the entry after both succeed. `messenger_inbox` drains
  pending entries and reports `outbox_errors`.
- **Durable inbox queue:** opened armor is persisted with `id/from/armor/
  created_at/state`; states `processing`, `reply_sent`, `failed`, `done` are
  updated from the Slint auto loop. `done` removes the entry, failures are
  returned again after restart; `messenger_inbox_mark` is the single writer.
  This closes the crash window between inbox and automation, while the existing
  contact/block/quarantine checks remain in place.
- **Password cache:** `change_password_with_history` clears the five-minute
  unlock cache after the change attempt, including ambiguous RPC errors; the
  Slint handler also clears it before reporting the result.
- **Balances:** available, awaiting and locked/immature are now separate UI
  fields; the old code displayed only `awaiting` after summing locked locally.
- **Pending center:** rows show stage-specific next action text and the section
  is named «Незавершённые переводы»; receive rows in `response_received` expose
  «Экспорт ответа», which rebuilds/copies the saved response without signing.
  It still does not claim network confirmation from message delivery.
- **Node errors/diagnostics:** error labels append the actionable hint
  (API secret/connection/network/retry); «Копировать диагностику» exports a
  redacted JSON snapshot (no password/seed/private key/API secret).
- **iOS contract:** [architecture/IOS_SHARED_CONTRACT.md](../architecture/IOS_SHARED_CONTRACT.md)
  defines platform-neutral stages/DTOs/retry rules; desktop and iOS remain
  separate directories/backends.
- **CI:** `tools/ci.ps1` now includes `cargo doc --workspace --no-deps` so the
  previous rustdoc E0463 class cannot hide behind tests.

**Tests added:** outbox envelope round-trip; inbound queue restart round-trip;
diagnostics redaction; pending action text. **CI:** 313 passed, 0 failed,
29 ignored; UI 15 passed; `cargo doc --workspace --no-deps` passed inside
`tools/ci.ps1`; selftest ok.
The previous rustdoc E0463 did not reproduce in this tree; the docs step is now
part of CI so it cannot be skipped.

---

## 64 — 2026-10-29 — age 0.11.5: curve fix без p256/crypto-bigint

- `age` понижен с 0.12.1 до **0.11.5**: там уже `x25519-dalek 2.0.1`, но нет
  p256/hpke-цепочки, которая в 0.12 тянет crypto-bigint 0.5.5 в сборку.
- API `Encryptor`/`Decryptor` тот же; slatepack/workspace тесты проходят.
- `Cargo.lock` хеш `66db2214…`; RustSec остаётся **2 accepted** (crypto-bigint
  и rsa), warnings 17.
- Vendor пересобран; старый `vendor-age12` сохранён в `.tmp`.
- Реальный testnet-receive 0.001 GRIN ранее прошёл на age 0.12.1/zip 9/clap 3;
  после финального age 0.11.5 отдельный live-прогон не повторялся.

---

## 63 — 2026-10-29 — clap 4.6.7, yaml-rust удалён

- `clap` обновлён до **4.6.7**; feature `yaml` больше не используется.
- Добавлен `serde_yaml 0.9.34` и собственный адаптер
  `third_party/grin-wallet/src/cli/yaml.rs`: превращает существующий
  `grin-wallet.yml` в `clap::Command`.
- Добавлен `ClapCompat` (`value_of`/`is_present`) для кода fork-а;
  `subcommand()` нормализован; CLI собирается.
- `grin-wallet --help` и `account --help` реально работают.
- `yaml-rust` полностью исчез из `Cargo.lock`; RustSec: **2 accepted,
  16 warnings**.
- Проверки: workspace 309 passed; CLI help; selftest.

---

## 62 — 2026-10-29 — реальный slatepack после age/zip/clap

- Стенд `C:\DHW\.tmp\ui-rustsec-live`, свежая копия funded-testnet default.
- default отправил настоящий **0.001 GRIN** на alice через loopback relay;
  alice приняла armor на бинаре с age 0.12.1, zip 9.1.3 и clap 3.2.25.
- pending: `receive ... state=response_received`; sender остался `locked`.
- Это подтверждает encrypt/decrypt нового slatepack и путь auto-receive;
  финализация/подтверждение этой микропередачи не доведены.

---

## 61 — 2026-10-29 — zip 9, clap 3, yaml-rust 0.4.5; RustSec 2

- `grin_util`: `zip 0.5.13 → 9.1.3`; адаптированы `SimpleFileOptions`,
  потребляющий `finish(self)`, fallible `mangled_name`, убран лишний
  `BufWriter`; `cargo test -p grin_util zip` — passed.
- `grin-wallet`: `clap 2.34 → 3.2.25` + `yaml-rust 0.4.5`. Сохранён YAML CLI;
  `subcommand()` нормализован helper-ом под старые match-арм-формы; `ArgMatches`
  без lifetime, `crate_version!` заменён на `CARGO_PKG_VERSION`.
- `cargo check --workspace` офлайн — passed; CLI-бинарь собирается.
- RustSec: **2 accepted, 0 unaccepted** (crypto-bigint, rsa); `yaml-rust` и
  `zip` исчезли.
- **arti/rsa blocker:** проверен arti 0.47 — `tor-key-forge 0.47` всё ещё
  зависит от `rsa 0.9`, патча RSA нет; обновление не убирает advisory.
  `crypto-bigint` в текущую сборку не входит.
- Vendor пересобран; старые `vendor-age`/`vendor-zip` сохранены в `.tmp`.

**Проверки:** полный workspace — 309 passed, 0 failed, 29 ignored; UI 14;
RustSec 2 accepted. Реальный testnet-receive после всех замен ещё не прогнан.

---

## 60 — 2026-10-28 — age 0.8.2 → 0.12.1, curve25519-dalek 3.2.0 закрыт

- Обновлён fork `grin_wallet_libwallet`: `age = "0.12"`; lock получил
  `age 0.12.1`, `x25519-dalek 2.0.1`, `curve25519-dalek 4.1.3`.
- Адаптирован API `age`: `Encryptor::with_recipients` теперь iterator + `Result`;
  `Decryptor` — struct без enum `Recipients`; поправлена сериализация
  `DalekSignature::to_bytes` под новую неоднозначность `AsRef`.
- Вендор пересобран через `cargo vendor` с сетью; старый vendor сохранён как
  `vendor-old-age` до финальной проверки.
- RUSTSEC-2024-0344 исчез: RustSec теперь **6 accepted, 0 unaccepted**;
  из `tools/rustsec.ps1` убран принятый ID.
- Проверки: workspace **309 passed, 0 failed, 29 ignored**; slatepack-тесты
  `grin_wallet_libwallet` — 8 passed; UI 14.

---

## 59 — 2026-10-28 — RustSec-обновления выделены в отдельный трек

- Создан [security/RUSTSEC_UPGRADES.md](security/RUSTSEC_UPGRADES.md): инвентарь
  семи advisories, владельцы перехода (`age`, `zip`, `clap`, `arti/crypto-bigint`,
  RSA), порядок работ, offline-vendor ограничения и гейты приёмки.
- Трек **не начат**; 7 advisories остаются accepted. Это не разрешает mainnet.
- Добавлены ссылки в `docs/README.md`, `docs/PLAN.md`, `docs/KRead.md` и
  `docs/status/STATUS.md`.
- RustSec-аудит остаётся зелёным: 7 accepted, 0 unaccepted.

---

## 58 — 2026-10-28 — batch, unknown sender, persist auto, локальные лимиты

- **Auto-чекбокс**: состояние сохраняется в `<data>/auto-slatepacks`,
  загружается при старте; unit round-trip + UI-проверка после перезапуска
  (файл `1`, чекбокс `On`).
- **Неизвестный отправитель**: настоящий armor от bob доставлен alice, когда
  bob не был в контактах; alice не создала receive-pending и не отправила
  ответ. После добавления bob в контакты его slate появился в pending —
  contact-gating проверен на реальном armor.
- **Batch**: после добавления контактов alice приняла два настоящих armor
  (0.05 GRIN и 0.002 GRIN) в одном включённом auto-цикле; появились два
  `response_received` pending для двух разных slate. Доставка/финализация
  обоих ответов отправителям не доведена до confirmed.
- **Local direct/per-IP**: unit direct onion transport — PASS; локальный
  per-IP bucket (изолированный от другого IP) — PASS. Реальный non-loopback
  per-IP путь не проверялся: loopback намеренно пропускает IP-bucket.
- **RustSec**: `tools/rustsec.ps1` — 7 accepted, 0 unaccepted; обновление
  fork-зависимостей остаётся отдельным треком.

**Проверки:** full workspace прогоняется; UI 14 passed; подробности ниже.

---

## 57 — 2026-10-28 — подтверждённый testnet-перевод и change

На стенде `C:\DHW\.tmp\ui-finish`:

- default отправил настоящий slate на **1 GRIN**;
- alice приняла его и отправила response;
- response был восстановлен идемпотентным `prepare_receive` и повторно
  доставлен на default;
- default выполнил `finalize_send`/`post_tx`;
- после скана и нескольких блоков обе стороны перешли в `confirmed`:

```text
alice: receive → confirmed, available 1.000000000 GRIN
default: send → confirmed, available 1397.390000000 GRIN
```

Баланс default корректен: `1398.413000000 - 1.000000000 - 0.023000000 = 1397.390000000`.

**Проверки:** язык переключается на лету; UI — 14 passed; полный workspace —
309 passed, 0 failed, 29 ignored; снимок `20261028-auto-local`; exe SHA-256
`e3128234…`. Реальный batch двух slate и unknown-sender остаются открытыми.

---

## 56 — 2026-10-28 — язык переключается на лету

- В `apply_ui_language` добавлен runtime-вызов
  `slint::select_bundled_translation("en_US")` / `""`; UI перерисовывается без
  перезапуска.
- Убрана подпись «(после перезапуска)»; после выбора показывается
  «Language switched to English» / «Язык переключён на русский».
- Проверено реальным кликом по ComboBox: файл `ui-language` стал `en`,
  интерфейс сразу перешёл на `Settings`, `Network`, `Interface language`.
- Обновлён EN-каталог (`Язык интерфейса`, `Переключаю язык…`).

**Проверки:** UI — 14 passed; полный workspace — 309 passed, 0 failed,
29 ignored; снимок `20261028-auto-local`; exe SHA-256 `e3128234…`.

---

## 55 — 2026-10-28 — переключение языка интерфейса

- `ui-language` сохраняется в корне данных; реальный клик по ComboBox
  «English» записал `ui-language=en`, после перезапуска интерфейс открылся
  на английском (`Open wallet`, `Wallet locked`).
- Для надёжности добавлен отдельный callback `language-save-requested(string)`
  вместо перегруженного `ui-language-changed`; ComboBox вызывает его.
- Переключение остаётся **после перезапуска** — это указано в UI.

**Проверки:** UI — 14 passed; полный workspace — 309 passed, 0 failed,
29 ignored; снимок `20261028-auto-local`; exe SHA-256 `e3128234…`.

---

## 54 — 2026-10-28 — реальный single-slate auto на alice

На сохранённом стенде `C:\DHW\.tmp\ui-real` (default funded, alice-получатель):

- отправлен настоящий slate на **0.1 GRIN**;
- чекбокс автофинализации на alice включён;
- фоновый цикл снял диагностику:
  - `prepare ok slate=14355d57… amount=100000000`
  - `finalize_receive ok`
  - `response sent`.
- То есть реальный входящий slate был автоматически подготовлен, финализирован и
  response отправлен без ручного нажатия «Принять».

Ограничение: response на default в этом прогоне не подтверждён (в истории
alice виден только входящий armor), batch из двух slate и confirmation/change
остаются открытыми.

**Проверки:** UI — 14 passed; полный workspace — 309 passed, 0 failed, 29 ignored;
снимок `20261028-auto-local`; пакет обновлён после удаления временного лога.

---

## 53 — 2026-10-28 — авто-ошибки не теряются при открытии messenger

**Изменение:** `on_messenger_open_clicked` больше не очищает `messenger-error`
и `messenger-note`, если включён `auto-process-slatepacks`. Раньше авто-ошибка
(например, `prepare_receive` для чужого/устаревшего slate) исчезала при
открытии переписки; теперь пользователь видит её после фонового цикла.

**Проверки:** UI — **14 passed**; полный workspace — **309 passed, 0 failed,
29 ignored**; снимок `20261028-auto-local` — 684 файла, 62 459 616 байт,
`match:true`; release exe SHA-256 `e3128234…`.

---

## 52 — 2026-10-28 — диагностика node-secret

**Изменения:**

- `crates/core/src/node.rs`: добавлен `fetch_native_status_result(...)`; прежний
  `fetch_native_status(...) -> Option` оставлен как обёртка. Ошибки 401/403 теперь
  дают явное «нода отклонила API-секрет (401/403)», отдельно от таймаута/HTTP.
- `crates/core/src/model.rs`: `NodeInfo.status_error` хранит причину, по которой
  native status не прочитан.
- `crates/core/src/service.rs`: ошибка native status попадает в `NodeInfo`;
  несовпадение сети/версии Grin тоже объясняется.
- `crates/slint-ui/src/main.rs`: при `NodePhase::Unknown` UI показывает
  `нода: <причина>` вместо безликого «статус ноды неизвестен».
- Добавлен тест: неверный owner-secret даёт ошибку с `API-секрет`.

**Проверки:** полный workspace — **309 passed, 0 failed, 29 ignored**; UI —
14 passed. Внешние PS и отложенные треки не затрагивались.

---

## 51 — 2026-10-28 — фоновый inbox и батч авто-финализации

**Изменения:** `crates/slint-ui/src/main.rs`

- добавлен `auto_inbox_timer`: при `unlocked && auto-process-slatepacks && !messenger-busy`
  раз в 10 секунд вызывается inbox-обработчик; кнопка больше не обязательна;
- убран `break` после первого armor-сообщения: теперь обрабатывается весь
  доступный батч, успехи и ошибки накапливаются и выводятся вместе;
- для ответа на отправку по-прежнему перебираются только `pending` send-записи;
  первая успешная финализация помечает ответ, остальные armor продолжают
  обрабатываться.

**Проверки:** `cargo fmt --all`, `cargo check -p flint-slint`, UI — **14 passed**;
полный набор — **309 passed, 0 failed, 29 ignored**; снимок
`20261028-auto-local` — 684 файла, 62 459 616 байта, `match:true`; release exe
SHA-256 `e3128234…`. Реальный первый входящий auto проверен: alice получила
funded-slate без ручного inbox-нажатия, фоновый таймер создал outgoing response
в истории. Доставка/финализация этого response на default в фоновом прогоне не
подтверждена; отдельный прогон ранее подтвердил автофинализацию response от
контакта. Реальный батч из двух armor одновременно не изолирован — нужен свежий
funded-slate после подтверждения. Скриншот реального slatepack —
`docs/assets/shots/flint-slatepack-real.png`.

---

## 50 — 2026-10-28 — реальный encrypted slatepack и автофинализация

**Что проверено:** полный UI-сценарий на двух testnet-профилях в одном процессе
`Flint.exe`, без перезапуска приложения.

- Взята тестовая funded-копия `C:\Flint\data`; нода запущена на
  `127.0.0.1:13413`, секрет owner API взят из `.node/testnet/node/.api_secret`.
  Без этого секрета native status был `Unknown` и отправка блокировалась.
- Выполнен полный скан: `баланс проверен`, доступно **4254.019 GRIN**.
- Создан профиль `alice`; default отправил **1 GRIN** на адрес alice,
  комиссия `23 000 000` nano. Получен настоящий armor:
  `BEGINSLATEPACK. 213a3F… ENDSLATEPACK.` (758 символов).
- Slate отправлен через loopback relay в messenger; профиль переключён
  `default → alice → default` через UI-dropdown **без перезапуска**.
- На alice входящий slate обработан вручную: «Принять» → ответ
  `BEGINSLATEPACK. 8Pxq…` (1923 символа), отправлен обратно.
- На default чекбокс автофинализации включён, контакт alice сохранён. При
  «Проверить входящие» система показала
  `новых: 1 · blocked: 0 · failed: 0 · ответ на отправку автофинализирован`;
  ручной приём response не потребовался.

**Ограничение:** на alice первый входящий оригинал был обработан вручную
(чекбокс на момент прихода был выключен). Авто-путь подтверждён для входящего
response от сохранённого контакта; первый входящий авто-путь отдельно не
изолирован. Подтверждение on-chain не проверялось.

**Данные:** тестовая копия удалена, приложение остановлено; секреты и seed в
журнал не записаны. Скриншот: `docs/assets/shots/flint-slatepack-real.png`.

---

## 49 — 2026-10-28 — актуальный portable-пакет и очистка

- `lastver/Flint-0.1.69-keyboard`: обновлены `Flint.exe` (release
  `20261028-auto-slatepack`, SHA-256 `122e0465…`), `flint-selftest.exe`,
  `selftest.json`, `README.txt` и единый `BUILD-INFO.json`.
- `C:\Flint`: обновлены `Flint.exe`, `flint-selftest.exe`, ресурсы
  node/tor/wallet и README; старый exe сохранён как `Flint.exe.bak-20261027-0201`.
  Каталоги `data/` и `data1/` не изменялись (там есть mainnet-данные), удалены
  только устаревшие вложенные build-артефакты `C:\Flint\lastver`.
- Пересобран актуальный `lastver/Flint-0.1.69-source.zip` (29.6 MB); его SHA-256
  хранится в `lastver/BUILD-INFO.json`.
- Удалены устаревшие `lastver/Flint-0.1.69-data-root-debug`,
  `lastver/Flint-slint-preview`, старые zip/source-архивы и старые
  `BUILD-INFO-*`; оставлен один rollback-exe `Flint.exe.bak-20261028-0852`.
- `.tmp` очищен: удалено **2351 тестовый каталог и 509 файлов** (включая
  chain-snapshot/chain-test/ci-sim и старые прогоны). Оставлены только
  `semantic`, `rustsec-tools`, `advisory-db` и `design-backup-20261028-0725`.
- Пакет проверен: selftest `ok:true`, release exe — текущий; исходники
  соответствуют снимку `20261028-auto-slatepack` (`match:true`).

---

## 48 — 2026-10-28 — авто-финализация slatepack и UI-восстановление

**Изменения:**

- В меню сообщений добавлен `CheckBox` «Обрабатывать слейтпаки автоматически»
  (по умолчанию **выключен**). При включении обрабатываются только armor-сообщения
  от **сохранённых контактов**.
- Авто-сценарий: `prepare_receive` → `finalize_receive` → автоматическая отправка
  response обратно контакту. Для входящего ответа на нашу отправку выполняется
  поиск pending send и `finalize_send`; ошибка показывается в messenger, частичная
  финализация не повторяется.
- UI-восстановление старой истории проверено на тестовом legacy-каталоге:
  пароль `LegacyUi!42`, результат «старая история восстановлена», статус
  «ключ истории открыт». Это отдельно от runtime-теста.
- Ширина полей создания пароля — 360 px, карточка 430 px; см. запись 47.

**Проверки:** UI-тесты — **13 passed**; полный набор — **305 passed, 0 failed,
29 ignored**; release — PASS. Mainnet не затрагивался.

**Границы:** реальный зашифрованный slatepack через messenger **не прогнан**:
funded-кошелёк восстановлен (1400.582 GRIN), но `pending_scan`/нода в состоянии
«статус неизвестен, 0 пиров» не дали подготовить отправку. Авто-финализация на
реальной паре профилей остаётся **NOT VERIFIED**; проверены UI-чекбокс, код и
unit-тест armor.

---

## 47 — 2026-10-28 — уже поля пароля при создании кошелька

- В «Новый кошелёк» поля пароля/повтора ограничены **360 px** вместо ширины
  карточки; карточка на шаге пароля — 430 px, на шаге seed — 650 px.
- Контент выровнен по верху; добавлен скриншот
  `docs/assets/shots/flint-create-wallet.png`.
- UI-тесты — **13 passed**; сборка release — PASS. Логика пароля и seed не
  менялась, mainnet не затрагивался.

---

## 46 — 2026-10-28 — распознавание slatepack в сообщениях

**Цель:** понять, можно ли распознать slatepack в переписке и сразу предложить
финализацию.

**Сделано:**

- `is_slatepack_armor`: распознаёт только текст, начинающийся с `BEGINSLATEPACK.`
  и заканчивающийся `ENDSLATEPACK.`, длиной > 40; обычный текст не считается.
- `MessageRow.slatepack`; в бабле появляется «Обработать slatepack», который
  открывает существующий диалог получения с уже вставленным armor. Пользователь
  сам нажимает «Принять», затем «Завершить приём».
- Unit-тест на armor/ложные срабатывания; UI-действие проверено на тестовой
  armor-строке через messenger между двумя профилями.

**Почему не автоматическая финализация:**

- Входящий armor может быть мусором/атакой; `prepare_receive` создаёт ответ и
  меняет состояние кошелька, поэтому это действие требует явного подтверждения.
- Ответ на нашу отправку нельзя безопасно автоматически связать с pending-записью
  без проверки slate id; `finalize_send` подписывает и публикует — авто-вызов
  недопустим без подтверждения суммы/получателя.
- Предложение: после `prepare_receive` показывать сумму/комиссию и кнопку
  «Завершить приём»; для ответов на отправку — отдельный подтверждённый сценарий
  с выбором pending, не auto-execute.

**Проверки:** полный набор — **305 passed, 0 failed, 29 ignored**; UI-тесты — 13
passed. Скриншот — `docs/assets/shots/flint-slatepack-detect.png`. Mainnet не
затрагивался.

**Границы:** авто-финализация не включена; реальный зашифрованный slatepack в
messenger не прогонялся, только armor-детектор и открытие диалога.

---

## 45 — 2026-10-28 — messenger end-to-end на двух профилях

**Цель:** закрыть живой сценарий Messenger, который оставался NOT VERIFIED.

**Изменения:**

- Прогнаны ignored live-тесты `crates/runtime/tests/multiwallet.rs` с настоящим
  `flint-relay`: два профиля (default/alice), отправка `default → alice`,
  `messenger_inbox` у alice, ответ `alice → default`, история с направлениями,
  рестарт и повторное открытие, неверный профиль и неверный пароль. Embedded и RPC —
  **3 passed, 0 failed**.
- Live UI на testnet: два профиля `default`/`alice`, loopback relay; отправка
  `default → alice`, `Проверить входящие` (1 сообщение), структурированная история
  с направлениями, ответ `alice → default` и inbox default. Скриншот —
  `docs/assets/shots/flint-messenger-live.png`.
- Live UI: контакт выбран, `Заблокировать` → адрес появился в списке,
  `Разблокировать` → список пуст; проверено через UIA.
- Найден и исправлен дефект: в списке блокировок длинный адрес вытеснял кнопку
  «Разблокировать»; теперь адрес обрезается, кнопка доступна.
- Добавлен и пройден тест `legacy_history_recovery_restores_the_old_key`: старый
  `.history.key` копируется, ключ открывается, `history_key_ready` = true.
- Контактные операции (сохранение/удаление), блокировка и пустая история уже
  покрыты `messenger_contacts_and_blocked_lists_share_the_history_key`.

**Проверки:** full suite после добавления теста — **304 passed, 0 failed,
29 ignored**; UI-тесты — 12 passed. Первый полный прогон дал флейк
`contacts::tests::parallel_books_do_not_lose_contacts`; изолированный повтор и
повторный полный прогон прошли. Mainnet не затрагивался.

**Границы:** живая отправка проверена через runtime API (оба backend), а не кнопкой
UI; успешное восстановление старой истории проверено runtime-тестом, UI-диалог
проверен на ошибке «старый каталог не найден».

---

## 44 — 2026-10-28 — экран заблокированного кошелька

**Цель:** собрать lock-screen по эскизу, не меняя проверку пароля и логику кошелька.

**Изменения:**

- Центрированная карточка 510 px, высота по содержимому; `ScrollView` сохраняет
  доступ к полям при уменьшении окна.
- Сверху знак Flint, имя, `GRIN / MIMBLEWIMBLE` и фактическая сеть из
  `status.network` (`TESTNET`); заголовок «Открыть кошелёк» и статус с нарисованным
  замком.
- Один переключатель «Кошелёк» с выпадающим списком всех профилей; плитки/чипы
  убраны. Имена обрезаются `overflow: elide`. Переключение идёт через существующий
  `wallet-switch-clicked`.
- Постоянная подпись «Пароль», подсказка, маскированное поле и компактная
  «Вставить»; под полем зарезервирована строка ошибки. Кнопки paste/undo/select и
  Enter с русской раскладкой сохранены; кнопка «Вставить» тоже запоминает значение
  для Ctrl+Z.
- Широкая мятная «Разблокировать», при `unlock-busy` — «Открываю…» и запрет
  повторного запуска; для ненастроенного кошелька — «Создать кошелёк»/«Восстановить».
- Внизу «Профили и сеть», «Новый профиль» и «Тестовая сеть · 0.1.69».
- Иконка человека в переключателе профиля заменена знаком Flint; в шапке
  выровнены по одной высоте testnet-бейдж, статус и кнопки.

**Проверки:** UI-тесты — 12 passed; полный набор — **303 passed, 0 failed,
29 ignored**. Проверены 1280×860, 900×760, **125% и 150%** (`SLINT_SCALE_FACTOR`,
карточка целиком видна при 150% в логическом окне 900×760), пустое/ненастроенное
состояние, переключение профиля, ошибка неверного пароля, выпадающий список,
**очень длинное имя** (обрезается с `…`), **«Открываю…»** (захвачено во время
операции), **автофокус** в поле пароля (UIA `FocusedElement = Edit / Пароль`).
Mainnet не затрагивался.

**Границы:** при 150% физическое окно 1350×1140 выше экрана 1080 px, поэтому
нижняя часть рабочего стола вне снимка; сам lock-контент помещается и доступен.
Очень длинные имена проверены скриншотом; фокус проверен после активации окна.

---

## 43 — 2026-10-28 — бренд Flint и двухпанельные «Сообщения»

**Цель:** применить эскизы к desktop-UI, не переименовывая продукт и технические
идентификаторы.

**Изменения:**

- Знак A «Складка»: `crates/slint-ui/ui/assets/flint-mark.svg`, PNG 16/24/32/48/64/128,
  `flint-mark.ico`, контактный лист размеров. В UI — 48 px, в окне — иконка.
- `Theme` сведён к графитовой палитре с мятой `#6ee7b7`; цвета-литералы из
  `spike.slint` вынесены в токены (`accent-soft`, `accent-line`, `scrim`, ...).
- Минимум окна — **900×760**; главный экран сжимает сайдбар/карточки, «Сообщения»
  переключают компактные подписи и ширины.
- «Сообщения»: контакты и заблокированные адреса слева; переписка, получатель и
  composer справа; заголовок с фактическим статусом, коротким своим адресом,
  копированием и раскрытием полного адреса; пустые состояния; диалоги контакта и
  восстановления; структурированные `MessageRow` (направление, текст, время).
- Ошибки/заметки скрываются под диалогами; пароль восстановления очищается после
  завершения и отмены; успешная отправка остаётся «отправлено», без доставки/прочтения.

**Проверки:** UI-тесты — 12 passed; полный набор **303 passed, 0 failed, 29 ignored**;
`cargo fmt --all -- --check` — чисто. Проверены пустые состояния, добавление контакта,
ошибка неверного адреса, восстановление без старого каталога (пароль очищен),
выбранный контакт, 900×760. Скриншоты — в `docs/assets/shots/flint-*.png`.

**Границы:** 125/150% DPI и успешное восстановление/живая отправка не проверены —
нет подходящего старого каталога и живого получателя. Mainnet не затрагивался.

---

## 42 — 2026-10-28 — финальная полировка главного экрана

**Цель:** довести макет до рабочего вида после первого прохода: бренд, единый
переключатель, характер кнопок, читаемость, пустая история и панель ноды.

**Изменения:**

- Бренд `Flint` с логотипом перенесён в сайдбар; профиль — один переключатель с
  выпадающим списком; группы меню разделены, testnet-бейдж.
- Header: testnet-бейдж, «Кошелёк открыт», компактные «Заблокировать»/«Обновить»
  (secondary, 38 px).
- Баланс: 44 px сумма, спокойный GRIN, статус-точка, «Доступно»/«Ожидает» только при
  данных, primary «Получить» 44 px и secondary «Отправить».
- Получение: короткий адрес, копирование, QR-модалка; кнопки full-width.
- Нода: отдельная панель, phase-статус, высота/пиры, «Запустить/Остановить ноду»,
  «Подробнее», скан и прогресс.
- История: контурный конверт, «Здесь появятся ваши операции», ссылка «Как получить
  GRIN», строка «Тестовая сеть».
- ActionButton: иконка, hover, primary/secondary/ghost, disabled opacity; убраны
  неподдерживаемые глифы (⚿/⟳/⧉/▦), которые рендерились квадратами.
- Phase-статус ноды: «статус ноды неизвестен» / «нода готова» вместо общего label.

**Проверки:** UI-тесты — 12 passed; полный набор **303 passed, 0 failed, 29 ignored**;
`cargo fmt --all -- --check` — чисто. Снимок `20261028-ui-final`: 673 файла,
62 380 445 байт, `match:true`. Скриншоты lock-экрана и главного экрана проверены.

**Границы:** иконки — глифы Unicode, без SVG-набора; «Скопировано» не гаснет через
2 секунды; адрес сокращён; при узком окне карточка получения пока не переносится под
баланс. Mainnet не затрагивался.

---

## 41 — 2026-10-28 — главный экран по макету Flint

**Цель:** перестроить главный экран по присланному макету и промпту: баланс, получение,
нода, история, единый переключатель профиля.

**Изменения:**

- Шапка: «Обзор кошелька», имя профиля, testnet-бейдж, «Кошелёк открыт», компактные
  «Заблокировать»/«Обновить».
- Баланс: отдельная карточка, крупная сумма + спокойный GRIN, короткий статус,
  «Доступно»/«Ожидает» только при наличии данных, primary «Получить» и secondary
  «Отправить».
- Получение: отдельная карточка с коротким адресом, копированием и QR-модалкой.
- Нода: самостоятельная панель со статусом, высотой/пирами, «Запустить/Остановить»,
  «Подробнее» и сканированием; при неизвестном статусе нейтральный серый.
- История: заголовок, «вся история →», иллюстрация и CTA в пустом состоянии,
  строка «Тестовая сеть».
- Сайдбар: один переключатель профиля с выпадающим списком (вместо карточек+чипов),
  разделители групп меню, иконки, testnet-бейдж.
- Для данных добавлены UI-свойства в `main.rs` (split balance, short address,
  node height/version/peers, phase label); это отступление от «только .slint»,
  иначе макет требует парсинга строк в Slint.

**Проверки:** UI-тесты — 12 passed; полный набор **303 passed, 0 failed, 29 ignored**;
`cargo fmt --all -- --check` — чисто. Снимок `20261028-ui-redesign`: 673 файла,
62 376 741 байт, `match:true`. Скриншоты lock-экрана и главного экрана проверены.

**Границы:** адрес показывается сокращённым (12…8), полный — в QR/карточке; QR — из
существующего `address-qr`; обратной связи «скопировано» хватает текущего note без
2-секундного таймера. Mainnet не затрагивался.

---

## 40 — 2026-10-28 — карточки профилей, чипы и меню с иконками

**Цель:** убрать «прямоугольники» из выбора профилей и оформить меню как аккуратные
кнопки.

**Изменения:**

- Новый `ProfileTile`: карточка 68/100 px с аватаром-человеком, подписью, мятной
  подсветкой активного профиля. Используется на lock-экране и в сайдбаре.
- Новый `ProfileChip`: строка всех профилей под последними тремя; активный чип
  подсвечен, ComboBox убран.
- `NavButton`: иконка (`◈`, `≡`, `⇄`, `✉`, `⚙`), высота 46 px, шрифт 14 px,
  левое выравнивание, мятная полоса и фон активного пункта.
- Проверено скриншотами lock-экрана и разблокированного кошелька.

**Проверки:** `cargo test -p flint-slint --bin flint-slint` — 12 passed; полный набор
**303 passed, 0 failed, 29 ignored**; `cargo fmt --all -- --check` — чисто. Снимок
`20261028-ui-tiles`: 673 файла, 62 359 957 байт, `match:true`.

**Границы:** иконки — текстовые глифы, без отдельного icon-font; стандартные
кнопки Slint сохраняют базовый размер шрифта. Mainnet не затрагивался.

---

## 39 — 2026-10-28 — крупнее типографика, аккуратнее UI и undo после вставки

**Цель:** увеличить мелкий текст и меню, сделать интерфейс приятнее и починить Ctrl+Z
после вставки длинного текста.

**Изменения:**

- `theme.slint`: мягче поверхности/границы, радиусы 20/12, gap 12.
- `nav-button.slint`: высота 44 px, шрифт 14 px, акцентная полоса у активного пункта.
- `spike.slint`: типографика 9→11, 10→12, 11→13, 12→14, 16→18, 18→20; сумма монет
  осталась 32 px; окно 1280×860, минимум 1040×820, сайдбар 250 px, три профиля по
  68×52; карточки 600/620 → 650/660, lock-карточка 500×540; меню переведены через
  `@tr`.
- Ctrl+Z после вставки: внешняя установка `password`/`restore-mnemonic` очищает
  нативный undo-стек Slint, поэтому для вставки сохраняется предыдущее значение и
  Ctrl+Z возвращает его; при обычном наборе по-прежнему работает нативный undo.

**Проверки:** `cargo test -p flint-slint --bin flint-slint` — 12 passed; полный набор
**303 passed, 0 failed, 29 ignored**; `cargo fmt --all -- --check` — чисто. Снимок
`20261028-ui-polish`: 673 файла, 62 357 266 байт, `match:true`. Lock-экран проверен
скриншотом; кнопки больше не выходят за карточку.

**Границы:** стандартные кнопки/поля Slint сохраняют свой базовый размер шрифта;
увеличены текстовые подписи и меню. Undo после вставки — один шаг, не полноценная
история. Mainnet не затрагивался.

---

## 38 — 2026-10-28 — Ctrl+Z и Ctrl+A в полях

**Цель:** добавить привычные сочетания к тем же полям, где уже работает резервный
Ctrl+V.

**Изменения:** в `ui/spike.slint` обработчики `key-pressed` полей пароля, seed-фразы,
адреса получателя и входящего slate получили:
`Ctrl+Z` → `self.undo()`, `Ctrl+A` → `self.select-all()` (с русской раскладкой
`я`/`ф`). У `TextEdit` seed-фразы нет публичного `undo`, поэтому Ctrl+Z там остаётся
нативным, а Ctrl+A добавлен. Остальные поля используют штатные Slint-сочетания.

**Проверки:** `cargo test -p flint-slint --bin flint-slint` — 12 passed;
полный набор остаётся **303 passed, 0 failed, 29 ignored**;
`cargo fmt --all -- --check` — чисто. Снимок `20261028-undo-select`: 673 файла,
62 355 411 байт, `match:true`.

**Границы:** обработчики добавлены только в четыре ключевых поля; в остальных
действует нативный Slint-механизм. Mainnet не затрагивался.

---

## 37 — 2026-10-28 — последние профили и 5-минутная разблокировка

**Цель:** ускорить переключение профилей: три последних — крупными кнопками, полный
список — ниже; не спрашивать пароль при каждом переключении, но блокировать после
5 минут бездействия.

**Изменения:**

- `crates/runtime/src/lib.rs`: `recent_profiles`/`remember_recent_profile` (файл
  `ui-recent-profiles.json`, максимум 3, активный всегда первый) и in-memory
  `unlock_cache` на 5 минут (`UNLOCK_GRACE`), пароль в `Zeroizing<String>`.
- UI: три крупные кнопки над ComboBox на боковой панели и экране блокировки; после
  успешной разблокировки/создания пароль запоминается; переключение в пределах 5 минут
  автоматически открывает профиль через `snapshot_after_switch`.
- Автоблокировка: таймер каждые 15 с проверяет системный простой через
  `GetLastInputInfo`; после 300 с без ввода кошелёк блокируется, кэш очищается.
  Ручная блокировка тоже очищает кэш.
- Тест `unlock_grace_cache_and_recent_profile_round_trip`.

**Проверки:** `cargo test --workspace --locked --offline` — **303 passed, 0 failed,
29 ignored**; `cargo fmt --all -- --check` — чисто. Снимок `20261028-profile-recents`:
673 файла, 62 353 271 байт, `match:true`.

**Границы:** пароль живёт в памяти до 5 минут (осознанный компромисс); автоблокировка
по системному простою работает на Windows, на других ОС таймера нет. Mainnet не
затрагивался.

---

## 36 — 2026-10-28 — Ctrl+V: резервная вставка через arboard

**Цель:** Ctrl+V не срабатывал в полях без явного клика; нативный Slint-путь на этой
сборке/клавиатуре оказался ненадёжным.

**Диагностика:** приложение запущено с копией данных, поле «Пароль» найдено через UI
Automation. `SetFocus` + Ctrl+V не дал текста; клик по координатам поля + Ctrl+V —
дал (значение замаскировано). То есть проблема в фокусе/нативной обработке, а не в
буфере.

**Изменения:**

- `ui/spike.slint`: `callback paste-text() -> string`; поля пароля, seed-фразы,
  адреса получателя и входящего slate получили `key-pressed` Ctrl+V, который берёт
  текст через этот callback. Кнопка «Вставить» у пароля использует тот же путь.
- `main.rs`: обработчик читает `arboard::Clipboard::get_text()`; пустой буфер даёт
  пустую строку, без паники.
- Вставка заменяет содержимое поля (курсор/выделение не учитываются) — достаточно для
  пароля, seed-фразы и адреса; это записано как граница.

**Проверки:** UIA-тест: клик по «Пароль» → Ctrl+V → поле заполнилось; кнопка
«Вставить» также заполняет. `cargo test --workspace --locked --offline` — **302 passed,
0 failed, 29 ignored**; снимок `20261028-paste-fallback`: 673 файла, 62 342 438 байт,
`match:true`.

**Границы:** вставка в длинные текстовые поля сообщений остаётся нативной; резервный
путь не вставляет в позицию курсора, а заменяет поле. Mainnet не затрагивался.

---

## 35 — 2026-10-28 — показ seed после повторного пароля

**Цель:** дать владельцу проверить seed-фразу активного профиля из интерфейса, не
сохраняя её в открытом виде и не ослабляя конверт.

**Изменения:**

- `crates/core/src/seed/store.rs`: `SeedStore::reveal_mnemonic(password)` — расшифровывает
  envelope (или legacy), строит BIP39-фразу и возвращает `Zeroizing<String>`; ничего не
  пишет и не логирует.
- `crates/core/src/service.rs`, `crates/runtime/src/wallet.rs`: display-only обёртки.
- UI: кнопка «Показать seed» в «Безопасности» и в настройках профиля; модальное окно с
  паролем, read-only `TextEdit` и предупреждением. После успеха поле пароля очищается,
  при закрытии/сбросе контекста стирается и фраза.
- ru/en-строки добавлены в каталоги.

**Проверки:** `cargo test --workspace --locked --offline` — **302 passed, 0 failed,
29 ignored**; новый тест `reveal_mnemonic_round_trips_and_rejects_wrong_password`;
`cargo fmt --all -- --check` — чисто. Снимок `20261028-seed-reveal`: 673 файла,
62 340 351 байт, `match:true`.

**Границы:** кнопки копирования фразы нет намеренно; seed показывается только после
пароля и не попадает в логи. Mainnet не затрагивался.

---

## 34 — 2026-10-28 — Enter в формах и Ctrl+V в полях

**Цель:** убрать неработающие Enter/Ctrl+V: в поле пароля стоял ручной `key-pressed`,
который перехватывал все клавиши и сам вызывал `paste()`, а Enter был только у
разблокировки и входящего slate.

**Изменения:**

- `ui/spike.slint`: удалён кастомный `key-pressed` у пароля — вставку снова обрабатывает
  нативный `LineEdit`; рядом с паролем добавлена кнопка «Вставить» (`password-field.paste()`).
- `accepted` добавлены: создание (повтор пароля), восстановление (повтор пароля),
  сумма и ответ в отправке, поле профиля и минимум подтверждений в настройках, повтор
  нового пароля, имя кошелька для удаления. Поле профиля по Enter сохраняет настройки,
  а не создаёт профиль (для создания есть отдельная кнопка).
- ru/en-каталог дополнен строкой «Вставить».

**Проверки:** `cargo test --workspace --locked --offline` — **301 passed, 0 failed,
29 ignored**; `cargo fmt --all -- --check` — чисто; снимок `20261028-keyboard`:
673 файла, 62 332 623 байта, `match:true`.

**Границы:** ручной ввод с клавиатуры автотестом не покрыт — нужна проверка живым
нажатием. Ctrl+V в остальных полях идёт через штатный Slint-механизм; если он не
работает и там, следующий шаг — кнопки «Вставить»/диагностика winit-clipboard.

---

## 33 — 2026-10-28 — мультипрофили: явное создание и переключение

**Цель:** убрать неочевидный путь «настройки → переименовать поле → сохранить»; дать
пользователю видимую кнопку создания второго профиля и объяснить, что профиль — это
отдельный кошелёк/seed.

**Изменения:**

- `ui/spike.slint`: раздел «Профили» в настройках (активный профиль, поле имени,
  кнопка «Создать профиль», пояснение и ошибки) и кнопка «Новый профиль» на экране
  блокировки; подсказка заменена на явную.
- `main.rs`: callbacks `profile-new-clicked` и `profile-create-clicked`; переключение
  через `wallet.switch`, сброс контекста, обновление snapshot; если профиль пуст —
  сразу открывается создание seed. Выбор пустого профиля в старом ComboBox тоже
  открывает создание.
- `profile_request_error` — быстрая валидация пустого/активного имени; тест
  `profile_creation_rejects_empty_and_active`.
- ru/en-строки добавлены в `@tr`-каталог и `i18n.rs`; SLINT.md/STATUS/PLAN обновлены.

**Проверки:** `cargo test --workspace --locked --offline` — **301 passed, 0 failed,
29 ignored**; `cargo fmt --all -- --check` — чисто. Изоляция профилей уже покрыта
`embedded_profiles_keep_seeds_addresses_and_scan_state_isolated`. Снимок
`20261028-profiles-ux`: 673 файла, 62 330 395 байт, `match:true`.

**Границы:** переименование профиля не поддерживается (новое имя = новый профиль);
удаление профиля отдельной кнопкой не добавлялось — удаление кошелька остаётся в
разделе безопасности. Mainnet запрещён.

---

## 32 — 2026-10-28 — полный EN-UI: fallback, каталог 264, форматтеры

**Цель:** довести английский интерфейс до состояния, когда русский текст не
показывается: закрыть динамические форматтеры и добавить безопасный fallback.

**Изменения:**

- `crates/slint-ui/src/i18n.rs`: 264 записи каталога; при промахе строка с кириллицей
  заменяется `Operation failed; details are in the log`, оригинал пишется в stderr;
  `missing_translations()` для тестов.
- Обёрнуты оставшиеся форматтеры: scan/send/swap/history/clipboard/security, балансы и
  activity direction; `node_line` имеет явную английскую ветку; ошибки и заметки
  `set_*_error`/`set_*_note` проходят `tr`.
- Диалог выбора каталога получает английское описание при EN; 29 `accessible-label`
  через `@tr`; добавлены ошибки config/node/Tor/service/runtime/gateway.
- `docs/ui/LOCALIZATION.md` переписан: fallback, охват, остаток.

**Проверки:** `cargo test --workspace --locked --offline` — **300 passed, 0 failed,
29 ignored**; `cargo fmt --all -- --check` — чисто. EN-прогон `flint-slint --data`
с `ui-language=en`: `ui language: en_US selected`, на старте **0** строк
`i18n: missing English translation`. Снимок `20261028-i18n-full`: 673 файла,
62 317 194 байта, `match:true`.

**Границы:** редкие строки вне каталога показываются generic-текстом (детали — в логе),
а не точным переводом; документация остаётся русской; NVDA/IME/Tab — внешний PS-11.
Mainnet запрещён.

---

## 31 — 2026-10-28 — динамическая локализация: слой i18n и каталог

**Цель:** начать полный англоязычный UI: перевести не только статические `@tr`, но и
строки, которые собираются в Rust (статусы, ошибки, форматированные сообщения), и
a11y-подписи.

**Изменения:**

- `crates/slint-ui/src/i18n.rs`: `set_english`/`tr`/`tr_with`, точные пары и шаблоны
  с `{}`/`{name}`, захват и подстановка; неизвестная строка остаётся русской
  (fallback, не выдуманный перевод). 174 записи каталога: UI-статусы, node/scan/Tor,
  `error.rs`, `node.rs`, `runtime/wallet.rs`, основные сообщения `service.rs`.
- `ui/spike.slint`: 29 `accessible-label` обёрнуты в `@tr`; каталог `en_US` дополнен.
- `main.rs`: ошибки и заметки в `set_*_error`/`set_*_note` проходят через `tr`;
  статус/нода/скан/Tor/мосты/буфер переводятся; английская ветка `node_line`;
  `set_english(true)` при выборе `en`.
- `docs/ui/LOCALIZATION.md`: зафиксирован охват и видимый остаток.

**Проверки:** `cargo test --workspace --locked --offline` — **299 passed, 0 failed,
29 ignored**; `cargo fmt --all -- --check` — чисто; снимок
`20261028-i18n-dynamic`: 673 файла, 62 303 226 байт, `match:true`.

**Границы:** это не полная локализация. Остались часть сообщений
`service`/contacts/gateway/seed/Tor и отдельные callback-форматтеры; в EN-режиме они
видны по-русски. Критерий закрытия — каталог покрывает все пользовательские строки,
EN-прогон не показывает русского, кроме данных. Mainnet запрещён.

---

## 30 — 2026-10-28 — локализация ru/en и справка восстановления

**Цель:** закрыть последний локальный пункт: английский UI для статических строк,
переключение языка и справка восстановления внутри приложения.

**Изменения:**

- `crates/slint-ui/ui/spike.slint`: 109 статических строк `text`/`placeholder-text`/
  `title` обёрнуты в `@tr`; добавлены выбор языка в настройках, баннер-подсказка
  «после перезапуска» и overlay «Как восстановить кошелёк» с четырьмя абзацами.
- `crates/slint-ui/lang/en_US/LC_MESSAGES/flint-slint.po` — английский каталог;
  `crates/slint-ui/build.rs` собирает его в бинарь
  (`with_bundled_translations`, контекст `None`). Язык `en_US`, а не `en`: Slint
  трактует `en` как язык по умолчанию (msgid = русский).
- `crates/runtime/src/lib.rs`: `ui_language`/`set_ui_language` — файл `ui-language`
  в корне данных, `ru` по умолчанию, неизвестный язык отвергается.
- `crates/slint-ui/src/main.rs`: выбор `en_US` после создания первого компонента
  (до этого Slint не регистрирует каталог), запись выбора в файл, начальное значение
  в UI; тесты полноты каталога и `ui-language`.
- `docs/ui/LOCALIZATION.md` — что переведено, что осталось русским и как добавить строку.

**Проверки:**

- `cargo test --workspace --locked --offline` — **296 passed, 0 failed, 29 ignored**;
  `cargo fmt --all -- --check` — чисто.
- `every_tr_literal_has_an_english_translation`, `english_catalog_is_non_empty`,
  `ui_language_round_trips_and_defaults_to_russian` — passed.
- Запуск `flint-slint --data <tmp>` с `ui-language=en` — в stderr
  `ui language: en_US selected`, ошибок перевода нет.
- Снимок `20261028-i18n`: 672 файла, 62 273 939 байт, `match:true`.

**Границы:** переведён статический UI и справка; динамические сообщения ядра/ноды и
a11y-подписи остаются русскими (зафиксировано в `docs/ui/LOCALIZATION.md`). Все
**12 локальных пунктов** закрыты; 17 внешних приёмок PS-01…PS-17 открыты, mainnet
запрещён.

---

## 29 — 2026-10-28 — уведомления о незавершённых/истекающих slate

**Цель:** не оставлять зависшие и требующие восстановления сделки незамеченными на
главном экране, не выдавая возрастной порог за протокольный TTL.

**Изменения:**

- `crates/core/src/service.rs`: `PendingSeverity`, `PendingAlert`, `pending_alerts`
  (critical для `finalizing`/`finalized_not_posted`/`failed_recoverable`/`response_lost`;
  warning после 30 минут для `prepared`/`locked`/`receive_intent`/`response_received`),
  экспорт из `flint_core`.
- `crates/runtime/src/wallet.rs`: `WalletRuntime::pending_alerts()` — read-only обёртка
  над очередью с текущим временем.
- UI: свойство `pending-alert`, баннер с «⚠» на главном экране; обновление раз в 60 с
  отдельным таймером (и при загрузке очереди); после ошибки очередь очищает баннер.
- Тесты: `pending_alerts_sort_critical_first_and_warn_only_after_30_minutes`,
  `fresh_or_finished_slates_do_not_alert`.

**Проверки:** `cargo test --workspace --locked --offline` — **293 passed, 0 failed,
29 ignored**; `cargo check -p flint-slint` и `cargo fmt --all -- --check` — успешно;
снимок `20261028-slate-alerts`: 671 файл, 62 253 783 байта, `match:true`.

**Границы:** уведомление внутри приложения, не системный toast; ручной UI-прогон
баннера не выполнялся. Остался 1 пункт этапа 5 — локализация/справка восстановления;
mainnet запрещён.

---

## 28 — 2026-10-28 — этап 2, большие истории и дизайн фоновой ноды

**Цель:** закрыть отказные сценарии сети/цепи, большие истории и зафиксировать дизайн
фоновой ноды без преждевременной реализации.

**Изменения:**

- `crates/core/src/service.rs`: stub-нода для `get_tip`/`get_version`/`get_status` и три
  поведенческих теста: flapping (закрытие слушателя снимает synced), чужой `chain`
  (mainnet вместо testnet) и ответ `Err` от `get_tip`. Физическая порча `chain_data` не
  воспроизводилась.
- `crates/messenger/src/history.rs`: `large_history_is_bounded_and_lists_newest_first` —
  120 записей при лимите 100, страница 20; проверены prune и хронологический порядок.
- `docs/architecture/BACKGROUND_NODE.md`: варианты supervisor/UI, контракт, отказные
  сценарии и пять гейтов до кода. Реализация не добавлялась.
- Счётчики и индексы обновлены: KRead 5→2, PLAN, PSWork, STATUS, README, AGENTS.

**Проверки:**

- `cargo test --workspace --locked --offline` — **291 passed, 0 failed, 29 ignored**
  (`.tmp/final-tests-291.log`); `cargo fmt --all -- --check` — чисто.
- Живой embedded scan cancel на высоте 3 311 753 — 1 passed, 97,98 с.
- Контрольный снимок `20261028-stage2-ux`: 671 файл, 62 245 990 байт, `match:true`.

**Границы:** остались 2 пункта этапа 5 — уведомления о незавершённых/истекающих slate и
локализация/справка восстановления. Физическая порча цепи и ручной UI остаются внешними
PS-03/PS-04; mainnet запрещён.

---

## 27 — 2026-10-28 — этапы 3–4: ACL, Tor, relay, lock, node/RPC, native hashes, SBOM, RustSec

**Цель:** закрыть локально выполнимые пункты этапов 3 и 4, отделяя unit/live-доказательства
и не выдавая симуляции за внешние гейты.

**Продуктовые изменения:**

- `crates/core/src/win_acl.rs`: Windows PowerShell 5.1, запущенный из pwsh 7, наследовал
  `PSModulePath` с модулями PS7 и не мог автозагрузить `Microsoft.PowerShell.Security`;
  `Get-Acl` падал, поэтому owner-only ACL ошибочно отвергался. Добавлен
  `powershell_command()` с явным модульным путём Windows PowerShell; тест
  `restrict_removes_a_pre_existing_explicit_everyone_ace` и все ACL-пути проходят.
- `crates/core/src/tor_listener.rs`: Windows-сборка Tor создаёт `DataDirectory` через
  ANSI-API и падает на Unicode-пути; `Log ... file` не принимает кавычки. Для Tor
  каталоги переводятся в 8.3-короткий ASCII-путь (`GetShortPathNameW`), лог пишется
  без кавычек; без короткого пути возвращается понятная ошибка. Добавлены тесты
  Unicode/пробелов/quotes, webtunnel, unreadable bridges.
- `crates/messenger/src/contacts.rs`: stale `contacts.lock` старше 30 с удаляется с
  диагностическим сообщением; свежая блокировка не трогается; `Drop` удаляет только
  свою блокировку по token, а не чужую заменённую. Три новых теста.
- `crates/core/src/gateway.rs`: flood 65-го соединения получает 503; degraded storage
  даёт 500 и не оставляет сообщение в памяти; квота одного получателя не блокирует
  другого.
- `crates/core/src/node.rs`: зависшая нода не вешает вызов — новый
  `hanging_node_status_is_bounded_by_the_timeout`. `crates/runtime/tests/rpc_lifecycle.rs`:
  RPC-ребёнок после stop/start открывает тот же адрес.
- `resources/runtime/vcruntime140.dll` — закреплённая копия вместо System32;
  `resources/native-assets.sha256` дополнен `vcruntime140.dll` и `pt_config.json`;
  `tools/package-slint.ps1` берёт runtime из `resources/runtime`.
- Новые `tools/verify-sbom.ps1`, `tools/rustsec.ps1`; `tools/sbom.ps1` пишет native
  компоненты; отчёт — [security/RUSTSEC_2026-10-28.md](security/RUSTSEC_2026-10-28.md).

**Проверки:**

- `cargo test --workspace --locked --offline` — **287 passed, 0 failed, 29 ignored**
  (`.tmp/final-tests-286.log`).
- Живые: `runtime_starts_opens_locks_and_stops_the_rpc_child` (stop/start/reopen) —
  1 passed, 0,90 с; `runtime_starts_and_stops_the_tor_listener` — 1 passed, 21,72 с;
  `tor_listener_starts_from_a_unicode_data_dir` — 1 passed, 24,90 с;
  `tor_publishes_the_address_derived_onion` — 1 passed;
  `embedded_scan_cancels_between_batches` — 1 passed, 97,98 с.
- `verify-native-assets.ps1` — OK для 6 assets; `sbom.ps1` — 1141 cargo + 6 native;
  `verify-sbom.ps1` — успешно; `rustsec.ps1` — db `99670cfc…`, 7 accepted,
  0 unaccepted, 21 warning.
- Link-check по `docs/` — 0 битых; контрольный снимок `20261028-stage34-local`:
  671 файл, 62 239 827 байт, `match:true`.

**Границы:** закрыты этапы 3 и 4 локально; остаются 5 пунктов (offline/flapping/битая
цепь и 4 UX-пункта этапа 5). Это не внешний аудит, не физический destructible-стенд и
не mainnet; mainnet запрещён.

---

## 26 — 2026-10-28 — инструменты DSH и независимое ревью Janus

**Цель:** зафиксировать в проекте настройку профиля DSH и результат ревью слоя
свопов, чтобы следующие сессии не повторяли разведку и не доверяли отчёту Codex
вслепую.

**Изменения:**

- [`DSH.md`](DSH.md) — новый документ: пять установленных бандлов, постоянный
  терминал, поиск по сессиям, делегации Codex, смысловой поиск и LM Studio, что не
  ставили и почему.
- [`security/JANUS_REVIEW_2026-10-28.md`](security/JANUS_REVIEW_2026-10-28.md) —
  датированный снимок ревью `crates/janus`: 6 подтверждённых находок, 1 опровергнута
  тестом; порядок исправления до подключения реальных средств.
- [`README.md`](README.md) — строки индекса: разбор Janus в «Безопасность», новая
  секция «Инструменты среды».
- `AGENTS.md` — строка `docs/DSH.md` в таблице и секция «Инструменты DSH» (4 правила).
- Продуктовый код не менялся; коммитов нет (в проекте нет `.git`).

**Проверки:**

- Терминал: `terminal_open` и два вызова — `$env:TMP` и PID (`10840`) пережили
  вызов инструмента.
- `session_search "Janus"` — 19 сессий, все с `cwd=C:\DHW`; чужие воркспейсы не
  подмешиваются.
- `plugin_market list_indexes` — `{"indexes":[]}`: Discover пуст без URL индекса.
- Codex: одна делегация (`subagent_codex`), отчёт из 7 находок. Шесть подтверждены
  чтением `model.rs`/`service.rs`; одна («критично», `write_atomic`) опровергнута
  изолированным тестом `.tmp/rename_check/main.rs` (`rustc -O`): `fs::rename` на
  Windows перезаписывает цель, вывод `second: OK, content={"v":2}`.
- `cargo build`/`cargo test` не запускались; тесты проекта не перезапускались.

**Границы:** ревью статическое — on-chain семантика `tip == lock_height` не
проверялась, реального двойного расходования никто не воспроизводил. Находки про
персистентность латентны: `crates/runtime/src/lib.rs:45` создаёт
`JanusService::demo(None)`, то есть файл свопов сейчас не читается и не пишется.
Вкладка инспектора сессий в UI глазами не подтверждена. Видимое окно Windows
Terminal при делегации Codex — штатное поведение, закрывается само.

---

## 25 — 2026-10-28 — упорядочивание дерева без переноса сборочных путей

**Цель:** убрать из корня нестроительные каталоги и файлы, не меняя `crates/`,
`tools/`, `resources/`, `third_party/`, `vendor/`, `dist/`, `lastver/` и офлайн-сборку.

**Изменения:**

- `shots/` → `docs/assets/shots/`; ссылки в `README.md`, `DESIGN.md`, `docs/ui/SLINT.md`
  обновлены.
- `docs/history/` объединён с `docs/historical/` (8 файлов); обновлены ссылки в STATUS,
  README, MAINNET_READINESS, MESSENGER, RPC_EXTRACTION, `historical/ROADMAP_2026-10-24.md`
  и комментариях `crates/core/src/seed/store.rs`, `crates/messenger/src/keywrap.rs`.
- `KimiDocs/` расформирован: снимки → `snapshots/`, инструменты → `snapshots/tools/`,
  патч F-04 → `archive/patches/`. `verify_snapshot.py` и `snapshot_sources.py` читают
  `snapshots/`.
- `archV/` → `archive/archV/`; `audit-recheck-test.log` и `grin.log` → `archive/logs/`;
  `selftest.json` → `archive/`; `codex-local-delegation.skill` → `docs/tools/`.
- `testnet-funded-app/` → `fixtures/`; `plugin-sources/` → `projects/plugins/`.
- `IosProject/` → `mobile/GrinFlint/` (git-репозиторий сохранён; пути в его docs обновлены,
  локальный коммит `f583264`, не запушен).
- В `AGENTS.md` и `docs/operations/TESTNET_WALLETS.md` зафиксировано: одноразовые
  testnet-реквизиты (`docs/testnet-funded.txt`, `docs/grinflint-ci-wallet.txt`) хранятся
  открыто осознанно, ценности не имеют; правило о секретах относится к реальным/mainnet.
- Не перемещались: `crates`, `tools`, `resources`, `third_party`, `vendor`, `dist`,
  `lastver`, `.cargo`, `.github`, `.node`, `.tmp`, `target`, `.secrets`.

**Проверки:**

- link-check по `docs/` и корневым `.md` — **0 битых** (`.tmp/reorg-linkcheck.py`).
- `cargo check --workspace --locked --offline` — успешно, 8,92 с.
- `verify_snapshot.py 20261028-data-root-selection` — `match:false`, changed только
  `crates/core/src/seed/store.rs` и `crates/messenger/src/keywrap.rs` (комментарии
  `docs/history` → `docs/historical`), missing/added пусты.
- Контрольный снимок `20261028-workspace-reorg`: 669 файлов, 62 218 854 байта,
  `match:true`.

**Границы:** продуктовый код не менялся, тесты не перезапускались; это перемещение
документации и вспомогательных каталогов. `lastver` и `C:\Flint` не трогались.
Mainnet запрещён.

---

## 24 — 2026-10-28 — выбор корня данных и debug-preview

**Цель:** подтвердить состав мультикошельков, добавить выбор общего каталога ноды и
кошельков в настройках и явно зафиксировать расположение актуальных исходников/билдов.

**Изменения:**

- `crates/core/src/config.rs`: добавлены `DataRootView`, атомарный `data-root.json`,
  абсолютная валидация пути, сброс и fail-closed чтение селектора. `FLINT_DATA_DIR`/`--data`
  сохраняют приоритет. В portable режиме селектор лежит рядом с exe, вне старого `data`.
- `crates/runtime/src/lib.rs`: чтение и сохранение корня для следующего процесса, запрет
  изменения во время скана. Текущий runtime не переносит и не переоткрывает данные.
- `crates/slint-ui/src/main.rs`, `ui/spike.slint`: активный путь, ручной ввод, системный
  обзор, стандартный путь и применение после перезапуска; busy-barrier и сообщение об
  отсутствии автоматического копирования. Повреждённый селектор показывается при старте.
- Добавлена инструкция `docs/operations/DATA_ROOT.md`; README, STATUS, PLAN и счётчики
  обновлены. Мультикошельки подтверждены как независимые seed-профили в одном окне с одним
  активным профилем, а не одновременное исполнение нескольких кошельков.

**Проверки:** три новых теста сначала упали из-за отсутствия реализации, затем прошли.
`cargo check -p flint-slint` успешен. Два общих test-запуска превысили 40 с и были
повторены раздельными командами: **277 passed, 0 failed, 28 ignored**. Снимок
`20261028-data-root-selection`: 669 файлов, 62 218 833 байта, `match:true`.
Debug `flint-slint` и selftest собраны; selftest из дерева и из пакета — `ok:true`,
`reopened_matches:true`. В обоих selftest сохранено предупреждение о занятом порту 13413;
внешняя нода отвечала, managed-запуск не засчитывался как успешный.

**Пакет:** `lastver/Flint-0.1.69-data-root-debug/Flint.exe` и
`lastver/Flint-0.1.69-data-root-debug.zip`, 50 221 834 байта, SHA-256
`6f556317152d90fdbae2997a6e8262cc032b7f0655bc153b4332d53ff547d2cf`.
Прежний release и `C:\Flint` не изменялись.

**Границы:** release-компоновка пять раз превысила 40-секундный лимит, поэтому новый
пакет является debug-preview, а не release. Интерактивный UI новой настройки и системный
диалог обзора вручную не проверены. Денежные операции и новые live-сценарии не выполнялись.

**Следующий шаг:** ручная UI-приёмка выбора корня на копии testnet-данных; затем можно
вернуться к offline/flapping/битой цепи на embedded/RPC.

---

## 23 — 2026-10-28 — консолидация документации в `docs/`

**Цель:** собрать актуальные план, статус, журнал и эксплуатационные инструкции в едином
наборе документов, отделить историю и проверить согласованность после перемещений.

**Изменения:**

- Созданы `docs/README.md`, `docs/PLAN.md` и этот текущий журнал; обновлены
  `docs/status/ROADMAP.md`, `docs/KRead.md`, `docs/PSWork.md`, `AGENTS.md` и корневой
  `README.md`.
- Прежние журнал, план, правила и Sentinel-разбор из `KimiDocs/` перенесены в
  `docs/historical/`; прежняя дорожная карта сохранена как
  `docs/historical/ROADMAP_2026-10-24.md`. Каталог `docs/history/` сохранён отдельно
  (28.10 объединён с `historical/`, см. запись 25).
  `KimiDocs/` теперь предназначен только для инструментов и снимков исходников
  (28.10 расформирован, см. запись 25).
- Актуализированы STATUS, backup/restore, destructible F-04, testnet wallet notes,
  ADR F-04, предложение фоновой ноды и мобильные ссылки. Устаревшие счётчики ignored,
  пути и утверждения исправлены; секретные реквизиты в документацию не включались.
- Пункт «Основные документы синхронизированы; история отделена от текущего статуса»
  закрыт. Остаются 12 локальных пунктов, 17 внешних приёмок и 3 отложенных трека.

**Проверка:** аудит 55 Markdown-файлов — битых ссылок, отсутствующих обязательных
документов и активных ссылок на прежние `KimiDocs/*.md` нет. Счётчик плана: этапы 1–5 —
0, 1, 4, 3, 4; mainnet-запрет сохранён. `verify_snapshot.py
20261028-restore-lifecycle-live` — `match:true`, changed/missing/added пусты.

**Граница:** продуктовый код в этой итерации не изменялся; `match:true` подтверждает
неизменность контролируемого снимка, а не новый прогон тестов. Контрольный результат
остаётся 274 passed, 0 failed, 28 ignored на `20261028-restore-lifecycle-live`.

**Следующий шаг:** offline/flapping/битая цепь на embedded и RPC, затем оставшиеся
эксплуатационные сценарии.
