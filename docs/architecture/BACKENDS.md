# Бэкенды: embedded и grin-wallet

**Статус 0.1.69:** оба бэкенда поддерживаются. Embedded нужен для мобильного
направления и полностью офлайн-сценария; `grin-wallet` — для миграционной
совместимости и функций, которых нет в embedded.

## Явные возможности

Команда `backend_capabilities` возвращает:

```json
{
  "mode": "embedded" | "grin-wallet",
  "wallet": true,
  "tor": false | true,
  "rpc_delete": true,
  "scan_progress": true | false,
  "offline_mobile": true | false
}
```

Это единственный контракт, по которому UI/автоматизация должны выбирать
доступные функции; строка `backend` в настройках не является capability API.

## Почему embedded пока не удалён

- мобильный сценарий не может запускать отдельный `grin-wallet.exe`;
- offline-кошелёк и `flint-selftest` опираются на embedded;
- удаление без `WalletBackend` trait означало бы переписать вызовы в runtime и
  потерять возможность тестировать оба пути.

## Жёсткий гейт на удаление embedded из desktop

Удаление embedded из desktop-сборки не раньше **0.2.0** и только после:

1. появления trait-абстракции `WalletBackend` с двумя реализациями;
2. parity-тестов create/open/restore/send/receive/scan/delete;
3. отдельного мобильного шелла, который использует embedded напрямую;
4. одного релиза, где обе реализации проходят полный suite.

До этого embedded остаётся функциональным, а не «legacy без поддержки».
