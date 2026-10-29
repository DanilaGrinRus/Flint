# Триаж ревью 0.1.68 в дереве 0.1.69

> **HISTORICAL:** документ относится к периоду до удаления Tauri (2026-10-26). Ссылки на `crates/app`/`ui/` и старые `--selftest`/UI-проверки — исторические.


Источник: внешний разбор `FLINT_0.1.68_REVIEW_RU.md` (архив 0.1.68). Текущее
дерево — 0.1.69, без релиза. Документ фиксирует, что из ревью уже закрыто, что
осталось и в каком порядке это брать.

## 1. Статус девяти основных пунктов

| № | Пункт 0.1.68 | Статус в 0.1.69 | Текущее место |
|---|---|---|---|
| 1 | Общий `history.key` ломает независимые пароли | **Закрыто по изоляции:** ключ профильный (`<root>/messenger/history/<network>/<profile>/.history.key`), старый общий мигрируется, `.next` переживает обрыв смены пароля. Ошибка истории больше не срывает open/create/restore. | [messenger.rs../../crates/app/src/messenger.rs), [MESSENGER.md](../architecture/MESSENGER.md) |
| 2 | Окно после RPC `finalize_tx` | **Закрыто частично:** durable `finalizing` до вызова, `reconcile_finalizing` публикует сохранённую сделку без повторной подписи, `retry_post` понимает `finalizing`. Осталось: подтверждение terminal-идемпотентности после `post_tx`. | [rpc_backend.rs../../crates/core/src/rpc_backend.rs) |
| 3 | Онлайн-отправки вне журнала, общий `rpc-pending` | **Закрыто:** `send_over_tor`/`send_local` пишут `RpcSendRecord` (locking → locked → finalizing → finalized_not_posted → posted), `rpc-pending` профильный, нечитаемые записи возвращаются в `wallet_pending.unreadable`. | [rpc_backend.rs../../crates/core/src/rpc_backend.rs), [main.rs../../crates/app/src/main.rs) |
| 4 | Нет свежей строки скана = успех | **Закрыто:** `ScanState` (`not_started/running/completed/failed`) хранится в профиле, `running` после перезапуска = прерванный скан, `pending_scan` снимает только `completed`; парсинг `owner.log` удалён. Отмена RPC-скана — kill процесса + `failed`, `pending_scan` сохраняется. Embedded-отмена — кооперативно между батчами через публичный `grin_wallet_libwallet::scan`, без kill; живой прогон выполнен ([SPIKE_EMBEDDED_SCAN_CANCEL.md](SPIKE_EMBEDDED_SCAN_CANCEL.md)). | [service.rs../../crates/core/src/service.rs), [config.rs../../crates/core/src/config.rs), [main.rs../../crates/app/src/main.rs) |
| 5 | Публичный Tor-gateway принимает unsigned ack | **Закрыто:** `TorListener` создаёт gateway с `require_signed_ack = true`; unit-тест конфигурации. | [tor_listener.rs../../crates/core/src/tor_listener.rs) |
| 6 | Ack раньше истории, poison, replay | **Закрыто частично:** `fetch_open` отделён от ack, `History` пишется с fsync до `ack_signed`, `History::contains` переживает перезапуск, битый конверт не обрывает пачку и не ack-ается (UI показывает число отклонённых), inbox/send сериализованы. Осталось: ограниченный локальный quarantine и явное удаление «плохих» конвертов после TTL. | [store.rs../../crates/messenger/src/store.rs), [history.rs../../crates/messenger/src/history.rs), [app/messenger.rs../../crates/app/src/messenger.rs) |
| 7 | Relay проверяет кандидата-отправителя | **Документация исправлена:** подпись публична, relay может проверить кандидата; это записано в модели угроз и `flint-relay --help`. Протокол v1 этого не скрывает — нужен v2/Sealed Sender. | [MESSENGER.md](../architecture/MESSENGER.md), [RELAY.md](../operations/RELAY.md) |
| 8 | Readiness и кошелёк смотрят на разные ноды | **Закрыто по коду, тестам и живым прогонам:** единый `NodeSnapshot` (endpoint/auth/network/min_confirmations), `check_node_api_http_addr` и `node_api_secret_path` переписываются перед запуском, `node_url` пересоздаёт RPC backend, tip проверяется на отставание от пиров. Живой endpoint-switch через локальный прокси и смена сети в пустом временном каталоге прошли. | [node.rs../../crates/core/src/node.rs), [rpc_backend.rs../../crates/core/src/rpc_backend.rs), [main.rs../../crates/app/src/main.rs) |
| 9 | Предохранители live/crash-скриптов | **Открыто:** live-тест берёт сеть из settings, `Stop-Wallets` убивает все `grin-wallet`. | [live_testnet.rs../../crates/core/tests/live_testnet.rs), [crash-test-rpc.ps1../../tools/crash-test-rpc.ps1) |

## 2. Дополнительные пункты

| Пункт | Статус |
|---|---|
| RPC-пароль вне общей политики | **Открыто:** `validate_password_strength` только в embedded create/restore/change. |
| PBKDF2-100 seed | **Закрыто в основном:** Argon2id-конверт у обоих бэкендов, миграция, mainnet-гейт; остаток — legacy до миграции и совместимость с официальным `grin-wallet`. |
| IPC собственных команд | **Открыто:** `build.rs` без `AppManifest::commands`, capabilities только `core:default`; утверждение «Tauri 2 не может» в docs неточно. |
| Graceful stop | **Закрыто:** `shutdown_children` на `ExitRequested`/`Exit`, watchdog-ах и `restart_app` снимает RPC/Tor, завершает `WalletProcess` и останавливает ноду; живой прогон с реальным `tor.exe` (bootstrap 100%, onion) после закрытия окна не оставил процессов, повторный запуск открыл кошелёк. |
| Квоты relay | **Открыто:** лимиты очередей есть, лимита соединений/скорости нет. |
| Tor-мосты | **Открыто:** пути в torrc без кавычек, ошибка чтения `bridges.txt` = тихий `None`. |
| Упаковка | **Открыто:** `package.ps1` по умолчанию 0.1.0, только exe+marker. |
| Матрица «функция × backend × путь × проверка» | **Открыто:** STATUS остаётся набором разделов, не матрицей. |

## 3. Что сделано в этом срезе

- профильный ключ истории, миграция общего ключа, `.next` для смены пароля,
  best-effort unlock (кошелёк не падает из-за истории), предупреждение в UI;
- `TorListener` → signed ack;
- RPC: durable `finalizing`, reconcile без повторной подписи, fsync журнала,
  журнал онлайн-отправок, профильный `rpc-pending`, `unreadable` в pending;
- явные состояния скана в профиле, корректный `pending_scan`, `min_confirmations`
  в живом RPC backend;
- единый `NodeSnapshot`, патч `check_node_api_http_addr`/секрета перед запуском,
  пересоздание RPC backend при смене `node_url`, проверка свежести tip;
- документация: модель угроз relay, keywrap, STATUS, RELAY, README.

## 4. Следующий порядок

1. **Ack/история:** основное сделано (history до ack, durable dedup, bad не
   обрывает пачку); осталось ограниченное локальное quarantine/удаление битых
   конвертов после TTL.
2. **Скан:** закрыто — явные состояния `not_started/running/completed/failed`,
   профильный `pending_scan`, без лог-эвристики; отмена RPC-скана (kill
   отдельного процесса, `failed`, перезапуск backend) и embedded-скана
   (кооперативно между батчами), живые прогоны выполнены.
3. **Нода:** закрыто по коду — snapshot endpoint/auth/network/min_confirmations,
   пересоздание backend при смене `node_url`, свежесть tip, живой endpoint-switch,
   graceful stop и реальный Tor bootstrap/остановка. Осталось: живой прогон с
   реальным mainnet-кошельком/сетью.
4. **Эксплуатация:** IPC permissions + отрицательный тест, квоты/rate limit
   relay, кавычки в torrc, безопасные live/crash-раннеры.
5. **Упаковка/доки:** manifest комплектации, версия, чистая Windows-проверка;
   матрица «функция × backend × путь × проверка».

Проверка на момент документа: `cargo test --workspace` — 212 passed, 0 failed,
13 ignored; `--selftest` — ok; `tools/verify-ui.mjs` — без проблем.




