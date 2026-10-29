# Messenger: протокол v1 и устойчивое хранение

Код: crates/messenger и crates/runtime/src/messenger.rs. Назначение — обмен
зашифрованными slatepack и короткими сообщениями по адресу кошелька. Группы,
вложения, ratchet и публичный relay-сервис в этот контракт не входят.

## Идентичность и envelope

Идентичность — bech32 ed25519 slatepack address: tgrin/testnet, grin/mainnet.
Новые долгосрочные ключи не создаются. Из того же адресного ключа выводится
Tor v3 onion; sender/recipient по одному адресу находят online peer без каталога.
Фиксированные векторы находятся в тестах crates/core и crates/messenger.

JSON envelope, бинарные поля base64:

| Поле | Тип / смысл |
|---|---|
| version | u8, 1 |
| id | UUID, идентичность доставки и ACK |
| recipient | Slatepack address получателя |
| created_at / ttl | Unix seconds / срок в секундах |
| ephemeral_key | 32-byte X25519 public отправителя |
| nonce | 12-byte ChaCha20-Poly1305 |
| ciphertext | Зашифрованный payload |
| pow_nonce / pow_bits | Proof-of-work nonce/сложность; 0 у legacy |
| signature | 64-byte ed25519 подпись отправителя |

Payload внутри ciphertext: sender, type=text|slatepack, text либо armor,
для slatepack encrypted=true, created_at. Plaintext slatepack (encrypted=false)
запрещён как при seal, так и при open. Адрес sender и тип не передаются отдельными
открытыми полями, но это не полное сокрытие отправителя — см. модель угроз.

## Crypto transcript

Ed25519 seed преобразуется через SHA512(seed)[..32] в X25519 scalar; public —
через Edwards→Montgomery. Sender создаёт ephemeral X25519, DH с recipient public,
HKDF-SHA256(salt=recipient X25519 public,
info=flint-messenger/v1/encryption) даёт 32-byte ключ.

ChaCha20-Poly1305 AAD связывает version, id, recipient, created_at и ttl
с length-prefix строк. Domain-separated ed25519 transcript связывает эти поля,
ephemeral_key, nonce, длину и ciphertext; у PoW-конверта также pow_nonce/pow_bits.
SHA-256 PoW связывает id/recipient/created_at/ttl, ciphertext commit и nonce.
Gateway с require_pow проверяет его до записи; recipient — до расшифровки.
Параметры/порог берутся из закреплённой версии кода, не меняются одной правкой docs.

Recipient после расшифровки видит sender и проверяет подпись его адресным ключом.
Relay не может подделать подпись, но может проверить известного ему кандидата:
подпись и transcript публичны. Исправление этой metadata-утечки требует нового
протокола со скрытой подтверждающей подписью, не переименования поля v1.

## Доставка, TTL и gateway

Доставка at-least-once; повтор доставляет прежний подписанный envelope с тем же id.
TTL проверяется mailbox и recipient; DEFAULT_TTL=93600 секунд (26 ч),
MAX_TTL=604800 (7 дней). Ограничение конкретного gateway может быть строже.
Дедупликация mailbox по id и тому же payload; конфликтующий payload отклоняется.
ACK после проверки содержимого/подписи и устойчивого локального решения.

| API | Тело / результат |
|---|---|
| POST /flint/v1/msg | {id,to,payload,ttl_secs} → {id,expires_at} |
| GET /flint/v1/msg?to=address | messages с id/to/payload/created_at/expires_at, без from |
| POST /flint/v1/msg/uuid/ack | {to,id,ts,signature}; подпись recipient, иначе 401/404 |
| GET /flint/v1/health | {ok:true} |
| POST /v2/foreign | Прокси foreign API только в wallet gateway; standalone relay — 404 |

Owner API не публикуется. id валидируется как UUID, traversal запрещён.
ACK transcript связывает to/id/ts; окно времени ±300 с. Знание адреса не даёт
права удалить чужое сообщение. Legacy неподписанный ACK допускается только
в явно внутреннем тестовом gateway. Relay и опубликованный Tor listener требуют
signed ACK. Серверу и клиенту нужны согласованные часы.

Relay defaults: до 100 сообщений на получателя, payload≤64 КиБ, 10000 всего;
TTL/rate/connection limits задаются GatewayConfig. P2P может использовать более
строгие defaults. Квоты учитывают сохранённый store после restart; переполнение
одного recipient не должно расходовать квоту другого, storage failure не выдаёт
ложный успех. Лимит ответа применяется до неограниченного чтения/аллокации.
Никаких текстов сообщений/секретов в логах relay.

Transport API: deliver(envelope), fetch(recipient,now,limit), ack и ack_signed.
MemoryMailbox служит тестам; FileMailbox — дисковым конвертам;
GatewayTransport — gateway/relay; DirectOnionTransport — onion адресата.

## Local store, history и contacts

FileMailbox хранит только подписанные encrypted envelopes: UUID-файлы в каталоге
recipient, атомарная запись, TTL pruning, квоты и signed ACK. Это не расшифрованная
история. History шифрует записи ChaCha20-Poly1305 под 32-byte профильным ключом,
AAD=peer+id, новый nonce на запись; на диске виден peer path, тело/отправитель
зашифрованы. Лимит и TTL ограничивают хранение; ключ zeroize при drop.

ContactBook шифрует адреса/имена тем же history-key в
messenger/contacts/network/profile/contacts.enc. Нормализация адреса, до 500
контактов, имя≤64 символов без control chars, cross-instance lock, уникальный temp,
атомарная запись. Устаревший lock диагностируется; его прежний holder не может
удалить уже заменённую чужую блокировку.

## Ключ истории и пароль

Ключ отдельный у каждого network/profile:
messenger/history/network/profile/.history.key. Keywrap v2: Argon2id 64 МиБ,
t=3,p=4 → ChaCha20-Poly1305; AAD связывает KDF/salt/nonce. V1 PBKDF2-HMAC-SHA256
100000 читается и после успешного unlock переупаковывается в v2. Запись с flush,
атомарным продвижением и readback. Абсурдные KDF parameters отвергаются до памяти.

Прежний общий messenger/history.key — только источник миграции. Если история уже
есть, но ключ отсутствует/не подходит, нельзя создать новый ключ поверх неё.
Ошибка истории отображается отдельно и не подменяет исходный кошелёк.
Смена пароля: подготовить .history.key.next, поменять wallet password, затем
продвинуть next. Crash после wallet change восстанавливает next новым паролем;
ошибка wallet change оставляет старый keywrap. Seed не расшифрует старую историю.

## Durable inbox/outbox и автоматизация

Outbox сохраняет уже созданный envelope до deliver; чувствительный kind хранится
зашифрованным (kind_ciphertext). Retry доставляет тот же id, не подписывает новый.
Успешное помещение в очередь отображается как queued, а не chain confirmation.

Полученный armor сначала durably ставится в inbox task, затем фиксируется history,
затем подписанный ACK. Ошибка ACK возвращается отдельно и не скрывает уже открытые
сообщения; следующий poll повторяет ACK. История — durable дедупликация показа.
Повреждённый envelope получает локальное quarantine/tombstone до попытки ACK,
чтобы poison message не ломал всю пачку; блокированный sender не попадает в историю.

Inbox использует received/processing/failed/needs_review/reply_sent и завершение
через done. reply_id связывает задачу с сохранённым ответом outbox; restart не должен
создавать новый денежный ответ вместо продолжения прежнего. В текущем API done
удаляет inbox entry: его нельзя описывать как вечный отдельный processed marker.
Инварианты повторного денежного эффекта обеспечиваются вместе с wallet recovery,
history и transport ids; проверяются отдельным ACK-loss/crash сценарием.

Автообработка выключена по умолчанию, opt-in только для сохранённых контактов.
Истинный S1 проходит prepare/finalize receive; S2 сопоставляется с pending send.
Ручной путь сохраняет проверку суммы/адреса. Ни очередь, ни HTTP 200 не означают
confirmed; доверенное подтверждение требует finalize/post/chain проверки.

## Tor/direct и пределы приватности

TorListener выводит onion из slatepack key и публикует loopback gateway, а не owner
API. Готовность onion — после bootstrap; outgoing использует SOCKS того же Tor.
Direct deliver идёт на onion получателя; fetch/ack — в локальный gateway.
Direct требует online peer; offline mailbox даёт relay. Общая эксплуатация и
мосты — [OPERATIONS.md](../OPERATIONS.md).

Relay может задерживать/удалять/дублировать сообщения, коррелировать recipient,
размер/время и проверять гипотезу sender. Не может прочитать payload или подделать
проверяемую подпись. Forward secrecy нет: долгосрочный recipient key позволяет
расшифровать старый трафик при его компрометации. Ratchet/Sealed Sender, push,
группы/вложения и distributed abuse control — отдельный будущий протокол/сервис.
Unit и локальный relay тест не подтверждают доступность публичного сервиса.

## Nostr transport v1 (NIP-44/NIP-59, ADR NOSTR-1)

Отдельный транспорт доставки тех же подписанных/зашифрованных Flint-envelope:
Nostr relay вместо HTTP gateway. Он не заменяет relay/direct и не включён по
умолчанию; выбор транспорта явный, тихого fallback нет.

### Ключи и данные

- Транспортный ключ — отдельный secp256k1 x-only ключ на профиль/сеть,
  `messenger/nostr/transport.key` (hex, 32 байта, ACL только владельцу).
  Он не выводится из wallet seed и не может распоряжаться средствами; его
  компрометация раскрывает сообщения, адресованные этому ключу, но не кошелёк.
- Ротация — замена файла; старые сообщения после ротации не расшифровываются,
  поэтому UI обязан предупредить до замены. Общий файл между профилями запрещён.
- Публичный ключ (hex, x-only) — адрес доставки, которым обмениваются контакты
  по доверенному каналу; сам по себе он не доказывает личность.

### Конверт

- Rumor: kind 14, `content` = JSON Flint-envelope (arms/поля сохраняются как есть),
  тег `p` = получатель, `created_at` = время создания envelope.
- Seal: kind 13, `content` = NIP-44 v2 (conversation key отправителя и получателя),
  подпись ключом отправителя; pubkey seal и есть sender transport key.
- Gift wrap: kind 1059, `content` = NIP-44 v2 от эфемерного ключа к получателю,
  тег `p` только получателя; `created_at` — случайное в пределах 2 суток назад.
- Отправитель и получатель проверяют подпись события до расшифровки (BIP-340).
  Номер версии NIP-44 обязан быть 2; другой — явная ошибка, не «пустое сообщение».

### Доставка, выборка и ACK

- `deliver` публикует gift wrap на настроенные relay и считается успешным только
  после подтверждения приёма relay; ошибка одного relay не выдаётся за успех всех.
- `fetch` подписывается на kind 1059 с `#p` = наш ключ за окно TTL, дедуплицирует
  по event id, распаковывает и возвращает Flint-envelope; повреждённый wrap
  отправляется в quarantine-счётчик, а не роняет пачку.
- `ack` — NIP-09 deletion request (kind 5, тег `e`) от ключа получателя,
  best-effort: relay может проигнорировать. Подтверждение перевода всё равно
  доказывается только цепью; ACK транспорта не означает confirmed.
- Релей-адрес: один `wss://` URL в настройках (v1). `ws://` разрешён только для
  loopback в тестах. Пустой адрес — транспорт выключен.

### Ограничения и угрозы

- Размер события ≤ 64 КиБ; envelope сверх лимита отклоняется до публикации.
- Метаданные видны relay: pubkeys, время (искажено только у wrap), размер.
  IP виден relay, если трафик не идёт через Tor; встроенного прокси v1 не имеет.
- Нет forward secrecy и пост-компрометационной защиты — как и у остального
  messenger v1; для высокорисковых сценариев это не рекомендуется.
- NIP-17-специфичные функции (группы, вложения, реакции) не входят в v1.
- Новые зависимости: `secp256k1` (BIP-340/ECDH) и WebSocket-клиент; это меняет
  vendor, SBOM и повторно открывает проверку зависимостей.

### Проверка

- Unit: официальные NIP-44 v2 векторы (conversation key, message keys, padding,
  encrypt/decrypt, invalid cases); roundtrip seal/gift wrap на фиксированных ключах;
  проверка подписи и отказа при подмене; лимит размера; неизвестная версия.
- Relay: отдельный live-тест только по явному флагу и на тестовом relay; без него
  сборка и unit-тесты не означают доступность публичного relay.
- Приёмка: сообщение, доставленное через Nostr, проверяется тем же Flint-envelope
  путём (подпись/дедуп/quarantine), а не «Nostr сказал ok».
