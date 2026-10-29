# Журнал действий Kimi

> **HISTORICAL:** записи 01–22 за 2026-10-27…2026-10-28. Актуальный журнал —
> [../WORK_LOG.md](../WORK_LOG.md).

Правила ведения архивного журнала — в
[KIMI_DOCS_README.md](KIMI_DOCS_README.md). Текущее состояние закрытия задач — в
[../KRead.md](../KRead.md).

---

## 2026-10-28 — 22. PT-05 закрыт на живом RPC restore/scan

Повторно сопоставлены требования PT-05 с кодом и результатом записи 21. Продуктовый код и
тесты не изменялись: существующий `rpc_full_restore_lifecycle` уже содержит необходимую
focused-проверку. `ScanRuntime::scan` отклоняет partial scan при pending restore до входа в
backend-ветку, а `scan_rpc` проверяет `node_info().phase.ready()` до `begin_scan`.

В живом RPC restore тест подтвердил `needs_scan`, отказ `scan(Some(1))`, успешный полный
`scan(None)`, chain-validated summary, снятие `needs_scan` и точный баланс после restart.
Лог `.tmp/restore-lifecycle-final-rpc-restore.log`: **1 passed, 0 failed**, 87,33 с.
Прогон относится к контрольному снимку `20261028-restore-lifecycle-live`; повторная сверка
снимка — `match:true`. Длинный live-scan не повторялся из-за нового ограничения команды в
40 секунд; использован уже полученный результат на неизменном снимке.

PT-05 закрыт. Остаётся **13** локальных пунктов; внешних 17, отложенных 3. Mainnet и
внешние гейты этим не закрываются, mainnet запрещён.

---

## 2026-10-28 — 21. Полный restore lifecycle закрыт для embedded/RPC

### Область и независимая сверка

Создан `crates/runtime/tests/restore_lifecycle.rs` с двумя ignored live-тестами:
`embedded_full_restore_lifecycle` и `rpc_full_restore_lifecycle`. В
`crates/runtime/Cargo.toml` добавлена dev-зависимость `grin_keychain`; `Cargo.lock`
обновлён offline. Продуктовый код не изменялся.

Каждый backend создал новый original wallet, получил ровно 100 000 000 nano = 0,1 GRIN,
восстановился из фразы в чистый каталог, отклонил partial scan, выполнил полный scan и
открылся после restart. Независимый от original оракул восстановил ту же фразу на
противоположном backend и выполнил собственный полный scan. Адрес дополнительно вычислен
напрямую из entropy. В финальной проверке совпали адрес и `spendable`, `awaiting`, `total`,
`immature`, `locked`. Оракул независим по backend и scan, но использует ту же testnet-ноду,
а не независимый chain source.

Временные original/restored/oracle удалены штатным `delete_wallet` в `trash`, уникальные
work roots удалены. Mainnet и пользовательский `FLINT_DATA_DIR` не использовались, секреты
в логи не выводились. Данные второго кошелька сохранены отдельным блоком в
`docs/testnet-funded.txt`; seed и пароли в этот журнал не копируются.

### Живые результаты

Из-за внешнего ограничения команды в 300 секунд сценарии разделены на последовательные
фазы `source-restore/source-scan → setup → restore → oracle → verify`. Все фазы повторены
на едином контрольном снимке:

- Embedded: setup 28,52 с; restore/full-scan/restart 82,13 с; RPC-oracle 83,70 с;
  final verify 12,41 с.
- RPC: setup с явным полным scan original 143,64 с; restore/full-scan/restart 87,33 с;
  embedded-oracle 83,00 с; final verify 12,56 с.
- Оба final verify: **1 passed, 0 failed**, `independent address and exact balance
  reconciliation passed (100000000 nano)`.

Логи: `.tmp/restore-lifecycle-final-{embedded,rpc}-{setup,restore,oracle,verify}.log`.
Восстановление источника funding и его полный scan заняли 96,96 с. После подтверждённого
funding повтор setup идемпотентен и не создаёт вторую транзакцию.

### Regression и снимок

Контрольный снимок `20261028-restore-lifecycle-live`: **669 файлов, 62 199 866 байтов**.
На нём успешно выполнены fmt, workspace check, debug Slint build и selftest:

- `cargo test --workspace --locked --offline` — **274 passed, 0 failed, 28 ignored**,
  37 suites;
- `flint-selftest` — `ok:true`, 2384 мс; собственный временный каталог удалён, sentinel
  в заданном `FLINT_DATA_DIR` не изменён;
- `verify_snapshot.py` — `match:true`, changed/missing/added пусты.

Логи: `.tmp/restore-lifecycle-v2-{fmt,check,tests,build,selftest,snapshot-verify}.log`.
Пункт полного restore lifecycle закрыт для testnet embedded/RPC. Это не mainnet restore:
внешние гейты и PS-16 не закрываются. Остаётся **14** локальных пунктов; внешних 17,
отложенных 3. Следующий шаг — offline/flapping/битая цепь. Mainnet запрещён.

---

## 2026-10-28 — 20. Полный transaction lifecycle закрыт для embedded/RPC

### Область и сценарий

Создан `crates/runtime/tests/transaction_lifecycle.rs` с двумя ignored live-тестами:
`embedded_full_transaction_lifecycle` и `rpc_full_transaction_lifecycle`. В
`crates/runtime/Cargo.toml` добавлена dev-зависимость `grin_core` для явного testnet
chain context; `Cargo.lock` обновлён offline. Продуктовый код не изменялся.

Общий сценарий на funded testnet проверяет полный scan независимых sender/receiver,
`prepare_send → prepare_receive → finalize_send/post → confirmation`, затем вторую
отправку и `cancel_pending`. Каждая подтверждённая передача составила 100 000 000 nano =
0,1 GRIN. После cancel проверены не только pending-журналы, но и точное восстановление
`spendable` и `locked`; те же значения повторно подтверждены после restart обоих
кошельков. Mainnet и пользовательский `FLINT_DATA_DIR` не использовались, секреты не
выводились и не записывались в журналы.

### Живые результаты

- RPC full scan: **1 passed**, 189,08 с.
- RPC money lifecycle после scan: **1 passed**, 179,85 с;
  `.tmp/transaction-lifecycle-rpc.log`.
- Embedded full scan: **1 passed**, 274,63 с.
- Embedded money lifecycle после scan: **1 passed**, 46,26 с;
  `.tmp/transaction-lifecycle-embedded.log`.

Scan-only и money lifecycle разделены из-за внешнего ограничения команды в 300 секунд:
это последовательные фазы одного сценария, а не пропуск проверки. Отдельные stdout-логи
scan-only были перезаписаны последующими запусками того же runner; наблюдавшиеся итоги
зафиксированы здесь, денежные фазы имеют отдельные логи.

### Regression и снимки

Prefix `20261027-transaction-lifecycle-prefix`: 662 файла, 62 151 212 байтов. Первый
live-снимок `20261027-transaction-lifecycle-live` не прошёл verify из-за параллельных
внешних изменений: удалены `tools/candidate-bot*`, изменён `tools/semantic/README.md`.
Эти файлы не относились к данной итерации, поэтому результат не выдавался за чистый
контрольный снимок.

Финальный снимок `20261027-transaction-lifecycle-live-v2`: **668 файлов,
62 178 912 байтов**. На нём повторены fmt, workspace check/tests, debug Slint build и
selftest:

- `cargo test --workspace --locked --offline` — **274 passed, 0 failed, 26 ignored**;
- `flint-selftest` — `ok:true`; sentinel в заданном `FLINT_DATA_DIR` не изменён,
  собственный каталог selftest удалён;
- `verify_snapshot.py` — `match:true`, changed/missing/added пусты.

Логи: `.tmp/transaction-lifecycle-v2-{fmt,check,tests,build,selftest,snapshot-verify}.log`.
Пункт полного transaction lifecycle закрыт. Остаётся **15** локальных пунктов; внешних
17, отложенных 3. Общий restore lifecycle обоих backend с полным chain-scan и независимой
сверкой адреса/баланса остаётся открытым; затем offline/flapping/битая цепь. Внешние
гейты и официальные P0 не смягчены, mainnet запрещён.

---

## 2026-10-27 — 19. Seed create/migrate/disable/cleanup: конкуренция и restart

### Правила и область

Изучен корневой `SKILL.md` (`ub-mode`): применены минимальные поведенческие проверки,
одна проверяемая единица за раз и журналирование. Сначала разобраны `SeedStore`, runtime
create/migrate/disable и существующий crash-harness. Продуктовый код не изменялся:
новые сценарии не воспроизвели дефект, поэтому исправление не потребовалось. Инструменты
crapper/dryer/mutator в среде отсутствуют и не подменялись выдуманными результатами;
commit/push не выполнялись, поскольку `.git` в проекте нет.

Созданы два тестовых файла:

- `crates/core/tests/seed_concurrency.rs` — две конкурентные миграции, два конкурентных
  disable и гонка cleanup/disable на независимых `SeedStore` одного testnet-корня;
- `crates/runtime/tests/seed_create_concurrency.rs` — два независимых `WalletRuntime`
  одновременно вызывают create на одном чистом корне для embedded и реального RPC.

Проверяется ровно один победитель create/migrate/disable, восстановимость seed и адреса
после restart, повторная миграция после disable и сохранение последнего legacy-seed при
позднем cleanup. Managed node отключён, существующие seed, средства, testnet-нода и
`FLINT_DATA_DIR` не использовались. Первый запуск core-теста не собрался из-за двух
нехватающих заимствований `&Path` в самом тесте; исправлены только они, продуктовый код
не затронут (`.tmp/seed-concurrency-core-first.log`).

### Фактические результаты

- Core concurrency: **3 passed, 0 failed**, 4,40 с;
  `.tmp/seed-concurrency-core.log`.
- Embedded concurrent create + restart: **1 passed, 0 failed**, 4,20 с;
  `.tmp/seed-create-embedded-first.log`.
- RPC concurrent create + restart с настоящим `grin-wallet`: **1 passed, 0 failed**,
  11,38 с; `.tmp/seed-create-rpc-first.log`.
- Feature-gated process-abort harness: **2 passed, 0 failed**, 12,24 с; все шесть точек
  M0–M5 восстановлены без слабых seed leftovers, затем disable успешно возвращает читаемый
  legacy. Лог `.tmp/seed-migration-crash.log`. Это deterministic crash injection, не
  физический power-loss.
- Create recovery при удалённой upstream `db/`: **1 passed, 0 failed**, 0,99 с; restart
  пересоздаёт базу из envelope с прежним адресом (`.tmp/seed-create-restart-crash.log`).

### Полный regression и снимки

Prefix `20261027-seed-concurrency-prefix`: 660 файлов, 62 142 159 байта. Контрольный
снимок `20261027-seed-concurrency-live`: **662 файла, 62 151 212 байтов**. После всех
проверок `match:true`; changed/missing/added пусты.

- `cargo fmt --all -- --check` — успешно.
- `cargo check --workspace --all-targets --locked --offline` — успешно;
  `.tmp/seed-concurrency-check.log`.
- `cargo test --workspace --locked --offline` — **274 passed, 0 failed, 24 ignored**;
  `.tmp/seed-concurrency-workspace-tests.log`. Добавились четыре обычных и один ignored
  RPC-тест; live RPC отдельно пройден выше.
- Debug `cargo build -p flint-slint --locked --offline` — успешно;
  `.tmp/seed-concurrency-slint-build.log`.
- `flint-selftest` — `ok:true`, 12 383 мс; `.tmp/seed-concurrency-selftest.log`.
  SHA-256 sentinel в заданном `FLINT_DATA_DIR` не изменился, собственный каталог selftest
  удалён.

Локальный пункт seed create/migrate/disable/cleanup при конкуренции/restart закрыт.
Остаётся **16** локальных пунктов; внешних 17, отложенных 3. Следующий шаг — полный
transaction/restore lifecycle обоих backend. Внешние гейты и официальные P0 не смягчены,
mainnet запрещён.

---

## 2026-10-27 — 18. Повторяемый lifecycle кошелька закрыт для embedded/RPC

### Изменение и границы сценария

Создан `crates/runtime/tests/wallet_lifecycle.rs`; продуктовый код не изменялся.
Embedded-сценарий входит в обычный workspace-набор, RPC-сценарий помечен ignored и
использует настоящий `grin-wallet`. Каждый backend прошёл **два независимых цикла** на
отдельных чистых original/restore testnet-корнях: create, lock, отказ адреса у
заблокированного кошелька, open с тем же адресом, shutdown/restart, повторный open,
restore mnemonic в независимый чистый корень, совпадение адреса, `needs_scan`, seed kind
`envelope` и delete в `trash`. Managed node отключён, mnemonic обёрнут в `Zeroizing`,
временные корни удаляются.

Это адресный lifecycle без полного chain-scan и независимой сверки баланса. Отдельный
общий restore lifecycle обоих backend остаётся открытым. Существующие seed, средства,
testnet-нода и `FLINT_DATA_DIR` не использовались; денежных операций не было.

### Живые результаты

- Embedded: **1 passed, 0 failed**, 11,51 с;
  `.tmp/wallet-lifecycle-embedded.log`.
- RPC: **1 passed, 0 failed**, 35,21 с;
  `.tmp/wallet-lifecycle-rpc.log`.

Каждый результат относится к тесту из двух независимых циклов, а не к одному прогону.

### Полный regression и снимки

До изменений создан prefix `20261027-wallet-lifecycle-prefix`: 659 файлов,
62 137 473 байта. Контрольный снимок `20261027-wallet-lifecycle-live`: **660 файлов,
62 142 159 байта**. После проверок `verify_snapshot.py` возвращает `match:true`;
changed/missing/added пусты.

- `cargo fmt --all -- --check` — успешно.
- `cargo check --workspace --all-targets --locked --offline` — успешно;
  `.tmp/wallet-lifecycle-check.log`.
- `cargo test --workspace --locked --offline` — **270 passed, 0 failed, 23 ignored**;
  `.tmp/wallet-lifecycle-workspace-tests.log`. Рост с прежних 269/22 объясняется одним
  новым обычным embedded-тестом и одним ignored RPC-тестом.
- Debug `cargo build -p flint-slint --locked --offline` — успешно;
  `.tmp/wallet-lifecycle-slint-build.log`.
- `flint-selftest` — `ok:true`, 11 936 мс;
  `.tmp/wallet-lifecycle-selftest.log`. SHA-256 sentinel в заданном `FLINT_DATA_DIR` не
  изменился, собственный каталог selftest удалён.

Повторяемый lifecycle закрыт для embedded/RPC. В активном плане остаётся **17**
локальных пунктов; внешних по-прежнему 17, отложенных 3. Следующий шаг — seed
create/migrate/disable/cleanup при конкуренции и restart. Внешние гейты и официальные
P0 не смягчены, mainnet запрещён.

---

## 2026-10-27 — 17. Мультикошельки: живые embedded/RPC-сценарии закрыты

### Возобновление и ошибка тестового restart

Работа продолжена со снимка паузы `20261027-multiwallet-messenger-pause`. Первые два RPC-
прогона завершились ошибкой `grin-wallet не запустился за 20 секунд` при открытии default
после пересоздания `WalletRuntime`. Контекстная ошибка показала, что harness не выполнил
обязательный запуск дочернего RPC перед `open`: после `switch` запуск делает сам runtime,
а после создания нового runtime его выполняет оболочка через `start_rpc_background()`.

В тест добавлен явный `start_rpc_background()` после restart; дополнительно ошибка open
содержит имя активного профиля. Продуктовый код не изменялся. Это не классифицируется как
дефект RPC recovery. Логи неуспешных попыток: `.tmp/multiwallet-rpc-live.log` и
`.tmp/multiwallet-rpc-live-context.log`.

### Живые доказательства

Оба сценария используют реальный loopback `flint-relay`, два независимых disposable seed,
сообщения в обе стороны, раздельные inbox/history, restart `WalletRuntime`, повторный inbox
без дублей, неверный профиль без смены generation/активной сессии и неверный пароль с
недоступной историей. Денежные операции и существующая нода не использовались.

- RPC: **1 passed, 0 failed**, 38,90 с; `.tmp/multiwallet-rpc-live-pass.log`.
- Embedded повтор на финальном тесте: **1 passed, 0 failed**, 17,81 с;
  `.tmp/multiwallet-embedded-live-final.log`.

### Полный regression и снимок

Контрольный снимок `20261027-multiwallet-messenger-live`: **659 файлов, 62 137 473 байта**.
После всех проверок `match:true`, changed/missing/added пусты.

- `cargo fmt --all -- --check` — успешно.
- `cargo check --workspace --all-targets --locked --offline` — успешно;
  `.tmp/multiwallet-live-check.log`.
- `cargo test --workspace --locked --offline` — **269 passed, 0 failed, 22 ignored**;
  `.tmp/multiwallet-live-workspace-tests.log`. Два новых live-сценария входят в ignored и
  отдельно пройдены выше.
- Debug `cargo build -p flint-slint --locked --offline` — успешно;
  `.tmp/multiwallet-live-slint-build.log`.
- `flint-selftest` — `ok:true`, 11 499 мс; `.tmp/multiwallet-live-selftest.log`. SHA-256
  sentinel в заданном `FLINT_DATA_DIR` не изменился, собственный каталог selftest удалён.

Локальный пункт messenger/history/restart/errors мультикошельков закрыт для embedded/RPC.
В активном плане остаётся **18** локальных пунктов; внешних по-прежнему 17, отложенных 3.
Ручной UI остаётся открытым PS-04. Общий F-04 и официальные P0 не смягчены, mainnet
запрещён. Следующий шаг — повторяемый lifecycle на чистых каталогах embedded/RPC.

---

## 2026-10-27 — 16. Пауза: embedded messenger мультикошельков прошёл, RPC ожидает

### Зафиксированное состояние

Работа остановлена по запросу владельца после первого живого сценария. Активные процессы
тестов/relay не оставлены. Денежные операции, funded seed, существующая testnet-нода и
`FLINT_DATA_DIR` не использовались: оба профиля создаются с независимыми disposable seed
в уникальном testnet-каталоге, managed node отключён, relay слушает только loopback.

До изменений создан prefix `20261027-multiwallet-messenger-prefix`: 659 файлов,
62 129 521 байт. На паузе создан и сразу проверен снимок
`20261027-multiwallet-messenger-pause`: **659 файлов, 62 137 242 байта, match:true**,
changed/missing/added пусты.

### Изменение и фактический результат

В `crates/runtime/tests/multiwallet.rs` добавлены два ignored-live сценария с общим
harness: настоящий `target/debug/flint-relay.exe`, два независимых профиля, сообщения в
обе стороны, inbox/history-границы, перезапуск `WalletRuntime`, неверное имя
профиля без потери активной сессии/generation, неверный пароль с заблокированной историей
и повторный inbox без дублей. Секреты и адреса в документацию не записывались.

Сборка relay и `multiwallet --no-run` завершилась успешно. Embedded-команда:

```powershell
cargo test -p flint-runtime --test multiwallet --locked --offline embedded_profiles_messenger_history_restart_and_switch_errors -- --exact --ignored --nocapture --test-threads=1
```

Результат: **1 passed, 0 failed**, 18,38 с самого теста. После прогона удалён только
неиспользуемый импорт `Path` из теста; это устранение warning, не изменение сценария.
Embedded на финальном снимке ещё не повторялся. RPC-сценарий **не запускался** и не
считается пройденным. Полный workspace regression на этой итерации ещё не выполнялся;
предыдущий итог 269 passed относится к прежнему снимку.

### Возобновление

Сначала выполнить RPC и повторный embedded на снимке
`20261027-multiwallet-messenger-pause`:

```powershell
cargo test -p flint-runtime --test multiwallet --locked --offline rpc_profiles_messenger_history_restart_and_switch_errors -- --exact --ignored --nocapture --test-threads=1
cargo test -p flint-runtime --test multiwallet --locked --offline embedded_profiles_messenger_history_restart_and_switch_errors -- --exact --ignored --nocapture --test-threads=1
```

Затем fmt/check/workspace tests/build/selftest и snapshot verify. Только после этого
закрывать локальный пункт мультикошельков. Ручной UI остаётся внешним PS-04; общий
формальный mainnet-гейт не изменён, mainnet запрещён.

---

## 2026-10-27 — 15. Внешние приёмки отделены от локального активного плана

### Решение и правила счёта

По запросу владельца создан [docs/PSWork.md](../PSWork.md). В него вынесено всё,
что нельзя достоверно выполнить непосредственно в текущей среде ИИ: внешние Windows-
приёмки, физические аварийные испытания, чистый hosted CI, подпись и независимая сверка,
организационные решения, человеческий аудит/recheck, ручные UX/clipboard-проверки и
mainnet dry-run. Это реестр открытых требований, не список завершённых работ.

Получено **17 уникальных внешних пунктов PS-01–PS-17**. Повторные упоминания второго
Windows-аккаунта и destructible F-04 объединены. Прежний объединённый пункт
мультикошельков разделён: ручной UI отнесён к PS-04, живые messenger/history/restart/
errors оставлены локальной работой. Поэтому новые числа нельзя напрямую сравнивать с
прежними 38 отметками `KRead` без этой оговорки.

В активном `docs/KRead.md` оставлено **19 локально выполнимых пунктов**: этап 1 — 1,
этап 2 — 5, этап 3 — 5, этап 4 — 3, этап 5 — 5. Этапы 0.5 и 6 имеют по 0 оставшихся
локальных пунктов. Janus, публичный relay и мобильная версия выделены как **3 отдельных
отложенных трека** и не входят ни в 19, ни в 17.

### Изменённые документы

- `docs/PSWork.md`: назначение, правила счёта, PS-01–PS-17 с причиной и условием
  закрытия, полный перечень 19 локальных задач и отложенные треки.
- `docs/KRead.md`: активные checkbox-элементы относятся только к локальному объёму;
  внешние пункты заменены ссылками на PSWork. Подготовка destructible harness отмечена
  завершённой кодовой работой, но физический F-04 не закрыт.
- `KimiDocs/PLAN.md`: ближайшие итерации начинаются с живых messenger/history/restart,
  затем testnet/эксплуатация, native hashes/SBOM/RustSec и локальные UX/документация.
- `docs/README.md`: PSWork добавлен в индекс актуальных документов.

Официальные `status/STATUS.md` и `status/MAINNET_READINESS.md` не изменялись. Внешние
гейты не закрыты, AI-review не заменяет человеческий аудит, принятая симуляция не равна
физическим evidence, mainnet остаётся запрещённым. Код не менялся, тесты для этой
документационной правки повторно не запускались.

Проверка `.tmp/check-plan-docs.py` подтвердила распределение **1/5/5/3/5**, всего 19;
внешних PS — 17; отложенных — 3; этапы 0.5/6 — 0; активных внешних checkbox — 0;
битых локальных ссылок в четырёх изменённых документах — 0.

Следующий локальный шаг — живые messenger/history двух профилей, restart и ошибки
переключения мультикошельков. Ручной UI остаётся открытым внешним PS-04.

---

## 2026-10-27 — 14. F-04: принята симуляция disk-full, VM-испытания прекращены

### Решение и границы

По решению владельца физические disk-full/power-loss на VM, гипервизоре или отдельном
стенде больше не выполняются в этой итерации. Вместо них использована узкая симуляция:
`ERROR_DISK_FULL` (Windows 112) возвращается только перед записью Flint response
journal, уже после upstream LMDB-commit. Intent, LMDB и остальные файлы остаются
доступными. Power-loss представлен ранее пройденными `exit 9`/restart сценариями
`upstream`/`journal`. Это fault injection и принятое сокращение проверки, **не**
физическое заполнение NTFS, не отключение питания и не формальное закрытие внешнего
destructible-гейта. Mainnet не использовался.

### Реализация

- `crates/core/src/crash.rs`: `receive_journal_write()` под `crash-injection` возвращает
  `std::io::Error::from_raw_os_error(112)` при
  `FLINT_SIMULATE_RECEIVE_DISK_FULL=1`; без feature это всегда `Ok(())`.
- `service.rs` и `rpc_backend.rs`: вызов находится непосредственно перед response
  journal и после upstream return/commit. Повторный receive обязан достать сохранённый
  upstream S2, не подписывая ответ заново.
- `tools/crash-test-receive.ps1 -DiskFullOnly`: требует ошибку 112, сохранённый intent,
  ровно один новый `TxReceived`, тот же slate id/S2 при retry и повторе, затем
  finalize/post и on-chain `get_kernel`. Evidence имеет
  `kind=simulated_disk_full_not_physical`.
- `embedded_cli kernel` исправлен для фактического формата `.grintx`: hex → binary
  `Transaction`, excess/signature выводятся в hex. Компактный `get_stored_tx` не
  используется.
- Добавлен test-only `crates/runtime/examples/rpc_wallet_init.rs`: создание/restore
  временного RPC wallet идёт через штатный owner API. Прямой `grin-wallet init -r`
  на этом Windows-хосте дважды отклонён с `os error 6`; это ограничение консольного
  ввода, а не сбой seed или сети.
- Disposable embedded wallets получили owner secret только внутри своих testnet node
  dirs. RPC receiver после create сканируется с текущего tip-1: без этого первый
  `activity/pending` начинал длительный full refresh и получал owner API timeout.
  Restored sender в обоих backend по-прежнему полностью сканировался с высоты 1.

### Фактические live-результаты

**RPC:** полный `-DiskFullOnly` PASS. Kernel `15444eb4-…` подтверждён в блоке
**3310738**, `receive_count_before=0`, `receive_count_after=1`. Evidence:
`.tmp/crash-receive-simulation/20261027-113110-rpc-b48ebc8b/disk-full-simulation.json`.
После PASS временный wrapper ошибочно посмотрел унаследованный `$LASTEXITCODE` и вывел
`Simulation failed`; это не failure сценария. Проверка удалена: исключение PowerShell
и так прерывает wrapper, а коды фоновых native-команд не являются итогом script PASS.

**Embedded:** все recovery/finalize assertions прошли; внешняя 300-секундная команда
истекла во время ожидания включения kernel, поэтому обычный PASS harness не выведен.
Тот же отправленный kernel позднее независимо найден в блоке **3310716**. До/после
`TxReceived`: 0/1, `finalize/post` подтверждён. Evidence честно содержит
`recovery_assertions=passed_before_chain_wait` и
`chain_confirmation=followup_after_outer_command_timeout`:
`.tmp/crash-receive-simulation/20261027-110357-embedded-a235621b/disk-full-simulation.json`.
Повторная сделка для завершения ожидания не создавалась.

Логи: `.tmp/f04-simulated-disk-full-embedded.log`,
`.tmp/f04-simulated-disk-full-rpc.log`, `.tmp/f04-embedded-chain-check.json`.
Seed, пароли и node owner secret в evidence/документацию не записывались. Существующая
testnet-нода не перезапускалась.

### Regression и снимок

- `cargo fmt --all -- --check` — успешно.
- `cargo check --workspace --all-targets --locked --offline` — успешно
  (`.tmp/f04-simulation-check.log`).
- `cargo test --workspace --locked --offline` — **269 passed, 0 failed, 20 ignored**
  (`.tmp/f04-simulation-tests.log`).
- Feature pause tests — **2 passed**; destructible guards — **8 passed**.
- Snapshot `20261027-f04-simulated-disk-full`: **659 файлов, 62 129 521 байт**,
  `match:true` (`.tmp/f04-simulation-snapshot-verify.log`).

Симуляция принята как достаточная для текущей итерации; формальный F-04 destructible
гейт остаётся невыполненным, но дальнейшие VM-испытания по решению владельца не
требуются. Следующий рабочий шаг — ручной UI и живые messenger/history мультикошельков.

---

## 2026-10-27 — 13. F-04: подготовлен destructible harness, аварийный гейт не выполнен

### Область и безопасность

По согласованному следующему шагу подготовлены реальные disk-full/power-loss сценарии
для отдельной одноразовой Windows VM. На текущей машине не заполнялись диски, не
создавались VHD, не выполнялись shutdown/Stop-VM, funded wallet и существующая нода не
использовались. Прежний snapshot `20261027-f01-raw-stored-recovery` не менялся; перед
работой создан prefix `20261027-f04-destructible-prefix`.

Добавлены [DESTRUCTIBLE_F04.md](../operations/DESTRUCTIBLE_F04.md) и
`tools/f04-destructible.ps1`. Матрица требует 8 независимых прогонов: DF-J и DF-W для
embedded/RPC, затем power-loss `upstream`/`journal` для embedded/RPC. DF-J требует
отдельный journal VHD через junction, поэтому не подменяется заполнением LMDB-тома.
DF-W требует отдельный wallet VHD; sender, нода и evidence остаются на других томах.
Скрипт не выполняет аварийное действие сам: оператор заполняет только FullTarget или
жёстко выключает VM с хоста между отдельными фазами.

### Реализация

- `crates/core/src/crash.rs`: рядом со старым exit 9 добавлен
  `FLINT_PAUSE_AFTER_RECEIVE`. Только под `crash-injection`, после явной проверки
  `.destructible-f04-stand`, hook создаёт с `create_new` несекретный JSON marker,
  делает `sync_all` и паркует процесс. Отсутствие evidence dir/marker завершает попытку
  с кодом 90; production build не читает эти env и сохраняет no-op.
- Harness проверяет оба wallet-маркера, запрет mainnet-путей/адресов, размещение внутри
  `StandRoot`, loopback testnet/no_sync, топологию VHD и неизменность SHA-256 binary
  после reboot. Resume power-loss требует новый boot, неизменный marker и явную
  аттестацию оператора `-PowerCutConfirmed`; необходимость хостового события отдельно
  записана в runbook.
- S1 и state сбрасываются через `Flush(true)` до сигнала `READY FOR OPERATOR POWER CUT`.
  State не содержит пароли/seed; заполненность доказывается и малым free space, и
  неудачной записью 2 MiB. После recovery проверяются тот же S2, ровно один новый
  `TxReceived`, повторный receive, sender finalize/post и независимый on-chain kernel.
- `crates/core/examples/embedded_cli.rs`: добавлена диагностическая команда `kernel`,
  читающая исходную `.grintx` и выводящая excess/signature. Она не использует lossy
  compact `get_stored_tx`; обычная команда finalize не изменена.
- `tools/test-f04-destructible-guards.ps1`: 8 negative-проверок без ноды и аварий.

### Проверки и найденные ошибки подготовки

- Guard-тесты: **8 passed** — непомеченный стенд, неверный marker, непомеченные
  кошельки, выход за StandRoot, mainnet path/address, remote node и resume без
  PowerCutConfirmed отклоняются.
- `crash::tests` с `crash-injection`: **2 passed** — отсутствие stand marker отклонено,
  marker уникален и не содержит wallet material.
- Crash examples собраны с `crash-injection`; `cargo check --workspace --all-targets
  --locked --offline` и `cargo fmt --all -- --check` успешны.
- Полный `cargo test --workspace --locked --offline`: **269 passed, 0 failed,
  20 ignored** (`.tmp/f04-destructible-workspace-tests.log`). Два pause-теста идут
  отдельным feature-набором и не прибавлены к этому числу.
- При подготовке исправлены собственные ошибки: embedded finalize возвращает id,
  поэтому kernel вынесен в чтение `.grintx`; Commitment/Signature выводятся через их
  JSON/hex-сериализацию. После редактирования PS1 повторно восстановлен UTF-8 BOM;
  оба файла разобраны Windows PowerShell 5.1 и guards повторены успешно.
- Финальный review поймал ложный DF-J: полный журнал до первого receive не пропустил бы
  даже intent и upstream commit не состоялся бы. В embedded/RPC добавлен feature-gated
  `receive_boundary("intent")`. Prepare сначала доказывает durable intent и нулевой
  новый TxReceived, только затем разрешает заполнение; RunDiskFull без
  `intent_armed_before_disk_full=true` отклоняется. Все проверки повторены на v2.

Итоговый snapshot `20261027-f04-destructible-harness-v2`: **658 файлов,
62 121 888 байт**, `match:true`. Это снимок подготовленного harness, не evidence
аварийных прогонов.
Реальные 8 сценариев не запускались; F-04 и пункты disk-full/power-loss в `KRead`
остаются открытыми. Следующий внешний шаг — выполнить матрицу на одноразовой VM и
сохранить 8 state с `status=passed` плюс хостовые события power-cut.

---

## 2026-10-27 — 12. F-01: потеря подписи в RPC recovery устранена

### Расследование зависшей транзакции

Повторный post/republish `f8af12ab-67c7-4741-b622-a1c86e267375` не оставлял запись
в pool, хотя node log содержал `Pushing transaction 7ffc3849e45f`. Независимый
`get_kernel` по исходной `.grintx` вернул `NotFound`. Причина найдена в границе
`grin-wallet` Owner RPC, а не в Dandelion relay:

- `get_stored_tx` возвращает компактный `VersionedSlate` с пустым `sigs`;
- преобразование такого slate обратно в `Transaction` реконструирует нулевые
  kernel excess и excess signature;
- RPC `post_tx` отвечал успехом, но такая реконструированная транзакция не
  сохранялась в pool и не могла подтвердиться;
- исходный файл `wallet_data/saved_txs/<slate-id>.grintx` содержал настоящие
  nonzero kernel/signature. Повторная подпись не требовалась и не выполнялась.

Исходная бинарная транзакция была опубликована напрямую через foreign
`push_transaction(fluff=true)`. Pool сразу показал ровно одну совпадающую запись,
затем `get_kernel` подтвердил включение в блок **3310555**. После scan отправитель
и получатель увидели `confirmed`; spendable отправителя восстановлен. Дубликатов и
новой сделки не создавалось. Доказательства: `.tmp/f01-transaction-original.json`,
`.tmp/f01-kernel-chain.json`, `.tmp/f01-node-pool-after-republish.jsonl` относится
только к предшествующему диагностическому republish и не является chain PASS.

### Исправление

- `crates/core/src/rpc_backend.rs`: recovery/retry/republish читают исходную
  `.grintx` через строгий UUID-путь, десериализуют `Transaction` и публикуют её
  напрямую в настроенную `NodeSnapshot`; компактный slate для этого больше не
  используется. Пустой kernel и нулевые excess/signature отклоняются до сети.
  Диагностический `stored_tx` теперь возвращает полную транзакцию, а не lossy slate.
- `crates/runtime/src/rpc.rs`: при start/restart backend получает тот же snapshot
  ноды, который используется owner API.
- `crates/core/examples/wallet_cli.rs`: crash CLI получает тот же node endpoint;
  добавленный временный diagnostic example удалён после извлечения `.grintx`.
- `tools/crash-test-rpc.ps1`: журнального `posted` недостаточно для PASS. Тест
  читает полную stored transaction, отклоняет пустую подпись и обязан найти её
  kernel в цепи в течение 180 секунд. UTF-8 BOM и синтаксис PowerShell проверены.

### Живые доказательства

1. Исправленный recovery `dba5c703-fc11-4dd0-aaf9-0a084d9dc777`: kernel найден в
   блоке **3310568**, обе стороны после scan видят `confirmed`. Логи —
   `.tmp/f01-funded-rpc-crash-fixed.log`, `.tmp/f01-fixed-kernel-chain.json`.
2. Финальный chain-gated сценарий `dbfe2160-1368-4b56-81ed-8f837198bb9b`:
   `locked → finalized_not_posted → posted`, повторный finalize идемпотентен,
   kernel подтверждён в блоке **3310581**. Обе стороны видят `confirmed`, баланс
   отправителя после refresh снова spendable. Лог —
   `.tmp/f01-funded-rpc-crash-chain-gate.log`.
3. Unit regression `stored_transaction_rejects_compact_slate_signature_loss`
   проходит. Полный workspace: **269 passed, 0 failed, 20 ignored**
   (`.tmp/f01-fix-workspace-tests.log`).

`cargo fmt --all -- --check`, workspace check и release Slint build успешны.
Selftest завершился `ok:true` на внешней testnet-ноде; managed node сообщил занятый
порт 13413, как и в предыдущем прогоне, это не failure selftest
(`.tmp/f01-fix-selftest.log`). Снимок `20261027-f01-raw-stored-recovery`:
**656 файлов, 62 077 531 байт**, `match:true`; verify —
`.tmp/f01-fix-snapshot-verify.log`.

F-01 funded crash/recovery/confirmation теперь закрыт на testnet. Это не закрывает
disk-full/power-loss, внешний человеческий аудит, второй Windows-аккаунт, релизные
гейты или mainnet. Следующий шаг по плану — destructible F-04: disk-full и
power-loss на отдельном стенде; затем ручной UI и живые messenger/history профилей.

---

## 2026-10-27 — 11. Funded testnet: F-04/C и F-01 crash recovery

### Стенд и безопасность

По явному указанию владельца использован seed из `docs/testnet-funded.txt` только на
testnet. Созданы одноразовые каталоги `.tmp/f04-funded`; восстановленный адрес совпал
с указанным в файле. Для копий заданы новые случайные пароли, seed в логи не писался.
Исходный `dist/data` не изменялся. Embedded и RPC-копии одного seed не запускались
одновременно: перед RPC-этапом выполнен полный scan с высоты 1. Начальный баланс после
embedded scan — 1451,862 GRIN; перед RPC crash-suite — 1451,763 GRIN.

### F-04/C: реальные process death, restart и деньги

Все четыре независимых сценария прошли:

- `embedded/upstream` и `embedded/journal`: PASS, evidence
  `.tmp/crash-receive/20261027-073435-embedded-d5587ebc`;
- `rpc/upstream` и `rpc/journal`: PASS, evidence
  `.tmp/crash-receive/20261027-074132-rpc-db56a3c2`.

Каждый сценарий доказал exit 9 в заданной точке, restart/retry, канонически тот же S2,
один pending и один новый `TxReceived`, повторный receive без изменения S2, затем sender
finalize/post. Обе RPC-транзакции (`e5ccd177-…` и `56f0e22c-…`) подтверждены в цепи,
change снова spendable. Embedded-транзакции также учтены последующим полным RPC scan.
Сводные выводы: `.tmp/f04-funded-embedded-crash.log`, `.tmp/f04-funded-rpc-crash.log`.
Это закрывает OS kill/restart и testnet receive/finalize/post/confirm часть F-04/C;
disk-full и power-loss не выполнялись и остаются отдельным destructible-гейтом.

### F-01: crash после prepare/finalize

`tools/crash-test-rpc.ps1` прошёл на funded RPC-кошельках: crash после send_prepare
сохранил `locked`, crash после finalize сохранил `finalized_not_posted`, новый процесс
опубликовал stored S3 без повторной подписи, повторный finalize идемпотентен. Лог:
`.tmp/f01-funded-rpc-crash-final.log`.

Отдельная chain-проверка НЕ завершилась: транзакция
`f8af12ab-67c7-4741-b622-a1c86e267375` остаётся `pending`, spendable отправителя 0.
`republish` завершился успешно, но после двух циклов ожидания (300 с и около 195 с)
подтверждения нет. Node log содержит повторный push той же транзакции в pool. Поэтому
crash-recovery отмечается пройденным, а confirmation этой конкретной транзакции —
открытым наблюдением, не PASS. Это не отменяет подтверждённые F-04/C транзакции.

### Исправления, найденные живым запуском

- `embedded_cli`: добавлены restore через существующий `WalletService`, activity и cancel;
  `wallet_cli`: добавлен scan с поддержкой seed envelope. Старые команды не изменены.
- PowerShell 5.1: handle процесса материализуется до выхода, иначе `ExitCode` терялся.
- Receive harness: RPC получает обязательные dir/password аргументы; одиночный Slatepack
  сохраняется как строка, а не Char; cleanup не падает на уже завершённом child PID;
  между сценариями ожидается spendable после подтверждения change.
- UTF-8 BOM восстановлен в обоих PowerShell harness, синтаксис проверен.

### Контрольный снимок

`cargo fmt --all -- --check`, workspace all-target check и debug Slint build успешны.
Полный suite: **268 passed, 0 failed, 20 ignored** (`.tmp/f04-live-tests.log`).
`flint-selftest`: `ok:true`, height 3310530, временный каталог удалён
(`.tmp/f04-live-selftest.log`). Снимок `20261027-f04-live-multiwallet-candidate`:
**656 файлов, 62 071 470 байт**, `match:true`, changed/missing/added пусты
(`.tmp/f04-live-snapshot-verify.log`). Документация не входит в build-relevant manifest.

Остались: confirmation F-01 `f8af12ab-…`, disk-full/power-loss, ручной UI и живые
messenger/history мультикошельков, внешние ACL/релизные/аудитные гейты. Mainnet запрещён.

---

## 2026-10-27 — 10. Мультикошельки и подготовка F-04/C

### Цель и границы

Добавить несколько независимых кошельков в одном процессе и окне Slint без изменения
существующих CLI-сигнатур. Подготовить воспроизводимый process-death suite для F-04/C.
Пользовательские каталоги, mainnet и реальные средства не использовались. Денежный
crash-прогон не запускался: нет двух funded disposable testnet-кошельков.

### Модель профилей и переключение

Legacy/default-путь сохранён, поэтому существующий кошелёк остаётся на прежнем месте.
Именованный профиль получает отдельные wallet/state/pending/RPC/messenger/history-пути.
`WalletRuntime` перечисляет профили и переключается внутри процесса: останавливает
scan/Tor/RPC/node, блокирует историю, создаёт свежий `WalletService` и пересоздаёт
node/RPC. При переключении увеличивается `context_generation`.

В Slint добавлен выбор активного кошелька на главном и lock screen. Смена контекста
очищает пароли, mnemonic, slate/prepared state, pending, контакты, messenger-поля,
seed-статусы и Tor-поля. Асинхронные ответы send/receive, scan, pending, messenger,
security, Tor, refresh/lock привязаны к generation; pending-ID дополнительно привязан
к generation. Барьер `wallet_context_busy` запрещает UI-смену профиля во время денежных
и security-операций, scan, messenger и Tor. Это защита конкретного UI-потока, а не
доказательство потокобезопасности произвольных внешних вызовов runtime.

### F-04/C harness

`tools/crash-test-receive.ps1` поддерживает `embedded` и `rpc`, два независимых S1 на
backend: `upstream` (exit после patched receive, до журнала Flint) и `journal` (после
журнала response, до armoring). Для каждого сценария предусмотрены process exit 9,
restart/retry, сравнение канонического JSON сохранённого S2, запрет дубля pending и
`TxReceived`, затем sender finalize/post.

Прогон допускается только при `.crash-test-ok` в обоих data roots, funded disposable
кошельках, testnet-адресе и локальной синхронизированной ноде. Отсутствие crash injection
или отсутствие exit 9 считается ошибкой. `tools/crash-test-rpc.ps1` ищет маркер в data
root. PowerShell 5.1 требует UTF-8 BOM в скрипте с русскими строками; BOM восстановлен,
синтаксис проверен. Немаркированные каталоги отклонены до обращения к ноде.

### Контрольные результаты

| Проверка | Итог | Лог |
|---|---|---|
| Embedded create/switch/reopen, изоляция seed/address/scan | Passed в полном workspace suite | `.tmp/kimi-workspace-test-final.log` |
| Реальный RPC create/switch/reopen двух профилей | **1 passed, 0 failed** | `.tmp/kimi-multiwallet-rpc-final.log` |
| `cargo fmt --all -- --check` | Успешно | текущий control run |
| `cargo check --workspace --all-targets --locked --offline` | Успешно | `.tmp/kimi-multiwallet-check-final.log` |
| `cargo test --workspace --locked --offline` | **268 passed, 0 failed, 20 ignored** | `.tmp/kimi-workspace-test-final.log` |
| Debug `cargo build -p flint-slint --locked --offline` | Успешно | `.tmp/kimi-multiwallet-slint-build-final.log` |
| `flint-selftest` | `ok:true`, временный каталог удалён | `.tmp/kimi-selftest-final.log` |
| Crash CLI с `crash-injection` | Успешно собран | текущий control run |
| Crash-suite preflight | `chain=test`, `no_sync`, tip 3310468 | текущий control run |

Selftest подключился к внешней testnet-ноде на высоте 3310478; managed node сообщил о
занятом порту 13413, это не failure selftest. Интерактивная приёмка Slint-переключателя
не проводилась. RPC smoke не является receive/finalize/post/confirm.

### Ограничения и следующий шаг

F-04/C и общий F-04 остаются открытыми P1. Не выполнялись денежные OS kill/restart,
реальный RPC receive/finalize/post/confirm, chain confirmation в crash-suite, disk-full
и power-loss. PASS harness также не закрывал бы disk-full/power-loss без отдельного
destructible-стенда. Мультикошельки проверены unit и RPC smoke; живые messenger/history
сценарии и ручной UI-аудит ещё нужны.

Следующий шаг — подготовить два funded disposable testnet-кошелька и выполнить обе
crash-матрицы (`upstream`/`journal`) для embedded и RPC, затем отдельно chain lifecycle
и destructible-проверки.

---

## 2026-10-27 — 09. F-04/B: интеграция Flint, RPC и поставка

### Цель и границы

Подключить проверенный в этапе A fork к embedded и управляемому RPC, сохранить
миграционную семантику старых журналов и обновить offline/source/SBOM. Реальные
wallet data, testnet/mainnet, OS kill/restart, disk-full и power-loss не использовались.
Префикс — `20261027-f04-integration-prefix`; кандидат —
`20261027-f04-integration-candidate-v3`.

### Зависимости и единый RPC

Пять `grin_wallet_*` и `grin_core/keychain/util` подключены явными path-зависимостями
к `third_party/grin-wallet`, поэтому registry и fork не создают несовместимые копии
типов. Вложенные upstream workspaces перечислены в root `workspace.exclude`.

Отдельный upstream RPC workspace с root vendor не собирался: его lock ожидал, например,
`log 0.4.33`, тогда как root vendor уже содержал `0.4.34`. Вместо второго расходящегося
vendor добавлен `crates/grin-wallet-rpc`: package `grin_wallet` компилирует неизменённые
upstream `src/lib.rs`, `src/bin/grin-wallet.rs` и build script в общем lock/vendor Flint.
Криптография и команды кошелька во Flint не дублируются. Общий `Cargo.lock` и vendor
обновлены через Cargo с сетью; последующие check/build выполнены `--locked --offline`.

`tools/fetch-wallet.ps1` больше ничего не скачивает. Он проверяет:
- Git HEAD fork/submodule при наличии `.git` и отсутствие незакреплённых изменений;
- полный список и SHA-256 **545 файлов** upstream fork/submodule по
  `third_party/grin-wallet-sources.sha256`, включая source-архив без `.git`;
- release-сборку `cargo build --release -p grin_wallet --bin grin-wallet --locked --offline`.

Собранный `resources/wallet/grin-wallet.exe` сообщает версию 5.5.1, SHA-256:
`c39c6a59bec0620c99b972c7a951326cbd20abee37de0221e0ce902184904e84`.
`resources/native-assets.sha256` обновлён; все четыре native asset прошли verify.
Прежний RPC binary сохранён как `.tmp/grin-wallet-prefix.exe`.

### Recovery, capability и миграция

Embedded повторяет `ReceiveIntent` с тем же S1. `TransactionAlreadyReceived`,
`ReceiveRecoveryUnavailable` и `ReceiveRecoveryIntegrity` переводятся в
`ResponseLost`; обычные ошибки остаются retryable. Старый `ResponseLost` не
восстанавливается автоматически.

RPC вызывает read-only `check_version` перед новым `receive_tx`; отсутствие
`receive_response_recovery >= 1` отклоняет операцию с русской ошибкой до wallet
mutation. Проверка есть и в `receive_prepare`, и до `init_send_tx` в `send_local`.
Уже сохранённый `received` armorится без этой проверки; `lost` остаётся запрещённым
для повтора. Схема журналов не изменилась: legacy record без `state` по-прежнему
читается как `received`, сохранённый `lost` — как `lost`.

Добавлены regression для capability absent/zero/wrong-type/supported, legacy lost и
обоих typed errors. Это проверяет решения state machine, но не является живым RPC
сетевым тестом.

### Source, SBOM и лицензии

- `third_party/GRIN_WALLET_FORK.toml` фиксирует base/commit, grin commit и capability;
  рядом находятся source manifest и копия патча `grin-wallet-flint.patch`.
- `tools/source.ps1` включает `.cargo` и `third_party`, рекурсивно исключая `.git`,
  `target`, `test_output`, vendor и build/data-каталоги. Архив сформирован:
  26,7 MiB; все 545 хешей в staged fork совпали, запрещённые каталоги отсутствуют.
- `tools/sbom.ps1` записывает оба commit и SHA-256 source manifest в metadata и
  15 компонентов fork/wrapper. Проверочный SBOM: 1141 component.
- `THIRD_PARTY_NOTICES.md` содержит точные commits; Apache-2.0 уведомления сохранены.
- Release pipeline теперь всегда пересобирает RPC из fork перед SBOM/package.

### Контрольные результаты

| Проверка | Итог | Лог |
|---|---|---|
| Scoped `cargo fmt -p flint-* -- --check` | Успешно | текущий control run |
| `cargo check --workspace --all-targets --locked --offline` | Успешно | `.tmp/f04-v2-check.log` |
| `cargo test --workspace --locked --offline` | **267 passed, 0 failed, 19 ignored** | `.tmp/f04-v3-tests.log` |
| `cargo build -p flint-slint --locked --offline` | Успешно | `.tmp/f04-v2-build.log` |
| `flint-selftest` | `ok:true`; sentinel и число файлов не изменились | `.tmp/f04-v2-selftest.log` |
| RPC release build + native verify | Успешно | `.tmp/f04-v2-fetch-wallet.log` |
| Source package | 545/545 source hashes совпали | `.tmp/f04-v2-source.log` |
| SBOM | 1141 components, 15 F-04 components | `.tmp/f04-sbom.json` |
| Snapshot verify | `match:true`, changed/missing/added пусты | `.tmp/f04-v3-snapshot-verify-final.log` |

Первый `cargo fmt --all` отформатировал один файл вложенного grin submodule. Это
обнаружено финальным Git/source контролем; файл возвращён к закреплённому содержимому,
manifest снова совпал. CI/release переведены на fmt только пяти Flint-пакетов, чтобы
больше не изменять upstream. Для честной привязки результатов создан v2 и весь
контрольный набор повторён. После финальной RPC-сборки изменился только хеш бинаря
и его native manifest; поэтому создан v3 и полный test suite повторён ещё раз.
Кандидат содержит **653 файла, 62 025 228 байт**. Документация после снимка не входит
в build-relevant область; финальная SHA-сверка повторена после записей.

### Ограничения и следующий шаг

Завершён этап B, но не F-04 целиком. Не выполнялись OS kill/restart обоих backend,
настоящий RPC receive/finalize/post/confirm, disk-full/power-loss и второй независимый
аудит. Полный upstream workspace test по-прежнему не объявляется зелёным из-за ранее
зафиксированного Windows certificate-store baseline; профильные наборы этапа A перечислены
в записи 08. Mainnet запрещён.

Следующий шаг — этап C на одноразовых testnet-кошельках: сначала process crash/restart
и восстановление того же S2, затем реальный RPC receive и chain lifecycle; disk-full/
power-loss только на отдельном destructible-стенде.

---

## 2026-10-27 — 08. F-04/A: upstream atomic receive recovery

### Цель и границы

Реализовать выбранный ADR в отдельном `grin-wallet`, не подключая его пока к Flint.
Реальные wallet data, testnet/mainnet и сетевые receive не использовались. Основной
workspace Flint и его `Cargo.lock` не менялись. Общий `vendor/` Flint не подменял
закреплённые зависимости upstream: использован изолированный
`.tmp/f04-cargo-home`; корневой `.cargo/config.toml` временно перемещался на время
команды и восстанавливался через `trap`.

### Закреплённый источник

- Каталог: `third_party/grin-wallet`.
- Upstream base: `f097811ee953fc81be4e70435d6ef845b6b2e8c9` (`v5.5.1`).
- Ветка: `flint-receive-recovery-v1`.
- Проверенный commit: `11046cd197f8296e6cd9313502a9f2cbce6e262a`.
- Submodule `grin`: `dbaff34f7751ee23316fbbfe5806b47c546776ca`, без изменений.
- Patch: `KimiDocs/patches/F04-grin-wallet-receive-recovery-11046cd.patch`.

### Реализация

`ReceiveRecovery` содержит форматную версию, account parent, slate UUID,
`VersionedSlate` исходного S1 и настоящего S2. Запись хранится в LMDB под отдельным
prefix. Standard receive теперь подготавливает output, tx-log и подпись в памяти,
после чего один batch/commit сохраняет output, `TxReceived(Standard2)`, kernel excess,
S1 и S2. Счётчик tx-log входит в тот же commit; увеличение derivation index остаётся
отдельным и при сбое может лишь пропустить индекс.

До новой подписи выполняется поиск recovery. Точный S1 возвращает сохранённый S2 без
нового output/tx-log. Изменённый S1 с тем же UUID, несоответствие состояния/output или
несколько `TxReceived` дают `ReceiveRecoveryIntegrity`. Старый `TxReceived` без
recovery даёт `ReceiveRecoveryUnavailable`; отменённая транзакция отклоняется.
`check_version` сообщает `receive_response_recovery = 1`; JSON без нового поля
десериализуется как `None`.

Обёртка старого invoice/legacy-пути сохранена. При финальном review в ней найден и
исправлен пропуск commit счётчика tx-log; после исправления отдельно пройден invoice
integration test. Debug-only failpoints одноразовые и thread-local: до commit и сразу
после него. В release этих failpoints нет.

### Доказательства

| Проверка | Итог | Лог |
|---|---|---|
| `cargo test -p grin_wallet_libwallet --locked` | **38 passed, 0 failed** | `.tmp/f04-libwallet-tests.log` |
| `cargo test -p grin_wallet_api --locked` | **94 passed, 0 failed** | `.tmp/f04-api-tests-final.log` |
| `cargo test -p grin_wallet_controller --test transaction --locked` | **5 passed, 0 failed** | `.tmp/f04-transaction-tests-final.log` |
| `cargo test -p grin_wallet_controller --test invoice --locked` | **1 passed, 0 failed** | `.tmp/f04-invoice-tests.log` |
| `cargo check --workspace --locked` | Успешно | `.tmp/f04-workspace-check.log` |
| pre-commit rustfmt | Успешно | вывод commit hook |

Crash/fault-тест доказывает: ошибка до commit не оставляет `TxReceived`/output; ошибка
после commit допускает повтор S1 и возвращает канонически тот же S2; повтор не создаёт
дубли; изменённый S1 отклоняется; восстановленный S2 успешно финализируется
отправителем. Отдельно проверены legacy-запись без recovery и совместимость VersionInfo.
Это in-process fault-injection, а не kill процесса, disk-full или power-loss.

Полный baseline `cargo test --workspace` ранее остановился на несвязанном
`grin_api router::tests::test_get`: Windows запретил открытие current user certificate
store (OS code 5), `no native root CA certificates found`. Поэтому полный upstream
workspace test не объявляется зелёным; использованы профильные наборы выше.

Снимок `20261027-f04-upstream-recovery` включает `third_party`, исключает `.git`,
`target`, `test_output` и другие build/data-каталоги: **649 файлов, 61 921 074 байта**.
Финальная сверка после восстановления `.cargo/config.toml`: `match:true`,
changed/missing/added пусты (`.tmp/f04-upstream-snapshot-verify.log`). Рабочее дерево
fork после тестов чистое.

### Ограничения и следующий шаг

Завершён только этап A. Fork не используется embedded/RPC Flint, упаковка/offline
source/SBOM не обновлены, миграция старых intent/lost и запрет старого внешнего RPC
ещё не реализованы. Живой RPC receive, restart процесса, disk-full/power-loss и
testnet не проверялись. F-04 остаётся открытым P1; mainnet запрещён.

Следующий шаг — этап B: закрепить источник fork в зависимостях Flint, проверить
совместимость сборки и перевести embedded/RPC recovery на новую capability и typed
errors. До этого нельзя считать денежный сценарий Flint восстановленным.

---

## 2026-10-27 — 07. F-04: ADR атомарного receive recovery

### Цель и границы

Выбрать архитектуру F-04 до изменения денежного кода. Проверены текущие embedded/RPC
журналы и исходники `mimblewimble/grin-wallet` тега `v5.5.1` через публичный GitHub.
Реальные wallet data, testnet/mainnet и live crash не использовались. Rust-код,
зависимости и RPC binary в этой итерации не менялись.

### Новый технический результат

- `get_stored_tx` не восстанавливает response: upstream создаёт Slate `Standard3`,
  тогда как получатель должен вернуть `Standard2`.
- `build_recipient_output` коммитит output/`TxReceived` до завершения S2; затем есть
  отдельные commit kernel excess и Slate state. Поэтому crash gap начинается раньше
  возврата `receive_tx`, и одного failpoint после return недостаточно.
- Локальная запись response до/после вызова не создаёт общей атомарности с LMDB.
  Копирование криптографического receive во Flint отклонено как более рискованное.

### Принятое решение

Создан `docs/security/ADR_F04_RECEIVE_RECOVERY.md`. Выбран закреплённый fork
`grin-wallet` 5.5.1: стандартный receive подготавливает S2 в памяти, затем один
LMDB-commit сохраняет recipient output, `TxReceived`, исходный S1 и настоящий S2.
Повтор того же S1 возвращает тот же S2 без новой подписи/output/tx-log. Тот же UUID
с другим Slate отклоняется. `check_version` должен сообщать capability
`receive_response_recovery = 1`; старый внешний RPC без неё не выполняет новый receive.

ADR также фиксирует миграцию старых intent/lost, недопустимость автоматической отмены,
offline/source/SBOM/лицензии, отсутствие автоматической очистки recovery в первой
версии и crash/fault-матрицу. Неизменным артефактом является сериализованный S2;
Slatepack armor после повторного шифрования может отличаться байтами.

### Документация и снимки

Обновлены `docs/KRead.md`, `docs/status/STATUS.md` и `KimiDocs/PLAN.md`. Порядок этапов
не изменён: ADR завершён, этап A (upstream), затем B (Flint), затем C (live).
Префикс `20261027-f04-adr-prefix`: 104 файла, 43 300 191 байт. По сравнению с
`20261027-f01-r01-candidate` исходники не изменились, но во время сессии внешней
установкой появился `tools/gh.exe` размером 41 785 144 байта. Файл не изменялся и не
включался в решение F-04; это внешнее изменение дерева учтено в новом baseline.

Cargo после документационной итерации не запускался: проверяемый Rust-код не менялся.
Итоговый снимок `20261027-f04-adr-candidate` и `verify_snapshot.py` фиксируют только
неизменность build-relevant дерева, а не работоспособность будущего upstream-патча.

### Следующий шаг

Этап A: создать отдельный закреплённый fork/рабочий источник `grin-wallet` 5.5.1,
реализовать recovery record, единый atomic commit, idempotent duplicate и capability.
До интеграции доказать upstream unit/crash-матрицу. F-04 остаётся открытым P1.

---

## 2026-10-27 — 06. F-01/R-01: fail-closed finalize и классификация receive

### Цель и границы

Согласовать денежные переходы embedded и RPC: ошибка finalize без доказанной подписи
не должна разрешать publish/retry, а ошибка receive без доказанной мутации не должна
уничтожать возможность повтора. Реальные wallet data, mainnet, testnet и живые
crash-сценарии не использовались. Префикс — `20261027-f01-r01-prefix`; кандидат —
`20261027-f01-r01-candidate` (103 файла, 1 515 047 байт). Изменены `error.rs`,
`service.rs`, `rpc_backend.rs`; схемы JSON и зависимости не менялись.

### Матрица переходов

| Вызов и результат | Состояние | Повтор/publish |
|---|---|---|
| embedded/RPC `finalize_tx`, любая ошибка | `FailedRecoverable`, журнал сохранён | Повторная подпись и auto-publish запрещены |
| `finalize_tx`, успешный возврат | `FinalizedNotPosted` | Publish разрешён по доказанной S3 |
| embedded/RPC `receive_tx`, обычная ошибка | `ReceiveIntent`, attempts и ошибка сохранены | Повтор receive разрешён |
| `receive_tx`, явный `TransactionAlreadyReceived` | `ResponseLost` | Повтор receive запрещён |
| ошибка ACL/JSON/I/O журнала | явная ошибка | Не принимается за отсутствие записи |

В embedded удалена эскалация `DB Not Found`/`Slate id` в `FinalizedNotPosted`.
В RPC одинаковый fail-closed переход применён к `send_finalize`, Tor online и local
online. `resume_receive` больше не скрывает ошибки `load_pending`. Partial S1 по-
прежнему не является доказательством подписи; текст ошибки им не становится.

### Regression

Добавлены шесть тестов:

- `only_duplicate_receive_proves_wallet_mutation`;
- `every_finalize_error_is_recoverable_and_not_retryable`;
- `receive_errors_preserve_intent_unless_duplicate_is_proven`;
- `no_finalize_error_text_is_treated_as_a_signature`;
- `receive_errors_preserve_retry_unless_duplicate_is_proven`;
- `resume_receive_does_not_hide_journal_errors`.

Core unit suite: **168 passed, 0 failed** (`.tmp/f01-r01-targeted-v2.log`). Полный
контрольный прогон одного кандидата:

| Проверка | Итог | Лог |
|---|---|---|
| `cargo fmt --all -- --check` | Успешно | `.tmp/f01-r01-control.log` |
| `cargo check --workspace --all-targets --locked --offline` | Успешно | `.tmp/f01-r01-control.log` |
| `cargo test --workspace --locked --offline` | **265 passed, 0 failed, 19 ignored** | `.tmp/f01-r01-control.log` |
| `cargo build -p flint-slint --locked --offline` | Успешно | `.tmp/f01-r01-control.log` |
| `flint-selftest` | `ok:true`, 11 650 мс | `.tmp/f01-r01-selftest.log` |

В заданном `FLINT_DATA_DIR` остался только неизменный `SENTINEL.txt`; собственных
`flint-selftest-*` каталогов после завершения нет. `verify_snapshot.py` вернул
`match:true`, changed/missing/added пусты (`.tmp/f01-r01-snapshot-verify.log`).

### Ограничения и следующий шаг

Это закрывает кодовую классификацию F-01/R-01, но не живой crash-прогон и не F-04.
Crash между `receive_tx` и durable response по-прежнему невосстановим без upstream
или эквивалентной атомарной обёртки; 19 ignored/live не запускались. Следующий шаг —
ADR для атомарного восстановления response с последующей реализацией и crash-тестами.

---

## 2026-10-27 — 05. SENT-01/F-05: защищённая запись и явные ошибки ACL

### Цель и границы

Устранить write-before-ACL и подавление ошибок `restrict()` во всех подтверждённых
местах, включая дополнительно найденные RPC send/receive journals. Изменялись только
код и тесты; реальные wallet data, mainnet и live-сетевые сценарии не использовались.
Префикс: `20261027-sent01-prefix`. Первый контрольный кандидат был заменён после
дополнительной проверки миграции и fault-injection; окончательный снимок —
`20261027-sent01-candidate-v2` (103 файла, 1 512 358 байт).

### Общий примитив записи

В `crates/core/src/win_acl.rs` добавлен `write_private_atomic(path, bytes)`:

1. создаёт уникальный temp в каталоге назначения через `create_new`;
2. вызывает `restrict(temp)` до записи любых секретных байтов;
3. записывает данные, делает `sync_all` и закрывает файл;
4. атомарно заменяет назначение: Windows — `MoveFileExW` с
   `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`, Unix — rename и sync parent;
5. повторно вызывает `restrict(final)` и возвращает любую ошибку;
6. удаляет temp при ошибке, но никогда не удаляет уже установленный final после
   неуспешной финальной проверки.

Новых зависимостей нет. Тестовый fault-injection thread-local, поэтому параллельные
тесты не влияют друг на друга; в release его нет.

### Исправленные вызовы

- `node.rs`: `.api_secret` и `.foreign_api_secret` создаются через secure write;
  существующие секреты проходят ACL до использования, ошибка запрещает запуск.
- `rpc_backend.rs`: `.node_api_secret`, send/receive journals и импорт старых записей
  используют secure write. Чтение существующих журналов предваряется ACL; ошибка
  миграции возвращается, а не маскируется пропуском.
- Upstream `.owner_api_secret`: Flint применяет и проверяет ACL до чтения. При сбое,
  timeout секрета или timeout порта child явно получает kill/wait. Интервал между
  созданием файла upstream и применением ACL не может быть устранён без изменения
  upstream; Flint этот файл не перезаписывает.
- `onion.rs`: Tor secret/public key files записываются через общий примитив, старый
  подавляемый вызов ACL удалён.
- `service.rs`: pending и archive используют secure write; чтение/list старых
  записей применяет ACL и возвращает ошибку.
- `history.rs`: temp/final ACL больше не подавляются; существующие wrapped keys
  проверяются до чтения, read-back после записи сохранён.
- `messenger.rs`: blocked list записывается и открывается только после успешного ACL.
- `config.rs`: backup и восстановленный settings записываются через secure write;
  backup проходит ACL до чтения. Ошибка ACL corrupt-копии добавляется в warning,
  ошибка основного файла по-прежнему возвращается как явное предупреждение.

### Regression и контрольный прогон

Fault-injection доказывает: сбой до записи сохраняет старый target и не оставляет
temp; сбой после rename возвращается наружу и не удаляет новый final; сбой при
открытии pending блокирует чтение. Интеграционный pending test затем подтверждает
восстановление записи после снятия сбоя.

| Проверка | Итог | Лог |
|---|---|---|
| `cargo fmt --all -- --check` | Успешно | `.tmp/sent01-v2-control.log` |
| `cargo check --workspace --all-targets --locked --offline` | Успешно | `.tmp/sent01-v2-control.log` |
| `cargo test --workspace --locked --offline` | **259 passed, 0 failed, 19 ignored** | `.tmp/sent01-v2-control.log` |
| `cargo build -p flint-slint --locked --offline` | Успешно | `.tmp/sent01-v2-control.log` |
| `flint-selftest` | `ok:true`, 11 691 мс | `.tmp/sent01-v2-selftest.log` |

Заданный `FLINT_DATA_DIR` содержал только sentinel и остался неизменным. Собственный
`.tmp/flint-selftest-48c3345d-2f80-4a1a-8547-2cb58d20cec7` удалён. После проверок
`verify_snapshot.py`: `match:true`, changed/missing/added пусты.

### Ограничения и следующий шаг

Кодовая часть SENT-01/SENT-02/F-05 завершена для указанного снимка. Общий этап 0.5
не закрыт: доступ из второго Windows-аккаунта и поведение на FAT/exFAT требуют
отдельной приёмки. Это AI/code review и regression, не человеческий pentest;
19 ignored/live не запускались. Следующий кодовый трек — F-01/R-01, затем F-04.

---

## 2026-10-27 — 04. SENT-02: настоящий owner-only DACL

### Цель и снимки

Исправить только примитив `win_acl::restrict` и доказать удаление заранее
существующего постороннего ACE. SENT-01 в вызывающих модулях в этой итерации не
изменялся. Префикс: `KimiDocs/snapshots/20261027-sent02-prefix`; проверенный
кандидат: `20261027-sent02-candidate` (103 файла, 1 508 363 байта).

### Выбор реализации

Старый `icacls /inheritance:r /grant:r <user>:F` сохранял другие explicit ACE.
Сначала проверен PowerShell `Set-Acl`: на одноразовом файле он потребовал
`SeSecurityPrivilege`, поэтому не принят. Затем проверен
`[System.IO.File]::SetAccessControl`: DACL успешно заменён без повышения прав.
Пробный SDDL содержит только SID текущего пользователя (`.tmp/sent02-powershell-probe.log`).

В `crates/core/src/win_acl.rs` теперь:

- SID берётся из `WindowsIdentity::GetCurrent().User`, а не из строки `USERNAME`;
- наследование отключается, все прежние access rules удаляются;
- добавляется один FullControl Allow для текущего SID;
- DACL применяется через `File.SetAccessControl`;
- файл открывается повторно через `Get-Acl`; проверяются protected DACL, ровно
  один ACE, SID, FullControl, Allow и `IsInherited=false`;
- путь передаётся через child environment `FLINT_ACL_PATH`, без интерполяции в
  командную строку; ошибка возвращается вызывающему коду.

Новых Rust-зависимостей нет, offline vendor не менялся. Путь к PowerShell
интерполируется только как имя команды; содержимое секретов не читается и не
выводится.

### Regression

`win_acl::tests::restrict_removes_a_pre_existing_explicit_everyone_ace` создаёт
одноразовый файл в каталоге с пробелами и Unicode, выдаёт `Everyone:F`, затем дважды
вызывает `restrict`. Независимый `Get-Acl ... .Sddl` подтверждает protected DACL,
ровно один ACE и отсутствие `WD` (Everyone). Временный каталог удаляется.

Прицельный прогон: **1 passed, 0 failed**, 0,80 с;
`.tmp/sent02-test-acl-unicode.log`.

### Полный контрольный прогон candidate

| Проверка | Итог | Лог |
|---|---|---|
| `cargo fmt --all -- --check` | Успешно | `.tmp/sent02-fmt.log` |
| `cargo check --workspace --all-targets --locked --offline` | Успешно | `.tmp/sent02-check.log` |
| `cargo test --workspace --locked --offline` | **257 passed, 0 failed, 19 ignored** | `.tmp/sent02-test-workspace.log` |
| `cargo build -p flint-slint --locked --offline` | Успешно | `.tmp/sent02-build-slint.log` |
| `flint-selftest` | `ok:true`, 10 161 мс | `.tmp/sent02-selftest.log` |

Core unit suite вырос с 159 до 160 тестов. `node_manager` снова прошёл, включая
`restarts_cleanly_after_a_normal_stop`. Заданный `FLINT_DATA_DIR` содержал только
sentinel; его hash и содержимое не изменились. Собственный каталог
`.tmp/flint-selftest-0361c779-5e08-4e0d-876d-ac9aac47b3ea` удалён.

После всех проверок `verify_snapshot.py`: `match:true`, changed/missing/added пусты
(`.tmp/sent02-snapshot-verify.log`).

### Ограничения и следующий шаг

SENT-02 исправлен по коду и покрыт regression. Это не закрывает SENT-01: вызовы
`let _ = restrict(...)`, write-before-ACL, archive без ACL и отдельный upstream
`.owner_api_secret` остаются. Второй Windows-аккаунт и FAT/exFAT не проверялись;
19 ignored/live не запускались. Mainnet по-прежнему запрещён.

Следующая итерация — SENT-01/F-05: единая политика secure write/existing-file ACL
для всех подтверждённых вызовов, затем повторный полный набор.

---

## 2026-10-27 — 03. Независимая проверка Sentinel-аудита

### Цель и границы

Проверить `docs/security/SENTINEL_AUDIT_2026-10-27.md`, выделить подтверждённые
новые риски, повторы и непроверенные утверждения. Исходный отчёт и код не менялись;
live/mainnet, сеть и wallet data не использовались. Перед ревью SHA-256 сверка
подтвердила соответствие кода `20261027-024832-candidate`.

### Подтверждено

- SENT-01: `let _ = win_acl::restrict(...)` подавляет ошибки для node API,
  `.owner_api_secret`, onion secret, pending и corrupt settings. Для node/onion
  секретные байты записываются до ACL.
- F-04: crash gap между wallet mutation и durable response остаётся в обоих backend.
- F-10: checksum `fetch-wallet` приходит из того же release; package покрывает
  четыре manifest EXE, но не vcruntime/pt_config; SBOM Cargo-only; подпись opt-in.
- Приведённые scanner-срабатывания по прочитанным местам являются ложными: TOML
  insert не SQL, пароли принадлежат одноразовым тестам. Сам scanner и метрика
  95 файлов не воспроизводились.

### Новый результат: SENT-02

Точная команда из `win_acl::restrict` проверена на одноразовом
`.tmp/sentinel-acl-probe.txt`. После предварительного `Everyone:F` и последующего
`icacls /inheritance:r /grant:r <USERNAME>:F` Everyone сохранил full access. SDDL
содержит `(A;;FA;;;WD)` наряду с текущим пользователем; DACL защищён (`D:PAI`).
Доказательство — `.tmp/sentinel-acl-probe-sddl.log`.

Следовательно, успешный `restrict()` не гарантирует owner-only для существующих
явных ACE. Это затрагивает и fail-closed seed/settings вызовы. Проверка не означает,
что каждый новый файл доступен Everyone, и не заменяет тест вторым аккаунтом.

### Пропущенная Sentinel область

- `rpc_backend.rs:395-402`: `.node_api_secret` пишется до ACL, конечный файл после
  rename не проверяется.
- `service.rs:2175-2193`: archive pending пишется вообще без `restrict`.
- `runtime/history.rs:405,410`: ошибки ACL обёрнутого history key подавляются.
- `runtime/messenger.rs:482-483`: blocked list пишется до подавляемого ACL.

Полное ревью сохранено в `KimiDocs/SENTINEL_REVIEW_2026-10-27.md`.

### Делегирование Codex

Локальному Codex в read-only режиме передана только проверка F-10. Задача:
`.tmp/codex-sentinel-f10.md`; результат: `.tmp/codex-sentinel-f10-output.md`;
JSONL: `.tmp/codex-local/run-20261027-030608-271.jsonl`; `exit_code:0`. Его выводы
по fetch/package/SBOM/signing самостоятельно сверены с исходными скриптами. Codex
ничего не изменял и не обращался к сети.

### Изменение плана

Перед F-01/R-01 добавлен этап 0.5: сначала SENT-02 (сам ACL-примитив), затем
SENT-01/F-05 (все вызовы и запись). Причина: обработка ошибок без исправления
примитива не доказывает owner-only. Денежные P1 не отменяются и остаются следующими.

### Следующий шаг

Создать новый исходный снимок и исправить SENT-02 с regression, который начинает
с постороннего explicit ACE. Затем устранить SENT-01 во всех подтверждённых местах.
Не отмечать второй Windows-аккаунт или FAT/exFAT закрытыми без реальных проверок.

---

## 2026-10-27 — 02. Этап 0: восстановление и проверка базового снимка

### Цель

Проверить актуальность синтаксической ошибки из повторного аудита, получить
неизменный исходный снимок и выполнить на нём локальный CI-набор без live-тестов.

### Исходное состояние и изменение

`crates/core/src/service.rs:718` действительно содержал вне комментария текст
`ecover_from_phrase refuses...`; файл не мог парситься. До правки сохранён снимок
`KimiDocs/snapshots/20261027-024832-prefix` (103 файла, 1 504 871 байт).
Исправлена только запись комментария: восстановлены `// ` и первая буква `r`.
Исполняемая логика не менялась.

После правки создан `KimiDocs/snapshots/20261027-024832-candidate`
(103 файла, 1 504 870 байт) с `MANIFEST.sha256` и `SNAPSHOT.json`. Добавлены
повторно используемые `KimiDocs/tools/snapshot_sources.py` и
`verify_snapshot.py`; область снимка — build-relevant исходники, без зависимостей,
бинарей, wallet data и build outputs.

### Команды и результаты

Все Cargo-команды запускались с `TMP=TEMP=C:\DHW\.tmp`. Первый запуск из Git Bash
не нашёл `cargo` в PATH; далее использовался установленный
`C:\Users\nurli\.cargo\bin\cargo.exe`. Это не изменило исходники.

| Проверка | Итог | Полный лог |
|---|---|---|
| `cargo fmt --all -- --check` | Успешно | `.tmp/stage0-fmt.log` |
| `cargo check --workspace --all-targets --locked --offline` | Успешно, 55,32 с | `.tmp/stage0-check.log` |
| `cargo test --workspace --locked --offline` | **256 passed, 0 failed, 19 ignored**, 57,87 с wall time | `.tmp/stage0-test-workspace.log` |
| `cargo build -p flint-slint --locked --offline` | Успешно, 50,37 с | `.tmp/stage0-build-slint.log` |
| `cargo run -p flint-runtime --bin flint-selftest --locked --offline` | `ok:true`, внутренние 9 059 мс | `.tmp/stage0-selftest.log` |

В полном test suite `restarts_cleanly_after_a_normal_stop` прошёл; весь
`node_manager` integration suite — **3 passed, 0 failed**, 27,80 с. Прежний timeout
не воспроизвёлся. Причина прошлого незавершённого запуска по этому результату не
установлена, но текущий тест нельзя считать зависшим или failing.

### Проверка границ selftest

Перед запуском создан `.tmp/stage0-flint-data-sentinel/DO_NOT_DELETE.txt`, а
`FLINT_DATA_DIR` указывал на этот каталог. До/после сверены SHA-256 файла и список
содержимого: изменений нет. Selftest использовал собственный
`.tmp/flint-selftest-7c1fa876-efb1-4a8d-aa02-eadc226eee4e`; после завершения этот
каталог отсутствовал. Sentinel оставлен как доказательство проверки, не содержит
секретов или данных кошелька.

### Целостность снимка

После fmt/check/test/build/selftest выполнена повторная SHA-256 сверка текущих
build-relevant источников с candidate: `match:true`, `changed:[]`, `missing:[]`,
`added:[]`. Лог — `.tmp/stage0-snapshot-verify-final.log`. Таким образом, итоги
относятся к одному неизменному снимку. Последующие изменения STATUS/KRead/WORK_LOG
не входят в build-relevant область manifest.

### Ограничения результата

- 19 ignored/live-тестов не запускались; этот прогон не закрывает F-01/F-04/R-01,
  реальный RPC receive, Tor/flood/ACL и другие живые гейты.
- Mainnet и реальные средства не использовались.
- Релизные артефакты не пересобирались и не подписывались.
- Предыдущий `AUDIT_RECHECK` остаётся историческим отчётом об inspected snapshot;
  исправление синтаксиса не изменяет его выводы по денежным state machines.

### Решение и следующий шаг

Этап 0 закрыт только для `20261027-024832-candidate`. Далее — этап 1: сначала
F-01 (недоказанный `FinalizedNotPosted`) и R-01 (ошибочный `ResponseLost`), затем
ADR/реализация F-04. Перед новыми правками создать следующий снимок и не переносить
текущие результаты на изменённый код без повторных проверок.

---

## 2026-10-27 — 01. Создание параллельной документации

### Цель и запрос

Пользователь согласовал предложенный приоритет разработки и поручил вести
параллельную документацию: подробный журнал в `KimiDocs/`, а трекер закрытых
пунктов — в `docs/KRead.md`. Записи должны позволять будущим участникам продолжить
работу без восстановления хода рассуждений по переписке.

### Что было изучено до этой записи

- `AGENTS.md` — границы проекта, команды и правила безопасности.
- `docs/README.md` — карта основной документации.
- `docs/status/STATUS.md` — заявленные результаты предыдущего дерева.
- `docs/status/ROADMAP.md` — фазы, messenger, фоновая нода и заморозка Janus.
- `docs/status/MAINNET_READINESS.md` — обязательные P0/P1 и запрет mainnet.
- `docs/architecture/BACKENDS.md`, `RPC_EXTRACTION.md`,
  `NODE_ARCHITECTURE.md` — границы embedded/RPC и жизненный цикл ноды.
- `docs/architecture/MESSENGER.md` — протокол, хранение, Tor и ограничения v1.
- `docs/operations/RELAY.md`, `BACKUP.md`, `RESTORE.md` — эксплуатационные runbook-и.
- `docs/security/TRUST_BOUNDARY.md`, `CALLBACK_AUDIT.md`,
  `INCIDENT_RESPONSE.md` — границы доверия и открытые внешние проверки.
- `docs/security/AUDIT_RECHECK_2026-10-27.md` — результат повторного AI-review.

### Принятый порядок

1. Восстановить и проверить единый исходный снимок.
2. Исправить F-01 и R-01.
3. Реализовать полное решение F-04 и объединить crash-проверки.
4. Выполнить testnet-приёмку и эксплуатационные проверки.
5. Завершить release engineering и независимый аудит.
6. Только после официальных гейтов рассматривать mainnet dry-run.

Фоновая нода, мобильная версия, публичный relay и Janus в ближайший план не
включены. Причина: открытые денежные P1 и отсутствие проверяемого текущего полного
workspace-прогона важнее новых функций.

### Выполнено в этой записи

- Создан каталог `C:\DHW\KimiDocs`.
- Созданы `README.md`, `WORK_LOG.md` и `PLAN.md`.
- Создан `docs/KRead.md` с гейтами и отдельными доказательствами для каждого этапа.
- Код, настройки, бинарные файлы и данные кошельков не изменялись.
- Команды сборки и тестирования в рамках создания документации не запускались.

### Исходные ограничения, которые нужно перепроверить при старте кода

Повторный аудит зафиксировал непарсящийся `crates/core/src/service.rs` в районе
строки 718, незакрытые F-01/F-04 и новую R-01. Это сведения об inspected snapshot,
а не утверждение, что файлы не менялись после аудита. Перед исправлением прочитать
актуальные участки заново.

Прежние `256 passed` из `docs/status/STATUS.md` относятся к предыдущему дереву.
Незавершённый `node_manager`-прогон нельзя считать доказанным failing test; его
нужно повторить и классифицировать на стабильном снимке.

### Следующий шаг

Начать этап 0 из `PLAN.md`: прочитать актуальный участок `service.rs`, восстановить
парсинг при необходимости, зафиксировать снимок и выполнить полный набор
fmt/check/test/selftest. Результаты и любые внешние изменения дерева внести новой
записью выше.
