# ADR F-04 — атомарное восстановление receive response

Решение принято 2026-10-27. ID **F-04** сохраняется после консолидации документов.
Область: embedded, managed/external grin-wallet RPC, receive journal и recovery.
Этапы upstream/Flint реализованы в закреплённом fork; process/live-проверки отражены
в [STATUS.md](../STATUS.md). Общий физический F-04 остаётся открытым до матрицы
[ACCEPTANCE.md](../ACCEPTANCE.md#f04), а не закрывается наличием этого ADR.

## Контекст и выбранное решение

Получатель S1→S2 меняет wallet state внутри upstream. Файловый журнал Flint после
возврата receive_tx не образует с LMDB общей транзакции: crash возможен до/после
любого внутреннего commit. Поэтому Foreign::receive_tx обязан сохранять **исходный
S1, настоящий S2, recipient output и TxLogEntry в одном атомарном LMDB commit**.
Повтор того же запроса возвращает сохранённый S2 без новой подписи/output/tx-log.

Fork основан на grin-wallet 5.5.1, commit
`11046cd197f8296e6cd9313502a9f2cbce6e262a`, grin
`dbaff34f7751ee23316fbbfe5806b47c546776ca`.
Канон: `third_party/GRIN_WALLET_FORK.toml`, source manifest и patch.
Историческая интеграционная контрольная точка:
`20261027-f04-integration-candidate-v3`; текущая идентичность поставки — её manifest.

## Неизменяемые свойства

1. Весь интервал от первой потенциальной мутации покрыт recovery, не только return.
2. После видимой мутации получить тот же сериализованный S2 возможно без подписи.
3. Повтор S1 не создаёт второй output/TxReceived; UUID одного недостаточно.
4. Проверяются account, полный канонический S1, UUID и согласованность wallet state.
5. S2 сохраняется до armoring и возврата транспортом.
6. Ошибка журнала Flint после upstream commit не теряет response.
7. Старый/битый backend без recovery не получает неявный повтор receive.
8. Armor может различаться из-за шифрования; response Slate остаётся тем же.
9. Seed, пароль, секретный Context/nonce/key не попадают в journal.
10. Embedded и RPC имеют одинаковые инварианты и самостоятельные проверки.

## Альтернативы, которые не дают гарантии

| Вариант | Почему отклонён |
|---|---|
| Только durable intent/response во Flint | Остаётся gap между внутренним upstream commit и файлом |
| Receive снова после duplicate/error | Возможна новая подпись либо отказ без прежнего S2 |
| get_stored_tx | Возвращает агрегированную транзакцию/Standard3, не настоящий Standard2 получателя |
| Response записать до wallet mutation | Можно выдать ответ для неучтённого output |
| Callback из upstream в файловый журнал | LMDB и обычный файл не становятся одной атомарной транзакцией |
| Копировать crypto/receive во Flint | Расхождение инвариантов с upstream и дополнительный код подписи |
| Собственный backend с отложенными commit | Существенно сложнее поддерживать, чем ограниченный upstream patch |

## Upstream record и атомарная граница

Логический ReceiveRecoveryV1 хранит `parent_key_id`, `slate_id`,
`request_slate_v1`, `response_slate_v1`. Ключ включает account+UUID. Запрос
сравнивается целиком после общей канонической сериализации; hash не заменяет S1.
Формат Slate берётся из поддерживаемого upstream VersionedSlate, собственный
несовместимый формат не вводится.

До commit проверяются S1/TTL/account и существующие recovery/tx-log. Output,
private context и S2 подготавливаются без промежуточных commits standard receive.
Один batch сохраняет output, TxReceived с kernel/state и recovery; только после
успешного commit S2 возвращается вызывающему. Invoice/finalize этим ADR не меняются.

- Есть recovery, совпадает S1 и wallet state: вернуть прежний S2.
- Есть recovery, но S1/account/state конфликтует: typed integrity error без мутации.
- Есть старый TxReceived, recovery отсутствует: ReceiveRecoveryUnavailable,
  без повторной подписи.
- Отмена/неоднозначность: fail closed.
- Нет видимой мутации: возможен первый receive через atomic path.

Неоднозначный результат commit разрешается чтением recovery/tx-log на следующем
вызове; отсутствие ответа RPC не является разрешением подписать снова.

## Capability и журнал Flint

Foreign check_version объявляет `receive_response_recovery >= 1`. Номер Foreign
API v2 не заменяет capability. Embedded получает её из закреплённого кода,
managed RPC — из того же fork binary. Внешний RPC без capability отклоняет новый
receive **до мутации**, без скрытого legacy-mode. Уже сохранённый валидный S2
можно armorить без нового receive.

Flint сначала durably пишет ReceiveIntent, после успешного receive — S2 в journal,
после — armor/ответ. При intent повтор передаёт прежний S1 upstream и завершает
journal из recovery. ReceiveRecoveryUnavailable/integrity error ведут к
ResponseLost/диагностике, без automatic retry/cancel/new trade. Обычная ошибка до
мутации обрабатывается по R-01. Corrupt journal не считается отсутствующим.

## Миграция и согласованность

| Сохранённое состояние | Действие |
|---|---|
| ResponseReceived и валидный S2 | Использовать journal, без нового receive |
| ReceiveIntent, мутации нет | Первый receive через recovery-capable backend |
| ReceiveIntent, upstream recovery существует | Получить тот же S2 и завершить journal |
| Старый TxReceived без recovery | Диагностика/manual recovery, новая подпись запрещена |
| Конфликт/повреждение | Сохранить исходные данные, fail closed |

Старый ResponseLost не объявляется исправленным установкой программы. Нельзя
удалять wallet state: при согласовании с отправителем отменяют старую сделку и
создают новую с новым UUID только как отдельное явное действие, не auto recovery.
Формат нового поля journal должен читаться обратно совместимо или иметь миграцию.

## Матрица отказов

| Точка | После restart |
|---|---|
| До intent | Мутации нет |
| После intent, до upstream | Intent существует, первый receive допустим |
| После подготовки S2, до LMDB commit | Нет output/TxReceived/recovery; повтор даёт одну операцию |
| После commit, до upstream return | Тот же S1 возвращает S2, ровно один output/TxReceived |
| После return, до Flint rename | Recovery завершает журнал |
| ACL/disk-full журнала | Сохранённый upstream S2 доступен без новой подписи |
| После Flint response, до armor | Armor прежнего S2 без receive |
| Тот же UUID, иной S1 | Отказ без мутации и выдачи чужого ответа |
| RPC без capability | Отказ до wallet mutation |

Проверяются сериализованный S2, output, число TxReceived, account/wrong-S1,
embedded/RPC process kill, timeout после commit и физические 8 сценариев.
Live recovery завершается finalize/post и on-chain kernel, а не строкой posted.

## Поставка и исходники

Нельзя подменять fork скачанным официальным неизменённым grin-wallet и называть
его recovery-capable. Native hashes, SBOM, source package и GPL notices включают
fork/patch/provenance. Исходники закрепляются явно, не молчаливой правкой vendor.
Сохранение полного signed transaction для sender reconcile — отдельная гарантия
F-01, а не попытка получить recipient S2 через get_stored_tx.

Источники исходной модели upstream 5.5.1:
[foreign](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/api_impl/foreign.rs),
[owner](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/api_impl/owner.rs),
[selection](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/internal/selection.rs),
[tx](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/internal/tx.rs).
Это происхождение решения, не новый live-прогон этой документационной уборки.
