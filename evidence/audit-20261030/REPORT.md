# Flint: desktop + iOS — локальный аудит 30.10.2026

## Объём и воспроизводимость

Прочитаны актуальные security-critical участки Rust/Slint runtime, Nostr,
Swift lifecycle/Keychain/журнала и Rust FFI. Это ограниченный аудит исходников
с локальными проверками, не доказательство отсутствия иных уязвимостей.
Реальные средства, mainnet и существующие пользовательские кошельки не использовались.
Исправления продуктового кода не вносились.

Desktop при старте: `972e876` с незакоммиченными изменениями Nostr/runtime.
Во время проверки другое рабочее действие зафиксировало их в `2e6d4bf`, затем
появился `7c8c8b7`. Наш аудит эти коммиты не создавал. Перечисленные ниже места
повторно проверены после изменения HEAD; тесты не объявляются проверкой
неизменного единственного снимка всего дерева. iOS: `ace59d2`; dirty только
известная перестройка документации, исходники приложения чистые.

P1 — блокирует приёмку соответствующего сценария; P2 — существенная ошибка
безопасности/целостности; P3 — ограниченный риск доступности.
«По коду» означает подтверждённую ветку исполнения, но не выполненный exploit.

## Результаты выполненных проверок

| Проверка | Результат | Предел |
|---|---|---|
| Desktop library tests: core/runtime/messenger/contract | PASS: 281 passed, 1 ignored | `desktop-lib-tests.log`; не Slint/UI и не полный integration suite |
| Полный desktop `cargo test --workspace --locked --offline` | НЕ ЗАВЕРШЁН | Два запуска остановлены LNK1104 для node_manager/rpc_lifecycle; причина блокировки файла не установлена |
| iOS SwiftPM core на Windows | PASS: 23 passed | `ios-swift-tests-retry.log`; реальные Core-файлы, без SwiftUI/Security/LocalAuthentication |
| iOS Rust FFI host roundtrip + ffi_memory | PASS: 18 passed | `ios-ffi-tests.log`; offline, Windows, временные кошельки; не SwiftUI и не live chain |
| Диагностический Rust-пример | FAIL защитных инвариантов, воспроизведён | `desktop-probes.rs`, `desktop-probes.log`; только искусственные данные, без сети |
| RustSec desktop Cargo.lock | 2 записи, 17 предупреждений | `desktop-rustsec.json` |
| RustSec iOS Cargo.lock | 6 записей, 11 предупреждений | `ios-rustsec.json`; наличие в lock не доказывает достижимость |

RustSec выполнен offline по базе `99670cf` от 27.10.2026; это не проверка
самой свежей базы на 30.10. Подробности Rust FFI исполнения — в `ios-ffi-tests.log`
и итоговой `RESULT.json`; частичный прогон не считается PASS полного набора.

## Desktop: подтверждённые находки

### D-A1 — P2: credential не привязан к endpoint (воспроизведено)

`crates/core/src/service.rs:475–505`: при отсутствии явного API-секрета берётся
`.api_secret` управляемой ноды независимо от выбранного `node_url`.
HTTPNodeClient передаёт его в Basic Auth
(`third_party/grin-wallet/impls/src/client_utils/client.rs:206–208`).
Оператор выбранной внешней ноды получает локальный credential. Это не seed
и не доказанная возможность подписывать денежные операции.

Дополнительно `config.rs:92–104,544,589` не применяет URL-политику при загрузке:
сохранённый remote HTTP URL сохраняется. Оба свойства воспроизведены на
искусственном testnet-корне, без HTTP-запросов. Нужна привязка credential к
точному endpoint; локальный fallback — только для собственной управляемой ноды;
валидация схемы/host при чтении, миграции и использовании настроек.

### D-A2 — P2: receive-cache обходит проверку исходного S1 (по коду)

`service.rs:1827–1831`, `rpc_backend.rs:1611–1634`: кешированный S2 выдаётся по
UUID без сверки содержимого нового S1. Upstream проверяет исходный запрос
(`third_party/grin-wallet/libwallet/src/api_impl/foreign.rs:130–141`), но кеш
обходит эту ветку. Изменённые amount/nonce/participants с тем же UUID могут
получить успешный ответ от иной версии запроса. Шифрование сохраняет прежнего
получателя; кража средств этой находкой не доказана.

Нужна каноническая идентичность исходного запроса и integrity error при
несовпадении. Регрессия: точный повтор успешен, изменённый S1 с тем же UUID
отклоняется в обоих backend. Не хешировать случайно меняющуюся armor-обёртку.

### D-A3 — P2: ошибки изменения RPC config подавляются (по коду)

`crates/core/src/rpc_backend.rs:346–374`: чтение/запись TOML могут не выполниться,
но `patch_node_config` продолжает запуск. Readiness проверяет новую ноду,
а wallet child может работать по старой конфигурации. Нужны Result, атомарная
запись и проверка фактической конфигурации до запуска. Тест: запрет записи
конфига должен запретить запуск, а не продолжить работу со старой нодой.

### D-A4 — P2: обход loopback-only для plaintext Nostr (воспроизведено)

`crates/messenger/src/nostr/ws.rs:47–59`: starts_with вместо разбора URL.
Приняты `ws://127.0.0.1.attacker.invalid/`, `ws://localhost.attacker.invalid/`
и `ws://127.0.0.1@attacker.invalid/`. Внешний plaintext endpoint проходит
политику. При этом вложенное сообщение остаётся зашифрованным: это обход
транспортной политики, риск наблюдения/подмены транспорта и доступности,
не демонстрация расшифрования содержимого.

Нужен URL parser, проверка parsed host/IP и userinfo, запрет неоднозначных
адресов. Регрессии должны покрыть DNS-суффиксы и userinfo, не только схемы.

### D-A5 — P2/P3: сетевой бюджет Nostr начинается после handshake (по коду)

`nostr/ws.rs:92–108`: blocking connect выполняется до установки read/write
timeout и до operation deadline. Сервис, зависший на TLS/WebSocket handshake,
может удерживать worker дольше заявленного бюджета. Отдельно в query
`seen.insert` выполняется после достижения лимита полезных событий
(`:270`), а WebSocket использует default 64 MiB message/16 MiB frame.
Бесконечная память не доказана: query имеет временной deadline. Нужны
сквозные connect/DNS/TLS/handshake/read бюджеты, малые лимиты кадров, общего
числа событий и суммарных байтов. Проверяется локальным slow/flood relay.

## iOS: подтверждённые находки

Все пути далее относительно `mobile/GrinFlint`.

### I-A1 — P1: restore может открыть другой существующий кошелёк (по коду)

`GrinFlintApp/GrinFlint/ViewModels/AppState.swift:100–130`: любая ошибка restore
перехватывается и сменяется `service.open()` с успешным завершением онбординга.
При прерванном создании A и попытке восстановления B можно открыть A без
доказательства соответствия введённой seed-фразе. Нужны типизированная причина
resume и сверка идентичности/энтропии; остальные ошибки должны сохраняться.
Тест с mock service A/B возможен на macOS CI без телефона.

### I-A2 — P1: журнал денежных операций допускает тихую потерю (по коду)

`Core/PendingOperation.swift:173–201`: corrupt/read error даёт пустой журнал,
ошибка save игнорируется. `Services/FFI/GrinWalletFFIService.swift:148–172`
сначала изменяет backend, затем пишет журнал; именно журнал блокирует новую
отправку. После сбоя записи/restart прежняя операция скрывается и разрешается
новая пользовательская отправка. Это не доказанный double-spend одних входов.
При receive аналогично может потеряться единственная доступная копия S2.

Нужны fail-closed corruption, транзакционный/восстанавливаемый intent, возврат
ошибок записи и сверка с backend. `Tests/GrinFlintCoreTests/JournalTests.swift:133`
сейчас закрепляет ошибочный контракт «corrupt → empty», поэтому зелёный suite
не опровергает находку. Fault injection доступен в Windows SwiftPM.

### I-A3 — P1/P2: несовместимые crash windows при finalize (по коду)

`GrinWalletFFIService.swift:210–237` сохраняет `.finalized` ещё до вызова FFI.
Crash в этом месте оставляет unsigned операцию с разрешённым только repost.
Обратное окно: `ffi/grin-wallet-ffi/src/lib.rs:665–670` уже финализировал
upstream, но ещё не сохранил repost JSON. Если запись не удалась, `:698–703`
не умеет восстановить repost из upstream stored transaction; cancel уже
запрещён. Swift определяет фазу по подстроке `post_tx` и может ошибочно
вернуть операцию на ожидание S2. Подписанная raw transaction остаётся в
upstream: необратимая потеря не доказана, штатное восстановление нарушено.

Нужны отдельное состояние finalizing, структурированный результат FFI,
reconcile по реальному backend и восстановление сохранённой подписи.
Проверить crash до FFI и после finalize_tx до repost-write без второй подписи.

### I-A4 — P2: неверная диагностика/отсутствие усиления Keychain ACL (по коду)

`Services/KeychainStore.swift:190–200` при отсутствии passcode создаёт v2
без userPresence. `:96–107` считает любую v2 защищённой и не усиливает её
после появления passcode. Нужна проверка свойств записи и безопасная миграция,
с сохранением прежней записи до контрольного чтения. Логику проверить mock
Keychain, реальные Security ACL — только macOS/iOS окружением/устройством.

### I-A5 — P2: ручной lock не сбрасывает недавнее разрешение (по коду)

`ViewModels/AppLockManager.swift:77–87,99–103` и
`Services/BiometricAuth.swift:54–61`: после ручного lock/background новый
unlock в пределах 10 секунд использует прошлое подтверждение. Generation
защищает старый async-result, но не новый unlock с cached timestamp.
Сбросить authentication context/cache при новой блокировке и отделить
одноразовую передачу результата Keychain от повторного открытия приложения.

### I-A6 — P2: refresh может выдать stale как актуальный баланс (по коду)

`ffi/grin-wallet-ffi/src/lib.rs:434–437` игнорирует validated из
retrieve_summary_info. Успех предыдущего get_tip не доказывает успех
последующих output/kernel запросов. Нужен validated/stale в контракте FFI
и UI. Тест: tip отвечает, следующий этап sync отказывает, старый баланс
нельзя маркировать успешно обновлённым.

### I-A7 — P2: секрет ноды не проходит из приложения в FFI (по коду)

`Services/FFI/GrinWalletFFIService.swift:38–46` не принимает apiSecret и вызывает
handle без него, хотя `Services/FFI/GrinFFI.swift:23` его поддерживает.
`Services/NodeProbe.swift:106–119` тоже не добавляет Authorization. Успех
низкоуровневого node_auth.rs не доказывает работу authenticated node в UI.
Нужна единая конфигурация endpoint/network/secret для Probe и реального handle;
проверка — HTTP-стенд + интеграция service/factory без физического телефона.

### Дополнительное усиление

Импорт файлов Send/Receive читает всё содержимое синхронно без лимита размера:
возможен UI freeze/OOM на большом файле (P3). Нужны размер до чтения, ограниченное
чтение и off-main обработка. iOS build workflow скачивает и патчит crates через
curl вне Cargo checksum-проверки и использует Cargo без --locked; стоит закрепить
хэши patch inputs и сделать drift ошибкой CI.

## Зависимости: классификация, а не число «эксплойтов»

Desktop lock: crypto-bigint 0.5.5 / RUSTSEC-2026-0387, rsa 0.9.10 /
RUSTSEC-2023-0071; проект имеет прежнее обоснование принятия этих записей.
Из-за новых зависимостей reachability следует перепроверить для release features.

iOS lock дополнительно содержит curve25519-dalek 3.2.0 /
RUSTSEC-2024-0344 и zip 0.5.13 / RUSTSEC-2026-0367/0368/0369.
Для dalek RustSec указывает исправление >=4.1.3, но это не разрешение
механически обновить несовместимый транзитивный major. Для zip нужно установить
использование extract/symlink/Unix permissions в поставке: достижимость атак
в приложении этим lock-анализом не доказана.

Источники: https://rustsec.org/advisories/RUSTSEC-2024-0344.html ;
https://rustsec.org/advisories/RUSTSEC-2023-0071.html ;
локальный advisory-db commit выше. Upstream iOS PBKDF2=100 — hardening concern,
но текущий случайный пароль из 32 bytes не делает реалистичным словарный перебор.

## Готовность и что закрывается без новых устройств

Desktop — продвинутый testnet RC, но текущий код имеет внутренние P2. Старый
READY_FOR_EXTERNAL_DEVICE_TESTS относится к идентифицированному принятому RC,
а не автоматически ко всем новым изменениям и не к mainnet.
iOS — рабочий прототип/кандидат с подтверждёнными внутренними P1; утверждение
«остались только устройства» для него пока неверно.

Порядок работ без телефона/нового ПК:
1. Исправить I-A1/I-A2/I-A3 и закрепить fail-closed fault-injection тестами.
2. Закрыть endpoint/auth/stale и desktop credential binding, S1 integrity, RPC errors.
3. Закрыть URL/handshake/лимиты Nostr, добавить adversarial relay tests.
4. Перепроверить dependency reachability по обеим release-сборкам и свежей базе.
5. На Windows прогнать Rust/SwiftPM, коррупцию/permission errors/restart и
   mock-node проверки; на существующей машине можно создать второй аккаунт,
   VM и тестовый VHD для части Windows-сценариев.
6. Через доступный macOS CI выполнить SwiftUI/unit/XCUITest с real FFI,
   A/B restore, lock-cache и journal crash сценарии. Это не требует своего iPhone,
   но требует macOS runner; этот аудит workflow не dispatch-ил и ничего не push-ил.
7. После исправлений — единый commit, полный gate и новая поставка с manifest.

За физическими устройствами остаются фактические Face ID/Touch ID/Keychain ACL,
камера, реальные OS-прерывания/сеть/энергопотребление; mixed-DPI/экранный диктор
и power-loss требуют подходящего стенда. VM-симуляция не равна физическому
отключению питания и не подтверждает конкретный iPhone/PC.
