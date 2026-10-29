# Аудит callback'ов Slint → runtime

Дата: 2026-10-27. Область: `crates/slint-ui/src/main.rs`,
`crates/slint-ui/ui/spike.slint`, 43 callback'а.

## Итог

Все денежные и seed-операции идут через `WalletRuntime`; прямого вызова
`WalletRpc`/HTTP RPC из callback'ов нет. Запуск RPC-процесса также перенесён
в runtime (`start_rpc_background()`), UI больше не дергает `rpc()` напрямую.

Пароли в callback'ах оборачиваются в `Zeroizing`; confirm-строки для
create/restore/change-password также переведены в `Zeroizing`.
Seed-фраза показывается только после создания, очищается по кнопке
«Я сохранил seed»; в файлы и логи не пишется.

## Матрица

| Callback | Runtime-цель | Проверка/секрет |
|---|---|---|
| unlock | `open`, `unlock_history_key` | пароль Zeroizing, ошибка без секрета |
| create | `create` | пароль+confirm Zeroizing, mnemonic только в UI |
| restore | `restore` | mnemonic/пароль Zeroizing, очистка после |
| change-password | `change_password_with_history` | все три строки Zeroizing |
| seed-disable/repair/migrate | `disable_envelope`, `repair_envelope`, `migrate_envelope` | пароль Zeroizing, подтверждение |
| delete-wallet | `delete_wallet` | текстовое подтверждение, не секрет |
| lock | `lock`, `lock_history_key` | без секретов |
| rotate-address | `rotate_address` | runtime-gate |
| refresh/history/scan/scan-cancel | `summary`, `activity`, `scan_runtime` | только чтение/скан |
| node-toggle | `node_start`/`node_stop` | runtime settings/phase |
| settings-save | `update_settings` | runtime-валидация |
| tor-* | `tor().*` | runtime, без денег |
| send-prepare | `prepare_send` | runtime, сумма разбирается до вызова |
| send-finalize | `finalize_send` | runtime + prepared snapshot |
| receive-prepare/finalize | `prepare_receive`, `finalize_receive` | runtime |
| pending reload/retry/cancel | `pending`, `retry_post`, `cancel_pending` | UI gate + runtime gate |
| messenger-open/inbox/send/history/contacts/block/recover | `messenger_*` | history key, runtime |
| swap-* | swap API | demo, runtime |
| copy-* | clipboard | текст, не seed |
| security/settings/swaps/messenger-open | UI state only | без секретов |

## Исключения и риски

1. Запуск RPC-процесса теперь идёт через `start_rpc_background()`; прямого
   `rpc()` в UI не осталось.
2. UI получает seed-фразу после создания для показа. Это неизбежно; свойство
   очищается по подтверждению. При аварийном завершении процесса seed остаётся
   только в памяти процесса.
3. UI хранит пароль в Slint-свойстве на время ввода. Поле masked, но host
   malware/дамп процесса может его прочитать; это вне границы одного процесса.
4. Runtime-проверки нельзя обойти скрытием кнопки: команды проверяют состояние
   в runtime. Проверено для pending/retry/cancel и password policy.

## Что осталось

- Добавить CI-grep, запрещающий `wallet.rpc()`/прямой seed в UI.
- Проверить освобождение seed-свойства при закрытии окна создания без
  подтверждения.
- Внешний pentest; этот аудит его не заменяет.
