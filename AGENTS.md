# AGENTS.md — Flint

Рабочий корень: **`C:\DHW`**. Копия в OneDrive
(`...\deepseek-harness\default-workspace`) — устаревшая: её не редактировать.
Если путь сессии указывает туда — работай только в `C:\DHW`.

## Что за проект

Десктопный кошелёк **Grin (Mimblewimble)** + слой атомарных свопов **Janus** (Monero ↔ Grin).
Rust workspace (edition 2021, MSVC) + Slint 1.18 (native UI). Tauri удалён в 0.1.69.
**DEV BUILD: не аудирован, не для реальных средств.** Сеть по умолчанию — testnet.

## Где что

| Путь | Содержимое |
|---|---|
| `crates/core` | кошелёк, `NodeManager`, slate-ы, gateway; бинарь `flint-relay` |
| `crates/messenger` | конверт, транспорты (direct/relay), история, контакты, Tor |
| `crates/janus` | свопы: состояния, таймлоки, абстракции борда (демо) |
| `crates/runtime` | `WalletRuntime`, RPC, Tor, messenger и ключ истории |
| `crates/slint-ui` | единственный UI: `flint-slint` |
| `crates/grin-wallet-rpc` | обёртка upstream `grin-wallet` из fork: бинарь `grin-wallet`, член workspace |
| `snapshots/` | снимки исходников (`<label>/workspace`) и инструменты в `tools/`; ~1.3 ГБ, не сканировать |
| `archive/` | `archV/` (старые сборки), `logs/`, `patches/`, `selftest.json`; не сканировать |
| `fixtures/` | `testnet-funded-app/` — одноразовая funded-testnet копия; данные не чистить |
| `projects/plugins/` | исходники sentinel-audit |
| `lastver/` | актуальные portable/debug-пакеты, исходники и BUILD-INFO; прежние файлы не перезаписывать без копии |
| `docs/README.md` | единый индекс документации и правила ведения |
| `docs/PLAN.md` | текущий рабочий план |
| `docs/WORK_LOG.md` | текущий журнал прогресса, изменений и замечаний |
| `docs/DSH.md` | инструменты DSH: терминал, поиск по сессиям, делегации Codex, LM Studio |
| `docs/historical/` | прежние записи Kimi и исторические срезы |
| `docs/KRead.md` | трекер локального плана: 0 пунктов и доказательства |
| `docs/PSWork.md` | 17 внешних приёмок PS-01…PS-17 |
| `docs/mobile/` | iOS-контекст и Android-план (мобильные треки) |
| `mobile/GrinFlint/` | отдельный iOS-порт GrinFlint (git), свой `AGENTS.md`; не часть desktop-сборки |
| `tools/` | `ci.ps1`, `release.ps1`, `source.ps1`, `sbom.ps1`, `verify-sbom.ps1`, `rustsec.ps1`, `verify-native-assets.ps1`, `package-slint.ps1`, `check-node-version.ps1`, `fetch-node.ps1`, `run-testnet-node.ps1`, `run-live-testnet.ps1`, `semantic-search.ps1` |
| `tools/semantic/` | смысловой поиск по `crates/` через локальный LM Studio; индекс — в `.tmp\semantic\` |
| `docs/status/STATUS.md` | что реально проверено — **источник правды**, важнее README |
| `docs/status/ROADMAP.md` | порядок фаз и гейты |
| `docs/status/MAINNET_READINESS.md` | чек-лист перед реальными деньгами |
| `docs/architecture/BACKENDS.md` | границы embedded/RPC и гейт удаления embedded |
| `docs/ui/SLINT.md` | Slint UI, GPL и MSRV 1.92 |
| `docs/historical/TAURI_REMOVAL.md` | журнал удаления Tauri и backup |

## Не читать и не сканировать

`target/` (build-кэш), `.node/` (~5.9 ГБ, данные testnet-ноды), `vendor/`
(~1.2 ГБ, офлайн-вендор), `archive/` (архив, логи, патчи), `dist/`, `data/`,
`.tmp/`, `fixtures/`, `snapshots/` (~1.3 ГБ, копии дерева), `mobile/GrinFlint/`
(~5 ГБ, отдельный проект; читать можно только его `AGENTS.md` и `docs/`).
Поиск и чтение — только по `crates/`, `tools/`, `docs/`,
`lastver/` (только манифест/исходники).

## Команды

```powershell
cargo test --workspace                                   # 307 тестов; 29 ignored — живые наборы по флагу --ignored
cargo build --release -p flint-slint                     # native Slint UI
cargo run -p flint-runtime --bin flint-selftest          # headless-проверка связки, ~10 с
cargo run -p flint-slint                                 # запуск UI
pwsh -File tools/package-slint.ps1                       # portable preview + selftest
pwsh -File tools/ci.ps1                                  # fmt/check/tests/build/selftest
pwsh -File tools/release.ps1                             # release + SBOM + SHA256SUMS
pwsh -File tools/run-testnet-node.ps1 start|status|stop  # долгоживущая testnet-нода
pwsh -File tools/semantic-search.ps1 -Rebuild            # индекс для смыслового поиска (нужен LM Studio и загруженная text-embedding-bge-m3)
pwsh -File tools/semantic-search.ps1 -Ru "запрос"        # поиск: сначала английский запрос, -Ru переводит (нужен 14B)
```

Смысловой поиск рассчитан на локальную многоязычную **`text-embedding-bge-m3`**:
она держит русский и английский в одном пространстве, поэтому русский запрос
работает без перевода. Перед первым запуском проверь, что активен именно
`bge-m3` (размерность 1024), а лишние эмбеддеры лучше выгрузить: LM Studio
игнорирует поле `model`, и при чужом эмбеддере индекс молча соберётся
неправильно. Команды проверки и размерности (1024 — bge-m3, 768 — nomic) — в
[`tools/semantic/README.md`](tools/semantic/README.md#проверка-модели-перед-запуском).

## Экономия контекста

1. Вывод команд не вставлять целиком: фильтр `Select-String 'error|warning|FAILED'`
   или хвост. Полный лог — в файл в `.tmp\`, читать диапазоном.
2. Файлы читать `offset`/`limit` или грепом по имени символа; целиком — только
   небольшие файлы.
3. `cargo build` / `cargo test` — фоновой задачей, из вывода брать итог; прогресс
   сборки не пересказывать.
4. Разведку по незнакомому коду отдавать сабагенту, в основной чат — только вывод.
5. `docs/status/STATUS.md` не пересказывать — ссылаться на раздел (`§3.1`).
6. Задача закрыта → предложить новую сессию вместо продолжения этой.

## Инструменты DSH

Подробности и проверки — [`docs/DSH.md`](docs/DSH.md); здесь только то, что
влияет на работу.

1. Долгие сборки и прогоны — в постоянном терминале (`terminal_open`): TMP
   выставляется один раз. Короткие команды дешевле делать одним `pwsh`.
2. Перед чтением `docs/` — `session_search` по истории сессий `C:\DHW`: часто
   ответ уже есть, и это дешевле, чем перечитывать журнал.
3. Делегации в Codex (`subagent_codex`) разрешены владельцем; квота Plus общая с
   его приложением, поэтому только узкие read-only разборы и второе мнение.
   **Находки Codex проверять по коду**: 28.10.2026 одна из семи оказалась ложной.
   Видимое окно Windows Terminal при делегации — норма, закрывается само.
4. Смысловой поиск — только по запросу: `bge-m3` грузить на время, `nomic`
   выгружать, индекс в `.tmp\semantic\` пересобирать после правок кода.

## Правила проекта

- Документация, UI-тексты и сообщения об ошибках — по-русски; тон — фактический,
  без обещаний («проверено тестом» ≠ «работает»).
- Версия одна на весь workspace: `Cargo.toml` → `workspace.package.version`
  (сейчас `0.1.69`). При релизе синхронно обновляются `README.md` и `docs/status/STATUS.md`.
- Лицензия проекта — **GPL-3.0-only** (`COPYING`, `THIRD_PARTY_NOTICES.md`).
  Не менять лицензию/зависимости с несовместимыми условиями без юридического
  решения; исходники релиза должны включать соответствующий source Slint.
- Сборка офлайн: `vendor/` + `.cargo/config.toml` с `offline = true`. Новая
  зависимость требует `cargo vendor vendor` с сетью, иначе сборка падает.
- Длинные пути ломают MSVC (`LNK1104`/`D8050`): перед сборкой
  `$env:TMP = $env:TEMP = 'C:\DHW\.tmp'`.
- **`.git` в проекте нет.** Никаких удалений и перемещений крупных каталогов без
  явного согласия; перед перезаписью — копия.
- `flint-selftest` не трогает `FLINT_DATA_DIR` и убирает за собой — это
  проверяемое свойство, не менять.
- Seed, пароль и приватные ключи реальных кошельков не попадают в логи, тесты и
  сообщения; каталоги mainnet не трогать — это реальные деньги.
- Одноразовые testnet-реквизиты (`docs/testnet-funded.txt`,
  `docs/grinflint-ci-wallet.txt`) хранятся открыто осознанно: это faucet-кошельки
  без ценности для живых прогонов. Их не шифровать и не скрывать.

## Перед изменениями

- Любые изменения кода → сначала прочитать [SKILL.md](SKILL.md) (`ub-mode`).
- Деньги, seed, сеть → сначала `docs/status/STATUS.md` и `docs/status/MAINNET_READINESS.md`.
- UI → `DESIGN.md`: мята — единственный акцент, суммы моноширинным `tabular-nums`.
- Смысловой поиск по-русски → LM Studio с активной `text-embedding-bge-m3`
  (проверка — [`tools/semantic/README.md`](tools/semantic/README.md#проверка-модели-перед-запуском)).
- Незнакомый модуль → грепом по имени, затем чтение нужного участка.




