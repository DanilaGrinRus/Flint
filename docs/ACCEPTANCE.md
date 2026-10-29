# Приёмка desktop RC

Только testnet и одноразовые данные. Текущий результат и ссылки на завершённые
прогоны — [STATUS.md](STATUS.md). Наличие сценария/скрипта не является PASS.

## Правило готовности

`READY_FOR_EXTERNAL_DEVICE_TESTS` означает: функциональность согласованного RC
реализована, автоматические и необходимые live-проверки завершены, подготовлен
идентифицированный portable-пакет; остались только физические сценарии ниже.
Unsigned test RC допускается с явной отметкой. Сертификат подписи отсутствует;
это свойство тестового артефакта, а не успешно выполненная Authenticode-проверка.
Внешний аудит и mainnet dry-run не входят в эту RC-задачу.

Для нового RC записать version, commit/dirty state, BUILD-INFO, полный SHA-256
exe/zip/source/контракта, SBOM и вид подписи. Артефакты разных сборок не смешивать.

## Автоматическая и live-приёмка

| Область | Что требуется |
|---|---|
| База | fmt/check, unit/integration, doctests/rustdoc, release build и selftest; selftest не трогает пользовательский FLINT_DATA_DIR |
| Оба backend | Create/open/lock, профильная изоляция, node readiness, проверенные summary/history, restore с полным scan |
| Перевод | Prepare/send/receive/finalize/post/chain confirmation, безопасная отмена, retry/reconcile после restart; совпадают slate/kernel/суммы |
| Pending | S1 повторяется без новой подписи; S2 экспортируется из сохранённой операции; неизвестный ID не выбирает чужую последнюю операцию |
| Recovery | ADR F-04: тот же S2, один output/TxReceived, capability fail-closed, конфликтующий S1 отклоняется |
| Messenger | Контакты/block/history, durable inbox/outbox, повтор с тем же envelope id, потеря ACK, restart без второго денежного эффекта |
| Транспорты | Реальный testnet, loopback relay и предусмотренный Tor/direct-onion; явные флаги/входные данные live-наборов |
| UI/локализация | Диалоги и keyboard flow, ru/en catalog check, доступность/контраст, локальный scale; device-вывод отдельно |
| Поставка | Вне исходного дерева запускаются portable exe/selftest; присутствуют необходимые native assets/DLL, исходники, лицензии, хэши и проверенный SBOM |
| Зависимости | Native source provenance и hashes; RustSec findings разобраны, новых непринятых advisory нет |

Запуск live-набора без необходимых флагов/кошельков должен завершать проверку
ошибкой конфигурации, а не ранним PASS. Stub/mock подтверждает только свою
ветку, не доступность реальной ноды, транспорта или кошелька.

<a id="devices"></a>
## Физические проверки D-01…D-10

Сейчас все строки ниже **NOT VERIFIED на отдельных физических устройствах**.
Перед началом: закреплённый RC и хэши, disposable funded testnet-кошельки,
второй неадминистраторский Windows-аккаунт, отдельный NTFS/exFAT-том,
реальные мониторы, NVDA и IME. Секреты в evidence не записывать.

| ID | Проверка | Действие и ожидаемый результат | Свидетельство |
|---|---|---|---|
| D-01 | Чистая Windows | Распаковать RC, запустить, создать testnet-кошелёк: нет недостающих DLL, сеть видна | BUILD-INFO/hash, версия Windows, кадр |
| D-02 | Второй аккаунт | Запустить под другим локальным пользователем: независимые данные, чужие секреты недоступны | ACL и результат без содержимого секретов |
| D-03 | NTFS/exFAT | Создание/открытие/scan на поддерживаемом тестовом томе: явная политика ACL, нет тихой потери данных | ФС, ошибки/диалоги, restart |
| D-04 | Реальный disk-full | Матрица F-04 ниже: journal и wallet/LMDB на отдельных полных томах | Результат до/после, тот же S2/kernel |
| D-05 | Power-loss | Жёсткое отключение отдельного одноразового стенда на согласованных crash-границах | Событие отключения и восстановление |
| D-06 | Реальные дисплеи/DPI | 100/125/150%, mixed-DPI: действия не обрезаны, диалоги читаются | Кадры и параметры дисплеев |
| D-07 | NVDA | Unlock, send/receive, pending: понятные имена контролов и порядок фокуса | Краткая запись диктора |
| D-08 | IME/клавиатура | RU/EN, пароль/адрес/сумма: нет потери символов и случайной отправки; Tab не застревает | Сценарий/видео без секретов |
| D-09 | Буфер обмена | Полный адрес и armor копируются точно; проверка подмены; seed/пароль не копируются автоматически | Сверка значения/хэша, без seed |
| D-10 | Независимый restore | Чистый каталог на другом устройстве, тот же адрес, полный scan, ожидаемые средства | До/после, состояния scan и балансы |

PS-01→D-02; PS-02→D-03; PS-03→D-04/D-05; PS-04/PS-11→D-06…D-08;
PS-07→D-01/D-10; PS-12→D-09. Прежние PS-05 (GitHub jobs) имеют job-level свидетельства
в STATUS: success конкретного job не означает success всего cancelled run. Организационные/mainnet PS не превращаются в оставшуюся
реализацию этого RC. ID нужны для поиска истории Git, второй список задач не ведётся.

<a id="f04"></a>
## F-04: destructible disk-full / power-loss

Скрипт — [tools/f04-destructible.ps1](../tools/f04-destructible.ps1), контракт —
[ADR F-04](contracts/RECEIVE_RECOVERY.md). Подготовленный harness, process kill,
exit 9 и in-process failpoint не доказывают физический power-loss.

Использовать только отдельную одноразовую Windows VM/тестовый стенд, testnet и
копии disposable-кошельков. Рабочая нода, хостовые диски, mainnet и пользовательские
wallet data исключены. Все DataA/DataB/Work/evidence — внутри выбранного StandRoot.
Оператор создаёт `.destructible-f04-stand` со строкой `F04-DESTRUCTIBLE-TESTNET-VM`,
а в обоих data root — `.crash-test-ok` и `.destructible-f04-ok`.
Нода по loopback обязана подтвердить chain=test, sync_status=no_sync и высоту >0.

| Семейство | Backend | Вариант |
|---|---|---|
| DF-J | embedded и RPC | Полон только том журнала Flint |
| DF-W | embedded и RPC | Полон том wallet/LMDB получателя |
| Power-loss | embedded и RPC | После upstream return, до Flint journal: upstream |
| Power-loss | embedded и RPC | После Flint response journal, до armoring: journal |

Итого **8 независимых прогонов**, новый S1 в каждом. Используется закреплённый
binary с feature crash-injection; RPC — из того же recovery-capable fork.
`Preflight` проверяет стенд. Последовательности действий скрипта:

- `ArmPowerLoss -Scenario upstream|journal`, действие оператора,
  `ResumePowerLoss -StateFile ... -PowerCutConfirmed`.
- `PrepareDiskFull -Scenario DF-J|DF-W`, оператор заполняет выбранный отдельный
  том, `RunDiskFull -StateFile ...`, освобождение места,
  `ResumeDiskFull -StateFile ...`.

Общие параметры: `-Backend`, `-StandRoot`, `-DataA`, `-DataB`, `-PassA`, `-PassB`,
`-DestB`, `-NodeUrl`, `-NodeSecretFile`, `-Work`. Значения секретов не сохранять
в state/evidence/журнал. Скрипт сам не создаёт VHD, не выключает машину, не выдаёт
администраторские права и не подтверждает факт отключения.

После аварии не удалять LMDB/pending/recovery, не подписывать повторно и не
начинать новую сделку вместо восстановления. Для PASS обязательны тот же UUID
и сериализованный S2, ровно один output/TxReceived, отказ конфликтующему S1,
finalize/post того же S2 и нахождение kernel независимым node get_kernel с высотой.
Сохранить итоговые балансы и hashes неизменённых binaries. Для DF-J intent остаётся
до восстановления журнала; для DF-W допустим нулевой промежуточный TxReceived до
успешного commit. Варианты armor могут отличаться, сам response Slate — нет.

Неожиданный результат останавливает матрицу с сохранением disposable-стенда.
Новое S1 после аварии, ручная правка данных, смена binary или отсутствие evidence
делают прогон незавершённым. Общий F-04 не закрывается до этой матрицы.

## Учёт результатов

У каждой строки: дата, RC/hash, среда, наблюдаемый результат, PASS/FAIL/NOT VERIFIED,
путь к минимальному обезличенному свидетельству. Реальная проблема остаётся FAIL;
отсутствующий доступ/стенд — NOT VERIFIED. Сырые логи не заменяют вывод.
Удалять можно лишь явно обозначенные disposable roots после завершения проверки;
рабочие данные, fixtures и mainnet не сбрасываются.
