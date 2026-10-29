# Flint relay — офлайн-доставка

Статус: **0.1.69, dev**. Relay хранит только подписанные и зашифрованные
конверты. Он не знает seed, пароль, тело сообщения и адрес отправителя;
конверт обязан содержать proof-of-work.

## Зачем

Прямой onion работает, только когда получатель онлайн. Relay — обычный
store-and-forward mailbox:

1. A отправляет конверт на relay;
2. B забирает его позже, когда запустит Flint;
3. B подтверждает получение **подписанным** ack, relay удаляет конверт.

Relay не заменяет onion: если B онлайн, приложение может работать напрямую.
Relay нужен для офлайна и как fallback.

## Запуск

```powershell
cargo build --release -p flint-core --bin flint-relay
.\target\release\flint-relay.exe `
  --listen 127.0.0.1:8080 `
  --store C:\FlintRelay\store `
  --ttl 93600 `
  --max-per 100 `
  --max-total 10000
```

| Флаг | Значение |
|---|---|
| `--listen` | адрес; по умолчанию `127.0.0.1:8080` |
| `--store` | каталог сообщений; по умолчанию `flint-relay-store` |
| `--ttl` | максимальный TTL в секундах, по умолчанию 26 ч |
| `--max-per` | сообщений на получателя, по умолчанию 100 |
| `--max-total` | сообщений всего, по умолчанию 10 000 |

Проверка:

```powershell
Invoke-WebRequest http://127.0.0.1:8080/flint/v1/health
# {"ok":true}
```

`POST /v2/foreign` в relay-режиме отвечает `404`: кошелька здесь нет.

## Публикация через Tor

Relay слушает loopback, наружу выставляется hidden service:

```text
SocksPort 0
DataDirectory C:\FlintRelay\tor-data
HiddenServiceDir C:\FlintRelay\hs
HiddenServicePort 80 127.0.0.1:8080
Log notice file C:\FlintRelay\tor.log
```

Запуск:

```powershell
.\tor.exe -f C:\FlintRelay\torrc
Get-Content C:\FlintRelay\hs\hostname
```

Получится `http://<...>.onion`. В Flint → «Сообщения» → **Relay / gateway URL**
вставьте этот URL. Для `.onion` приложение использует SOCKS своего Tor, если
«Приём по Tor» включён; иначе `FLINT_TOR_SOCKS` (по умолчанию
`127.0.0.1:9050`).

**Не выставляйте `--listen 0.0.0.0:8080` в интернет как plain HTTP.** Если нужен
публичный HTTPS, ставьте relay за reverse proxy с TLS; Tor-вариант предпочтительнее
для метаданных.

## API

| Запрос | Тело | Ответ |
|---|---|---|
| `POST /flint/v1/msg` | `{id,to,payload,ttl_secs}` | `{id,expires_at}` |
| `GET /flint/v1/msg?to=<addr>` | — | `{messages:[...]}` без `from` |
| `POST /flint/v1/msg/<uuid>/ack` | `{to,id,ts,signature}` | 200/401/404 |
| `GET /flint/v1/health` | — | `{ok:true}` |

Ack в публичном relay обязателен:

```json
{
  "to": "tgrin1…",
  "id": "UUID сообщения",
  "ts": 1760000000,
  "signature": "hex(ed25519)"
}
```

Подпись покрывает `flint-messenger-ack-v1 || len(to) || to || len(id) || id || ts`.
Relay проверяет ключ по адресу, `id` и окно `ts` ±300 секунд. Старый ack
`{to}` принимается только внутренним тестовым gateway
(`require_signed_ack = false`); `TorListener`, публикующий onion, требует
подпись. Подпись, снятая с одного сообщения, не подходит к другому `id`.

## Что видит relay

- адрес получателя, UUID, размер и время;
- ciphertext и подпись отправителя; relay не расшифровывает адрес, но может
  проверить **кандидата**-отправителя по публичной подписи;
- ничего о тексте и slatepack-е.

Он может удалять, задерживать и дублировать сообщения. Получатель проверяет
подпись отправителя после расшифровки и не должен доверять relay.

## Ограничения

- **PoW обязателен**: relay с `require_pow` отклоняет конверт без валидного
  proof-of-work; старые клиенты без PoW не совместимы. Есть TTL, квоты и
  120 POST/мин на получателя + 120 POST/мин на исходный IP, но
  распределённого rate limit нет.
- `GET /flint/v1/msg?to=` авторизуется знанием адреса: кто знает адрес, может
  скачать ciphertext (но не расшифровать). Ack без подписи удалить не может.
- Один relay не даёт гарантий доставки; это не федеративная сеть.
- Нет push: получатель сам нажимает «Забрать входящие».
- Часы relay и клиента должны совпадать в пределах 5 минут для ack.
- Квоты считаются в памяти и по файлам; при огромном store нужен внешний
  монитор/БД.
- Публичный инстанс ещё не развёрнут; протестирован локальный relay и
  перезапуск с сохранением очереди.

## Проверки

В `flint-core`:

- `relay_mode_has_no_wallet_and_requires_signed_acks` — `/v2/foreign` 404,
  unsigned ack 401, signed ack удаляет;
- `relay_store_survives_a_process_restart` — конверт переживает остановку
  процесса;
- `sdk_transport_exchanges_messages_through_gateway` — два SDK-клиента.

В `flint-messenger`:

- `ack::tests` — подпись, чужой адрес/id/ts, подмена подписи.
