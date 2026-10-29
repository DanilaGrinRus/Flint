# Сборка Flint из исходников

## Что внутри

```
Cargo.toml, Cargo.lock     workspace (Rust 2021)
crates/core                кошелёк Grin + менеджер ноды + gateway; bin/flint-relay
crates/messenger           конверт, transports, history, signed ack, contacts
crates/janus               слой свопов (машина состояний, таймлоки)
crates/runtime             WalletRuntime: RPC, Tor, messenger, ключ истории
crates/slint-ui            единственный UI: flint-slint
crates/grin-wallet-rpc     обёртка upstream grin-wallet (бинарь grin-wallet)
tools/package-slint.ps1    portable preview + selftest
tools/fetch-node.ps1       загрузка официальной ноды grin v5.5.2
tools/run-testnet-node.ps1 testnet-нода для живых гейтов (start/status/stop)
docs/                      статус (STATUS.md), Slint (SLINT.md), удаление Tauri
docs/assets/shots/         скриншоты для README и SLINT
```

Зависимости **не включены** (в рабочем каталоге они занимали ~926 МБ в `vendor/`).
При первой сборке cargo скачает их с crates.io.

## Требования

- Rust stable (MSVC toolchain) — `rustup default stable-msvc`
- Visual Studio Build Tools 2022 (C++ build tools) + Windows SDK
- WebView2 **не нужен**: UI нативный Slint

## Сборка

```powershell
cargo build --release -p flint-slint
cargo build --release -p flint-runtime --bin flint-selftest
cargo build --release -p flint-core --bin flint-relay
```

Relay — отдельный бинарник, см. [docs/operations/RELAY.md](docs/operations/RELAY.md).

Для портативной сборки нужны `resources/node/grin.exe`,
`resources/wallet/grin-wallet.exe` и Tor expert bundle:

```powershell
tools\fetch-node.ps1
tools\fetch-wallet.ps1
pwsh -File tools\package-slint.ps1
```

## Офлайн-сборка

Если сети нет, зависимости можно завендорить:

```powershell
cargo vendor vendor
```

и добавить в `.cargo/config.toml`:

```toml
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
```

## Запуск

```powershell
cargo run -p flint-slint                     # окно
cargo run -p flint-runtime --bin flint-selftest  # headless-проверка
pwsh -File tools\package-slint.ps1           # preview + selftest
```

## Проверка

```powershell
cargo test --workspace          # 274 теста; 26 #[ignore] — цепь/RPC/Tor/relay
pwsh -File tools/ci.ps1         # fmt/check/tests/Slint build/selftest
```

Тесты: жизненный цикл кошелька, RPC, скан, Tor, messenger, переключение сети,
свопы Janus, старт/стоп реальной ноды, pending-записи, seed-envelope.
