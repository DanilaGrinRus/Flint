# RustSec: отдельный трек обновления зависимостей

Статус: **выполнен настолько, насколько позволяют upstream/fork.** Закрыты
`curve25519-dalek 3.2.0` (age 0.11.5), `yaml-rust 0.3.5` (clap 4.6.7) и
`zip 0.5.13` (zip 9.1.3). Осталось **2 advisories**: `crypto-bigint` (не
собирается текущими features) и `rsa` (нет патча; arti 0.47 всё ещё rsa 0.9).
Это не приёмка безопасности и не разрешение на mainnet. Исходный аудит —
[`RUSTSEC_2026-10-28.md`](RUSTSEC_2026-10-28.md).

## Зачем отдельный трек

Семь исходных advisories нельзя было убрать точечным `cargo update`: они приходили
через fork `grin-wallet`, upstream `arti` и CLI-стек. Обновление затрагивает
cryptographic-формат (`age`/slatepack), распаковку txhashset (`zip`) и аргументы
`grin-wallet` (`clap`). Поэтому оно ведётся отдельным треком, а не в общей
очереди автофинализации.

Ограничения:

- сборка офлайн (`vendor/` + `.cargo/config.toml`), поэтому любое обновление
  требует `cargo vendor vendor` с сетью и проверки diff вендора;
- нет `.git`: перед изменением `Cargo.lock`, `third_party/` и `vendor/` делать
  снимок и проверять `snapshots/tools/verify_snapshot.py`;
- testnet only; mainnet запрещён до отдельного решения;
- внешние PS, публичный relay, Janus и mobile в трек не входят.

## Инвентарь

| Advisory | Пакет | Текущий | Патч | Владелец перехода | Работа |
|---|---|---|---|---|---|
| RUSTSEC-2026-0387 | `crypto-bigint` | 0.5.5 | `>= 0.7.6` | `elliptic-curve`/`p256`/`k256` → arti | Поднять кривую/arti; сейчас пакет не входит в `cargo tree` текущей сборки |
| RUSTSEC-2024-0344 | `curve25519-dalek` | 4.1.3 (locked) | `>= 4.1.3` | `x25519-dalek 2.0.1` → `age 0.11.5` → fork `grin-wallet` | **Готово:** 0.11.5 уже на x25519-dalek 2 и не тянет p256/crypto-bigint, как 0.12 |
| RUSTSEC-2023-0071 | `rsa` | 0.9.10 | нет патча | `ssh-key-fork-arti` → `tor-key-forge` → `arti-client 0.43` | **Upstream-блокер:** arti 0.47 всё ещё использует rsa 0.9; патча нет |
| RUSTSEC-2018-0006 | `yaml-rust` | удалён | `>= 0.4.1` | clap 4.6.7 + собственный YAML-адаптер | **Готово:** yaml-rust ушёл; YAML CLI работает через `serde_yaml` |
| RUSTSEC-2026-0367/68/69 | `zip` | 9.1.3 (locked) | `>= 9.0.0` | `grin_util` fork | **Готово:** zip.rs адаптирован, `grin_util` zip-тест зелёный |

## Порядок

1. ~~**age / curve25519-dalek**~~ — **выполнено**: `age 0.11.5` в
   `third_party/grin-wallet/libwallet`, уже с `x25519-dalek 2.0.1`; версия 0.12
   не используется, потому что добавляет p256/crypto-bigint. API
   `Encryptor`/`Decryptor` адаптирован; slatepack/workspace зелёные.
2. ~~**zip / grin_util**~~ — **выполнено**: zip 9.1.3, `SimpleFileOptions`,
   `finish(self)` и fallible `mangled_name`; `cargo test -p grin_util zip`.
3. ~~**clap / grin-wallet CLI**~~ — **выполнено** на clap 4.6.7: собственный
   YAML→`Command` адаптер на `serde_yaml` вместо удалённого clap-YAML;
   `yaml-rust` полностью ушёл из lock. `grin-wallet --help` и подкоманды
   работают.
4. **arti / rsa + crypto-bigint**: поднять arti до 0.47 проверено по исходникам —
   `tor-key-forge 0.47` всё ещё зависит от `rsa 0.9`, патча RSA нет. Это
   **upstream-блокер**; `crypto-bigint` в текущую сборку не входит.
5. После каждого шага — `tools/rustsec.ps1`; удалять из accepted только те ID,
   которые действительно исчезли из отчёта.

## Гейты приёмки

Для каждого шага:

```powershell
$env:TMP=$env:TEMP='C:\DHW\.tmp'
cargo fmt --all -- --check
cargo test --workspace
pwsh -File tools/rustsec.ps1
pwsh -File tools/ci.ps1
```

Дополнительно:

- `cargo tree -i <package>` фиксирует, что старый пакет больше не собирается;
- `cargo vendor vendor` — диф вендора осмотрен, сборка офлайн проходит;
- `flint-selftest` — `ok:true`, `reopened_matches:true`;
- реальный testnet: минимум один receive и один response/`finalize_send` после
  изменения `age`/`zip`; mainnet не запускать;
- `snapshots/tools/verify_snapshot.py` — `match:true`.

## Критерий закрытия трека

- все семь advisories исчезли из `cargo-audit`, либо для каждого оставшегося
  есть явное решение «нет патча upstream» с пересмотренной оценкой;
- `Cargo.lock` и `vendor/` воспроизводимы офлайн;
- полный набор тестов и selftest зелёные;
- обновлены `docs/security/RUSTSEC_2026-10-28.md`, `docs/status/STATUS.md`,
  `docs/WORK_LOG.md` и этот файл;
- пакет собран и проверен на testnet, но **не** объявлен mainnet-ready.

## Текущее решение

- `age`/curve25519-dalek, `yaml-rust`/clap и `zip` закрыты по коду и
  unit-гейтам. Реальный testnet-receive 0.001 GRIN (`response_received`) прошёл
  на age 0.12.1/zip 9/clap 3; после финального age 0.11.5 и clap 4 отдельный
  live-прогон не повторялся. Sender finalize/подтверждение микропередачи не
  доведены.
- Осталось **2 advisories**: `crypto-bigint` (не собирается текущими features)
  и `rsa` (upstream-блокер arti 0.47).
- `tools/rustsec.ps1` зелёный: 2 accepted, 0 unaccepted. Любое новое advisory
  блокирует трек до разбора.
