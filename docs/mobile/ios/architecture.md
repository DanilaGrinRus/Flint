# Архитектура GrinFlint

## Общая схема

```
Views (SwiftUI)  →  WalletViewModel (@Observable)  →  WalletService (protocol)
                                                          │
                                    ┌─────────────────────┴─────────────────────┐
                             MockWalletService                        GrinWalletFFIService
                             (демо, сейчас)                           (grin-wallet через FFI, фаза 1)
```

UI никогда не обращается к блокчейну напрямую — только через протокол
`WalletService`. Благодаря этому демо-режим и продакшен отличаются одной
строчкой: чем инициализирован `WalletViewModel`.

## Слои

### Views — `GrinFlint/Views/`

| Файл | Назначение |
|---|---|
| `PortfolioView.swift` | Портфель: баланс, действия, последние операции |
| `SendFlowView.swift` | Отправка: сумма → сообщение → комиссия → Slatepack; `SendResultSheet` |
| `ReceiveView.swift` | Адрес кошелька + приём Slatepack; `ReceiveResultSheet` |
| `TransactionsView.swift` | История с группировкой по дням + `TxDetailView` |
| `SettingsView.swift` | Нода, seed, Face ID, о приложении |
| `Components.swift` | `TxRow`, `CopyableField`, `ErrorBanner` — общие элементы |

Навигация: `ContentView` (TabView) → `NavigationStack` на каждом табе.
Вью-модель достаётся через `@Environment(WalletViewModel.self)`.

### ViewModel — `GrinFlint/ViewModels/WalletViewModel.swift`

`@Observable`-класс. Держит копию `Balance` и `[TransactionRecord]`,
синхронизирует их с сервисом после каждой операции. Все методы async —
сервис может быть медленным (сеть, Rust).

### Сервисный слой — `GrinFlint/Services/`

**`WalletService` (protocol)** — контракт блокчейн-слоя:
`refresh()`, `prepareSend(amount:message:feeTier:)`, `receive(slatepack:message:)`,
`exportSlatepack(txID:)`, плюс свойства `balance`, `transactions`, `slatepackAddress`.

**`MockWalletService`** — демо: память, блокировка/разблокировка средств,
симуляция подтверждения на refresh.

**`Bech32`** — кодировщик BIP-0173 (нужен для Slatepack).

**`SlatepackCodec`** — формат сообщений Grin. Сейчас демо-полезная нагрузка
(без шифрования); в продакшене заменяется на вызовы `slatepack_encrypt/decrypt`
grin-wallet.

### Модели — `GrinFlint/Models/WalletModels.swift`

`Balance`, `TransactionRecord`, `TxType`, `TxStatus`, `FeeTier`,
`SendProposal`, `WalletError`, форматирование `GrinFormat` (9 знаков).

## Тема — `GrinFlint/Theme.swift`

Тёмная тема в духе Flint: фон почти чёрный, поверхности `surface`,
акцент — фирменный жёлтый Grin. `GrinCard`, `GrinButtonStyle`,
`SectionHeader` — общий UI-язык всех экранов.

## Конвенции

- iOS 17+: `@Observable`, `ContentUnavailableView`, `presentationDetents`.
- Суммы — только `Decimal`, никогда `Double` (9 знаков наногрин).
- Ошибки домена — `WalletError: LocalizedError`, показываются алертами/баннером.
- Секреты не логировать; seed живёт только в Keychain (фаза 1) и в памяти.
