# Удаление Tauri — выполнено

> **HISTORICAL:** журнал решения; актуальная оболочка — `crates/slint-ui`.

Tauri удалён из проекта **26 октября 2026** после явного согласия пользователя.
Production- и единственный UI — нативный Slint (`crates/slint-ui`).

## Backup

Перед удалением сохранены:

- `crates/app` (Tauri-шелл, `tauri.conf.json`, `capabilities/`, `--selftest`);
- `ui/` (ES-модули, CSS, vendor qrcode);
- `tools/` и `docs/` в состоянии до удаления;
- `Cargo.toml`, `Cargo.lock`, `README.md`, `AGENTS.md`.

Каталог: `backup/tauri-removal-20261026-091749/`. Восстановление — скопировать
обратно и вернуть `crates/app`/`ui` в `Cargo.toml` и `tauri`-зависимости.
Целостность: `BACKUP_MANIFEST.sha256`, проверено **80/80** файлов.

## Что удалено

- `crates/app/` и `ui/`;
- workspace-member `crates/app`, зависимости `tauri`/`tauri-build`;
- Tauri-only tools: `verify-ui.mjs`, `serve.mjs`, `package.ps1`,
  `live-scan-cancel.mjs`, `live-rpc-*.mjs`, `live-history-recovery.mjs`,
  `measure-ui.mjs`, `check-qr.mjs`, `check-address-tail.mjs`;
- Tauri-проверки в `ci.ps1`; release/package/source/sbom переведены на Slint.

## Что осталось в runtime и Slint

- `WalletRuntime`: кошелёк, RPC `grin-wallet`, скан, деньги, seed, switch/settings,
  shutdown;
- `TorRuntime`: start/stop/status/bridges/log, остановка на restart RPC;
- messenger и ключ истории (`messenger.rs`, `history.rs`), включая recovery;
- Slint: кошелёк, история, pending, свопы, настройки, Tor (мосты/лог), messenger
  (отправка/входящие/контакты/blocked/recovery).

## Гейты после удаления

- `cargo fmt --all -- --check`;
- `cargo check --workspace --all-targets --offline`;
- `cargo test --workspace` — **256 passed, 0 failed, 19 ignored**;
- funded testnet send на копии `dist/data`: 1 GRIN, fee 23 500 000 nano,
  tx `dbcd23a3…`; prepared snapshot аннулируется при правке адреса;
- `cargo run -p flint-runtime --bin flint-selftest` — `ok:true`;
- `cargo build -p flint-slint` и package `tools/package-slint.ps1`;
- `flint-selftest.exe` из preview — `ok:true`.

## Приёмка доступности/DPI (2026-10-26)

Проверено на текущем мониторе **DPI 96 (100%)**, окно 1120×760:

- UIA: у lock screen названы `Пароль`/`Разблокировать`; Tab-порядок
  `Пароль → Разблокировать` повторяется, без безымянных остановок;
- кошелёк: кнопки `Заблокировать`, `Настройки`, `Сообщения`, `Копировать адрес`,
  `Получить`, `Отправить`, `Сканировать` имеют имена и `IsOffscreen=false`;
- Settings: `Мосты Tor`, `Запустить Tor`, `Сохранить мосты`, `Показать лог`
  доступны без offscreen;
- Messenger: поля контакта, `Сохранить`, `Заблокировать`,
  `Восстановить старую историю` доступны; offscreen-контролов **0**;
- Unicode: контакт `Тест` вставлен через ValuePattern и отрисован.

Не проверено в этой сессии:

- NVDA/Narrator, реальный IME (ввод через IME, не вставка Unicode),
  полный Tab-аудит остальных модалок;
- фактическая чистая Windows без UCRT/VC++ (runtime теперь bundled,
  проверены packaged selftest и имитация отсутствующего импорта);
- подпись release-сертификатом.

Проверено дополнительно: 900×760 Settings, `SLINT_SCALE_FACTOR=1.25/1.5`,
перенос окна на второй монитор.
