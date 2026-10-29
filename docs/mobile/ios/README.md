# Документация GrinFlint (iOS)

> **Устаревшая копия (с 2026-10-28 не канон).** Каноническая документация
> живёт в самом репозитории: `C:\DHW\mobile\GrinFlint\docs\mobile\ios\`
> (`AGENTS.md` — точка входа, README, roadmap, architecture,
> grin-wallet-integration). Файлы в этой папке оставлены как история;
> правки вносить только в репозиторий, иначе копии разъезжаются.

**Точка входа для агентов/инструментов: [PROJECT_CONTEXT.md](PROJECT_CONTEXT.md)** —
текущее состояние, правила секретов, ловушки окружения, типовые задачи.

- [roadmap.md](roadmap.md) — дорожная карта по фазам (демо → FFI → безопасность → релиз) и риски
- [architecture.md](architecture.md) — слои приложения, контракты, конвенции
- [grin-wallet-integration.md](grin-wallet-integration.md) — контракт FFI; актуальная реализация
  см. в `ffi/grin-wallet-ffi/src/lib.rs` и `PROJECT_CONTEXT.md` (раздел «C-API FFI»)

Проект: `../GrinFlintApp/GrinFlint.xcodeproj` · README проекта: `../GrinFlintApp/README.md`

Статус на 2026-10-27: FFI собран и проверен вживую (roundtrip 50 tGRIN
floonet), Face ID/автоблокировка написаны (сборка зелёная, на устройстве
не проверялись), UI пока на моке. Подробности — в `PROJECT_CONTEXT.md`.
