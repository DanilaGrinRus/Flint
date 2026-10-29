# ADR F-04 — атомарное восстановление receive response

Дата: 2026-10-27  
Статус: **этапы A/B реализованы; upstream commit `11046cd197f8296e6cd9313502a9f2cbce6e262a`, снимок Flint `20261027-f04-integration-candidate-v3`; этап C и общий F-04 остаются открытыми P1**  
Область: embedded `grin_wallet_libwallet`, управляемый и внешний RPC `grin-wallet`,
receive-журналы Flint, crash/restart recovery.

## 1. Решение

Использовать поддерживаемый Flint fork `grin-wallet` на базе **5.5.1** и сделать
`Foreign::receive_tx` идемпотентным для одного и того же входящего Slate.

Upstream должен сохранять исходный запрос S1 и настоящий ответ S2 в **одном
атомарном LMDB-commit вместе с recipient output и TxLogEntry**. Повторный вызов с
тем же S1 обязан вернуть сохранённый S2 без новой подписи, нового output или новой
записи в tx-log. Другой Slate с тем же UUID должен быть отклонён.

Журнал Flint остаётся вторым, независимым слоем: он хранит получателя, сумму,
пользовательское состояние и response для повторного armoring. Он не заменяет
upstream recovery.

Простая обёртка вокруг существующего `receive_tx`, повторный receive после
`TransactionAlreadyReceived` и восстановление через `get_stored_tx` решением не
являются.

## 2. Контекст и доказательства

### 2.1 Текущий Flint

В embedded `service.rs` сначала вызывается `foreign.receive_tx`, затем
`save_pending(ResponseReceived)`. В RPC `rpc_backend.rs` сначала выполняется
`foreign_call("receive_tx")`, затем `save_receive(received)`. Durable intent уже
сохраняется до вызова; обычная ошибка оставляет повтор, а явный
`TransactionAlreadyReceived` переводит запись в `ResponseLost`.

Если процесс исчезнет после мутации кошелька, но до durable response в Flint,
повторный вызов upstream не возвращает прежний S2. Это мешает завершить конкретный
интерактивный обмен и может оставить входы отправителя заблокированными до отмены.
Не доказана необратимая потеря seed или монет.

### 2.2 Граница шире одного `return`

В `grin-wallet` 5.5.1 `selection::build_recipient_output` делает commit recipient
output и `TxReceived` ещё до завершения построения S2. Затем
`add_output_to_slate` отдельно обновляет kernel excess, а
`update_tx_slate_state` отдельно сохраняет состояние Slate.

Следовательно, сохранение response только «сразу после возврата receive_tx» или
только в последнем commit оставляет более ранние crash-окна. Реализация обязана
охватывать **весь интервал от первой потенциальной мутации**, а не только точку
между `receive_tx` и записью журнала Flint.

### 2.3 Почему `get_stored_tx` не подходит

`get_stored_tx` в 5.5.1 читает сохранённую агрегированную транзакцию и создаёт Slate
в состоянии `Standard3`. Ответ получателя — `Standard2` с его публичными данными
подписи. Первая операция не восстанавливает вторую; подменять S2 таким результатом
запрещено.

Источники upstream указаны в разделе 13. Это статическая проверка версии 5.5.1,
а не живой testnet-эксперимент.

## 3. Требуемые свойства

1. После любой видимой мутации receive существует способ получить тот же S2.
2. Ни один crash/restart не вызывает повторную подпись получателя.
3. Повтор одного S1 не создаёт второй output или второй `TxReceived`.
4. Совпадения UUID недостаточно: проверяются аккаунт и полный исходный Slate.
5. S2 сохраняется до попытки armoring и до возврата ответа транспортом.
6. Ошибка записи журнала Flint после upstream commit не делает ответ потерянным.
7. Старый или повреждённый backend без recovery не получает неявный повтор receive.
8. Повторное armoring может отличаться байтами из-за шифрования; неизменным должен
   быть именно сериализованный response Slate, а не Slatepack armor.
9. Seed, пароль, секретный nonce и секретный ключ в журнал Flint не попадают.
10. Embedded и RPC имеют одинаковую модель восстановления и приёмочную матрицу.

## 4. Рассмотренные альтернативы

| Вариант | Результат | Причина |
|---|---|---|
| Только durable intent/response во Flint | Отклонено | Между внутренним commit upstream и журналом остаётся crash gap |
| Повторять `receive_tx` после любого duplicate | Отклонено | Текущий upstream возвращает ошибку; новая подпись без recovery недопустима |
| Восстанавливать через `get_stored_tx` | Отклонено | Возвращается `Standard3`, а не исходный `Standard2` |
| Писать response перед wallet mutation | Отклонено | После неуспешной мутации можно выдать ответ для неучтённого output; проблема согласованности остаётся |
| Callback из upstream в файловый журнал Flint | Отклонено как основная гарантия | LMDB и файл нельзя считать одной атомарной транзакцией без отдельного протокола восстановления |
| Копировать receive/аггрегацию подписей во Flint | Отклонено | Дублирование криптографического и wallet-state кода увеличивает риск расхождения с upstream |
| Собственный WalletBackend с отложенными commit | Не выбрано | Требует воспроизвести внутренние инварианты LMDB и receive; поддержка сложнее точечного upstream-патча |
| Идемпотентный upstream receive с LMDB recovery | **Принято** | Response и wallet state получают общую атомарную границу и общий путь для embedded/RPC |

## 5. Архитектура upstream-патча

### 5.1 Recovery record

В wallet DB добавляется отдельное keyspace/запись, версионируемая независимо от
Flint JSON. Логическая схема:

```text
ReceiveRecoveryV1 {
    parent_key_id,
    slate_id,
    request_slate_v1,       // каноническая версионированная сериализация исходного S1
    response_slate_v1       // каноническая версионированная сериализация настоящего S2
}
```

Ключ включает `parent_key_id` и `slate_id`, чтобы self-send и разные аккаунты не
пересекались. Конкретное бинарное представление выбирается при реализации из уже
поддерживаемых upstream `Slate`/`VersionedSlate` форматов; несовместимый собственный
формат Slate вводить нельзя.

Полный сохранённый запрос сравнивается с новым запросом побайтно после общей
канонической сериализации. Один UUID с другим содержимым — ошибка целостности, а не
разрешение выдать response. Хеш может использоваться дополнительно для диагностики,
но не вместо сохранённого запроса.

### 5.2 Одна атомарная граница

Для standard receive подготовка output отделяется от commit:

1. Проверить S1, TTL, аккаунт и существующий recovery/tx-log.
2. Построить recipient output, private context и криптографический S2 в памяти.
3. Не выполнять промежуточные LMDB-commit из старого
   `build_recipient_output`/`add_output_to_slate` на этом пути.
4. В одном batch сохранить:
   - recipient `OutputData`;
   - `TxReceived` с kernel excess и состоянием S2;
   - `ReceiveRecoveryV1` с исходным S1 и ответом S2.
5. Выполнить один `batch.commit()` и только после него вернуть S2.

Invoice и finalize в этом ADR не переписываются. Общие upstream helpers можно
рефакторить только при сохранении их прежнего поведения на остальных путях.

До commit повтор начинает receive заново: видимой мутации ещё нет. После commit
повтор сначала находит recovery и возвращает тот же S2. Ошибка с неоднозначным
результатом commit разрешается тем же поиском recovery/tx-log при следующем вызове,
а не новой подписью.

### 5.3 Идемпотентный повтор

`receive_tx` сохраняет текущую сигнатуру API, но меняет обработку повтора:

```text
если recovery(аккаунт, slate_id) существует:
    если исходный S1 совпадает и wallet state согласован:
        вернуть сохранённый S2
    иначе:
        вернуть typed integrity/recovery error
иначе если TxReceived уже существует:
    вернуть ReceiveRecoveryUnavailable; не подписывать повторно
иначе если запись отменена или состояние неоднозначно:
    отклонить операцию; не подписывать повторно
иначе:
    выполнить новый receive через единый atomic commit
```

Перед выдачей recovery проверяются наличие соответствующего `TxReceived`, аккаунт,
UUID и согласованность output/tx-log. Запись без wallet state или wallet state без
recovery не «чинится» повторной подписью.

`get_stored_tx` не меняет смысл и не участвует в receive recovery.

## 6. Capability и совместимость RPC

`check_version` расширяется обратно совместимым optional/default полем capabilities.
Первая capability: `receive_response_recovery = 1`. Само число Foreign API v2 без
этой capability ничего не доказывает.

- **Embedded:** capability обеспечивается компиляцией с пропатченным fork.
- **Управляемый RPC:** в пакет включается `grin-wallet` из того же закреплённого
  fork/commit; checksum и SBOM обновляются.
- **Внешний RPC:** Flint вызывает `check_version` до нового receive. Без capability
  новый приём отклоняется до wallet mutation с понятной русской ошибкой.
- Уже сохранённый `ResponseReceived` можно повторно armorить и без новой capability:
  вызова receive и новой подписи в этом пути нет.
- Старые `ReceiveIntent`/`ResponseLost` не объявляются восстановленными только из-за
  обновления программы.

Отказ старого внешнего RPC от нового receive — осознанный fail-closed компромисс.
Скрытого «legacy mode» с прежним crash gap не будет.

## 7. Изменения Flint после upstream

1. Оставить durable `ReceiveIntent` до вызова.
2. После успешного вызова сохранить S2 в Flint journal до armoring, как сейчас.
3. При `ReceiveIntent` повторять тот же исходный S1; patched upstream вернёт тот же
   S2 либо выполнит первый receive, если commit не состоялся.
4. `ReceiveRecoveryUnavailable` и integrity errors переводить в `ResponseLost`/
   отдельное диагностическое состояние без автоматического retry, cancel или новой
   сделки.
5. Обычные ошибки до мутации сохраняют retry согласно R-01.
6. Ошибка Flint journal после успешного upstream commit оставляет `ReceiveIntent`;
   следующий повтор восстанавливает S2 из upstream и повторяет запись журнала.
7. Не добавлять секретный `Context`, nonce или ключи в JSON Flint.

Схему журналов желательно сохранить. Если для typed diagnostics потребуется поле,
добавляется optional `journal_version`/код ошибки с backward-compatible чтением;
это отдельная миграция, не повод менять существующие записи молча.

## 8. Миграция и старые аварийные записи

| Исходное состояние при обновлении | Действие |
|---|---|
| `ResponseReceived` с валидным S2 | Продолжить использовать Flint journal |
| `ReceiveIntent`, wallet mutation отсутствует | Выполнить новый receive уже с recovery |
| `ReceiveIntent`, upstream имеет recovery | Вернуть сохранённый S2 и завершить журнал |
| Старый `TxReceived`, recovery отсутствует | `ResponseLost`/manual recovery; повторная подпись запрещена |
| Повреждённые/конфликтующие записи | Fail closed, сохранить данные для диагностики |

Для старого `ResponseLost` автоматического восстановления нет. Runbook: не удалять
wallet data, не повторять подпись, согласовать с отправителем отмену старой сделки и
создание новой с новым UUID. Автоматическая отмена по одной ошибке журнала запрещена.

## 9. Crash/fault-матрица

Failpoint требуется не только во Flint, но и вокруг upstream commit.

| Точка | Ожидаемый результат после restart |
|---|---|
| До Flint intent | Мутации нет; новый receive допустим |
| После intent, до upstream | Есть только intent; новый receive допустим |
| После подготовки S2, до LMDB commit | Нет output/TxReceived/recovery; повтор создаёт одну операцию |
| Сразу после LMDB commit, до возврата upstream | Тот же S1 возвращает тот же S2; ровно один output и TxReceived |
| После upstream return, до temp/rename журнала Flint | Повтор восстанавливает S2 из upstream; журнал завершается |
| Ошибка ACL/disk-full журнала после commit | S2 остаётся доступен; повторная подпись отсутствует |
| После Flint response, до armoring/ответа UI | Повторное armoring того же S2; receive не вызывается |
| Тот же UUID, изменённый S1 | Отказ без мутации и без выдачи чужого response |
| Старый RPC без capability | Отказ до wallet mutation |

Для каждого recovery сравниваются сериализованный S2, recipient output и число
`TxReceived`. Сравнивать байты двух зашифрованных Slatepack armor не требуется.

Обязательные наборы:

- upstream unit/integration: commit до/после, duplicate, wrong-S1, account isolation;
- embedded Flint: process kill/restart и ошибки журнала;
- RPC Flint: kill `flint`/restart, kill/restart `grin-wallet`, timeout после commit;
- destructible-стенд: реальный disk-full и power-loss по
  [runbook](../operations/DESTRUCTIBLE_F04.md); подготовка harness не закрывает этот гейт;
- живой testnet: receive → crash → тот же S2 → finalize → post → confirmation;
  отдельно реальный RPC receive, уже требуемый F-03/F-04.

## 10. Поставка, offline и лицензии

1. Fork фиксируется полным commit SHA; база `v5.5.1` и список патчей записываются в
   релизные материалы. Ожидание принятия upstream PR не требуется для сборки Flint,
   но патч следует предложить upstream.
2. Rust crates подключаются через явный `[patch]`/path на закреплённый исходник,
   а не через непроверенную замену файлов в `vendor/`.
3. `Cargo.lock`, offline source package и source snapshot обновляются; новые
   сторонние зависимости требуют отдельного `cargo vendor` и лицензионной проверки.
4. Для RPC больше нельзя скачивать неизменённый официальный 5.5.1 и называть его
   recovery-capable. `tools/fetch-wallet.ps1` заменяется воспроизводимой сборкой/
   получением подписанного Flint fork артефакта с независимым SHA-256.
5. `native-assets.sha256`, SBOM, THIRD_PARTY_NOTICES и source package включают fork.
   Apache-2.0 upstream уведомления сохраняются; лицензия Flint не меняется.
6. До выполнения этих пунктов release и RPC-конфигурация не считаются закрытыми по
   F-04, даже если embedded regression уже проходит.

## 11. Этапы реализации

### Этап A — upstream fork

- Добавить recovery record и единый atomic commit standard receive.
- Сделать duplicate идемпотентным, добавить typed recovery/integrity errors.
- Добавить capability и upstream crash/fault-тесты.
- Зафиксировать commit, источники, лицензии и сборочную инструкцию.

### Этап B — интеграция Flint

- Подключить fork в embedded и собрать RPC binary из него.
- Добавить проверку capability, typed mapping и recovery regression во Flint.
- Обновить migration/runbook, package, SBOM и source snapshot.
- Прогнать fmt/check/workspace tests/build/selftest без изменения wallet data.

### Этап C — живые гейты

- Выполнить crash-матрицу обоих backend на одноразовых testnet-кошельках.
- Провести disk-full/power-loss на destructible-стенде.
- Завершить реальный receive/finalize/post/confirm и независимо сверить результат.
- Только после этого отметить F-04 закрытым; человеческий аудит остаётся отдельным
  внешним гейтом.

## 12. Последствия

Положительные:

- одна гарантия покрывает embedded и RPC;
- crash после commit больше не требует новой подписи;
- журнальная ошибка Flint становится повторяемой операцией записи, а не потерей S2;
- старые конфигурации обнаруживаются явно.

Издержки:

- появляется поддерживаемый fork и обязательство переносить патч при обновлениях;
- wallet DB получает новый версионируемый формат и дополнительное хранилище S1/S2;
- старый внешний RPC теряет возможность нового receive до обновления;
- recovery нельзя задним числом создать для уже потерянных ответов.

В первой версии recovery не удаляется автоматически по времени: отправитель может
повторно запросить S2, пока сделка не завершена. Политика очистки подтверждённых/
отменённых записей требует отдельного безопасного retention-решения и не должна
задерживать основной патч.

## 13. Источники

Локальный код:

- `crates/core/src/service.rs` — embedded receive/resume и pending journal;
- `crates/core/src/rpc_backend.rs` — RPC receive/resume, capability, journal;
- `crates/grin-wallet-rpc/Cargo.toml` — сборка неизменённого upstream binary в
  общем lock/vendor Flint;
- `third_party/GRIN_WALLET_FORK.toml` и `grin-wallet-sources.sha256` — commits и
  проверка исходников без `.git`;
- `tools/fetch-wallet.ps1` — проверяемая release-сборка RPC из fork, не загрузка
  официального binary.

Upstream 5.5.1:

- [foreign receive implementation](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/api_impl/foreign.rs);
- [owner `get_stored_tx`](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/api_impl/owner.rs);
- [recipient output commit](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/internal/selection.rs);
- [receive/output helpers](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/libwallet/src/internal/tx.rs);
- [Foreign API boundary](https://github.com/mimblewimble/grin-wallet/blob/v5.5.1/api/src/foreign.rs).

Связанные документы: `AUDIT_EXTERNAL_2026-10-27.md` F-04,
`AUDIT_RECHECK_2026-10-27.md` F-04/R-01, `SENTINEL_AUDIT_2026-10-27.md` §4,
`../KRead.md` этап 1 и `../PLAN.md` §1.3.
