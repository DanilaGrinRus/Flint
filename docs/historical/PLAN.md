# Рабочий план стабилизации Flint

> **HISTORICAL:** редакция 2026-10-27…2026-10-28. Актуальный план — [../PLAN.md](../PLAN.md).

Дата согласования: 2026-10-27. Основание: актуальные проектные документы и
`docs/security/AUDIT_RECHECK_2026-10-27.md`.

Это план разработки, а не разрешение на реальные средства. Официальный mainnet-гейт
остаётся в `docs/status/MAINNET_READINESS.md`. Закрытие 13 оставшихся локально
выполнимых пунктов отслеживается в `docs/KRead.md`; 17 внешних приёмок и человеческих
гейтов — в `docs/PSWork.md`. Три отложенных трека не входят в активный счёт. Перенос
внешних задач не закрывает официальные гейты.

## Общие правила

- Сначала корректность и восстановимость кошелька, затем новые возможности.
- Один контрольный результат относится только к одному неизменному снимку.
- Денежные переходы проверяются для embedded и RPC; UI-гейт не заменяет backend-гейт.
- Неоднозначное состояние не превращается в доказательство подписи, публикации или
  потери ответа.
- Unit-тест, живой testnet, destructible-стенд и внешний аудит — разные доказательства.
- Предупреждение `DEV BUILD` сохраняется до выполнения официального P0.

## Этап 0 — проверяемая база

### Работы

1. Проверить актуальность зафиксированной аудитом синтаксической ошибки в
   `crates/core/src/service.rs`; при наличии исправить минимально.
2. Зафиксировать снимок исходников копией и манифестом хешей.
3. Выполнить:
   - `cargo fmt --all -- --check`;
   - `cargo check --workspace --all-targets --locked --offline`;
   - `cargo test --workspace --locked --offline`;
   - `cargo run -p flint-runtime --bin flint-selftest`.
4. Повторить и классифицировать незавершённый `node_manager`-сценарий. Не скрывать
   зависание увеличением timeout без объяснения.
5. Синхронизировать STATUS и рабочий трекер только с фактически полученными данными.

### Гейт

Все команды завершены на одном снимке. Есть точные итоги passed/failed/ignored;
каждый timeout классифицирован. Selftest не затрагивает пользовательский
`FLINT_DATA_DIR` и убирает собственные временные файлы.

## Этап 0.5 — реальная защита локальных секретов (SENT-02/SENT-01)

Добавлен после независимой проверки `SENTINEL_AUDIT_2026-10-27.md`; основание и
доказательства — `KimiDocs/SENTINEL_REVIEW_2026-10-27.md`.

### Работы

1. Сначала исправить `win_acl::restrict`: успешный exit code не должен считаться
   доказательством owner-only. Явный ACE постороннего principal должен удаляться,
   итоговый DACL — проверяться по утверждённому allowlist.
2. Устранить write-before-ACL и подавление ошибок для node/RPC/onion секретов.
   Upstream-created `.owner_api_secret` требует отдельного безопасного сценария.
3. Охватить также `.node_api_secret`, archive pending, history key, blocked list и
   corrupt settings. Политика fail-closed/warning выбирается по чувствительности.
4. Добавить fault-injection и regression с заранее существующим чужим explicit ACE.
5. Выполнить внешние приёмки: второй Windows-аккаунт и ФС без ACL.

### Гейт

Ни успешный запуск backend, ни успешная запись секрета не считаются безопасными при
неподтверждённом ACL. Одноразовый SDDL-probe с Everyone проходит, второй аккаунт не
читает секреты, FAT/exFAT даёт понятный отказ. До исправления SENT-02 обработка
ошибок `restrict()` сама по себе гейт не закрывает.

## Этап 1 — денежные state machines

Порядок: F-01, затем R-01, затем F-04.

### 1.1 F-01 — finalize/retry/reconcile

- Убрать вывод `FinalizedNotPosted` из текста `DB Not Found`/`Slate id`.
- Любой неподтверждённый результат финализации переводить в `FailedRecoverable`.
- Создавать `FinalizedNotPosted` только после успешной подписи или другого
  проверяемого доказательства из wallet/transaction log.
- Не публиковать и не отменять неоднозначную запись автоматически.
- Добавить общую матрицу допустимых переходов для embedded/RPC.

Тесты: ошибки до подписи, после подписи, crash до/после записи журнала, restart,
ручной retry и auto-reconcile. Отдельно проверить отсутствие повторной подписи.

### 1.2 R-01 — классификация receive

- Ошибку без доказанной мутации оставлять в `ReceiveIntent` и разрешать повтор.
- `ResponseLost` назначать только при доказанной мутации/неоднозначности после неё.
- Не переводить любые ошибки разбора, сети и RPC в `lost`.
- Сохранить запрет повторного receive там, где он действительно небезопасен.

Тесты: ошибка до мутации, ошибка после мутации, повтор с сохранённым response,
crash между возвратом response и journal rename.

### 1.3 F-04 — атомарное восстановление response

- [ADR F-04](../security/ADR_F04_RECEIVE_RECOVERY.md) принят: fork
  `grin-wallet` 5.5.1, идемпотентный receive и один LMDB-commit для output,
  tx-log, исходного S1 и настоящего S2. `get_stored_tx` не используется.
- Этап A завершён в commit `11046cd197f8296e6cd9313502a9f2cbce6e262a`: recovery
  record, typed errors, capability, единый LMDB-commit и upstream fault-тесты.
  Снимок `20261027-f04-upstream-recovery` сверен по SHA-256.
- Этап B завершён на `20261027-f04-integration-candidate-v3`: единый fork для embedded/
  RPC, fail-closed capability, typed mapping и legacy migration semantics; общий
  offline lock/vendor, проверенный RPC binary, source package и SBOM. Контрольный
  набор: 267 passed, 0 failed, 19 ignored; live-сценарии не запускались.
- Этап C на funded testnet: embedded/RPC × `upstream`/`journal` прошли process exit 9,
  restart/retry и возврат того же S2 без дублей; sender finalize/post выполнен,
  RPC-транзакции подтверждены в цепи. Снимок
  `20261027-f04-live-multiwallet-candidate`, 268 passed, 0 failed, 20 ignored.
- F-01 RPC crash/recovery/confirmation завершён на `20261027-f01-raw-stored-recovery`:
  выявлена потеря подписи при round-trip компактного `get_stored_tx`; recovery теперь
  использует исходную `.grintx`. Три сделки подтверждены в цепи без повторной подписи,
  crash-script содержит обязательный on-chain kernel gate; 269 passed, 0 failed.
- Для остатка этапа C подготовлены [destructible runbook](../operations/DESTRUCTIBLE_F04.md),
  фазовый harness и pause-hook. По решению владельца вместо VM выполнена принятая
  симуляция DF-J: ошибка 112 после upstream commit и до response journal. Embedded/RPC
  восстановили тот же S2 без дублей и подтвердили kernels в цепи. Power-loss представлен
  прежними exit 9/restart. Снимок `20261027-f04-simulated-disk-full`: 269 passed,
  0 failed, 20 ignored, `match:true`.
- Формальные 8 физических disk-full/power-loss прогонов не выполнены, общий F-04 не
  отмечается полностью закрытым. VM-испытания по решению владельца прекращены; в этой
  итерации принятая симуляция достаточна, но не подменяет внешнее доказательство.

### 1.4 Мультикошельки

Реализованы независимые seed-профили в одном процессе и окне. Legacy/default-путь
сохранён; именованные профили изолируют wallet/state/pending/RPC/messenger/history.
Runtime пересоздаёт активный контекст, UI очищает секреты и transient state, generation
отбрасывает поздние async-результаты, а busy-barrier не даёт сменить профиль во время
операции. Существующие CLI-сигнатуры не изменены.

Проверено embedded regression и живым RPC smoke create/switch/reopen двух профилей.
Живые embedded/RPC messenger/history/restart/errors через реальный `flint-relay` пройдены;
доказательства и снимок — запись 17 в WORK_LOG. Ручной UI-сценарий остаётся внешним
PS-04 в `docs/PSWork.md`. Busy-barrier UI не считать доказательством сериализации
произвольных внешних вызовов runtime.

### Гейт этапа 1

Crash в каждой контрольной точке не вызывает повторную подпись, необоснованный
publish, ложный Posted или невосстановимую потерю response. Embedded и RPC дают
эквивалентный безопасный результат; живой RPC receive повторён.

## Этап 2 — повторяемый testnet-стенд

### Покрытие

- create/open/lock/restore/delete;
- seed create/migrate/disable/cleanup, включая конкурентные операции;
- send/receive/finalize/post/confirm/cancel на обоих бэкендах;
- kill/restart на границах журнала;
- restore из фразы в чистый каталог, полный scan и независимая сверка адреса/баланса;
- offline, флаппинг сети, битая цепь;
- disk-full и power-loss на отдельном destructible-стенде.

Повторяемый `create → lock → open → restart → open → restore → delete` подтверждён
двумя независимыми циклами на чистых каталогах для embedded и реального RPC
`grin-wallet`. Адрес сохраняется после restart/restore, delete перемещает кошелёк в
`trash`. Контрольный снимок `20261027-wallet-lifecycle-live`: 270 passed, 0 failed,
23 ignored. Этот адресный restore не закрывает отдельный полный chain-scan/баланс
lifecycle. Доказательства — запись 18 в WORK_LOG.

Seed create/migrate/disable/cleanup проверен при конкуренции и restart: независимые
сессии embedded/RPC имеют ровно одного победителя create, общий `SeedStore` сериализует
migrate/disable, cleanup не удаляет последний recoverable seed. Все шесть process-abort
точек M0–M5 восстановимы, создание после потери upstream `db/` открывает прежний адрес.
Снимок `20261027-seed-concurrency-live`: 274 passed, 0 failed, 24 ignored. Это не
физический power-loss. Доказательства — запись 19 в WORK_LOG.

Полный transaction lifecycle прошёл на embedded и реальном RPC: full scan, отправка,
receive/finalize/post, on-chain confirmation, вторая отправка и cancel. После отмены
`spendable` и `locked` точно восстановлены и проверены повторно после restart обоих
кошельков. Контрольный снимок `20261027-transaction-lifecycle-live-v2`: 668 файлов,
62 178 912 байтов, 274 passed, 0 failed, 26 ignored, selftest `ok:true`, `match:true`.
Доказательства — запись 20 в WORK_LOG.

Полный restore lifecycle прошёл на embedded и реальном RPC: восстановление из фразы в
чистый каталог, запрет partial scan, полный chain-scan и reopen после restart. Отдельный
оракул на противоположном backend подтвердил адрес и все поля баланса; сумма —
100 000 000 nano = 0,1 GRIN. Оракул независим по backend и scan, но использует ту же
testnet-ноду. Контрольный снимок `20261028-restore-lifecycle-live`: 669 файлов,
62 199 866 байтов, 274 passed, 0 failed, 28 ignored, selftest `ok:true`, `match:true`.
В этапе 2 остаются offline/flapping/битая цепь. Доказательства — запись 21 в WORK_LOG.

### Гейт

Сценарии повторяемы на чистых testnet-каталогах и привязаны к версии снимка и
бинарей. Нет небезопасных автоматических переходов и потери последнего seed.
Результаты destructible-стенда не подменяются обычными kill/restart.

## Этап 3 — эксплуатационная надёжность

Допускается параллельно после F-01/R-01 или во время внешнего ожидания по F-04.

- F-05: ошибка ACL corrupt-копии видна; доступ второго Windows-пользователя проверен.
- F-07: реальный Tor, пробелы/Unicode в путях, obfs4/snowflake/webtunnel.
- F-08/relay: flood, честные клиенты, fairness, resource limits, degraded storage.
- PT-04: диагностика и восстановление stale ContactBook lock.
- PT-05: живой RPC scan, готовность ноды, запрет partial restore scan.
- Node/RPC lifecycle: restart, hang, scan cancel, graceful shutdown.

### Гейт

Каждая проверка имеет живой результат либо явно остаётся внешним ограничением.
Ошибки инфраструктуры не скрываются и не дают ложного успешного состояния.

## Этап 4 — релизная инженерия и внешний аудит

1. Закрепить хеши всех native assets, включая `vcruntime140.dll` и `pt_config.json`.
2. Объединить Cargo и native SBOM; выполнить RustSec.
3. Получить успешный чистый GitHub-hosted CI.
4. Настроить Authenticode, timestamp и независимую сверку сборки.
5. Провести clean Windows package/restore drill.
6. Утвердить security-контакт, PGP, ответственных и провести учебный отзыв сборки.
7. После закрытия денежных P1 передать стабильный снимок независимому аудитору:
   код, `grin-wallet/grin` integration, threat model, host и relay.
8. Исправить замечания и получить повторное ревью.

### Гейт

Есть подписанный проверенный артефакт и письменный внешний результат. AI-review,
локальные тесты и SBOM сами по себе этот гейт не закрывают.

## Этап 5 — UX, документация и эксплуатация

- NVDA/Narrator, реальный IME, полный Tab-аудит, 200% DPI.
- Живой clipboard substitution drill.
- Большие истории, длительные сканы, уведомления о незавершённых/истекающих slate.
- Локализация и справка по восстановлению.
- Обновление STATUS/ROADMAP/AUDIT_RESPONSE и индекса документов; исторические
  записи отделяются от текущего состояния.

Фоновая нода проектируется только после предусмотренных ROADMAP фаз. Для неё нужны
отдельные гейты службы, IPC, heartbeat, ACL, upgrade, ресурсов и power-loss.

## Этап 6 — mainnet dry-run

Только после закрытия официального P0 и внешнего аудита:

- подписанная сборка;
- малая сумма под наблюдением;
- send → receive → finalize → confirmation;
- restore из фразы с независимой сверкой адреса и баланса;
- письменный результат без раскрытия seed/паролей.

До этого любые реальные средства запрещены действующей политикой проекта.

## Отдельные отложенные треки

### Janus

Остаётся демонстрационным и замороженным. Будущая работа начинается со спецификации,
модели угроз, криптографического обоснования, тест-векторов и реальной интеграции
Monero/Grin. Симуляция не является основанием заявлять поддержку свопов.

### Messenger и публичный relay

Не расширять до стабилизации кошелька. При возврате к треку отдельно рассматривать
метаданные v1, отсутствие forward secrecy, replay после restart, distributed abuse,
push и эксплуатацию публичного инстанса.

## Ближайшие три итерации

1. Offline/flapping/битая цепь на обоих backend. Restore lifecycle завершён;
   доказательства — запись 21 в WORK_LOG.
2. Эксплуатационные сценарии без VM. Физический destructible-гейт PS-03 и ACL-приёмки
   PS-01/PS-02 остаются невыполненными внешними задачами.
3. Native hashes, Cargo + native SBOM и RustSec; затем локальные UX/документация.
   Независимый аудит и остальные внешние гейты ведутся отдельно в `docs/PSWork.md`.
