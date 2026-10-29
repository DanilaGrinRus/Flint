# F-04 — destructible-стенд: disk-full и power-loss

Дата подготовки: 2026-10-27  
Статус: **сценарий и harness подготовлены; реальные аварийные прогоны не выполнены**.
Этот документ не закрывает F-04 и не разрешает mainnet.

Проверяется восстановление одного входящего обмена S1 → S2 после реальной ошибки
носителя или жёсткого отключения одноразовой Windows VM. Обычный `exit 9`, process
kill и upstream in-process failpoint являются другими видами проверок и не заменяют
этот стенд.

## 1. Неснижаемые ограничения

- Только отдельная одноразовая Windows VM, только Grin **testnet**, только копии
  одноразовых кошельков. Исходные каталоги пользователя, mainnet, реальные средства,
  рабочая нода и хостовые диски не участвуют.
- Все wallet data и evidence располагаются внутри выбранного `StandRoot`. В корне
  оператор вручную создаёт `.destructible-f04-stand` с единственной строкой
  `F04-DESTRUCTIBLE-TESTNET-VM`.
- В каждом из двух data root оператор отдельно создаёт `.crash-test-ok` и
  `.destructible-f04-ok`. Harness не создаёт эти разрешения сам.
- Нода доступна только по loopback HTTP и обязана вернуть `chain=test`,
  `sync_status=no_sync` и ненулевую высоту. Harness не запускает и не останавливает её.
- Для каждого прогона используется новый S1. Нельзя засчитывать повтор одного
  результата как независимый сценарий.
- Seed, пароли, owner/node secrets и приватные ключи не копируются в state, evidence,
  журнал работ или сообщения. Пароли передаются только параметрам текущей команды.
- Harness не создаёт и не заполняет VHD, не раздаёт права администратора и не
  выключает машину. Наполнение тома и жёсткое отключение — отдельные действия
  оператора между фазами.
- После аварии запрещено удалять LMDB, pending, recovery record или создавать новую
  подпись/новую сделку вместо recovery. При неожиданном результате сохранить копию
  одноразового стенда и остановить матрицу.

## 2. Матрица и критерий PASS

| Семейство | Backend | Случаи |
|---|---|---|
| DF-J: полон только отдельный том Flint journal | embedded, RPC | по одному прогону |
| DF-W: полон выделенный том wallet/LMDB получателя | embedded, RPC | по одному прогону |
| Power-loss после upstream return, до Flint journal | embedded, RPC | `upstream` |
| Power-loss после Flint response journal, до armoring | embedded, RPC | `journal` |

Итого **8 независимых прогонов**. Для каждого после восстановления обязательны:

1. Тот же slate id и тот же сериализованный S2. Для `journal` S2 дополнительно
   сравнивается с сохранённым до аварии JSON; байты Slatepack armor не сравниваются.
2. Ровно один новый `TxReceived`; повторный receive не меняет S2 и не создаёт дубль.
3. Для DF-J во время полного журнала остаётся `ReceiveIntent`/`intent`, а upstream
   уже содержит ровно один `TxReceived`. После освобождения места повтор достаёт S2
   из upstream recovery и завершает Flint journal без новой подписи.
4. Для DF-W ошибка может произойти до видимого commit: нулевой промежуточный
   `TxReceived` сам по себе не является failure. После освобождения места итоговая
   проверка обязана показать ровно один и отсутствие повреждения прежнего состояния.
5. Sender finalize/post выполнен для того же S2; полный signed transaction имеет
   ненулевые kernel excess/signature.
6. Независимый node `get_kernel` находит kernel в цепи; фиксируется height.
7. Сохранены итоговые балансы обеих сторон. Наличие строки `posted` без on-chain
   kernel не является PASS.

Любой ручной обход, новый S1 после аварии, повторная подпись, изменение binary между
фазами или отсутствие evidence делает прогон незавершённым, а не PASS.

## 3. Подготовка VM и томов

### 3.1 Общая конфигурация

1. Создайте новую VM без доступа к исходным wallet data. Отключите общие папки хоста
   или предоставьте только отдельный каталог с исходниками и копиями кошельков.
2. Разверните закреплённый исходный снимок Flint, соберите debug examples с
   `crash-injection` и используйте RPC `grin-wallet` из Flint fork. Release без этой
   feature не подходит для pause-hook.
3. Запустите и полностью синхронизируйте отдельную testnet-ноду внутри VM. Порты и
   data directory существующей пользовательской ноды не используются.
4. Создайте два funded disposable wallet и выберите, например:
   `C:\F04-STAND` как `StandRoot`. Evidence и `Work` остаются на незаполняемом томе.
5. Для power-loss используйте fixed/virtual disk в режиме write-through без
   неподтверждённого volatile write cache. Хост должен позволять жёстко выключить
   именно эту VM и сохранить событие отключения.

Пример переменных (пути и пароли заменяются оператором, значения не записываются в
документацию):

```powershell
$stand = 'C:\F04-STAND'
$a = "$stand\sender"
$b = "$stand\receiver"
$passA = Read-Host 'Пароль disposable A'
$passB = Read-Host 'Пароль disposable B'
$destB = 'tgrin1...' # адрес именно disposable B
```

### 3.2 DF-J — только журнал на малом VHD

Нужен отдельный небольшой NTFS VHD, например `J:`. Он не должен содержать LMDB,
sender data, node data, state/evidence или исходники. Системный том запрещён.

1. Остановите все процессы disposable wallet B.
2. Скопируйте существующий каталог журнала B на `J:\pending` с сохранением файлов:
   - embedded: `$b\testnet\pending`;
   - RPC: `$b\rpc-pending`.
3. Внутри одноразовой копии переименуйте исходный каталог в `pending.pre-f04` и
   создайте junction с прежним путём на `J:\pending`. Не удаляйте исходник до конца
   проверки.
4. Убедитесь, что `Get-Item <journal>` показывает junction и абсолютный target на
   `J:`. Wallet B и sender A обязаны остаться на других томах.
5. `Preflight -Scenario DF-J` отклонит отсутствие junction, совпадение томов или
   расположение evidence на заполняемом томе.

Эта топология обязательна: заполнение всего wallet-тома не доказывает окно
«upstream commit состоялся, Flint journal записать нельзя».

### 3.3 DF-W — wallet/LMDB на отдельном VHD

Нужен отдельный NTFS VHD, например `W:`, содержащий **только disposable wallet B**.
Sender A, нода, `StandRoot`, state/evidence и исходники должны быть на других томах.

Разместите каталог B на `W:` и подключите его внутрь `StandRoot` junction'ом. Путь
`$b` остаётся лексически внутри `StandRoot`, но `Get-VolumeRoot` в harness увидит
фактический target. Системный том и общий том с sender запрещаются.

### 3.4 Как наполнять и освобождать том

Создайте на целевом VHD один заранее обозначенный non-sparse filler-файл и
увеличивайте его только внутри этого VHD до исчерпания свободного места. Не
используйте случайные пути, не наполняйте `C:` и не удаляйте wallet-файлы для
освобождения места.

`RunDiskFull` не доверяет показанию свободного места отдельно: требуется не более
1 MiB свободно **и** неудачная контрольная запись 2 MiB с `Flush(true)`. Если запись
удалась, сценарий останавливается без PASS.

После фазы ошибки удалите **только созданный filler-файл**. `ResumeDiskFull` требует
не менее 16 MiB свободно и успешную контрольную запись 1 MiB. Пороги относятся к
малому тестовому VHD и не являются рекомендацией для эксплуатации.

## 4. Команды harness

Все команды выполняются из корня исходников внутри VM. Сначала можно выполнить
безаварийный preflight; он не создаёт сделку:

```powershell
pwsh -File tools/f04-destructible.ps1 -Action Preflight `
  -Backend embedded -Scenario DF-J -StandRoot $stand `
  -DataA $a -DataB $b -DestB $destB
```

Для RPC замените `-Backend embedded` на `-Backend rpc`. Каждая новая фаза
`ArmPowerLoss`/`PrepareDiskFull` собирает examples с `crash-injection`, если не задан
`-SkipBuild`. Resume не пересобирает binary и отклоняет изменение SHA-256.

### 4.1 DF-J или DF-W

```powershell
# 1. Создать S1/state. Для DF-J также записать intent и выйти до wallet mutation.
pwsh -File tools/f04-destructible.ps1 -Action PrepareDiskFull `
  -Backend embedded -Scenario DF-J -StandRoot $stand `
  -DataA $a -DataB $b -PassA $passA -PassB $passB -DestB $destB

# 2. Оператор заполняет только FullTarget, указанный в state.

# 3. Подтвердить ошибку на полном томе. PASS здесь не выводится.
pwsh -File tools/f04-destructible.ps1 -Action RunDiskFull `
  -StateFile '<run-dir>\state.json' -PassA $passA -PassB $passB

# 4. Оператор удаляет только filler и освобождает место.

# 5. Повторить тот же S1, проверить S2/дубли/finalize/post/kernel.
pwsh -File tools/f04-destructible.ps1 -Action ResumeDiskFull `
  -StateFile '<run-dir>\state.json' -PassA $passA -PassB $passB
```

Для DF-J фаза Prepare специально вызывает `FLINT_CRASH_AFTER_RECEIVE=intent`:
журнал уже содержит durable intent, а `TxReceived` ещё отсутствует. Этот exit 9 —
только подготовка границы, не disk-full evidence. Без состояния
`intent_armed_before_disk_full=true` последующий RunDiskFull отклоняется. Повтор на
полном журнале использует тот же S1, выполняет upstream commit и только затем получает
ошибку записи response. Для DF-W этот preparatory crash не нужен.

Для DF-W используйте `-Scenario DF-W` при Prepare. Resume берёт конфигурацию из state;
повторно передавать backend/пути не требуется.

### 4.2 Power-loss `upstream` или `journal`

```powershell
# 1. Создать S1 и остановиться в pause-hook после durable marker.
pwsh -File tools/f04-destructible.ps1 -Action ArmPowerLoss `
  -Backend embedded -Scenario upstream -StandRoot $stand `
  -DataA $a -DataB $b -PassA $passA -PassB $passB -DestB $destB
```

Дождаться `READY FOR OPERATOR POWER CUT`. Не нажимать Ctrl+C, не завершать wallet и не
выполнять recovery до отключения. Проверить по state:

- `status=ready_for_power_cut`;
- slate id, stage, binary hashes, pause marker и его SHA-256;
- для `journal` сохранён S2 до аварии;
- зафиксированы `boot_before` и `ready_for_power_cut_utc`.

2. **С хоста** ещё раз идентифицировать одноразовую VM и выполнить жёсткое отключение
   питания именно её. Не использовать shutdown внутри гостя, graceful restart,
   suspend, checkpoint или process kill. Записать VM id, хостовое время и событие
   отключения в отдельный несекретный протокол стенда.

3. Загрузить VM, не запуская другие операции кошелька, и выполнить:

```powershell
pwsh -File tools/f04-destructible.ps1 -Action ResumePowerLoss `
  -StateFile '<run-dir>\state.json' -PassA $passA -PassB $passB `
  -PowerCutConfirmed
```

`ResumePowerLoss` требует новый boot, неизменный pause marker и явное подтверждение
оператора. `PowerCutConfirmed` — аттестация действия оператора; технически отличить
жёсткое отключение от graceful reboot внутри гостя невозможно, поэтому хостовый
протокол обязателен для окончательного аудита.

Повторите с новым S1 для `journal`, затем для RPC в обеих точках.

## 5. Evidence и разбор failure

`state.json` обновляется durable-записью и содержит только несекретные данные:
backend/scenario, конфигурацию томов, tip ноды, SHA-256 binary, slate id, S2 hash,
счётчики `TxReceived`, boot/marker timestamps, kernel excess/height и балансы. stdout
и stderr CLI сохраняются в run-dir; перед передачу внешнему аудитору их нужно
проверить на присутствие чувствительных путей или локальных данных.

Успешная строка harness:

```text
PASS: backend=... scenario=... same_S2=true one_TxReceived=true kernel_height=...
```

Она относится только к одному state и одному снимку binary. Для закрытия гейта нужны
все 8 state с `status=passed`, хостовые события четырёх power-cut и подтверждение
топологии/заполнения четырёх disk-full. Итог заносится в `docs/KRead.md`,
`docs/status/STATUS.md` и `docs/WORK_LOG.md` отдельно от подготовки harness.

При failure не повторять слепо: сохранить state, pause marker, журнал и копию VM,
зафиксировать последнюю успешную фазу. `ResponseLost`, повреждённый S2, второй
`TxReceived`, нулевой kernel или невозможность открыть LMDB после освобождения места
являются блокерами денежного recovery.

## 6. Что уже проверено без аварийного стенда

Подготовка harness, синтаксис PowerShell, unit-тесты pause marker и обычные regression
проверяются на рабочей машине без заполнения диска и без выключения. Эти результаты
подтверждают только тестируемость сценария и fail-closed guards. Они не являются
реальным disk-full/power-loss и не закрывают F-04.

## 7. Принятое сокращение проверки без VM

По решению владельца от 2026-10-27 дальнейшие VM-испытания в этой итерации прекращены.
Вместо них выполнен `tools/crash-test-receive.ps1 -DiskFullOnly`: feature-gated hook
возвращает `ERROR_DISK_FULL=112` только перед записью response journal, после upstream
commit. Для embedded и RPC проверены intent, восстановление того же S2 без повторной
подписи, один `TxReceived`, повторный receive, finalize/post и on-chain kernel.
Power-loss представлен ранее пройденными process exit 9/restart сценариями.

Это принятое владельцем сокращение объёма, а не выполнение матрицы из раздела 2.
Физический полный том, отключение питания, DF-W, топология VHD, новый boot и хостовые
power-cut события не проверялись. Формальный destructible-гейт остаётся невыполненным;
mainnet это не разрешает. Evidence и точные оговорки — запись 14 в
`docs/WORK_LOG.md` и `.tmp/crash-receive-simulation/`.
