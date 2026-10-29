# Ответ на внешний AI-аудит 0.1.69

Дата: 2026-10-27. Аудит: [AUDIT_EXTERNAL_2026-10-27.md](AUDIT_EXTERNAL_2026-10-27.md).
Это независимый AI-review, не заменяет человеческий pentest/аудит.

## Статус находок

| ID | Статус | Что сделано / что осталось |
|---|---|---|
| F-01 | исправлено по коду | Embedded `Finalizing` не публикует stored tx; все ошибки `finalize_tx` уходят в `FailedRecoverable`, новая подпись разрешена только из `prepared`/`locked`. Нужен live crash-прогон. |
| F-02 | исправлено | RPC `finalize_tx` разрешён только для `prepared`/`locked`; `failed_recoverable` не финализируется и не отменяется из UI. |
| F-03 | исправлено | RPC receive читает строго `amt`, отклоняет missing/non-numeric/zero; добавлен unit-тест. Нужен probe настоящего Slate. |
| F-04 | частично | Durable `ReceiveIntent` пишется до `receive_tx`; при ошибке запись становится `response_lost` и повторный receive запрещён. Настоящее восстановление ответа после мутации невозможно без сохранения response upstream — остаётся P1. |
| F-05 | частично | temp ограничивается до записи, primary/backup — после; recovery сообщает об ошибке ACL в warning. Старые файлы и второй Windows-аккаунт не проверены. |
| F-06 | исправлено по коду | `quarantine_unreadable_envelope` берёт общий SeedLock до проверки и rename. Гонка не воспроизводилась. |
| F-07 | исправлено | Torrc пути quoted; ошибки чтения bridges кроме NotFound возвращаются. Нужны Unicode/live Tor тесты. |
| F-08 | исправлено по коду | `Gateway` ограничивает активные connection threads 64 и отвечает 503; нагрузочный тест не проводился. |
| F-09 | исправлено по коду | `node_url` с remote plain HTTP отклоняется; HTTPS/loopback разрешены. |
| F-10 | частично | `resources/native-assets.sha256` и проверка хешей в package добавлены; SBOM/обязательная подпись остаются открытыми. |
| F-11 | исправлено по коду | crash-script завершает только зарегистрированные child PID и требует testnet-маркер. |
| F-12 | исправлено | HTTP response читается через `take(MAX_RESPONSE+1)`. |

## Повторная проверка замечаний 01:24

1. **scan → switch**: старый прямой `WalletRuntime::scan` удалён; Slint
   использует `ScanRuntime`. `ScanRuntime::begin_switch()` резервирует
   transition-slot, `begin_call()` отказывает скану во время смены, а
   generation-проверка не даёт записать результат в другой профиль. Исправлено
   по коду; нужен concurrency-тест.
2. **Смена пароля**: Slint вызывает `change_password_with_history`, а не только
   seed. Runtime переупаковывает `.history.key`. Замечание относилось к старому
   снимку; исправлено.
3. **RPC process publish**: `start`/`restart` проверяют generation/shutdown под
   общим `commit` mutex; `stop`/`shutdown` берут тот же mutex. `shutdown()`
   теперь ещё и отменяет активный скан. Остался вопрос join/ожидания worker при
   выходе — отдельный lifecycle-тест.
4. **NoSync**: исправлен на ненулевой tip/пиры без `sync_info`; независимая
   актуальность высоты пиров не проверяется.
5. **RPC backend vs embedded**: `WalletRuntime` всё ещё хранит режим, но
   `open_wallet/summary/send/scan` идут в embedded `WalletService`. Для preview
   Slint создаёт только embedded; подключение RPC требует dispatch/trait.

## PT-01…PT-05

| ID | Статус | Комментарий |
|---|---|---|
| PT-01 seed cleanup/disable race | исправлено по коду | `remove_verified_seed` удаляет legacy только при наличии читаемого envelope; добавлен regression-тест «stale cleanup не удаляет legacy после disable». |
| PT-02 relay response >4 MiB | исправлено по коду | `/msg` отдаёт страницу ≤2 MiB и `limit`; клиент запрашивает ≤100. Добавлен тест очереди 40×60 KiB. |
| PT-03 quota после restart | исправлено по коду | `Gateway::start` загружает persisted messages до POST; добавлен restart-тест. |
| PT-04 ContactBook race | исправлено по коду | cross-instance `contacts.lock` + unique tmp; добавлен parallel-books тест. |
| PT-05 ScanRuntime RPC readiness/partial | исправлено по коду | partial scan при `pending_scan` запрещён; RPC scan проверяет `node_info().phase.ready()` внутри runtime. Live-тест не проводился. |

## Проверки

- `cargo test --workspace` — 256 passed, 19 ignored.
- `flint-selftest` — ok.
- package/C:\Flint обновлены.

## Следующий порядок

1. F-04 (P1, receive journal) — до mainnet.
2. PT-02/PT-03/PT-04, F-08/F-09/F-10/F-11.
3. Live crash-прогоны F-01, PT-01; RustSec; внешний человеческий аудит.
