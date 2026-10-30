# Flint — фактическое состояние

30.10.2026, локальный аудит desktop+iOS: [REPORT.md](../evidence/audit-20261030/REPORT.md).
281 desktop library tests, 23 SwiftPM core и 18 Rust FFI host tests — PASS;
полный desktop suite не завершён (LNK1104). Обнаруженные внутренние ошибки
требуют исправления и повторной приёмки; старый статус принятого RC не
переносится на текущий код. Исправления продуктового кода аудитом не вносились.

30.10.2026, после исправлений: desktop P2 D-A1…D-A5 исправлены — секрет ноды
привязан к loopback, receive сверяет отпечаток S1, ошибки RPC-конфига
возвращаются, Nostr URL проверяется разбором и имеет сетевые/размерные бюджеты,
сохранённый открытый node_url заменяется локальным с предупреждением. Проверки:
core 195, messenger 83 (+1 ignored), workspace 355 passed / 0 failed / 33 ignored;
LNK1104 не воспроизвёлся. Подробности — evidence/security-fixes-20261030/README.md.
iOS-находки остаются в mobile/GrinFlint.

Актуализировано: 2026-10-30. Это единственный текущий статус desktop-проекта.
История изменений хранится в Git; правила работы — в [AGENTS.md](../AGENTS.md).

**DEV BUILD: не аудирован, не для реальных средств. Сеть — testnet.**
Mainnet-данные и реальные кошельки не участвуют в проверках и уборке.

Публичный репозиторий — <https://github.com/DanilaGrinRus/Flint>, сайт —
<https://danilagrinrus.github.io/Flint/> (GitHub Pages из ветки gh-pages).
Testnet-preview опубликован как prerelease `0.1.69-ui-20261030`
(tag `v0.1.69-ui-20261030`): binary zip sha256
`1868678557a9f0afa12d0fd66f79abf4d2f2f101515e3019f4f5803bdec9e5b3`,
source zip sha256 `d00025385deb392fcb422696466ee43874ce8c9aaf09e73359769a6108212aea`;
SHA256SUMS, sbom.json, selftest.json и BUILD-INFO.json; сборка unsigned.
Публикация не меняет статус приёмки: физические D-01…D-10 остаются открытыми.

## Объём и готовность

Последний принятый desktop RC имеет статус **READY_FOR_EXTERNAL_DEVICE_TESTS**.
Это готовность к физическим проверкам тестовой unsigned-сборки, а не разрешение
публичного/mainnet-релиза. Исходная точка генеральной уборки — Git `bb4b7f3`.

В объём входят оба backend (embedded и управляемый grin-wallet RPC): создание,
открытие, блокировка, восстановление и backup; профили; нода; проверенные балансы
и история; send → receive → finalize → post → confirmation; отмена и продолжение
операций; messenger и поддерживаемые relay/Tor-транспорты; общий Rust-контракт;
CI, native assets, SBOM, исходники и portable-пакет.

Вне этого RC: настоящие Janus-свопы, публичный relay как сервис, реализация iOS,
встроенная мобильная нода, фоновая Windows-служба, магазинная публикация.
Внешний аудит исключён из объёма RC и не добавляется в список оставшихся работ.

## Доказательства принятого RC

Ниже сохранены результаты приёмки до генеральной уборки. Они не выдаются за
повторно выполненные команды этой уборки. Минимальные локальные свидетельства
и их происхождение находятся в [evidence/rc-20261029](../evidence/rc-20261029/).

| Проверка | Результат | Свидетельство / граница |
|---|---|---|
| Полный локальный CI | PASS | В последней записи: 323 passed, 0 failed, 31 ignored; Slint UI — 21. Ignored не считаются выполненными обычным suite. |
| Чистый Windows build-test | PASS | [GitHub run 40799284192](https://github.com/DanilaGrinRus/Flint-desktop-private/actions/runs/40799284192), head ca9ec7af |
| Embedded + RPC funded-testnet full cycle | PASS | [GitHub job 123242083353](https://github.com/DanilaGrinRus/Flint-desktop-private/actions/runs/40811329947/jobs/123242083353) — success; run целиком cancelled из-за другого build-test job |
| Tor live | PASS | [GitHub job 123248032240](https://github.com/DanilaGrinRus/Flint-desktop-private/actions/runs/40813171901/jobs/123248032240) — success; run целиком cancelled из-за другого build-test job |
| Restore/full scan и профильная изоляция | PASS в принятой приёмке | Совпадение адреса, полный скан и сверка spendable/awaiting/total/immature/locked; это не mainnet restore |
| RPC S1 resend / S2 export | PASS в live-приёмке | `rpc_pending_export_live`: тот же S1 без новой подписи, тот же slate_id у S2, finalize и confirmation; 115,20 с |
| Durable messenger и ACK-loss replay | PASS в принятом drill | Сохранённый reply_sent, restart, повтор без второго S2; локальные свидетельства вынесены из временных каталогов |
| Slint UI и локальное масштабирование | PASS в локальной приёмке | 125%; 150% при окне 1920×1290; реальные mixed-DPI, NVDA/IME остаются физическими проверками |
| Зависимости/native assets | PASS с явно принятым остатком | 2 accepted advisories, 0 unaccepted, 17 warnings в датированном отчёте; [SECURITY.md](SECURITY.md#dependencies) |
| Portable RC, SBOM, исходники, запуск вне дерева | PASS до уборки | Только для идентифицированного принятого пакета; новая упаковка имеет собственные хэши |
| Физические Windows-устройства | NOT VERIFIED | Сценарии D-01…D-10 в [ACCEPTANCE.md](ACCEPTANCE.md) |

CI job status проверен отдельно от run status; отменённый run не объявляется
полностью зелёным. Снимок подтверждения — evidence/rc-20261029/github-jobs.json.

## Идентичность артефактов

В прежней матрице последнего RC указаны префиксы: exe `a61557b5…`, zip
`39048993…`, source `6833c2f4…`; общий контракт —
`ca7ebbc51a73fd1275422b9e5b697028879264d16ec5501e92d0c4395ac6d377`.
Это исторические идентификаторы, не способ проверить новый файл.

До уборки обнаружено несколько более ранних BUILD-INFO с другими хэшами.
Они не считаются манифестом одного и того же пакета. Для конкретной поставки
источником идентичности служат её BUILD-INFO, SHA256SUMS и SBOM; проверяется
полный SHA-256 фактических файлов, а не приведённый здесь префикс.

Новая поставка: `dist/0.1.69-cleanup-20261029/`, state=release, unsigned.
Проверены CI (324 passed, 32 ignored), clippy с предупреждениями, release build,
SBOM/hashes, source archive и его cargo check с текущим locked vendor.
Startup smoke пройден; новой визуальной/device-приёмки не было.
Selftest ok=true; отдельный managed-node start ограничен занятым портом 13413.
Git baseline уборки bb4b7f3; изменения уборки закоммичены (`5de99ae`), снимок
дерева до коммита сохранён как `refs/backups/cleanup-20261029` (f8abb9d8).
Итог: evidence/cleanup-20261029/RESULT.json.

2026-10-29: после коммита corresponding source пересобран из закоммиченного дерева,
поскольку прежний архив отставал от него по документации. Бинарники, SBOM и
BUILD-INFO не менялись; актуальные SHA-256 — в SHA256SUMS поставки, прежний архив
сохранён вне дерева (`C:\DHW-backups\delivery-20261029\`).

2026-10-30: заложен Nostr-транспорт messenger (ADR NOSTR-1, контракт в [MESSENGER.md](contracts/MESSENGER.md)): NIP-44 v2 с официальными векторами, события NIP-01/BIP-340, NIP-59 seal/gift wrap, отдельный transport key на профиль, Transport-обёртка с MemoryRelay; 14 новых тестов, lint-messenger 73/73. Зависимости secp256k1/chacha20/hmac, vendor пересобран, offline-резолв проверен. WebSocket-клиент relay добавлен (`WsRelay`, rustls+ring, таймауты, OK/REQ/EOSE): live-проба чтения на `wss://nos.lol` PASS, публичный relay damus отвечает `auth-required` (NIP-42 в v1 нет). Проводка: настройка `nostr_relay_url` (отдельно от `relay_url`), поле `nostr` у контакта, `AppTransport::Nostr` и поле статуса `transport`. UI: настройки разбиты на разделы «Профиль и язык», «Seed и пароль», «Relay», «Tor», «Нода и данные»; поле Nostr relay, Nostr-ключ в диалоге контакта, свой ключ с кнопкой копирования в панели контактов. Loopback end-to-end drill (publish → OK → REQ → EVENT → EOSE → fetch → ack по реальному сокету) — PASS; 77 тестов messenger. Визуальная проверка в реальном окне (одноразовый testnet-профиль) — PASS, evidence/ui-nostr-20261030; копирование ключа проверено через буфер. Остаются: публикация на внешнем relay (NIP-42), приёмка зависимостей и поставка.
2026-10-30: UI-полировка. Собственный набор иконок 24×24 (навигация, действия, пустые состояния); `ActionButton` во всех кнопках вместо `Button` (primary/secondary/ghost/danger, контурный disabled); настройки разбиты на разделы «Профиль и язык», «Seed и пароль», «Relay», «Tor», «Нода и данные»; «История» переверстана (шапка, «Очередь»/«Операции» в ScrollView, пустые состояния, закреплённый подвал); «Получение» и «Безопасность» — прокрутка с закреплённым подвалом, QR 160×160 pixelated. Проверено в окне 1300×900 и 1000×800; evidence/ui-icons-20261030, ui-polish-20261030, ui-history-20261030, ui-buttons-20261030. Тесты: 342 passed / 0 failed / 33 ignored; flint-slint 21. Остаётся: визуальная проверка диалога «Отправка» (в тестовом профиле недоступна), новая поставка, публикация Nostr на внешнем relay.

2026-10-30: приёмка зависимостей текущего дерева — RustSec PASS (db 99670cfc0bc5, lock 449647BDF4CE; 2 accepted, 0 unaccepted, 17 warnings), SBOM 1161 компонент (1155 cargo + 6 native) сгенерирован и проверен; evidence/deps-20261030. Новые зависимости Nostr непринятых advisories не добавили.
2026-10-30: поставка `dist/0.1.69-ui-20261030` (build id `ui-20261030`): release build, 342 tests, RustSec PASS, SBOM 1161 проверен, selftest ok=true, signed=false; corresponding source 13.1 МБ без `evidence/`; prerelease `v0.1.69-ui-20261030` (6 ассетов) и ссылки/хэши на сайте обновлены. Доказательства: `dist/0.1.69-ui-20261030/SHA256SUMS`, `BUILD-INFO.json`, `selftest.json`.
2026-10-30: live testnet прогон send → receive → confirm → cancel — PASS (122.33 s): кошельки восстановлены из одноразовых seed в `docs/testnet-funded.txt` (A `tgrin1flemga…` 1396.014 GRIN, B `tgrin16g2mhw…` 0.027 → 0.227 GRIN), нода Ready height 3315128–3315130, 8 пиров; отмена вернула spendable без потерь. Причина, почему в свежем тестовом профиле отправка недоступна: `send_enabled` требует `node.phase == Ready` (не только баланс), а новый кошелёк ещё и не просканирован. Диалог «Отправить» проверен в окне 1000×800 и 1300×900; исправлена обрезанная кнопка «Подготовить» (170 px) и добавлен разделитель подвала; evidence/live-send-20261030. Временные кошельки и примеры удалены.
2026-10-30: NIP-42 в Nostr-клиенте: AUTH kind 22242 (`relay`/`challenge`, пустой content), ответ на challenge, ожидание OK, повтор ожидающего EVENT/REQ, лимит двух challenge, явная ошибка при `auth-required` без challenge. 79 тестов messenger (+2: форма AUTH-события и loopback-relay с обязательным AUTH). Live: damus присылает challenge, наш AUTH доходит, но damus отвечает своей ошибкой `relay needs serviceUrl…`; snort/primal/nos.lol не требуют AUTH для анонимных фильтров. Остаётся выбрать relay с рабочим AUTH для live-приёмки. Clippy новых предупреждений не дал.
2026-10-30: автофинализация slatepack не зависит от транспорта. Inbox ставит armor-задачу для любого fetch (relay, direct onion, Nostr), а `auto_slatepack_targets` фильтрует по Flint-адресу из контактов; ответ уходит выбранным транспортом. Добавлен тест `slatepack_armor_survives_over_nostr` (armor возвращается байт-в-байт) и пояснение в runtime; 80 тестов messenger. Условия: чекбокс включён, кошелёк разблокирован, отправитель сохранён контактом (Nostr-ключ нужен только для доставки ответа), сообщение — slatepack armor.
2026-10-30: конкурентный доступ усилен: `<data>/flint.lock` удерживается эксклюзивно (share=0) на весь процесс, поверх mutex — второй сеанс/алиас junction не открывает каталог; тест `data_root_lock_is_exclusive`. Embedded `open_legacy`/`open_envelope` больше не прописывают managed `.api_secret` внешней ноде. Подтверждение операции и откат бэкапа — следующий шаг: нужен маркер подтверждения в журнале и chain-сверка входов.

Повторный полный локальный CI на закоммиченном дереве (`64793c5`): PASS —
324 passed, 0 failed, 32 ignored; i18n ok (216 static entries); Slint собран;
0 error-строк, предупреждения того же класса, что в принятой приёмке.

Harness-модули 2026-10-29: в профиле desktop установлен только LSP-набор
0.2.1-rc.3 и проверен реальными запросами к Rust-символам; worktree, computer use
и browser automation отклонены с условиями повторного рассмотрения в
[DSH.md](DSH.md). Поставка, Cargo и vendor не менялись. Кэш LSP `target/` (≈1,7 ГиБ)
удалён при завершении сессии, дерево снова 7,93 ГиБ; первый LSP-запрос строит кэш
заново (тем же путём, что проверен сегодня).

## Что осталось

Только физические проверки принятого RC — [D-01…D-10](ACCEPTANCE.md#devices).
Для новой сборки сначала повторяются относящиеся к изменению автоматические,
live и packaging-проверки; незавершённый такой прогон нельзя назвать device-only.

Изменение backend, seed/recovery, денежных состояний, протокола или зависимостей
повторно открывает соответствующую приёмку. Результаты старой сборки сохраняют
дату и идентификатор, но не переносятся автоматически на новый код.

## Технические ограничения и отложенные треки

- Embedded и RPC поддерживаются одновременно. Capability определяет доступные
  действия; удаление backend требует отдельного решения и parity-проверок.
- ADR F-04 реализован в fork и локальных сценариях recovery; физические disk-full
  и power-loss остаются открытыми. ID F-01, R-01, SENT-01/SENT-02 и PT-01…PT-05
  сохранены в [SECURITY.md](SECURITY.md#finding-ids).
- Messenger v1 не скрывает все метаданные и не имеет forward secrecy. Публичного
  relay-сервиса эта приёмка не подтверждает.
- Установленный Windows-клиент и мобильный проект имеют свои данные/жизненный
  цикл. Desktop-проверки не подтверждают iOS-сборку.
- Канон iOS — [mobile/GrinFlint](../mobile/GrinFlint/); в desktop нет копии его
  документации. Общие правила переводов — [contracts/TRANSFER.md](contracts/TRANSFER.md).
- Android остаётся [проектным планом](mobile/android/PLAN.md), без реализованного приложения.

## Как обновлять статус

Записывать только изменившийся факт, остаток и завершённую проверку: дата,
commit/build, команда или CI run, PASS/FAIL/NOT VERIFIED, ссылка на сохранённое
свидетельство и предел вывода. Большие логи, переписка и промежуточные планы
не копируются сюда. Удаление исходного evidence требует либо сохранённого
эквивалента, либо явной пометки, что доступна только историческая запись.






