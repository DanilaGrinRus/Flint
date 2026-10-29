# Интеграция grin-wallet (FFI) — инструкция

Цель: заменить `MockWalletService` на реальный блокчейн-слой, реализовав
протокол `WalletService` поверх официального
[grin-wallet](https://github.com/mimblewimble/grin-wallet) (Rust).

## Шаг 1. Сборка Rust-библиотеки

На macOS с установленным Rust:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
git clone https://github.com/mimblewimble/grin-wallet
cd grin-wallet

# статическая библиотека для устройства и симулятора
cargo build --release --target aarch64-apple-ios
cargo build --release --target aarch64-apple-ios-sim
```

Результат: `target/<triple>/release/libgrin_wallet_ffi.a`
(имя зависит от того, есть ли готовый FFI-модуль; при необходимости добавить
тонкий crate-обёртку `grin-wallet-ffi` с `#[no_mangle] pub extern "C"` функциями).

## Шаг 2. C-API (cbindgen)

Минимальный набор функций обёртки:

```rust
#[no_mangle]
pub extern "C" fn grin_wallet_init(mnemonic: *const c_char, password: *const c_char) -> *mut WalletHandle;
#[no_mangle]
pub extern "C" fn grin_wallet_recover(mnemonic: *const c_char, password: *const c_char) -> *mut WalletHandle;
#[no_mangle]
pub extern "C" fn grin_wallet_balance(handle: *mut WalletHandle) -> *const c_char; // JSON
#[no_mangle]
pub extern "C" fn grin_tx_create(handle: *mut WalletHandle, amount: u64, message: *const c_char) -> *const c_char; // slatepack
#[no_mangle]
pub extern "C" fn grin_tx_finalize(handle: *mut WalletHandle, slatepack: *const c_char) -> i32;
#[no_mangle]
pub extern "C" fn grin_tx_receive(handle: *mut WalletHandle, slatepack: *const c_char) -> *const c_char; // ответный slatepack
#[no_mangle]
pub extern "C" fn grin_scan_outputs(handle: *mut WalletHandle) -> i32;
```

Генерация заголовка:

```bash
cargo install cbindgen
cbindgen --lang c --crate grin-wallet-ffi -o include/grin_wallet_ffi.h
```

## Шаг 3. XCFramework

```bash
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libgrin_wallet_ffi.a -headers include \
  -library target/aarch64-apple-ios-sim/release/libgrin_wallet_ffi.a -headers include \
  -output GrinWalletFFI.xcframework
```

Добавить `GrinWalletFFI.xcframework` в проект Xcode (embed: не требуется для
статической lib, но `Headers` должны быть видны модулям Swift через bridging
header или `module.modulemap`).

## Шаг 4. Swift-обёртка

```swift
actor GrinWalletFFIService: WalletService {
    private var handle: UnsafeMutableRawPointer?

    init(mnemonic: String, password: String) throws {
        handle = grin_wallet_init(mnemonic, password)
        guard handle != nil else { throw WalletError.notImplemented("init failed") }
    }

    func prepareSend(amount: Decimal, message: String, feeTier: FeeTier) async throws -> SendProposal {
        // 1) grin_tx_create(amount_in_nanogrin, message) -> slatepack
        // 2) обновить локальный кэш баланса (locked)
        // Никогда не вызывать FFI из главного потока — actor это гарантирует.
    }

    func receive(slatepack: String, message: String) async throws -> TransactionRecord {
        // grin_tx_receive(slatepack) -> ответный slatepack + зачисление
    }

    func refresh() async throws {
        // grin_scan_outputs() + баланс по выходам с ноды
    }
}
```

Инициализация во `ContentView`: `WalletViewModel(service: GrinWalletFFIService(...))`.

## Шаг 5. Хранение секретов

- Seed (24 слова BIP-39) — Keychain (`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`),
  никогда в UserDefaults / файлах.
- БД выходов — SQLCipher, ключ в Keychain.
- Пароль кошелька — держать в памяти только внутри actor-сессии, вытеснять
  по таймауту (фаза 2).

## Шаг 6. Нода

REST API ноды Grin (по умолчанию порт 3413): `GET /v1/status`, `GET /v1/chain`,
`GET /v1/txhashset/outputs`. Для продакшена — свой узел или доверенный публичный
(например, grinnode.live), TLS обязателен.

## Что заменить в существующем коде

| Сейчас | После интеграции |
|---|---|
| `SlatepackCodec.encode` (демо-payload) | `grin_tx_create` — настоящий зашифрованный Slatepack |
| `SlatepackCodec.isValid` (проверка префикса) | `grin_tx_receive` + полный парсинг у Rust |
| `MockWalletService` баланс из памяти | `grin_scan_outputs` + баланс из ноды |
| Демо-slatepackAddress | адрес из публичного ключа кошелька (X25519) |

## Тестирование

1. Юнит-тесты `MockWalletService` — уже являются контрактными тестами протокола;
   прогнать их же против `GrinWalletFFIService` в тестнете.
2. Сценарий «кошелёк A → кошелёк B»: создать Slatepack в A, принять в B,
   вернуть Slatepack в A, финализировать.
3. CI: GitHub Actions macOS runner — `cargo build` + `xcodebuild test`.
