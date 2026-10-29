# iOS ↔ desktop: минимальный контракт переводов

Статус: **проектный контракт, не реализованный в iOS.** Он фиксирует, что
можно вынести в общий Rust-слой без слияния каталогов и без изменения текущего
дизайна desktop. Источник истины по iOS — `mobile/GrinFlint/docs/mobile/ios/`;
этот документ не превращает iOS-проект в часть workspace Flint.

## Границы

- Desktop: `crates/core` + `crates/runtime` + Slint, backend embedded/RPC.
- iOS: отдельный SwiftUI-проект и отдельный Rust FFI-крейт
  `mobile/GrinFlint/ffi/grin-wallet-ffi`.
- Общий код — только **чистые DTO и правила состояний**, без UI, без путей
  Windows и без хранения секретов. Каталоги `crates/` и `mobile/GrinFlint/`
  не объединяются.
- Обе платформы используют upstream `grin-wallet` **5.5.1**; desktop —
  закреплённый fork в `third_party/`, iOS — FFI над официальным 5.5.1.
  Это разные сборки, поэтому контракт версионируется, а не шарится бинарь.

## Общие состояния перевода

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferStage {
    Prepared,        // slate создан локально, ещё не доставлен
    RequestSent,     // запрос доставлен транспортом, но не подтверждён сетью
    ResponseReady,   // получен ответный slate; нужен экспорт/финализация
    Finalized,       // финализирован, публикация ещё не подтверждена
    Posted,          // публикация отправлена в ноду
    Confirmed,       // подтверждено сетью
    Cancelled,
    Failed,
}
```

`RequestSent` **не означает** подтверждение перевода. UI обязан показывать
«ожидает ответа/подтверждения», а не «доставлено» или «подтверждено».

## DTO (JSON, стабильные имена)

```json
{
  "schema": 1,
  "slate_id": "uuid",
  "direction": "send|receive",
  "stage": "prepared|request_sent|response_ready|finalized|posted|confirmed|cancelled|failed",
  "amount_nano": "100000000",
  "fee_nano": "23000000",
  "peer": "tgrin1...",
  "next_action": "resend|export_response|finalize|repost|wait|cancel|restore_scan",
  "can_resend": false,
  "can_export_response": false,
  "can_finalize": false,
  "can_repost": false,
  "can_cancel": false,
  "last_error": null,
  "updated_at": 0
}
```

Правила:

- `next_action` — одно из перечисленных значений, не произвольный текст;
- action разрешён только при безопасном состоянии (см. ниже);
- `last_error` не содержит паролей, seed, приватных ключей и полного slatepack;
- `posted` не равно `confirmed`.

## Переходы и идемпотентность

| Stage | Допустимое действие | Запрещено |
|---|---|---|
| Prepared | resend, cancel | финализация |
| RequestSent | wait, resend того же envelope id | повторная подпись |
| ResponseReady | export_response, finalize | resend запроса |
| Finalized | repost | повторная подпись |
| Posted | wait/reconcile | cancel |
| Confirmed/Cancelled | — | любые денежные действия |

Очередь исходящих сообщений должна хранить **уже подписанный envelope** и
повторять `deliver` с тем же id; повторная подпись создаёт другой перевод.
Входящий ответ нельзя считать подтверждённым, пока sender не выполнил
`finalize_send`/`post`.

## Восстановление

Контракт восстановления:

```json
{
  "schema": 1,
  "kind": "restore",
  "needs_scan": true,
  "scan_state": "not_started|running|completed|failed",
  "old_funds_visible": false,
  "address": "tgrin1...",
  "last_error": null
}
```

После restore в чистый каталог `old_funds_visible` меняется на `true` только
после завершённого полного скана и положительного spendable/awaiting balance.
До этого UI показывает «баланс неизвестен», не ноль.

## Зависимости

- iOS FFI и desktop используют `grin-wallet 5.5.1`, но поставляются раздельно;
  общий DTO-крейт не должен зависеть от `slint`, `tokio`, Windows ACL или
  платформенных FFI.
- Криптографические операции остаются в соответствующем backend; общий слой
  только читает/возвращает DTO.
- iOS-изменения собираются и проверяются только в iOS CI; desktop-проверки не
  доказывают сборку iOS.

## Что не делаем этим контрактом

- не переносим `crates/` в iOS и не создаём общий каталог платформ;
- не обещаем одинаковый UI;
- не делаем mainnet-ready вывод;
- не заменяем независимый аудит.
