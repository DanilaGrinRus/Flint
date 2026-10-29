# Flint 0.1.69 — внешний технический аудит

Дата: 2026-10-27  
Объект: Rust workspace `0.1.69`, embedded `grin-wallet`, RPC `grin-wallet`,
`grin`, Slint UI, Tor, messenger/gateway и демонстрационный Janus.

> **Итог: mainnet и публичный релиз для реальных средств не разблокированы.**
> Явных P0 по принятой шкале не выявлено, но F-01, F-02 и F-04 затрагивают
> денежные переходы и должны быть исправлены до mainnet. Этот документ — независимая
> от реализации проверка в рамках агентской сессии, а не замена внешнему
> человеческому аудиту, пентесту или mainnet dry-run из MAINNET_READINESS.

## 1. Область и модель угроз

Проверялись границы, где ошибка может привести к потере/блокировке средств,
повторной подписи, неверному отображению суммы, раскрытию ключей или отказу
сервиса:

- seed, пароли, ключи истории, ACL и атомарность файловых операций;
- prepare → receive → finalize → post → reconcile/cancel для embedded и RPC;
- совместимость сериализации Slate/Slatepack с `grin-wallet` 5.5.1;
- managed/external node, owner API, RPC-процесс, Tor и hidden service;
- messenger, relay/gateway, ограничения размеров и ресурсов;
- внешние бинарники, packaging, SBOM, подпись и crash-инструменты;
- Janus — только как демонстрационная абстракция, не как боевой atomic-swap
  протокол Monero ↔ Grin.

Угрозы: вредоносные/скомпрометированные нода и relay, сетевой MitM при небезопасной
конфигурации, удалённый flood публичного gateway, сбой процесса/ФС между денежной
операцией и журналом, другой локальный пользователь и подмена артефактов поставки.
Полностью скомпрометированный хост вне возможностей данной архитектуры; один
процесс UI/runtime не является изоляцией (см. TRUST_BOUNDARY).

Не проводились: mainnet-операции, работа с реальными средствами, live/ignored
сценарии с внешней сетью, fuzzing, динамический пентест, power-loss/disk-full,
аудит исходников upstream Grin и криптографическая экспертиза Janus.

## 2. Шкала и сводка

P1 — блокер mainnet/денежного сценария или существенный отказ; P2 — требует
исправления до публичного релиза; P3 — укрепление надёжности/защиты.

| ID | Уровень | Находка | Статус |
|---|---|---|---|
| F-01 | P1 | Embedded публикует непроверенный stored slate из `finalizing` | открыто |
| F-02 | P1 | Неоднозначная сделка допускает повторную финализацию/отмену в RPC | открыто |
| F-03 | P2 | RPC-приём читает `amount` вместо `amt`, получает `0` | подтверждено probe |
| F-04 | P1 | Crash gap между `receive_tx` и durable response journal | открыто |
| F-05 | P2 | `node_api_secret` в settings/backup без owner-only ACL | открыто |
| F-06 | P3 | Repair envelope не использует общий `SeedLock` | открыто |
| F-07 | P2 | Tor-пути без quoting; ошибки чтения bridges подавляются | открыто |
| F-08 | P2 | Неограниченные connection threads в gateway/relay | открыто |
| F-09 | P2 | Owner API secret может передаваться на remote HTTP | конфигурационный риск |
| F-10 | P2 | Неполное закрепление и учёт bundled-бинарников | открыто |
| F-11 | P2 | Crash-script завершает все процессы `grin-wallet` | открыто |
| F-12 | P3 | Лимит HTTP response применяется после неограниченного чтения | открыто |

## 3. Денежные переходы и интеграция Grin

### F-01 — P1: embedded автоматически публикует непроверенное состояние

**Доказательства.** `crates/core/src/service.rs:1477-1483` направляет
`Finalizing` в `retry_post`; `1573-1638` разрешает получить `get_stored_tx`,
вызвать `post_tx` и записать `Posted`; `1960-1975` запускает это при reconcile.
При большинстве ошибок `finalize_tx` запись возвращается в `Locked`
(`1514-1535`), хотя локальная причина ошибки не доказывает отсутствие всех
необратимых побочных эффектов.

В RPC-бэкенде уже зафиксирован контрпример: `rpc_backend.rs:947-953` прямо
указывает, что `init_send_tx` сохраняет partial S1, поэтому успешный
`get_stored_tx` не доказывает подпись. Там автопубликация запрещена.

**Воздействие.** Embedded может попытаться опубликовать partial S1 и после успеха
вызова неверно отметить запись как `Posted`; при неоднозначной ошибке возможен
возврат к повторной финализации. Реальная потеря средств в этом аудите не
воспроизводилась, но сам переход небезопасен и является mainnet-блокером.

**Исправление.** Унифицировать state machine с fail-closed RPC: `Finalizing` →
`FailedRecoverable`, без publish/cancel/re-sign до доказательства состояния.
Публиковать только при durable-доказательстве завершения `finalize_tx`; ошибку
не переводить автоматически в `Locked`. Добавить общие conformance-тесты обоих
бэкендов для crash до/во время/после finalize и post.

### F-02 — P1: неоднозначная RPC-сделка снова допускается к необратимым действиям

**Доказательства.** `rpc_backend.rs:863-919` отклоняет отсутствие записи,
`posted`, `finalized_not_posted` и `finalizing`, но не имеет allowlist для новой
подписи: `failed_recoverable` доходит до `finalize_tx`.
`crates/runtime/src/wallet.rs:29-35,460-496` разрешает cancel для
`failed_recoverable`. Embedded, наоборот, правильно запрещает такую отмену в
`service.rs:1842-1860`.

**Воздействие.** Повторная подпись или отмена при неизвестном состоянии S1/S3 и
блокировок входов может противоречить уже выполненной операции. Это не доказанная
двойная трата, но недопустимая неоднозначность денежного API.

**Исправление.** Для новой финализации разрешить только `Prepared|Locked` (и
явно проверить соответствие response/record). `FailedRecoverable` перевести в
отдельный read-only recovery workflow: запрос tx-log/chain и ручное подтверждение,
без finalize/cancel/post по умолчанию. Синхронизировать UI, runtime и оба backend.

### F-03 — P2: сумма RPC-приёма отображается и журналируется как ноль

**Доказательства.** `rpc_backend.rs:1441-1445` читает строку `amount` с fallback
`0`; `1492-1496` сохраняет её. Отдельный probe сериализовал настоящий
`grin_wallet_libwallet::Slate` и получил:

```text
slate_json={"amt":"123456789","coms":[],"fee":"23000000",...}
amount_json=None
```

Поле комиссии `fee` читается корректно. Значение попадает в UI и RPC receive
record, но не меняет криптографическую сумму внутри Slate.

**Воздействие.** Пользователь видит неверную сумму входящей сделки; нарушаются
журнал и проверка интерфейса. Сам по себе дефект не доказывает приём другой суммы.

**Исправление.** Строго читать `amt` как decimal string, отклонять отсутствующее,
нечисловое и нулевое значение без `unwrap_or(0)`. Тест должен создавать/сериализовывать
настоящий Slate, а не использовать вручную написанный JSON. Лог probe:
`.tmp/audit-2026-10-27-slate-json.log`.

### F-04 — P1: ответ получателя не журналируется до мутации кошелька

**Доказательства.** В embedded `service.rs:1741-1761` сначала выполняется
`foreign.receive_tx`, затем `save_pending`. В RPC `rpc_backend.rs:1475-1504`
сначала `foreign_call("receive_tx")`, затем `save_receive`. Resume работает
только при уже существующей записи (`service.rs:1784-1811`,
`rpc_backend.rs:1455-1473`). Повторный `receive_tx` даёт
`TransactionAlreadyReceived`, а текущий путь не восстанавливает response из
`get_stored_tx`.

**Воздействие.** Crash или ошибка журнала после мутации кошелька, но до durable
record оставляет отправителя без ответа и не даёт штатно завершить конкретную
интерактивную сделку; входы отправителя могут оставаться заблокированными. Это не
утверждение о необратимой потере seed или монет.

**Исправление.** Ввести двухфазный receive journal: intent до `receive_tx`, response
после него; при восстановлении различать «не принят», «принят, response сохранён»,
«принят, response утерян». Проверить возможность штатного восстановления Slate из
upstream tx-log. Добавить failpoint сразу после возврата `receive_tx` и до rename
журнала для обоих бэкендов, включая disk-full/ошибку ACL.

## 4. Локальные секреты и файловые границы

### F-05 — P2: секрет ноды наследует общие права settings

`Settings` содержит `node_api_secret` (`config.rs:22-45`). `save_settings`
(`444-466`) создаёт primary, temp и `.bak` без `win_acl::restrict`; повреждённые
копии также сохраняются для диагностики. В отличие от seed-файлов, owner-only ACL
здесь не является обязательным условием записи.

**Исправление.** Предпочтительно вынести секрет в отдельный файл с fail-closed ACL.
Иначе ограничивать temp до записи, primary, backup и corrupt-копии; проверять ACL
при чтении. Не выводить значение в diagnostics/log. Это секрет ноды, не seed.

### F-06 — P3: repair envelope вне общего seed-lock

`store.rs:707-747` (`quarantine_unreadable_envelope`) проверяет и переименовывает
envelope без `SeedLock::acquire`, тогда как migrate/disable/restore его удерживают.
Вызов находится в `runtime/src/seed.rs:255-264`.

**Воздействие.** Гонка repair с migrate/password/restore может дать устаревшую
проверку или переименование изменившегося файла. Plaintext seed этот код не создаёт;
отдельный дефект «потери исходного ACL legacy seed» при миграционном rollback не
подтвердился и в находки не включён.

**Исправление.** Удерживать тот же lock на проверку → rename, сравнивать identity/
bytes перед переименованием и добавить concurrency/crash тесты.

Дополнительное укрепление: `core/src/onion.rs:38-40` игнорирует ошибку ACL
hidden-service secret; лучше fail-closed либо явная диагностика с блокировкой
запуска. `runtime/src/history.rs:425-436` при неудачном upgrade history key делает
best-effort rollback без гарантии восстановления — требуется crash-тест и явный
путь восстановления.

## 5. Сеть, Tor и доступность

### F-07 — P2: ошибки конфигурации Tor скрываются

`tor_listener.rs:126-130` превращает любую ошибку чтения `bridges.txt`, не только
NotFound, в «мосты не заданы». `263-269` подставляет DataDirectory,
HiddenServiceDir и путь лога в torrc без quoting/escaping.

**Воздействие.** Пути с пробелами могут дать неверную/нерабочую конфигурацию;
при ошибке чтения мостов пользователь не узнает, что запрошенная bridge-политика
не применена. Это не означает автоматический переход messenger на clearnet.

**Исправление.** Корректно экранировать аргументы torrc; NotFound обрабатывать
отдельно, остальные ошибки возвращать; отображать фактический режим и причину
отказа. Добавить тесты Unicode/пробелов/кавычек и permission-denied bridges.

### F-08 — P2: публичный relay не ограничивает число соединений

`gateway.rs:133-147` создаёт `std::thread::spawn` на каждое accepted connection.
Таймаут ставится лишь внутри handler (`185-198`), PoW проверяется позже и от
connection flood не защищает. Standalone `flint-relay` использует тот же Gateway.

**Исправление.** Bounded worker pool/семафор, лимит pending accepts и глобальный
rate limit, ранний размер/время header budget, метрики отказов. Нагрузочный тест
должен доказать ограниченную память/число threads и сохранение обслуживания честных
клиентов.

### F-09 — P2: незащищённый remote HTTP для owner API

`config.rs:91-96` принимает произвольный `node_url`; `node.rs:936-941` отправляет
`Authorization: Basic base64("grin:<secret>")`. Для не-loopback HTTP канал не
защищает node credential и ответы от активного MitM.

**Исправление.** По умолчанию loopback; для remote требовать TLS с проверкой
сертификата либо SSH/VPN-туннель. Небезопасный remote HTTP запретить или допускать
только после отдельного явного предупреждения, без сохранения секрета до подтверждения.

### F-12 — P3: response limit не ограничивает чтение

`messenger/src/transport.rs:128-134` вызывает `read_to_end`, а `MAX_RESPONSE`
проверяет после получения всего буфера. Вредоносный relay может исчерпать память
до проверки.

**Исправление.** `take(MAX_RESPONSE + 1)` и отдельный строгий header limit; отказ
до выделения большого буфера, тест с медленным/бесконечным ответом.

## 6. Поставка и эксплуатационные инструменты

### F-10 — P2: supply chain внешних бинарников закреплён не полностью

- `tools/fetch-wallet.ps1:16-35` скачивает архив и checksum из одного release;
  ожидаемый wallet hash не закреплён в репозитории. Нода имеет более сильный pin.
- `package-slint.ps1:20-57` проверяет наличие wallet/Tor, но не версии и хэши wallet,
  Tor/lyrebird при packaging.
- `sbom.ps1` включает Cargo components, но не bundled grin/grin-wallet/Tor/lyrebird/
  vcruntime и их происхождение/хэши.
- `release.ps1` допускает неподписанный результат без `-RequireSigned`.

**Исправление.** Pinned SHA-256 и версии всех native assets, verify на fetch и
package, полный native+Cargo SBOM, подпись и timestamp обязательны для production.
Checksum из того же канала полезен против повреждения, но не является независимой
доверенной аттестацией подменённого release.

### F-11 — P2: crash-script может завершить чужой кошелёк

`tools/crash-test-rpc.ps1:33-36` находит все процессы `grin-wallet` по имени и
выполняет `taskkill /T /F`. Параметры принимают произвольные data dirs; preflight
testnet/fresh dirs отсутствует. Скрипт в аудите не запускался.

**Исправление.** Завершать только PID собственных child processes, запретить
непустые/непомеченные каталоги и mainnet, требовать отдельный testnet marker.
Никогда не использовать process-name kill в деструктивных тестах.

## 7. Что подтверждено положительно

- Argon2id v2 envelope, read-back verification, fail-closed миграция, opt-in
  удаление слабых backup и mainnet gate на legacy.
- UUID/path-проверки и ограничение pending paths; seed не выводится в логи.
- RPC owner API: loopback, bearer secret, ожидание читаемого secret; onion не
  exposes owner API напрямую.
- Managed node: loopback/CORS-stripping proxy, проверки chain/user-agent/readiness.
- Messenger: подписанные и зашифрованные envelopes, PoW, signed ACK,
  history-before-ack и отдельные ключи истории.
- RPC `reconcile_finalizing` уже содержит правильную защиту от partial S1;
  проблема — несогласованность embedded и последующих действий RPC.
- Janus честно обозначен как симуляция/демо-борд; `selftest` не является
  доказательством реального атомарного свопа.

Эти механизмы снижают риск, но не отменяют открытые находки и отсутствие независимого
криптографического/пентест-ревью.

## 8. Выполненные проверки

Окружение: Windows/MSVC, Rust/Cargo `1.99.1`, offline locked workspace.

```text
cargo fmt --all -- --check                                      PASS
cargo check --workspace --all-targets --locked --offline       PASS
cargo test --workspace --locked --offline                      245 passed, 0 failed, 19 ignored
cargo test -p flint-core --features crash-injection \
  --test migration_crash --locked --offline                     2 passed, 0 failed
cargo run -p flint-runtime --bin flint-selftest \
  --locked --offline                                            ok: true
cargo tree --workspace --locked --offline --duplicates          выполнен
```

Selftest запустил managed node и открыл порт, но в момент результата
`connected:false`, `height:0`; Janus вернул симулированный `Completed`. Поэтому
selftest подтверждает связку, а не синхронизацию, реальный денежный lifecycle или
реальный своп. Live/ignored тесты в этой сессии не запускались.

`cargo audit` не установлен (`cargo: no such command: audit`). Сканирование известных
уязвимостей RustSec **не выполнено**; нельзя заключать, что уязвимых зависимостей нет.
Наличие duplicate versions в `cargo tree` само по себе не является уязвимостью.

Логи: `.tmp/audit-2026-10-27-{fmt,check,test,crash,selftest,duplicates,slate-json}.log`.

## 9. Решение и порядок закрытия

1. **До любого mainnet:** исправить F-01/F-02/F-04; общими тестами доказать
   невозможность повторной подписи, ложного `Posted`, запрещённой отмены и потери
   recoverable response на всех crash boundaries. Исправить F-03 до приёмки UI/API.
2. **До публичного релиза:** F-05–F-12, RustSec/dependency review, native SBOM,
   pinned binaries, обязательная подпись, нагрузочные и filesystem fault-тесты.
3. **Отдельно сохранить гейты MAINNET_READINESS:** внешнее человеческое ревью,
   mainnet dry-run малой суммой под наблюдением, restore на чистой машине,
   power-loss/disk-full/chain/network стенд.
4. Не представлять Janus как поддержку реальных атомарных свопов до отдельной
   спецификации протокола, имплементации криптографических доказательств/таймлоков,
   интеграции с Monero/Grin и самостоятельного аудита.

**Вердикт:** для testnet-разработки код собирается и основной набор тестов проходит;
для реальных средств и публичного production-релиза текущая версия не готова.
