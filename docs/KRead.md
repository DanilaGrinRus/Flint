# KRead — закрытие рабочего плана Kimi

Начат: 2026-10-27. Статус: **все 12 локальных пунктов закрыты локально с проверяемыми
доказательствами: этапы 2–5 — offline/flapping/битая цепь, Tor/relay/PT-04/node-RPC,
native hashes/SBOM/RustSec, большие истории, slate-уведомления, дизайн фоновой ноды,
локализация статического UI ru/en и справка восстановления. Физические
disk-full/power-loss, внешние ACL/PS-приёмки и ручной UI остаются внешними; mainnet
запрещён.**

Это трекер выполнения плана, а не замена `status/STATUS.md` или
`status/MAINNET_READINESS.md`. Подробный план — [PLAN.md](PLAN.md),
хронология и результаты команд — [WORK_LOG.md](WORK_LOG.md),
правила ведения — [README.md](README.md). Внешние приёмки и
человеческие гейты вынесены в [PSWork.md](PSWork.md): **17 уникальных пунктов**, все
остаются открытыми. В этом файле активных локально выполнимых пунктов не осталось;
три отложенных трека считаются отдельно. Перенос внешнего пункта не закрывает
официальный гейт, mainnet по-прежнему запрещён.

## Как читать отметки

- `[x]` — есть проверяемое доказательство, указанное рядом.
- `[~]` — часть сделана либо код есть, но обязательная проверка не завершена.
- `[ ]` — не выполнено в рамках локального активного плана.
- **Внешний гейт** — требуется другой человек, машина, сертификат, стенд или upstream;
  такие пункты зарегистрированы в `PSWork.md`, а не в активном счёте этого файла.
- Timeout без итога — незавершённая проверка, а не passed/failed.

## Текущие блокеры

SENT-01/SENT-02 и кодовая часть F-05 исправлены, но второй аккаунт и FAT/exFAT ещё
не проверены (PS-01/PS-02), поэтому общий этап 0.5 не закрыт. F-04/C OS crash/testnet,
F-01 RPC crash/recovery/confirmation и принятая симуляция disk-full прошли на funded
disposable кошельках. Формальный F-04 остаётся открытым без физических аварийных
evidence (PS-03), но дальнейшие VM-испытания в этой итерации отменены владельцем.
Мультикошельки прошли живые messenger/history/restart/errors на embedded/RPC через
реальный loopback `flint-relay`; ручной UI остаётся внешним PS-04. Повторяемый lifecycle,
seed-операции при конкуренции/restart, полный transaction и restore lifecycle пройдены
на обоих backend. Полный набор Flint на `20261028-stage34-local`:
**287 passed, 0 failed, 29 ignored**; ignored этим не закрываются. Текущий
контрольный прогон (2026-10-28): **314 passed, 0 failed, 29 ignored**, UI — 15
passed, снимок `20261029-plan12`; 0 локальных активных пунктов.
Реальный encrypted slatepack через messenger и смену профилей без перезапуска
прогнан (default → alice, 1 GRIN); входящий response от контакта
автофинализирован на default, первый входящий оригинал принят вручную.

## Этап 0 — проверяемая база

- [x] `service.rs` парсится: строка 718 возвращена в комментарий, логика не менялась.
- [x] Снимки `20261027-024832-prefix` и `20261027-024832-candidate` сохранены в
      `snapshots/`; у candidate 103 файла и SHA-256 manifest.
- [x] `cargo fmt --all -- --check` — успешно (`.tmp/stage0-fmt.log`).
- [x] `cargo check --workspace --all-targets --locked --offline` — успешно
      (`.tmp/stage0-check.log`).
- [x] `cargo test --workspace --locked --offline` — **256 passed, 0 failed,
      19 ignored** (`.tmp/stage0-test-workspace.log`).
- [x] `restarts_cleanly_after_a_normal_stop` — passed; node_manager suite:
      **3 passed**, прежний timeout не воспроизвёлся.
- [x] `flint-selftest` — `ok:true`; sentinel в заданном `FLINT_DATA_DIR` не изменён,
      собственный временный каталог selftest удалён (`.tmp/stage0-selftest.log`).
- [x] Debug `cargo build -p flint-slint --locked --offline` — успешно.
- [x] После fmt/check/test/build/selftest дерево совпало с candidate: changed/missing/
      added пусты (`.tmp/stage0-snapshot-verify-final.log`).
- [x] STATUS и этот трекер обновлены по результатам одного снимка.

**Гейт:** закрыт для `20261027-024832-candidate`. Подробности — запись 02 в
`WORK_LOG.md`. Live/ignored и денежные P1 этим гейтом не закрываются.

## Этап 0.5 — защита локальных секретов (SENT-02/SENT-01)

Основание: [historical/SENTINEL_REVIEW_2026-10-27.md](historical/SENTINEL_REVIEW_2026-10-27.md).

- [x] SENT-02: `restrict()` заменяет DACL через `File.SetAccessControl`, удаляет все
      прежние ACE, добавляет SID текущего пользователя и перечитывает результат для
      проверки allowlist. Успешный exit code без этой проверки не принимается.
- [x] Regression начинается с `Everyone:F`; после restrict остаётся один ACE,
      Everyone отсутствует, повтор идемпотентен, путь содержит пробелы и Unicode.
      Проверка: `.tmp/sent02-test-acl-unicode.log`, 1 passed.
- [x] Node/RPC/onion секреты: ошибка ACL fail-closed, запись до ACL устранена.
- [x] Для upstream-created `.owner_api_secret` ACL проверяется до чтения; при сбое
      или timeout дочерний `grin-wallet` завершается через kill/wait.
- [x] `.node_api_secret`, pending/archive, history key, blocked list, settings/backup,
      corrupt settings и RPC send/receive journals включены в явную политику ACL;
      молчаливого fallback нет.
- [x] Fault-injection покрывает ошибку ACL до записи, после rename и при открытии.
      Полный набор на `20261027-sent01-candidate-v2`: **259 passed, 0 failed,
      19 ignored**; fmt/check/build/selftest и сверка SHA-256 успешны.
Локально выполнимых оставшихся пунктов: **0**. Внешние приёмки не выполнены и
вынесены в [PSWork.md](PSWork.md): PS-01 — второй Windows-аккаунт, PS-02 — FAT/exFAT.

**Гейт:** не закрыт внешними приёмками PS-01/PS-02. SENT-01/SENT-02 и кодовая часть
F-05 исправлены и проверены на `20261027-sent01-candidate-v2`; перенос внешних задач
не меняет этот статус. Подробности — запись 05 в `WORK_LOG.md`.

## Этап 1 — денежные state machines

### F-01 — finalize/retry/reconcile

- [x] Текст ошибки больше не создаёт `FinalizedNotPosted`.
- [x] Неподтверждённая финализация переходит только в `FailedRecoverable`.
- [x] Publish/retry/auto-reconcile не работают без доказанной подписи.
- [x] Общая regression-матрица embedded/RPC проходит: 269 passed, 0 failed.
- [x] Живой RPC crash-прогон: `locked → finalized_not_posted → posted`, stored S3
      опубликован новым процессом без повторной подписи, повторный finalize идемпотентен.
- [x] Найденная потеря подписи в компактном `get_stored_tx` устранена: recovery/retry/
      republish используют исходную `.grintx`, отклоняют нулевой kernel и публикуют
      полный `Transaction` напрямую. Crash-script требует on-chain kernel для PASS.
- [x] `f8af12ab-…` подтверждён в блоке 3310555, `dba5c703-…` — в 3310568,
      `dbfe2160-…` — в 3310581; обе стороны видят `confirmed`, change spendable.
      Повторная подпись и создание замены не выполнялись. Подробности — запись 12.

### R-01 — классификация receive

- [x] Обычная ошибка сохраняет `ReceiveIntent` и допускает повтор.
- [x] `ResponseLost` назначается при `TransactionAlreadyReceived` и typed
      `ReceiveRecoveryUnavailable`/`ReceiveRecoveryIntegrity`.
- [x] Regression embedded/RPC и ошибок журнала проходит.
- [x] Crash после wallet mutation и восстановление response проверены живым прогоном
      во всех четырёх сценариях F-04/C.

### F-04 — восстановление response

- [x] [ADR](security/ADR_F04_RECEIVE_RECOVERY.md) выбран: fork `grin-wallet` 5.5.1,
      идемпотентный receive и один LMDB-commit для output, tx-log, S1 и S2.
- [x] Этап A: commit `11046cd197f8296e6cd9313502a9f2cbce6e262a`; atomic LMDB-commit,
      idempotent S1→S2, typed errors, capability и crash/fault-тесты реализованы.
      Libwallet 38, API 94, transaction 5 и invoice 1 проверок passed; workspace check
      успешен. Снимок `20261027-f04-upstream-recovery`, `match:true`.
- [x] Этап B: embedded и RPC используют один fork; RPC проверяет capability до нового
      receive; старые `received`/`lost` сохраняют прежнюю семантику. RPC binary собран
      из fork, SHA-256 обновлён; source-архив и SBOM содержат идентичность источника.
      Полный набор: **267 passed, 0 failed, 19 ignored**; fmt/check/build/selftest,
      native hashes и snapshot verify успешны. Снимок
      `20261027-f04-integration-candidate-v3`, `match:true`.
- [x] Этап C, OS crash/testnet: embedded/RPC × `upstream`/`journal` прошли exit 9,
      restart/retry, тот же S2 без дублей и sender finalize/post. RPC-транзакции
      подтверждены в цепи, change spendable. Evidence и логи перечислены в записи 11.
- [x] [Destructible runbook](operations/DESTRUCTIBLE_F04.md), фазовый harness и
      feature-gated pause-hook подготовлены. Восемь fail-closed guards и два pause unit
      прошли; workspace — 269 passed, 0 failed, 20 ignored. Снимок
      `20261027-f04-destructible-harness-v2`, `match:true`. Это не аварийные evidence.
- [x] По решению владельца выполнена принятая симуляция DF-J без VM: точечный
      `ERROR_DISK_FULL=112` после upstream commit и до response journal. Embedded/RPC
      восстановили тот же S2 с одним `TxReceived`, finalize/post выполнен; kernels
      подтверждены в блоках 3310716 и 3310738. Power-loss представлен прежними exit 9/
      restart. Evidence и оговорка о timeout embedded — запись 14.

Формальный внешний гейт PS-03 в [PSWork.md](PSWork.md) не выполнен: 8 физических
disk-full/power-loss прогонов отсутствуют. По решению владельца VM-испытания прекращены
и в этой итерации не требуются; симуляция не приравнивается к внешнему доказательству.

**Гейт:** OS crash/restart, живой receive/finalize/post/confirm и принятая симуляция
завершены. Общий F-04 не объявляется формально закрытым: физические аварийные evidence
отсутствуют, но дальнейшая работа продолжается без VM по решению владельца. Подробности —
записи 06–14 в `WORK_LOG.md`.

### Мультикошельки

Локально выполнимых оставшихся пунктов в этапе 1: **0**.

- [x] Legacy/default-путь сохранён; именованные профили изолируют wallet/state/pending/
      RPC/messenger/history. CLI-сигнатуры не изменены.
- [x] Runtime переключает контекст в одном процессе; UI очищает секреты/transient state,
      generation отсекает поздние async-результаты, busy-barrier защищает операции.
- [x] Embedded create/switch/reopen и изоляция seed/address/scan проверены regression;
      реальный RPC smoke двух независимых профилей — 1 passed.
- [x] Живые messenger/history двух профилей, restart и ошибки переключения проверены на
      embedded/RPC через реальный `flint-relay`: обмен в обе стороны, изоляция истории,
      повторный inbox без дублей, неверный профиль/пароль. Оба сценария — 1 passed.
      Снимок `20261027-multiwallet-messenger-live`, полный набор 269 passed, 0 failed,
      22 ignored; доказательства — запись 17.

Ручной UI вынесен в PS-04 [PSWork.md](PSWork.md) и остаётся открытым.

**Граница:** это переключение активного профиля в одном окне, не одновременное
исполнение нескольких кошельков. UI busy-barrier не доказывает сериализацию
произвольных внешних вызовов runtime.

## Этап 2 — testnet-приёмка

Локально выполнимых оставшихся пунктов: **0**. Физические disk-full/power-loss —
внешний PS-03 в [PSWork.md](PSWork.md), он не выполнен.

- [x] Повторяемый `create → lock → open → restart → open → restore → delete` подтверждён
      двумя независимыми циклами на чистых каталогах embedded/RPC. Адрес совпадает после
      restart/restore, delete перемещает кошелёк в `trash`. Снимок
      `20261027-wallet-lifecycle-live`, доказательства — запись 18.
- [x] Seed create/migrate/disable/cleanup проверен при конкуренции и restart: один
      победитель create на embedded/RPC и migrate/disable на общем `SeedStore`; cleanup
      не удаляет последний seed. Все M0–M5 process-abort точки восстановлены. Снимок
      `20261027-seed-concurrency-live`, доказательства — запись 19.
- [x] Полный send/receive/finalize/post/confirm/cancel прошёл на embedded/RPC. После
      cancel `spendable` и `locked` точно восстановлены и проверены повторно после
      restart обоих кошельков. Снимок `20261027-transaction-lifecycle-live-v2`,
      доказательства — запись 20.
- [x] Restore из фразы в чистый каталог, полный scan и независимая сверка адреса/баланса
      прошли на embedded/RPC с reopen после restart. Оракул на противоположном backend
      подтвердил адрес и все поля баланса; используется общая testnet-нода. Снимок
      `20261028-restore-lifecycle-live`, доказательства — запись 21.
- [x] Offline/флаппинг сети/битая цепь проверены на уровне node API: недоступная нода не
      считается synced (прежний тест), закрытие слушателя после успешного ответа снимает
      synced (flapping), чужой `chain` и ответ `Err` от `get_tip` не дают Ready; отправка и
      скан уже блокируются без ready-ноды (прежние gate-тесты). Физическая порча
      `chain_data` и реальный обрыв сети не воспроизводились — это остаётся живой
      проверке/PS-03.

**Гейт:** не закрыт. Живой testnet не считается mainnet dry-run.

## Этап 3 — эксплуатационная надёжность

Локально выполнимых оставшихся пунктов: **0**. Приёмка вторым Windows-аккаунтом —
внешний PS-01 в [PSWork.md](PSWork.md), она не выполнена.

- [x] F-05 (код): ошибка ACL corrupt-копии включена в пользовательское warning.
- [x] F-07: Tor проверен с пробелами/Unicode и поддерживаемыми мостами. Найдено, что
      Windows-сборка Tor создаёт DataDirectory через ANSI-API и падает на Unicode-пути;
      для Tor теперь используется 8.3-короткий ASCII-путь, лог пишется без кавычек.
      Unit-тесты: кавычки/Unicode/пробелы, obfs4/snowflake/webtunnel, отсутствие
      lyrebird и нечитаемый bridges.txt. Живой `tor_listener_starts_from_a_unicode_data_dir`
      — 1 passed, 24,90 с; живой `tor_publishes_the_address_derived_onion` — 1 passed.
      Подключение через реальный мост не проверялось: нужен достижимый мост.
- [x] F-08/relay: flood, честные клиенты, fairness и degraded storage проверены.
      Новые тесты: 65-е соединение получает 503; переполнение одного получателя не
      расходует квоту другого; сбой storage даёт 500 и не оставляет сообщение в памяти;
      квоты и rate-limit проверены прежними тестами. Публичный relay и distributed
      abuse остаются отложенным треком.
- [x] PT-04: stale ContactBook lock диагностируется и восстанавливается: блокировка
      старше 30 с удаляется с диагностическим сообщением, свежая не трогается, старый владелец не
      удаляет чужую заменённую блокировку. Тесты `stale_lock_is_recovered`,
      `fresh_lock_is_not_treated_as_stale`, `old_holder_does_not_delete_a_replaced_lock`.
- [x] PT-05: живой RPC restore/full-scan и запрет partial restore scan проверены на
      `20261028-restore-lifecycle-live`; 1 passed, chain-validated summary, доказательства —
      запись 22.
- [x] Node/RPC restart, hang, scan cancel и graceful shutdown проверены: node
      start/stop/restart/hard-kill и `Shutdown complete` — прежние тесты; новый
      `hanging_node_status_is_bounded_by_the_timeout`; RPC-ребёнок перезапускается и
      открывает тот же адрес; живой embedded scan cancel — 1 passed, 97,98 с.

**Гейт:** не закрыт.

## Этап 4 — релиз и независимая проверка

Локально выполнимых оставшихся пунктов: **0**. Внешние PS-05–PS-10 перечислены в
[PSWork.md](PSWork.md) и не выполнены: GitHub-hosted CI, подпись и независимая сверка,
clean Windows drill, организационные меры, человеческий аудит и внешний recheck.

- [x] Все native assets, включая `vcruntime140.dll` и `pt_config.json`, закреплены хешами.
      `resources/runtime/vcruntime140.dll` — закреплённая копия вместо System32;
      `resources/native-assets.sha256` содержит 6 записей; `tools/verify-native-assets.ps1`
      проходит; `package-slint.ps1` берёт runtime из `resources/runtime`.
- [x] Cargo + native SBOM сформирован и проверен: `tools/sbom.ps1` пишет 1141 cargo и
      6 native компонентов с SHA-256; `tools/verify-sbom.ps1` сверяет каждый пакет
      `cargo metadata`, каждый native asset и identity F-04. Оба запуска успешны.
- [x] RustSec выполнен, найденное разобрано: `cargo-audit 0.22.2`, advisory-db
      `99670cfc…`; **2** уязвимости (crypto-bigint, rsa) остаются
      транзитивными; curve25519-dalek, yaml-rust и zip закрыты обновлениями.
      17 предупреждений разобраны.
      Отчёт — [security/RUSTSEC_2026-10-28.md](security/RUSTSEC_2026-10-28.md),
      воспроизводимая проверка — `tools/rustsec.ps1`.

**Гейт:** не закрыт. AI-review не заменяет человеческий аудит или pentest. Исправления
будущих находок можно внести локально, но PS-10 требует внешнего подтверждения.

## Этап 5 — UX и документация

Локально выполнимых оставшихся пунктов: **0**. NVDA/Narrator, IME, Tab/DPI — внешний
PS-11; живой clipboard substitution drill — внешний PS-12. Оба открыты в
[PSWork.md](PSWork.md).

- [x] Большие истории и длительные сканы проверены: `large_history_is_bounded_and_lists_newest_first`
      (120 записей, лимит 100, страница 20 — prune и хронологический порядок); живой
      полный скан с cancel на высоте 3 311 753 — 1 passed, 97,98 с. UI-список на
      десятки тысяч записей вручную не прокручивался.
- [x] Уведомления о незавершённых/истекающих slate реализованы: `pending_alerts` даёт
      critical для `finalizing`/`finalized_not_posted`/`failed_recoverable`/`response_lost`
      и warning после 30 минут для `prepared`/`locked`/`receive_intent`/`response_received`;
      UI показывает баннер `pending-alert` на главном экране и обновляет его раз в 60 с
      (и при загрузке очереди). Тесты `pending_alerts_sort_critical_first_and_warn_only_after_30_minutes`,
      `fresh_or_finished_slates_do_not_alert`. TTL у pending-сделки нет: 30 минут —
      возрастной порог, не expiry.
- [x] Локализация и справка восстановления: 109 статических строк UI обёрнуты в
      `@tr`, английский каталог `lang/en_US/LC_MESSAGES/flint-slint.po` встроен в
      бинарь, в настройках есть «Русский / English» (после перезапуска), язык хранится
      в `ui-language`. Справка восстановления — отдельный overlay с четырьмя
      абзацами. Тесты: `every_tr_literal_has_an_english_translation`,
      `english_catalog_is_non_empty`, `ui_language_round_trips_and_defaults_to_russian`;
      запуск с `ui-language=en` печатает `ui language: en_US selected`. Динамические
      сообщения идут через `i18n.rs` (264 записи); непереведённая русская строка
      заменяется английским generic-сообщением и пишется в лог, поэтому EN-UI не
      показывает русский. a11y-подписи (29) переведены; полный a11y-аудит — PS-11.
      Детали — [ui/LOCALIZATION.md](ui/LOCALIZATION.md).
- [x] Основные документы синхронизированы; история отделена от текущего статуса.
      Доказательства — запись 23 в `WORK_LOG.md`.
- [x] Фоновая нода имеет отдельный дизайн и гейты; реализация не начата:
      [architecture/BACKGROUND_NODE.md](architecture/BACKGROUND_NODE.md) — варианты,
      контракт, отказные сценарии и пять гейтов до кода.

**Гейт:** не закрыт.

## Этап 6 — mainnet dry-run

Локально выполнимых оставшихся пунктов: **0**. Все пять внешних пунктов открыты в
[PSWork.md](PSWork.md): PS-13 — подтверждение официальных P0; PS-14 — подписанная сборка
и малая сумма под наблюдением; PS-15 — денежный цикл; PS-16 — restore и независимая
сверка; PS-17 — письменный результат без секретов.

**Гейт:** не начат и запрещён до выполнения официальной политики. **Mainnet запрещён.**

## Отложенные треки

Эти **3** трека не входят ни в 12 активных локальных, ни в 17 внешних пунктов.

- [ ] Janus — заморожен; спецификация/криптография/реальная интеграция не начаты.
- [ ] Публичный relay — не развёрнут; distributed abuse и эксплуатационные гейты открыты.
- [ ] Мобильная версия — после desktop-стабилизации и backend interface gate.

## Отдельный технический трек

Не входит в счёт 12 локальных/17 внешних и не разрешает mainnet:

- [ ] RustSec-обновления зависимостей (`zip`, `clap`, `arti/crypto-bigint`, RSA) —
  план, порядок и гейты в
  [security/RUSTSEC_UPGRADES.md](security/RUSTSEC_UPGRADES.md); `age` уже обновлён
  (curve25519-dalek, yaml-rust и zip закрыты), осталось **2 advisories**
  accepted: crypto-bigint и rsa.

## Документация

- [x] Единый индекс и правила ведения находятся в `docs/README.md`.
- [x] Текущие план и журнал находятся в `docs/PLAN.md` и `docs/WORK_LOG.md`.
- [x] Прежние записи Kimi и дорожная карта сохранены в `docs/historical/`.
- [x] Этот `docs/KRead.md` является трекером локального закрытия.
- [x] `docs/PSWork.md` отделяет 17 внешних пунктов; три отложенных трека считаются
      отдельно, официальные гейты не смягчены.

## Ближайший шаг

Этапы 2–4 закрыты локально: offline/flapping/битая цепь на уровне node API, Tor с
Unicode/пробелами и мостами, relay flood/fairness/degraded storage, PT-04, node/RPC
hang/restart/scan cancel/shutdown, native hashes, Cargo+native SBOM, RustSec, большие
истории, дизайн фоновой ноды, slate-уведомления, локализация ru/en статического UI
и справка восстановления. Локальных активных пунктов не осталось; границы перевода
(динамические сообщения и a11y) зафиксированы в
[ui/LOCALIZATION.md](ui/LOCALIZATION.md). Ручной UI остаётся внешним PS-04.
Mainnet запрещён.
