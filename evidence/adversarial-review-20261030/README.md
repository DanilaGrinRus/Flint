# Проверка устойчивости Flint — предварительный разбор 2026-10-30

Матрица составлена по коду и существующим тестам; это не прогон всех сценариев.
Статусы: PASS — проверено; PARTIAL — механизм есть, сценарий не доказан;
NOT VERIFIED — проверки нет.

| Сценарий | Статус | Что есть | Чего не хватает |
|---|---|---|---|
| Откат данных в прошлое | NOT VERIFIED | попытка сверки входов S1 откатана: compact S1 не хранит inputs, полный набор есть только в upstream-контексте; иначе блокировались все отправки | нужен источник входов до компактирования + живой backup→send→restore drill |
| Два Flint над одной БД | PASS | именованный mutex `Local\FlintWalletSingleInstance` + эксклюзивный `<data>/flint.lock` (share=0) на весь процесс; второй процесс/алиас не открывает каталог; тест `data_root_lock_is_exclusive` | другой пользователь и RPC-профиль не проверены |
| Подмена после подтверждения | PASS (механизм + e2e S1) | маркер `SendConfirmation` с digest S1 и контекстом; finalize сверяет токен, network/index/destination/amount/fee; legacy pending fail-closed; автофинализация отправок отключена | есть e2e-подделка S1 с тем же UUID; нет живого окна с переключением профиля |
| Ложный localhost | PASS (политика, граница доверия) | chain/version identity, managed secret только loopback, внешний plain HTTP отклоняется; в SECURITY явно зафиксировано, что loopback — граница доверия того же пользователя | криптографической идентичности ноды нет и быть не может без изменения протокола; same-user процесс уже имеет доступ к данным |
| Две копии профиля | PASS (одна машина/пользователь) | per-address lock `%LOCALAPPDATA%\Flint\locks`; вторая копия получает clone warning, отправка/подпись блокируются | другая машина/пользователь и клон с другим адресным индексом не обнаруживаются |
| Несовместимый откат версии | PASS (защита новых версий) | `DATA_ROOT_CONFIG_VERSION`; битые pending не удаляются, а показываются (`broken_pending_files_are_reported_not_dropped`) | нет явной версии формата журнала/истории; поведение старого бинаря на новых данных не проверено |
| Нода догоняет цепь | PASS (политика) | Headers/Blocks → только после явного чекбокса; `send_sync_gate` в core, RPC и UI; Starting/Unknown/NotRunning запрещены | нет живого окна с реально догоняющей нодой |\n| Прыжки системного времени | PASS | срок ограничен `MAX_TTL_SECS` от получения; будущий timestamp отправителя не продлевает хранение; тест `future_clock_cannot_extend_storage` | отдельного теста «часы назад» нет; дедуп по id остаётся |
| Разные представления пути | PASS (single-instance) | mutex глобальный; проверено: второй процесс через junction завершился; ACL на данных | сетевой путь, другой пользователь, каноникализация AppPaths не проверены |
| «Честный, но противоречивый» relay | PARTIAL | dedup/ack/replay, durable outbox; повторная доставка после ack не возвращается | нет теста «старый ответ после нового», «подтвердил и скрыл», «повтор старого конверта после restore» |
| Показ vs подпись | PARTIAL | суммы — целые nano, `format::amount` и `parse_nano`, фиксированные 9 знаков; тест `nano_is_formatted_with_nine_decimals` | нет кнопки Max; запятая отклоняется; локали/округление комиссии не сопоставлены с подписанным значением |

## Выполненные проверки

- `cargo test -p flint-core --lib reconcile` — 2 passed.
- `cargo test -p flint-messenger store::tests` — 18 passed.
- `cargo test -p flint-slint` — 21 passed.
- Два экземпляра: data-root lock (`data_root_lock_is_exclusive`); вживую первый запущен, второй с тем же каталогом и через junction
  `mklink /J` завершился; в процессах остался один.

Полный workspace на момент разбора: 358 passed / 0 failed / 33 ignored.
## NIP-42 live acceptance

Проверено с текущим клиентом (kind 1059, только чтение):
`relay.damus.io` присылает AUTH, но отвечает своей ошибкой
`relay needs serviceUrl to be configured before AUTH can work`;
`relay.snort.social`, `relay.primal.net`, `nos.lol` не требуют AUTH для
анонимного фильтра; `relay.nostr.bg` — resolve timeout, `nostr21.com`,
`relay.nostrcheck.me`, `relay.nostr.net`, `nostr.mom` — connect timeout.
Публичный relay с рабочим AUTH в этом окружении не найден: live acceptance
остаётся **NOT VERIFIED**, клиентская часть закрыта loopback-тестом.

## Что не прогонялось

- Матрица disk-full / power-loss для денежных операций: embedded crash-suite (upstream/journal, process kill) PASS, см. `evidence/crash-receive-20261030/`; реальное заполнение диска и power cut — вручную.
- Полный набор гонок «операция → переключение профиля → поздний ответ» (generation-проверки есть, матрица не прогонялась).
- Реальный второй пользователь Windows и сетевые пути (single-instance/junction проверены).
- Живой backup→send→restore drill (проверка stale pending только тестами).
