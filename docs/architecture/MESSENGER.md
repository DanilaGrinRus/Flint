# Мессенджер Flint — протокол v1 (M2 + M3 direct onion/relay)

Статус: **M2 + M3 direct onion + relay + адресная книга + Tor-мосты** (0.1.69).
Крейт: `crates/messenger` (`flint-messenger`). Desktop-UI, gateway, encrypted
history, прямой обмен через Tor, standalone relay, шифрованная адресная книга и
obfs4/snowflake-мосты подключены.

Цель — обмен slatepack-ами и короткими служебными сообщениями по адресу, без
копирования armor-блоков через чужие мессенджеры. Это не мессенджер общего
назначения: без вложений и групп, ключи остаются в кошельке.

## Идентичность

Идентичность = slatepack-адрес: bech32(ed25519 public key), префиксы `tgrin`
(testnet) и `grin` (mainnet). Новых ключей нет; подпись и шифрование используют
ключ адреса.

Тест-вектор (seed `[0x07; 32]`, hrp `tgrin`):

```
tgrin1af9xcclzn3fq40h42pa3xtk9lx25wa4wh6l8hyjzrm4xj9zx6gkqcpfcf7
```

Из этого же ключа детерминированно выводится Tor v3 onion (см.
`crates/core/src/address.rs`), поэтому «знаю адрес → отправляю» работает без
каталога и без relay, когда получатель онлайн.

## Конверт

JSON-объект; бинарные поля — base64.

| Поле | Тип | Смысл |
|---|---|---|
| `version` | u8 | сейчас `1` |
| `id` | UUID | идемпотентность и ack |
| `recipient` | string | slatepack-адрес получателя (единственные метаданные о людях) |
| `created_at` | u64 | unix-секунды |
| `ttl` | u64 | секунды; по умолчанию 93 600 (26 ч), максимум 604 800 (7 дней) |
| `ephemeral_key` | 32 Б | X25519 public отправителя |
| `nonce` | 12 Б | ChaCha20-Poly1305 |
| `ciphertext` | bytes | зашифрованный payload |
| `pow_nonce` | u64 | nonce proof-of-work; `0` у legacy-конверта |
| `pow_bits` | u8 | сложность PoW; `0` = без PoW (legacy) |
| `signature` | 64 Б | ed25519 отправителя |

Payload (внутри ciphertext):

```json
{
  "sender": "tgrin1…",
  "type": "text" | "slatepack",
  "text": "…",              // type=text
  "armor": "BEGINSLATEPACK…", // type=slatepack
  "encrypted": true,          // type=slatepack, false запрещён
  "created_at": 1760000000
}
```

**Адрес отправителя в открытом виде не передаётся.** Relay видит только
получателя, размер и время; тип сообщения тоже внутри шифра.

## Криптография

1. `ed25519 seed → SHA512(seed)[..32]` — X25519 scalar.
2. `ed25519 public → Montgomery u` (`curve25519-dalek`,
   `CompressedEdwardsY::decompress().to_montgomery()`).
3. Отправитель генерирует эфемерный X25519 ключ, считает DH с ключом получателя.
4. `HKDF-SHA256(salt = recipient X25519 public, info = "flint-messenger/v1/encryption")`
   → 32-байтный ключ.
5. `ChaCha20-Poly1305`, AAD = `version || id || recipient || created_at || ttl`
   (с length-prefix для строк).
6. Подпись ed25519 покрывает domain-separated транскрипт:
   `version, id, recipient, created_at, ttl, ephemeral_key, nonce, len(ciphertext), ciphertext`
   и, для PoW-конвертов, `pow_nonce, pow_bits`.
7. `pow_bits` — SHA-256 proof-of-work над `id/recipient/created_at/ttl`,
   commit-хэшем ciphertext и nonce. Release требует 20 бит, debug — 12;
   gateway с `require_pow` проверяет до записи, получатель — до расшифровки.

Проверка отправителя выполняется **получателем после расшифровки**: только он
видит `sender` и проверяет подпись его ключом. Relay не может подделать подпись
или выдать себя за отправителя, но **может проверить кандидата**: подпись и
транскрипт публичны, поэтому relay, знающий предполагаемый slatepack-адрес,
может убедиться, что сообщение отправил именно он. Это осознанная утечка
метаданных v1; скрыть отправителя может только протокол, где подтверждающая
подпись тоже недоступна relay (Sealed Sender/ratchet), с новой версией формата.

## Правила

- **Plaintext-slatepack (`encrypted: false`) запрещён** — `Envelope::seal` и `open`
  возвращает `PlaintextSlatepack`. Через relay не проходит читаемая сделка.
- TTL проверяется дважды: mailbox при приёме и получатель при открытии.
- Дедупликация по `id`; доставка at-least-once, повтор не создаёт вторую копию.
- Ack удаляет сообщение только после успешной расшифровки и проверки подписи.
- Квоты mailbox: **P2P** — 30 сообщений на получателя, 32 КиБ payload,
  12 ч TTL, 30 POST/мин; **relay по умолчанию** — 100/64 КиБ/26 ч/120 POST/мин.
  P2P и relay требуют PoW; relay дополнительно требует подписанный ack.
- Локальный **block/mute**: заблокированный отправитель ack-ается и не попадает
  в историю; список хранится в `messenger/blocked/...`.
- Никаких тел сообщений в логах relay.

## API

```rust
let messenger = Messenger::new(MemoryMailbox::default());
let envelope = messenger.send(&alice, bob.address(), kind, DEFAULT_TTL_SECS)?;
let inbox = messenger.receive(&bob, 10)?; // Vec<Opened>, уже проверенные
```

`Transport` — единственная точка интеграции:

```rust
fn deliver(&self, envelope: &Envelope) -> Result<()>;
fn fetch(&self, recipient: &Address, now: u64, limit: usize) -> Result<Vec<Envelope>>;
fn ack(&self, recipient: &Address, ids: &[String]) -> Result<usize>;
fn ack_signed(&self, recipient: &Identity, ids: &[String]) -> Result<usize>; // default: ack
```

`MemoryMailbox` — для тестов. Реальный путь: [`FileMailbox`] на диске,
[`GatewayTransport`] через loopback/relay и [`DirectOnionTransport`] напрямую на
onion получателя.

## Gateway API (M3, loopback/Tor)

Gateway (`crates/core/src/gateway.rs`) — единственный listener за onion. Он
хранит только конверт и адрес получателя и маршрутизирует два префикса:

| Запрос | Тело | Ответ |
|---|---|---|
| `POST /v2/foreign` | JSON-RPC кошелька | ответ `grin-wallet` foreign API; owner API недоступен; в relay-режиме 404 |
| `POST /flint/v1/msg` | `{id, to, payload, ttl_secs}`; `id` — UUID конверта, `payload` — opaque envelope JSON | `{id, expires_at}`; повтор с тем же `id` и payload идемпотентен, иной payload — 400 |
| `GET /flint/v1/msg?to=<addr>` | — | `{messages:[{id,to,payload,created_at,expires_at}]}`; **без `from`** |
| `POST /flint/v1/msg/<uuid>/ack` | `{to,id,ts,signature}` | 200 только при валидной подписи получателя и совпадении `id`; иначе 401/404 |
| `GET /flint/v1/health` | — | `{ok:true}` |

- `id` валидируется как UUID (traversal закрыт); ack без получателя невозможен.
- Квоты: 64 КиБ payload, 100 сообщений на получателя, 10 000 всего; TTL
  ограничивается конфигом gateway.
- Принятый сокет принудительно переводится в blocking: под Tor заголовки могут
  прийти следующим пакетом, и nonblocking-чтение иначе закрывало соединение без
  ответа. Есть регрессионный тест с запросом, разбитым на два пакета.
- **Ack** подписан ключом получателя; подпись покрывает `to`, `id` и `ts`.
  Публичный relay и `TorListener` (onion) требуют её
  (`require_signed_ack`); только внутренний тестовый gateway может принимать
  старый `{to}`. Подпись одного сообщения нельзя переиграть на другом `id`;
  окно `ts` — ±300 с.
- `GatewayTransport` в SDK — блокирующий HTTP-клиент к этому API.
  `Messenger::receive` вызывает `ack_signed`, поэтому relay-путь автоматически
  отправляет proof; старый relay просто игнорирует лишние поля.

## Офлайн-relay (M3, 0.1.66)

[`flint-relay`](../operations/RELAY.md) — тот же `Gateway` с `wallet_proxy = false`,
`require_signed_ack = true` и диском. Запускается отдельным процессом, наружу
публикуется через Tor hidden service; кошелька и seed на relay нет.

- A отправляет конверт на relay, даже если B офлайн;
- B забирает позже и удаляет сообщение подписанным ack;
- store переживает перезапуск relay (`relay_store_survives_a_process_restart`);
- квоты/TTL те же; `/v2/foreign` в relay-режиме отвечает 404;
- приложение использует SOCKS собственного Tor для `.onion`-relay, если
  «Приём по Tor» включён, иначе `FLINT_TOR_SOCKS`.

Ограничения: нет PoW/распределённого rate limit, один relay не даёт гарантий
доставки, часы должны совпадать в пределах 5 минут. Публичный инстанс ещё не
развёрнут.

## Адресная книга (0.1.67)

`ContactBook` хранит пары «адрес → имя» одним ChaCha20-Poly1305-блобом под тем
же history-ключом, что и переписка. Файл
`messenger/contacts/<network>/<profile>/contacts.enc` не содержит ни адреса, ни
имени в открытом виде; есть проверка на байты.

- лимит 500 контактов, имя до 64 символов, без control-char;
- адрес нормализуется и валидируется как slatepack;
- атомарная запись, обновление по адресу, удаление;
- UI: список, добавить/изменить/удалить, «Написать» подставляет адрес,
  в истории показывается имя вместо голого адреса.

## Tor-мосты (0.1.69)

Если Tor-узлы блокируются, `TorListener` читает
`<data>/<network>/tor/bridges.txt` и добавляет в `torrc`:

```text
UseBridges 1
ClientTransportPlugin obfs4 exec "…\lyrebird.exe"
ClientTransportPlugin snowflake exec "…\lyrebird.exe"
ClientTransportPlugin webtunnel exec "…\lyrebird.exe"
Bridge obfs4 …
```

Поддерживаются obfs4 / snowflake / webtunnel через единый `lyrebird.exe`
(0.8.x). Он берётся из Tor expert bundle/`pluggable_transports` или задаётся
`FLINT_LYREBIRD_BIN`. Без lyrebird PT-мост даёт явную ошибку. Инструкция и
формат строк — [docs/operations/TOR_BRIDGES.md](../operations/TOR_BRIDGES.md); в UI «Сеть и кошелёк»
есть textarea мостов, статус транспортов/клиентов, перезапуск Tor и `tor.log`.
Автоматического получения мостов нет.

## Локальное хранилище (M2)

`FileMailbox` пишет на диск только подписанный/зашифрованный конверт:

- по одному JSON на сообщение, имя файла — UUID конверта, каталог — адрес
  получателя; traversal невозможен;
- атомарная запись (tmp → rename), повторная доставка идемпотентна;
- TTL-записи удаляются при fetch, ack удаляет только UUID-файлы;
- квоты те же: 100 сообщений / 64 КиБ;
- плейнтекст и адрес отправителя на диск **не попадают** — это проверяется тестом.

Ограничение: это хранилище **конвертов**, а не расшифрованной истории диалогов.
История для UI (шифрованная) — следующий слой M2.

### Шифрованная история (M2)

`History` хранит открытые сообщения для UI:

- ключ 32 байта передаёт приложение (обёрнут `keywrap`, см. ниже);
- ChaCha20-Poly1305, nonce на запись, AAD = адрес peer + id;
- на диске `peer/<uuid>.msg`, виден только адрес peer; тело и отправитель
  зашифрованы;
- лимит на peer (2000), удаление старейших, `conversations()` и `delete()`;
- `expires_at` и удаление просроченных записей при `record/list/conversations`;
  старые записи без поля сохраняются;
- ключ zeroize при drop.

Тесты: roundtrip/порядок, на диске нет plaintext, неверный ключ/подмена,
удаление/UUID, prune, TTL. Всего messenger-тестов — 54 (плюс вектор
onion-адреса и подписанный ack).

### Ключ истории (keywrap, M2)

Случайный 32-байтный ключ истории оборачивается паролем пользователя. Текущий
формат — версия 2: Argon2id (64 МиБ, t=3, p=4) → ChaCha20-Poly1305; AAD связывает
параметры KDF, соль и nonce. Старые версии 1 (PBKDF2-HMAC-SHA256, 100 000
итераций) **читаются** и после успешной разблокировки автоматически
переупаковываются в v2; при смене пароля `rewrap` тоже сразу пишет v2. Приложение
заменяет файл атомарно (tmp → fsync → rename → проверка чтением), поэтому обрыв
не оставляет полузаписанную обёртку.

Ключ теперь **профильный**: `<root>/messenger/history/<network>/<profile>/.history.key`,
рядом с историей этого профиля. Старый общий `<root>/messenger/history.key`
используется только как источник миграции: при первом открытии профиля ключ
переносится в профильный файл, если пароль подходит; общий файл не перезаписывается
другим профилем. Если у профиля уже есть история, но ключ не найден/не подходит,
создавать новый ключ нельзя — сообщения останутся недоступны, а UI покажет
причину. Ошибка истории больше не срывает открытие/создание/восстановление
кошелька: кошелёк открывается, `messenger_status.history_error` объясняет проблему.

Смена пароля подготовлена транзакционно: новая обёртка сначала пишется в
`.history.key.next`, затем меняется пароль кошелька, затем `.next` фиксируется
как основной файл. Обрыв после смены пароля оставляет `.next`, и следующий
unlock с новым паролем подхватывает его; при ошибке смены пароля `.next`
удаляется, старый ключ остаётся.

Это KDF для истории, не для seed; seed — `wallet.seed.v2`, см.
[ARGON2ID.md](../historical/ARGON2ID.md).

8 тестов keywrap: v2 create/unwrap/rewrap, фиксированный v1-блоб и апгрейд,
неверный пароль/подмена, версия, отказ от абсурдных параметров Argon2 до
выделения памяти, короткий nonce без паники, пустой пароль, свежие salt/nonce.
Плюс 5 app-тестов: отдельный ключ на профиль, миграция общего ключа, promotion
`.next`, отказ при истории без ключа, чужой общий ключ не перезаписывается.

### Входящие: история до ack (0.1.69)

`Messenger::fetch_open` только читает и открывает пачку; `ack_signed` вызывается
отдельно. Приложение сначала пишет открытые сообщения в `History` (файл записи
пишется с `fsync`), затем ack-ает. `History::contains` — durable-маркер
дедупликации: повтор после перезапуска/relay-replay не показывается снова.
Битый конверт попадает в `failed` и не обрывает остальные; он не ack-ается, а
UI сообщает число отклонённых. `messenger_op` сериализует inbox/send, чтобы два
вызова не записали и не подтвердили один конверт одновременно.

## Desktop UI (M2)

Экран «Сообщения» (0.1.63, обновлён 0.1.65): адрес с копированием, relay URL,
получатель, текст, «Отправить», «Забрать входящие», история диалогов. Identity —
ключ slatepack-адреса кошелька (embedded/RPC); seed зануляется. History-ключ
создаётся/разворачивается на unlock (Argon2id keywrap v2; v1 обновляется
автоматически), очищается на lock и переворачивается при смене пароля. Relay —
настройка `relay_url`.

**Прямой Tor включается пустым `relay_url` при активном «Приём по Tor».** В этом
режиме `messenger_send` доставляет конверт на onion получателя, а
`messenger_inbox` читает локальный gateway; экран показывает «Прямая доставка по
Tor». Если поле relay заполнено, используется relay/gateway. Прямой режим доступен
только с RPC-бэкендом, потому что `TorListener` привязан к его foreign API.

## Onion-транспорт и публикация gateway (M3, 0.1.65)

- `TorListener` публикует onion из ключа slatepack-адреса и направляет hidden
  service не на кошелёк, а на loopback `Gateway`. Один onion обслуживает
  `POST /v2/foreign` и `/flint/v1/msg*`; owner API наружу не выставлен.
- `TorListener::start` ждёт `Bootstrapped 100% (done)` в `tor.log`, поэтому UI не
  показывает onion, который ещё не умеет строить цепь. SOCKS-порт того же Tor
  используется для исходящих.
- `DirectOnionTransport::new(local_gateway, socks)`:
  - `deliver` выводит onion из адреса получателя и делает POST туда через SOCKS;
  - `fetch`/`ack` идут в локальный gateway, где лежит входящая очередь;
  - relay не участвует.
- Для `.onion` таймаут соединения — 60 с (первая цепь и rendezvous дольше
  clearnet-запроса); clearnet — 15 с.
- Gateway под Tor: принятый сокет переводится в blocking, иначе первый
  nonblocking-read при разбитом запросе закрывал соединение без ответа. Это
  исправлено и покрыто тестом `gateway_waits_for_a_request_split_across_packets`.

Живые прогоны (игнорируются в обычном `cargo test`):

- `tests/direct_onion.rs`: один реальный Tor, два onion из ключей двух
  кошельков, два gateway; Alice → Bob и Bob → Alice доставлены и открыты
  (40,77 с, включая bootstrap).
- `tests/tor_listener_gateway.rs`: путь приложения — `TorListener` публикует
  onion, gateway за ним принимает прямой конверт (37,54 с с ожиданием bootstrap).
- Повтор 0.1.66 с подписанным ack: direct 38,52 с, `TorListener` 70,22 с
  (медленный bootstrap в тот прогон); оба теста зелёные.

**Ограничение:** прямой режим работает, когда получатель онлайн и его gateway
опубликован. Офлайн-доставку даёт только relay/mailbox; TTL 26 ч. Нет
forward secrecy/ratchet, push, групп и вложений.

## Модель угроз

Relay может: удалять/задерживать/дублировать сообщения, видеть получателя,
размер и время, коррелировать трафик, проверять кандидата-отправителя по
публичной подписи. Не может: прочитать payload, подделать отправителя
(получатель проверит подпись), выдать себя за получателя.

Не покрыто в v1 и вынесено в следующие шаги:

- **Нет forward secrecy для получателя:** статический ключ получателя + эфемерный
  ключ отправителя; компрометация долгосрочного ключа расшифрует старые
  сообщения. Нужен ratchet (Double Ratchet/Sealed Sender) в v2.
- **Relay не аутентифицирует отправителя** (только квоты); спам-защита — квоты,
  PoW и проверка на стороне получателя. Ack получателя подписан, поэтому
  удалять чужие сообщения по знанию адреса нельзя.
- **Хранилище переписки** шифрованное (`History`), но replay-набор живёт в
  памяти процесса; после перезапуска защита от повторной выдачи уже ack-нутых
  сообщений опирается на удаление в gateway/relay.
- **Прямой onion** требует онлайн-получателя; offline-очередь даёт relay
  (0.1.67), но публичный инстанс ещё не развёрнут.
- Мобильный MVP — после десктопа, на встроенном кошельке.

## Гейт M1

- [x] спецификация конверта и крипто-схемы;
- [x] SDK-крейт с `Transport`/`Store`;
- [x] тест-вектор адреса, roundtrip, wrong recipient, tamper, forgery, TTL,
  plaintext refusal, квоты, ack/дедупликация, replay и FileMailbox
  (48 тестов крейта);
- [x] keywrap: history-ключ под Argon2id (64 МиБ/t=3/p=4) + ChaCha20-Poly1305,
  чтение v1 и автопереупаковка;
- [x] два клиента обмениваются сообщением через тестовый relay: 13 gateway-тестов,
  включая Alice→Bob через `GatewayTransport`, прямой `DirectOnionTransport`
  и standalone relay с подписанным ack;
- [x] M2-хранилище: `FileMailbox` — только конверты, атомарно, TTL/квоты/ack;
- [x] шифрованная история диалогов (`History`, 0.1.59);
- [x] живой обмен двух реальных кошельков через gateway (`messenger_cli`),
  0.1.62: B открыл сообщение, проверил подпись, ack очистил очередь;
- [x] desktop-UI, шифрованная история, keywrap (0.1.63);
- [x] профильный ключ истории и миграция общего ключа; смена пароля через
  `.history.key.next` (0.1.69);
- [x] `TorListener` требует signed ack на опубликованном onion (0.1.69);
- [x] публикация gateway как hidden service из `TorListener` (0.1.65);
- [x] прямой peer-to-peer onion без relay, живой обмен двух кошельков (0.1.65);
- [x] standalone relay с wallet-free режимом, подписанным ack и store через
  перезапуск (0.1.66);
- [x] шифрованная адресная книга и имена в истории (0.1.67);
- [x] Tor-мосты obfs4/snowflake/webtunnel через `bridges.txt` (0.1.68).
