# Мобильные треки

**iOS — отдельный проект:** [mobile/GrinFlint](../../mobile/GrinFlint/).
Его актуальные правила и документы находятся внутри этого репозитория.
Копии desktop docs/mobile/ios удалены: несинхронизируемый снимок не является
вторым источником текущего состояния. Desktop не собирает iOS-приложение.

Общий Rust-контракт — [contracts/TRANSFER.md](../contracts/TRANSFER.md),
backend/recovery — [ARCHITECTURE.md](../ARCHITECTURE.md) и
[ADR F-04](../contracts/RECEIVE_RECOVERY.md). Совпадение версии upstream не
доказывает одинаковую реализацию: нужны pin/patch/capability и общие fixtures.
SwiftUI, Keychain и lifecycle остаются платформенными.

**Android — только [план](android/PLAN.md)**, реализации в этом дереве нет.
Данные mobile/grinflint-backups не относятся к кэшу и не удаляются с build outputs.
Все мобильные проверки — testnet; desktop READY не переносится на мобильные сборки.
