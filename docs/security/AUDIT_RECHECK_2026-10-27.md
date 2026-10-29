# Flint 0.1.69 — повторный аудит заявленных исправлений

Дата: 2026-10-27  
Основание: утверждения о закрытии F-01–F-12 и PT-01–PT-05, текущий `C:\DHW`.

> **Итог: большинство исправлений действительно присутствует, но заявление о полном
> закрытии F-01 неверно, F-04 остаётся P1, а текущее дерево не проходит `cargo fmt`
> из-за синтаксической ошибки. Mainnet по-прежнему заблокирован.**

Это повторный AI-review кода и доступных локальных проверок, не человеческий
пентест. Исходный код аудитором не менялся. Во время проверки файлы и структура
`docs/` менялись вне этого аудита; результаты ниже привязаны к указанным участкам
и моментам запуска, а не к неизменному release snapshot.

## 1. Сводная таблица

| ID | Проверенный статус | Основание / остаток |
|---|---|---|
| F-01 | **Частично, P1** | Allowlist и запрет publish из `Finalizing` есть; ветка по тексту `DB Not Found`/`Slate id` всё ещё создаёт `FinalizedNotPosted`, который затем auto-publish |
| F-02 | **Исправлено по коду** | `prepared`/`locked` allowlist, `failed_recoverable` не подписывается и не отменяется |
| F-03 | **Исходный дефект исправлен** | Строгий `amt`, отсутствие/нечисло/ноль отклоняются; настоящий Slate probe был в исходном аудите, живой RPC receive не повторялся |
| F-04 | **Частично, P1** | Durable intent есть, но crash после мутации и до response остаётся невосстановимым; добавлена ошибка классификации R-01 |
| F-05 | **Частично** | ACL temp/primary/backup и миграция на open есть; ошибка ACL corrupt-копии подавляется, второй Windows-аккаунт не проверен |
| F-06 | **Исправлено по коду** | `quarantine_unreadable_envelope` удерживает `SeedLock` |
| F-07 | **Исходный дефект исправлен по коду** | Основные пути quoted, ошибки bridges кроме NotFound возвращаются; live Tor не проверен |
| F-08 | **Исправлено по коду** | 64 активных handler, переполнение получает 503; нагрузочный тест не выполнен |
| F-09 | **Исправлено по коду** | Remote plain HTTP отклоняется в общем `update_settings` |
| F-10 | **Частично** | Четыре exe совпадают с manifest; vcruntime/pt_config не pinned, native SBOM и обязательная production-подпись открыты |
| F-11 | **Исправлено по коду** | Маркер каталогов и owned child PID; crash-script не запускался |
| F-12 | **Исправлено по коду** | `take(MAX_RESPONSE + 1)` до `read_to_end` |
| PT-01 | **Исходная гонка закрыта по коду** | Cleanup не удаляет legacy при отсутствии/нечитаемости envelope; regression-тест прошёл |
| PT-02 | **Исправлено по коду** | Страница ≤2 MiB, клиент ≤100; regression-тест очереди прошёл |
| PT-03 | **Исправлено для нормального restart** | Persisted messages загружаются до POST; restart-тест прошёл |
| PT-04 | **Исправлено** | Cross-instance lock + unique tmp; тест двух ContactBook прошёл |
| PT-05 | **Исправлено по коду** | Readiness внутри RPC scan, partial restore scan запрещён; live не проверен |

«Исправлено по коду» не означает live/приёмочную проверку или готовность mainnet.

## 2. Оставшиеся P1

### F-01 — текст ошибки по-прежнему считается доказательством подписи

Положительные изменения подтверждены:

- `service.rs:1517-1525` разрешает новую финализацию только из
  `Prepared|Locked`;
- `1624-1636` переводит `Finalizing` в `FailedRecoverable` без publish;
- `2059` автоматически обрабатывает только `FinalizedNotPosted`;
- общие ошибки `finalize_tx` больше не возвращаются в `Locked`.

Но `service.rs:1550-1576` сохраняет исключение: если текст содержит
`DB Not Found` или `Slate id`, код утверждает, что подпись уже создана, и пишет
`FinalizedNotPosted`. Такая запись разрешена для `retry_post`, а reconcile делает
это автоматически. Следовательно, именно недоказанный переход
«строка ошибки → signed → stored tx publish» остался.

**Вывод:** первоначальный сценарий `Finalizing → publish` закрыт, F-01 целиком —
нет. Нужно переводить любой неподтверждённый результат в `FailedRecoverable`;
`FinalizedNotPosted` создавать только после успешного возврата `finalize_tx` или
иного криптографического/transaction-log доказательства подписи. Live crash-прогон
остаётся обязательным, но его отсутствие — не единственная причина статуса.

### F-04 — durable intent полезен, но не устраняет crash gap

Embedded `service.rs:1816-1857` и RPC `rpc_backend.rs:1520-1566` сохраняют intent
до `receive_tx`, а response — после. Если процесс исчезнет между возвратом
`receive_tx` и durable response, сам response в журнале отсутствует. Повторный
`receive_tx` не восстанавливает его.

**Вывод:** утверждение «частично, максимум без upstream» корректно. До полного
закрытия требуется upstream API/патч, сохраняющий response атомарно с wallet
mutation, либо собственная реализация с эквивалентной гарантией. Это P1 для
интерактивных денежных операций, а не доказанная потеря монет.

## 3. Новая регрессия R-01 — любой ошибочный receive становится `response_lost`

Embedded `service.rs:1832-1840` и RPC `rpc_backend.rs:1534-1553` переводят запись
в `ResponseLost`/`lost` при **любой** ошибке `receive_tx`, а не только при
`TransactionAlreadyReceived` или доказанной неоднозначности.

Ошибка разбора/сети/RPC/временной недоступности могла произойти до мутации, но
следующий вызов уже запрещён (`service.rs:1797-1807`,
`rpc_backend.rs:1488-1518`). Комментарий embedded «retrying is safe» относится
только к состоянию intent до этого ошибочного перехода.

**Уровень:** P2, доступность конкретной интерактивной сделки.  
**Исправление:** оставлять `ReceiveIntent` для ошибок, где мутация не доказана;
`ResponseLost` назначать только при `TransactionAlreadyReceived` после
неоднозначной попытки либо явном recovery. Добавить тесты: ошибка до мутации,
ошибка после мутации, повтор с сохранённым response, crash между response и
journal rename.

## 4. Проверка остальных пунктов

### Seed: F-06 и PT-01

- `seed/store.rs:712-749` берёт общий `SeedLock` до проверки и rename — F-06
  подтверждён.
- `seed/store.rs:900-914` перед cleanup требует существующий читаемый envelope;
  regression `stale_cleanup_cannot_delete_a_concurrent_disable_legacy_seed`
  присутствует и прошёл. Исходный сценарий «disable завершился, stale cleanup
  удалил последний legacy» закрыт.
- Это не заменяет живой многопроцессный create/restore/disable drill и более
  общее доказательство владения/идентичности seed.

### Settings: F-05

`config.rs:465-492` ограничивает temp до записи, backup и primary после записи;
`395-405` мигрирует ACL существующих primary/backup при открытии и возвращает
warning. Но `413-417` применяет ACL к corrupt-копии через `let _ =`, поэтому её
ошибка остаётся скрытой. Доступ из второго Windows-аккаунта не проверялся.

### Tor: F-07

`tor_listener.rs:126-142` экранирует основные пути и различает NotFound;
`275-281` использует quoting для DataDirectory, HiddenServiceDir и лога.
Отдельно `177-180` формирует ClientTransportPlugin другим способом, без doubling
backslashes; это требует live-проверки путей Windows и не должно скрываться под
общим статусом «все пути проверены».

### Gateway/relay: F-08, PT-02, PT-03

- `gateway.rs:167-182` ограничивает активные handlers 64, лишние соединения
  получают 503. Stress/fairness/ресурсы под реальным flood не проверялись.
- `532-581` ограничивает страницу 2 MiB; клиент запрашивает до 100 сообщений
  (`messenger/transport.rs:309-311`). Тест полной очереди прошёл.
- `127-153` загружает persisted messages при старте до приёма POST; restart-тест
  прошёл. Ошибки чтения каталога/отдельных файлов по-прежнему пропускаются, то
  есть degraded storage не является fail-closed quota source; это отдельный
  остаточный риск, не опровержение нормального restart-сценария.

### ContactBook: PT-04

`messenger/contacts.rs:47-73,173-197` сериализует load-modify-store через
`contacts.lock`; `226-233` использует уникальный temp. Тест
`parallel_books_do_not_lose_contacts` прошёл. Crash может оставить stale lock и
временно блокировать изменения — операционный P3, не прежняя потеря обновлений.

### Scan: PT-05

`runtime/scan.rs:186-215` запрещает partial restore scan и проверяет
`node_info().phase.ready()` внутри RPC-ветки до `begin_scan`. Исходный mock-обход
закрыт по коду; live node и concurrency lifecycle не проверялись.

### Node URL: F-09

`service.rs:188-211` разрешает plain HTTP только loopback; общий runtime
`update_settings` вызывает эту проверку. HTTPS и loopback остаются допустимыми.

### Native assets и crash tooling: F-10/F-11/F-12

Все четыре строки `resources/native-assets.sha256` независимо сверены через
SHA-256: `grin.exe`, `grin-wallet.exe`, `tor.exe`, `lyrebird.exe` — совпадения.
`package-slint.ps1:29-39` действительно отклоняет mismatch. При этом package
копирует `vcruntime140.dll` и `pt_config.json` вне manifest; `sbom.ps1` по-прежнему
перечисляет Cargo packages, а `release.ps1` требует подпись только с
`-RequireSigned`.

`crash-test-rpc.ps1:33-68` требует marker, отклоняет пути с `mainnet` и завершает
только зарегистрированные `grin-wallet` child PID. Скрипт не запускался.
`messenger/transport.rs:128-134` ограничивает чтение до проверки размера.

## 5. Результаты проверок и повреждение текущего дерева

До последнего изменения `service.rs` была получена часть workspace-прогона:

- core unit suite: **159 passed, 0 failed**;
- lifecycle: **4 passed**;
- live suites: ignored;
- запуск остановлен по timeout 300 секунд на `node_manager` после начала
  `restarts_cleanly_after_a_normal_stop`; это **незавершённый прогон, не доказанный
  failing test**.

После этого messenger проверен отдельно на текущем дереве:

- **55 passed, 0 failed, 1 ignored**, включая parallel ContactBook.

Повторный `cargo fmt --all -- --check` на текущем дереве **FAIL**:

```text
crates/core/src/service.rs:718:20
ecover_from_phrase refuses to work unless the seed directory exists;
                         ^ expected Rust syntax
```

В строке 718 вне комментария находится фрагмент `ecover_from_phrase...`; файл не
парсится. Поэтому `cargo check --workspace` и полный `cargo test --workspace` для
текущего снимка не завершены, selftest не запускался. Заявленные в STATUS
`256 passed` относятся к предыдущему проверенному дереву и не могут засчитываться
как текущий результат.

Не выполнялись: live/ignored, funded testnet, mainnet, реальный RPC receive,
второй Windows-аккаунт, реальный Tor, flood, power-loss/disk-full, RustSec,
человеческий pentest и проверка подписанного release artifact.

## 6. Решение

1. Сначала восстановить парсинг `service.rs` и получить чистые fmt/check/test на
   неизменном снимке. Артефакты предыдущего дерева не считать пересобранными.
2. Убрать текстовую эскалацию F-01 в `FinalizedNotPosted`; закрепить regression
   для всех ошибок finalize и auto-reconcile.
3. Исправить R-01, затем реализовать полное решение F-04 через upstream/atomic
   response storage. До этого mainnet остаётся запрещён.
4. Закрыть остатки F-05/F-10 и выполнить live crash/RPC receive/Tor/flood/ACL
   проверки на отдельном testnet-стенде.
5. Не менять статус MAINNET_READINESS: человеческий аудит, RustSec, подпись,
   mainnet dry-run и destructible-стенд по-прежнему не заменены этими правками.

**Вердикт:** работа по закрытию аудита реальная и существенная, но сводка
«F-01/F-04 и всё остальное, кроме внешних гейтов, закрыто» не подтверждается.
Текущие блокеры — F-01, F-04 и непарсящийся исходный снимок; R-01 требует
исправления вместе с receive recovery.
