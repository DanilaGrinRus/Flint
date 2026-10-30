# Архитектура Flint

Действующий desktop: Rust workspace edition 2021 / MSVC, native Slint 1.18.1,
минимальный Rust 1.92. Tauri удалён. Общие правила — [AGENTS.md](../AGENTS.md),
текущее доказанное состояние — [STATUS.md](STATUS.md).

## Слои и владельцы

| Каталог | Ответственность |
|---|---|
| crates/core | Grin wallet service, embedded/RPC backend, state/pending, SeedStore, NodeManager, gateway и flint-relay |
| crates/runtime | WalletRuntime: публичный API кошелька, RPC/Tor lifecycle, messenger, ключ истории, поколения профиля |
| crates/messenger | Конверт, crypto, direct/relay transports, mailbox, history, contacts и keywrap |
| crates/transfer-contract | Платформенно независимые суммы, сети, стадии и разрешённые действия |
| crates/slint-ui | Единственный desktop UI, презентация и обработчики действий |
| crates/grin-wallet-rpc | Сборка управляемого grin-wallet из закреплённого fork |
| crates/janus | Демо state machine свопов; настоящего атомарного обмена нет |
| third_party | Поддерживаемый fork и его source manifest/patch |
| resources | Проверяемые native assets, необходимые для поставки |
| tools | Сборка, проверки, упаковка и управляемые тестовые стенды |
| mobile/GrinFlint | Самостоятельный iOS-репозиторий со своими правилами и сборкой |

Slint и WalletRuntime работают в одном процессе. Это граница ответственности,
а не изоляция от malware. UI обращается к публичным runtime-методам, не читает
seed и не обходит проверки прямым RPC. Runtime владеет дочерними процессами,
секретами и денежными проверками. Отложенный результат получает generation
профиля: после переключения он не может изменить другой кошелёк.

## Backend и совместимость

Поддерживаются embedded и управляемый grin-wallet RPC. Доступность функций
определяется `backend_capabilities`, а не строкой backend в настройках. В ответе
учитываются mode, wallet, tor, rpc_delete, scan_progress и offline_mobile.
Действие в UI допустимо только если его разрешают backend и runtime-состояние.

Существующее ограничение удаления embedded сохранено: не раньше 0.2.0, после
явной WalletBackend-абстракции, parity create/open/restore/send/receive/scan/delete,
рабочего мобильного shell и релиза с обоими проверенными путями. Это отдельное
архитектурное решение; уборка дерева его не выполняет.

Закреплённая база grin-wallet — 5.5.1, base commit
`f097811ee953fc81be4e70435d6ef845b6b2e8c9`; recovery-capable fork commit
`11046cd197f8296e6cd9313502a9f2cbce6e262a`; grin commit
`dbaff34f7751ee23316fbbfe5806b47c546776ca`.
Канон происхождения — `third_party/GRIN_WALLET_FORK.toml`, source hashes и patch;
номер версии бинаря сам по себе не доказывает capability. Embedded и managed RPC
собираются из одного закреплённого источника.

## Денежные операции и recovery

Суммы — целые nanogrin; checked arithmetic, без float. Доставка сообщения,
получение ответа, публикация в ноду и chain confirmation — разные события.
Нельзя автоматически считать posted подтверждением или заменять неясное
состояние новой подписью. Finalize/retry/reconcile продолжают сохранённую
операцию и сверяют backend/chain. Отмена разрешается runtime только там,
где она безопасна, а не по одному скрытому/показанному UI-контролу.

- Общие стадии/DTO: [contracts/TRANSFER.md](contracts/TRANSFER.md).
- Получатель S1→S2 и атомарная граница: [ADR F-04](contracts/RECEIVE_RECOVERY.md).
- Messenger, persisted reply и ACK: [contracts/MESSENGER.md](contracts/MESSENGER.md).

Первое восстановление из seed требует полного scan. До его завершения старый
баланс неизвестен, а не нулевой. Неготовая/несовпадающая сеть ноды не допускает
отправку или восстановительный scan. Явная node/API error остаётся видимой.

## Жизненный цикл ноды

NodeManager запускает официальный grin.exe как управляемый дочерний процесс,
не блокируя открытие окна синхронизацией. Фазы: starting → headers → blocks → ready.
При закрытии Flint принадлежащая ему нода останавливается graceful Ctrl-Break
с ожиданием, затем kill как fallback. Внешняя здоровая нода только подключается
и не становится собственностью UI. Порт, секрет, chain и user_agent проверяются;
чужой сервис на порту — ошибка, не повод молча стартовать вторую ноду.

Отдельная testnet-нода `tools/run-testnet-node.ps1` может жить дольше UI.
Автоматическая синхронизация при открытом Flint не означает установленный
Windows service/daemon или автозапуск при входе в систему.

## ADR: решения и предложения

| ID | Решение / статус |
|---|---|
| F-04 | Принято и реализовано: S1/S2/output/tx-log в одной upstream LMDB-транзакции; физическая приёмка открыта |
| BACKENDS-0.2 | Действующее ограничение: два поддерживаемых backend, удаление embedded только по гейтам выше |
| UI-SLINT | Действует: Slint единственная desktop-оболочка, GPL-3.0-only и corresponding source Slint в поставке |
| NODE-BACKGROUND | Предложение, не реализовано: отдельный supervisor официальной ноды; согласование запуска/IPC ещё требуется |
| NOSTR-1 | Принято для messenger v1, выключено по умолчанию: Nostr как отдельный транспорт (NIP-44 v2 + NIP-59 + kind 14/1059), свои транспортные secp256k1-ключи на профиль, один wss:// relay, ACK — NIP-09 best-effort; relay-аутентификация — NIP-42 (kind 22242 с тегами relay/challenge, ответ на challenge, повтор REQ/EVENT после OK; live AUTH на публичном relay пока NOT VERIFIED). Без forward secrecy и без встроенного Tor; компрометация transport key раскрывает переписку, но не средства. Зависимости: secp256k1 и WebSocket-клиент — vendor/SBOM обновляются, приёмка зависимостей открывается повторно. Проверка: NIP-44-векторы, roundtrip wrap, live-relay только по флагу |

Для нового существенного решения фиксируются контекст, альтернативы, выбранный
вариант, последствия и проверка. Стабильный ID не меняется при перемещении файла;
заменённое решение ссылается на новое, а не переписывается как будто его не было.

Фоновый supervisor — отдельный будущий этап. Сохраняются требования: один writer
chain DB, opt-in autostart, loopback+auth/ACL, отсутствие seed у ноды, явный stop,
backoff и пределы логов/диска, отказ неизвестной ноде, безопасное обновление.
Открыты выбор отдельного binary или режима приложения, расположение state,
политика обновления, tray и число supervisor на сеть/профили. До реализации
нужны ADR и проверки close/reopen/reboot, второго экземпляра, crash, disk-full,
повреждённой цепи и удаления профиля. Bootstrap chain snapshot — предложение:
происхождение/целостность/проверка узлом и возраст checkpoint должны быть
доказаны до использования; это не доверенный shortcut к балансу.

## Платформы и границы проекта

iOS не включается в desktop Cargo workspace. Общими могут быть доменные правила,
форматы и fixtures; SwiftUI/Keychain/lifecycle и Windows UI/ACL/process management
остаются платформенными. Desktop-пути не должны попадать в portable контракт.
Канон iOS — [его репозиторий](../mobile/GrinFlint/), Android —
[краткий план](mobile/android/PLAN.md). Равный номер upstream не доказывает
одинаковую сборку или recovery: сравниваются pin/patch/capability и тесты.

Janus остаётся demo и не может принимать реальные средства. Публичный relay,
ratchet messenger v2, фоновая нода и Android не являются незавершёнными пунктами
текущего desktop RC и требуют самостоятельного объёма/приёмки.


