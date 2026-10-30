# Исправления desktop-находок аудита 2026-10-30

Источник находок: `evidence/audit-20261030/REPORT.md` (D-A1…D-A5). Ниже — что
изменено в коде и какими тестами это закреплено. iOS-находки относятся к
`mobile/GrinFlint` и в этой сессии не правились.

| № | Находка | Исправление | Регрессия |
|---|---|---|---|
| D-A1 | локальный `.api_secret` уходил на внешнюю ноду; сохранённый `http://` внешний адрес принимался | `node_api_secret` читает файл только для literal loopback (`localhost`, 127/8, ::1); `Settings::sanitized` заменяет недопустимый сохранённый URL локальной нодой и даёт предупреждение; `rpc_backend` не прописывает managed secret внешней ноде и очищает stale `node_api_secret_path` | `local_node_secret_never_goes_to_an_external_node`, `disallowed_node_url_is_replaced_on_load`, `persisted_node_url_policy_rejects_open_transport` |
| D-A2 | повтор receive по UUID не сверял новый S1 | embedded `PendingSlate` и RPC `RpcReceiveRecord` хранят canonical JSON исходного S1; повтор с другим содержимым отклоняется, запись без отпечатка (legacy) повтор не разрешает | `repeated_receive_must_match_the_original_request`, `repeated_receive_must_match_the_journaled_slate`, `receive_request_fingerprint_survives_restart`, `receive_request_fingerprint_survives_journal_restart` |
| D-A3 | ошибки записи TOML скрывались, запуск шёл со старой конфигурацией | `set_toml_string`/`patch_node_secret`/`patch_wallet_paths` возвращают `Result`; старт `grin-wallet` завершается ошибкой вместо stale config | `config_write_errors_are_not_swallowed` |
| D-A4 | `ws://localhost.attacker.invalid` проходил проверку префикса | URL разбирается через `url::Url`; `ws://` разрешён только для literal loopback host; проверка используется и в настройках | `url_policy` (включая `ws://localhost.attacker.invalid`, `ws://127.0.0.1.attacker.invalid`) |
| D-A5 | бюджет начинался после handshake; дефолтные лимиты кадров | общий 10-секундный бюджет на DNS-резолв, TCP connect, TLS и WebSocket handshake; `WebSocketConfig` 1 MiB message / 256 KiB frame; query-лимиты 4096 сообщений / 8 MiB с ошибкой вместо тихого усечения | `handshake_is_bounded_by_the_connect_budget`, `query_stops_a_message_flood`, `query_rejects_an_oversized_message`, `url_policy` |

## Остаточный риск

- DNS-резолв ОС нельзя прервать; вызов возвращается по бюджету через helper-thread,
  но зависший системный resolver может оставить поток. Транспортный worker не
  блокируется дольше 10 с.
- Legacy-записи receive без отпечатка S1 теперь требуют ручного разбора: повтор
  автоматически не выполняется. Это осознанный fail-closed.
- Внешняя нода с явно заданным секретом поддерживается: секрет передаётся и
  хранится в per-wallet файле, а не берётся из managed node.

## Команды

```
cargo test -p flint-core --lib --offline       # 195
cargo test -p flint-messenger --offline        # 83 + 1 ignored
cargo test --workspace --offline               # 354 passed / 0 failed / 33 ignored
```
## Что осталось непроверенным

- Матрица disk-full / power-loss для денежных операций не выполнялась; покрыты
  только отдельные crash-точки и запрет повторной подписи.
- Полный набор гонок «операция → переключение профиля → поздний ответ» не
  подтверждён; generation-проверки есть, но матрица не прогонялась.
- Реальный второй пользователь Windows, разные файловые системы и вся цепочка
  временных файлов не проверялись; ACL-тест удаления чужого `Everyone` проходит.
- End-to-end изменённый S1 с валидной криптографией не собирался: сравнение
  отпечатка проверено на уровне решения и перезапуска журнала, но не через
  подделку зашифрованного slatepack.
