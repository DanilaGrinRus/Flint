# Спайк: grin-wallet как бэкенд Flint (2026-10-23)

> **HISTORICAL:** документ относится к периоду до удаления Tauri (2026-10-26). Ссылки на `crates/app`/`ui/` и старые `--selftest`/UI-проверки — исторические.


Проверено на официальном **grin-wallet 5.5.1** (win-x86_64, SHA256 сверен с
`grin-wallet-v5.5.1-win-x86_64-sha256sum.txt`), одноразовый каталог
`.tmp/spike-wallet`, нода `grin 5.5.2` на 13413.

## Что выяснено

### Запуск

```powershell
grin-wallet.exe --testnet -t <dir> owner_api -l 3422 [--run_foreign]
```

- Процесс **стартует без пароля**: кошелёк закрыт, ключи не расшифрованы.
- Пароль вводится позже через RPC `open_wallet` → возвращает `Token`
  (keychain mask), который передаётся в каждый последующий вызов.
- Это снимает главную проблему «пароль в аргументах процесса».

### HTTP-поверхность

| Путь | Что | Аутентификация |
|---|---|---|
| `/v3/owner` | owner API (все операции кошелька) | HTTP Basic: `base64("grin:" + secret)` |
| `/v2/foreign` | foreign API (если `--run_foreign`) | без пароля, секрет не проверяется |

Секрет лежит в `<dir>/.owner_api_secret`, генерируется при `init`.
Неверный путь (`/v2/owner`) даёт 404 — это не ошибка авторизации.

### Шифрование сессии (обязательное)

Owner API v3 не принимает открытые запросы. Протокол:

1. `init_secure_api` (единственный открытый вызов) с нашим ECDH-публичным ключом
   (secp256k1, hex) → ответ содержит публичный ключ сервера.
2. Общий секрет = ECDH(наш приватный ключ, ключ сервера), 32 байта.
3. Каждый следующий запрос:

```json
{"jsonrpc":"2.0","method":"encrypted_request_v3","id":1,
 "params":{"nonce":"<hex, 12 байт>","body_enc":"<base64 AES-256-GCM(JSON)>"}}
```

4. Ответ: `{"result":{"Ok":{"nonce":…,"body_enc":…}}}`, расшифровывается тем же
   ключом. Внутри — обычный JSON-RPC ответ (`Ok`/`Err`).

Реализуемо нашими зависимостями: secp256k1 уже есть через `grin_util::secp`,
`ring` (AES-256-GCM) завендорен.

### Методы, которых хватает для GUI

`accounts`, `retrieve_outputs`, `retrieve_txs`, `retrieve_summary_info`,
`init_send_tx`, `tx_lock_outputs`, `finalize_tx`, `post_tx`, `cancel_tx`,
`get_stored_tx`, `scan`, `node_height`, `get_slatepack_address`,
`create_slatepack_message`, `slate_from_slatepack_message`,
`decode_slatepack_message`, `open_wallet`, `close_wallet`, `create_wallet`,
`get_mnemonic`, `init_secure_api`.

То есть весь текущий функционал Flint покрывается один-в-один.

## Что это значит для архитектуры

| Сейчас | После миграции |
|---|---|
| `grin_wallet_*` внутри процесса | отдельный `grin-wallet.exe` |
| наш журнал `finalizing/failed_recoverable` | tx-log кошелька + тонкое отображение |
| наш приём slatepack (был баг с lock) | `slate_from_slatepack_message` + `post_tx` через API |
| сборка 5–8 минут, vendor ~900 МБ | минус весь крипто-стек |

## Риски и решения

1. **Шифрование сессии** — обязательный протокол; клиент нужно покрыть тестами
   против реального бинаря (handshake, неверный ключ, чужой секрет).
2. **Два процесса** — нода и кошелёк; нужен супервизор и проверка «кто владеет
   каталогом» (та же идея, что 
ode.lock`).
3. **Порт owner API** — по умолчанию 3420; занимать произвольный свободный и
   хранить в состоянии.
4. **Секрет** `.owner_api_secret` — читать из каталога кошелька, не логировать.
5. **Пароль** — только через `open_wallet`, никогда в argv.
6. **Обновление** — версия кошелька и ноды должны быть совместимы (5.5.x).

## Следующий шаг

Клиент `wallet_rpc` в `flint-core`:

1. ECDH-хендшейк + AES-256-GCM сессия (ring + grin_util::secp).
2. Тонкие обёртки: `open/close`, `summary`, `txs`, `send`, `receive`, `scan`.
3. Интеграционный тест против реального `grin-wallet.exe` (одноразовый каталог).
4. Только после зелёного теста — переключать GUI на новый бэкенд под флагом.

## Проверено 2026-10-24: адрес = onion

- `grin-wallet listen` поднимает Tor hidden service **из ключа адреса**:
 ```
  Starting Tor Hidden Service for API listener at address
  qezlq34gn32544wwh6u23dnnuxp3t2574p3bjuxl3gmu2vmwl5zguvyd,
  binding to 127.0.0.1:3425
 ```
- Тот же onion получен независимо из адреса
 `tgrin1syetsmuxdm6auukk8756mrdd5h0mn6alu0mpf5htmxv564vktaeq622f2w`
  нашей функцией `onion_from_address` (`crates/core/src/address.rs`).
- Значит, для прямого Tor-платежа достаточно адреса: получатель запускает
 `listen`, отправитель вычисляет `.onion` и шлёт slate на foreign API.
- Ограничение: `listen` должен быть запущен и кошелёк разблокирован — автоприём
  подписывает ответ только в этом состоянии.
## Tor: что проверено и где блокер (2026-10-24)

- `OnionService` (arti через `grin_wallet_impls`) поднимает hidden service на
  ключе адреса: onion совпадает, `state/keystore/hss/<onion>/ks_hs_id` создан.
- В логах `tor_hsservice` видно успешную загрузку дескриптора в 5 HSDir
  (`descriptor uploaded successfully`), плюс отдельные ретраи с
 `could not build circuit to HsDir`.
- Клиент `tor_post` к тому же onion получает
 `EndReceived(EndReason(RESOLVEFAILED))` — дескриптор не находится.
- Контроль: публичный onion по тому же клиенту отвечает (400 от nginx), то есть
  Tor и клиент исправны; проблема в нашем дескрипторе/времени публикации.
- Следующий шаг: смотреть логи `tor_hsclient` на клиенте (какой HSDir и какой
  отказ), проверить время публикации >5 минут и совпадение time period.