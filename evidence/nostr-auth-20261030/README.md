# NIP-42 в Nostr-клиенте — проверка 2026-10-30

## Loopback (обязательный AUTH)

```
cargo test -p flint-messenger nostr::ws::tests::query_answers_auth_challenge_and_retries --offline
```

PASS. Тестовый relay при подключении отправляет `["AUTH","chal-loopback"]`,
проверяет у ответного события kind 22242 теги `challenge`/`relay`, подпись
BIP-340 и возвращает `OK true`; клиент повторяет `REQ`, получает `EOSE`.

## Живые relay (только чтение, kind 1059)

| Relay | Результат |
|---|---|
| `wss://relay.damus.io` | присылает `AUTH`, наш ответ доходит, relay отвечает своей ошибкой `error: relay needs serviceUrl to be configured before AUTH can work` |
| `wss://relay.snort.social` | PASS без AUTH (анонимный DM-фильтр отдаёт EOSE) |
| `wss://relay.primal.net` | PASS без AUTH |
| `wss://nos.lol` | PASS без AUTH |
| `wss://relay.nostr.band` | connect error (сеть/DNS) — NOT VERIFIED |
| `wss://nostr.wine` | HTTP 403 — NOT VERIFIED |

Команда:

```
$env:FLINT_NOSTR_LIVE_RELAY='wss://relay.snort.social'
$env:FLINT_NOSTR_LIVE_RELAY_KIND='1059'
cargo test -p flint-messenger live_relay_req_returns_eose --offline -- --ignored --nocapture
```

## Граница приёмки

Клиентская часть NIP-42 покрыта loopback-тестом; на реальной сети подтверждено,
что challenge разбирается и AUTH-событие отправляется (damus). Принять AUTH на
публичном relay в этом окружении не удалось: damus сломан на своей стороне
(`serviceUrl`), остальные доступные relay не требуют AUTH для анонимных фильтров.
Пункт «live AUTH accepted» остаётся NOT VERIFIED до выбора relay с рабочим NIP-42.
