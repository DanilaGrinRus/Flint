# Tor-мосты для заблокированных сетей

Статус: **0.1.69, dev**. Если провайдер блокирует публичные Tor-узлы, обычный
onion не поднимется. Flint умеет запускать Tor с **obfs4 / snowflake /
webtunnel** мостами, если положить строки мостов в файл.

Это конфигурация для продвинутых: мосты не публикуются в приложении и не
скачиваются автоматически.

## Что нужно

1. Tor expert bundle: `tor.exe` и `pluggable_transports\lyrebird.exe`.
   Lyrebird 0.8.x — единый клиент для obfs4, snowflake и webtunnel.
2. Строки мостов, полученные у Tor Project:
   - <https://bridges.torproject.org/>;
   - письмо `bridges@torproject.org` (отправлять с Gmail/Riseup);
   - готовые obfs4/snowflake/webtunnel-строки.
3. Flint с RPC-бэкендом (`grin-wallet`) — мосты использует `TorListener`.

В портативный zip Tor уже входит; отдельно скачивать expert bundle не нужно.

## Файл мостов

Положите строки в:

```text
<data>\<network>\tor\bridges.txt
```

Для портативной testnet-сборки это обычно:

```text
data\testnet\tor\bridges.txt
```

Формат — по одной строке, `#` — комментарий:

```text
# пример; строки ниже замените настоящими
obfs4 192.0.2.10:443 A1B2C3D4E5F60718293A4B5C6D7E8F9012345678 cert=EXAMPLE iat-mode=0
obfs4 192.0.2.11:8443 1234567890ABCDEF1234567890ABCDEF12345678 cert=EXAMPLE iat-mode=0
```

Можно смешивать типы:

```text
snowflake 192.0.2.12:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://snowflake-broker.example/ fronts=cdn.example/ ice=stun:stun.example:3478
webtunnel 192.0.2.13:443 0123456789ABCDEF0123456789ABCDEF01234567 url=https://webtunnel.example/ ver=0.0.3
```

Flint сам добавит:

```text
UseBridges 1
ClientTransportPlugin obfs4 exec "…\lyrebird.exe"
ClientTransportPlugin snowflake exec "…\lyrebird.exe"
ClientTransportPlugin webtunnel exec "…\lyrebird.exe"
Bridge …
```

Lyrebird реализует все три транспорта; отдельные `snowflake-client.exe` и
`webtunnel-client.exe` не нужны. Если `lyrebird.exe` не найден, запуск Tor
завершится ошибкой с подсказкой. Путь можно задать явно:

```powershell
$env:FLINT_LYREBIRD_BIN = "C:\Tor\pluggable_transports\lyrebird.exe"
```

## UI

В окне «Сеть и кошелёк» есть секция **«Мосты для проблемных сетей»**:

- textarea для строк мостов и `bridges.txt` в `<data>\<network>\tor`;
- кнопка «Сохранить мосты»;
- «Сохранить и перезапустить Tor»;
- статус: сколько строк, какие транспорты и найден ли клиент;
- `tor.log` с последними 200 строками и кнопкой обновления;
- кнопка включения/выключения Tor и onion-адрес.

Мосты по-прежнему не получаются автоматически: строки нужно вставить из
BridgeDB/письма Tor Project.

## Запуск

1. Остановите Tor в Flint (кнопка «Приём по Tor» / перезапуск).
2. Создайте `bridges.txt`.
3. Запустите Tor снова; Flint запишет `UseBridges` в `torrc` и запустит нужный
   `*-client.exe`/`lyrebird.exe`.
4. Проверьте статус: onion должен появиться после `Bootstrapped 100% (done)`.

Проверки: `cargo test -p flint-core tor_listener::tests` — obfs4, snowflake с
единым lyrebird, отсутствие lyrebird и plain bridge. Slint Settings → «Мосты Tor»
сохраняет строку obfs4 и показывает диагностику; UIA-прогон на
`lastver/Flint-0.1.69-keyboard/Flint.exe` показывает найденный клиент и строки
`Bootstrapped`, то есть Tor и PT уже в zip.

Если Tor всё равно не поднимается:

- проверьте, что строки мостов не истекли (BridgeDB меняет их);
- проверьте `data\<network>\tor\tor.log` — там будет причина;
- попробуйте другой транспорт (obfs4 → snowflake);
- некоторые сети блокируют и мосты; тогда нужен другой канал (VPN/прокси),
  который Flint не обходит.

## Ограничения

- Автоматического получения мостов нет.
- Мосты не дают анонимности лучше обычного Tor; оператор моста видит трафик,
  как любой Tor-узел.
- Flint не логирует тела сообщений; `tor.log` содержит только диагностику Tor.
- Публичный relay для этого не нужен: мосты влияют на транспорт, а не на
  доставку.
