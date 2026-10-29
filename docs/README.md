# Документация Flint

Актуализировано: 2026-10-28. Вся рабочая документация находится в `docs/`.
Снимки исходников и инструменты — `snapshots/` (вне `docs/`), старые сборки, логи и
патчи — `archive/`; это не источники текущего статуса.

## Текущая база

- Flint **0.1.69**, testnet, DEV BUILD; mainnet запрещён.
- Полный набор: **314 passed, 0 failed, 29 ignored**; UI — **15 passed**.
- Реальный encrypted slatepack через messenger: **PASS** (default → alice,
  смена профиля без перезапуска; ответ автофинализирован от сохранённого контакта).
- Контрольный снимок: `snapshots/20261029-plan12` — 685 файла,
  62 517 489 байт, `match:true`.
- Актуальный portable-пакет: `lastver/Flint-0.1.69-keyboard/`; SHA-256 exe
  `f311fe52…`; `C:\Flint` обновлён вручную (data/data1 сохранены).
- Локальных активных пунктов **0**; внешних приёмок — **17**.

## Единые рабочие документы

| Документ | Назначение |
|---|---|
| [status/STATUS.md](status/STATUS.md) | Фактическое состояние и проверенные сценарии; главный источник правды. |
| [PLAN.md](PLAN.md) | Текущий рабочий план стабилизации и порядок оставшихся работ. |
| [KRead.md](KRead.md) | **0** локальных пунктов: все доказательства закрытия. |
| [PSWork.md](PSWork.md) | **17** внешних приёмок и человеческих гейтов. |
| [WORK_LOG.md](WORK_LOG.md) | Новые записи о прогрессе, изменениях, проверках и замечаниях. |
| [status/ROADMAP.md](status/ROADMAP.md) | Официальные фазы и гейты проекта. |
| [status/MAINNET_READINESS.md](status/MAINNET_READINESS.md) | Запрет реальных средств и условия mainnet; сейчас **не готово**. |

Счёты разделяются: 0 локальных пунктов + 17 внешних пунктов; Janus, публичный
relay и мобильная версия являются тремя отдельными отложенными треками.

## Правила ведения

1. Сначала фиксировать факт в `status/STATUS.md` и доказательство в `WORK_LOG.md`,
   затем отмечать пункт в `KRead.md`. Наличие кода не равно живой проверке.
2. Новые записи журнала добавлять сверху: цель, изменённые файлы, команды и их итог,
   timeout/незавершённые проверки, ограничения доказательства и следующий шаг.
3. Большой вывод хранить в `.tmp/`; в журнал переносить только итог и путь к логу.
   Прерванная команда не считается passed или failed без фактического результата.
4. Контрольный результат привязывать к одному снимку. Изменения документации не
   выдавать за изменение или повторную проверку продуктового кода.
5. План менять согласованно в `PLAN.md`, `KRead.md` и `PSWork.md`; расхождение счётчиков
   считать ошибкой документации.
6. Seed, пароли, приватные ключи и owner-секреты в журнал и индексы не включать.
7. Исторические документы не использовать как текущий статус. Их утверждения относятся
   к указанным дате и снимку.

## Безопасность

| Документ | Назначение |
|---|---|
| [security/JANUS_REVIEW_2026-10-28.md](security/JANUS_REVIEW_2026-10-28.md) | Независимое ревью логики свопов Janus; 6 подтверждённых пунктов, 1 опровергнут тестом. |
| [security/AUDIT_RESPONSE_2026-10-27.md](security/AUDIT_RESPONSE_2026-10-27.md) | Ответ на AI-аудит и остаточные риски на дату. |
| [security/AUDIT_RECHECK_2026-10-27.md](security/AUDIT_RECHECK_2026-10-27.md) | Повторный AI-аудит; датированный снимок. |
| [security/SENTINEL_AUDIT_2026-10-27.md](security/SENTINEL_AUDIT_2026-10-27.md) | Статический Sentinel-аудит; датированный снимок. |
| [historical/SENTINEL_REVIEW_2026-10-27.md](historical/SENTINEL_REVIEW_2026-10-27.md) | Проверка Sentinel и доказательства SENT-01/SENT-02. |
| [security/AUDIT_EXTERNAL_2026-10-27.md](security/AUDIT_EXTERNAL_2026-10-27.md) | Исходный внешний AI-аудит; датированный снимок. |
| [security/FLINT_PENTEST_AUDIT_COMPARISON_2026-10-27_RU.md](security/FLINT_PENTEST_AUDIT_COMPARISON_2026-10-27_RU.md) | Локальный пентест и сопоставление; датированный снимок. |
| [security/CALLBACK_AUDIT.md](security/CALLBACK_AUDIT.md) | Аудит callback-вызовов Slint → runtime. |
| [security/TRUST_BOUNDARY.md](security/TRUST_BOUNDARY.md) | Граница доверия Slint ↔ runtime. |
| [security/ADR_F04_RECEIVE_RECOVERY.md](security/ADR_F04_RECEIVE_RECOVERY.md) | Архитектурное решение восстановления receive response. |
| [security/INCIDENT_RESPONSE.md](security/INCIDENT_RESPONSE.md) | Черновик политики инцидентов; контакт не утверждён. |
| [security/RUSTSEC_2026-10-28.md](security/RUSTSEC_2026-10-28.md) | RustSec: 2 транзитивные уязвимости и 17 предупреждений; age, clap/yaml-rust и zip закрыты. Проверка — `tools/rustsec.ps1`. |
| [security/RUSTSEC_UPGRADES.md](security/RUSTSEC_UPGRADES.md) | Отдельный трек обновления RustSec-зависимостей: age, clap/yaml-rust и zip выполнены; остался upstream-блокер rsa + crypto-bigint; mainnet не разрешает. |

AI-review и статические отчёты не заменяют независимый человеческий аудит.

## Эксплуатация

| Документ | Назначение |
|---|---|
| [operations/BACKUP.md](operations/BACKUP.md) | Backup drill и восстановление. |
| [operations/RESTORE.md](operations/RESTORE.md) | Восстановление кошелька. |
| [operations/DATA_ROOT.md](operations/DATA_ROOT.md) | Несколько seed-профилей и выбор каталога данных. |
| [operations/DESTRUCTIBLE_F04.md](operations/DESTRUCTIBLE_F04.md) | Аварийный стенд и принятая симуляция; физический гейт открыт. |
| [operations/RELAY.md](operations/RELAY.md) | Офлайн-relay. |
| [operations/TOR_BRIDGES.md](operations/TOR_BRIDGES.md) | Tor-мосты. |
| [operations/TESTNET_WALLETS.md](operations/TESTNET_WALLETS.md) | Тестовые кошельки и безопасное обращение с реквизитами. |

## Архитектура и UI

| Документ | Назначение |
|---|---|
| [architecture/BACKENDS.md](architecture/BACKENDS.md) | Embedded и `grin-wallet`, границы и гейт удаления embedded. |
| [architecture/NODE_ARCHITECTURE.md](architecture/NODE_ARCHITECTURE.md) | Нода и кошелёк. |
| [architecture/MESSENGER.md](architecture/MESSENGER.md) | Протокол мессенджера. |
| [architecture/RPC_EXTRACTION.md](architecture/RPC_EXTRACTION.md) | Журнал выноса RPC в runtime. |
| [architecture/BACKGROUND_NODE.md](architecture/BACKGROUND_NODE.md) | Дизайн и гейты фоновой ноды; реализация не начата. |
| [architecture/IOS_SHARED_CONTRACT.md](architecture/IOS_SHARED_CONTRACT.md) | Минимальный Rust/JSON-контракт переводов для iOS; каталоги платформ не объединяются. |
| [nodeidea.md](nodeidea.md) | Предложение фоновой ноды; реализация не разрешена раньше гейтов. |
| [ui/SLINT.md](ui/SLINT.md) | Единственный UI, сборка, GPL и MSRV. |
| [ui/LOCALIZATION.md](ui/LOCALIZATION.md) | ru/en: статический UI и справка; границы перевода. |
| [mobile/README.md](mobile/README.md) | Мобильные треки; отдельный iOS-проект — `../mobile/GrinFlint/`, не входит в desktop-сборку. Канон документации iOS — в репозитории: `mobile/GrinFlint/docs/mobile/ios/`. |

## Инструменты среды

| Документ | Назначение |
|---|---|
| [DSH.md](DSH.md) | Профиль DSH: постоянный терминал, поиск по прошлым сессиям, делегации Codex, смысловой поиск и LM Studio. |
| [tools/codex-local-delegation.skill](tools/codex-local-delegation.skill) | Навык локальной делегации в Codex (перенесён из корня 28.10.2026). |

## История

Прежние записи Kimi и старая дорожная карта:

- [historical/WORK_LOG.md](historical/WORK_LOG.md) — записи 01–22;
- [historical/PLAN.md](historical/PLAN.md) — прежний рабочий план;
- [historical/KIMI_DOCS_README.md](historical/KIMI_DOCS_README.md) — прежние правила;
- [historical/ROADMAP_2026-10-24.md](historical/ROADMAP_2026-10-24.md) — дорожная карта ред. 3.

Более ранняя проектная история — в `historical/`: STATUS, удаление Tauri, Argon2id,
migration review, spike-исследования. Каталог `history/` объединён с `historical/`
28.10.2026. Эти материалы являются доказательствами и журналом решений, а не описанием
текущего состояния.
