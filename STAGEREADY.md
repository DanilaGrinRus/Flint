# STAGEREADY — Flint desktop RC gate

This file fixes the requested task scope: bring the selected Flint desktop RC
to `READY_FOR_EXTERNAL_DEVICE_TESTS` on isolated testnet, with automated checks,
live testnet/relay/Tor evidence and a portable artifact. External security
audit is out of scope and is not a remaining-work item here.

## Scope frozen for this RC

Included:

- create/open/lock/restore/backup wallet; profiles; node connection; balances
  and history; full send/receive/finalize/post/confirmation; safe cancel and
  continuation; messenger contacts/block/history and supported transports;
- both backends: embedded and real `grin-wallet` RPC;
- transfer-contract v1 source package and fixtures for the separate iOS
  project;
- CI, live testnet, relay and Tor gates; dependencies/native assets;
- immutable portable package, BUILD-INFO, SHA-256, SBOM, sources and licenses.

Explicitly excluded: real Janus atomic swaps, public relay deployment as a
service, iOS application implementation, external audit.

## Authoritative documents

- [AGENTS.md](AGENTS.md), [SKILL.md](SKILL.md), [DESIGN.md](DESIGN.md)
- [docs/status/STATUS.md](docs/status/STATUS.md)
- [docs/status/MAINNET_READINESS.md](docs/status/MAINNET_READINESS.md)
- [docs/PLAN.md](docs/PLAN.md), [docs/PSWork.md](docs/PSWork.md)
- [docs/WORK_LOG.md](docs/WORK_LOG.md)
- [docs/architecture/IOS_SHARED_CONTRACT.md](docs/architecture/IOS_SHARED_CONTRACT.md)
- [crates/transfer-contract/README.md](crates/transfer-contract/README.md)
- [RC_ACCEPTANCE_MATRIX.md](RC_ACCEPTANCE_MATRIX.md)
- [DEVICE_ACCEPTANCE.md](DEVICE_ACCEPTANCE.md)

## Required external build inputs

No signing key is available for an unsigned test RC. A signed package cannot be
produced without the project signing identity and timestamp service; the RC is
therefore explicitly **unsigned** unless those inputs are supplied. CI live
jobs require the labelled self-hosted runners and their testnet/Tor material.
These are inputs, not substitutes for device verification.

## Acceptance rule

`READY_FOR_EXTERNAL_DEVICE_TESTS` may be assigned only when every non-device row
in [RC_ACCEPTANCE_MATRIX.md](RC_ACCEPTANCE_MATRIX.md) is PASS and all remaining
items are exclusively the physical-device checks in
[DEVICE_ACCEPTANCE.md](DEVICE_ACCEPTANCE.md). A local failure, missing live
infrastructure, unreviewed advisory or missing package component is not moved
into the device checklist.

## IOS кошелек

### Задание (IC FrintFlint → READY_FOR_EXTERNAL_DEVICE_TESTS)

Доведи iOS-кошелёк GrinFlint в `C:\DHW\mobile\GrinFlint` до состояния
`READY_FOR_EXTERNAL_DEVICE_TESTS`: обязательная функциональность завершена,
автоматическая приёмка выполнена, подготовлена устанавливаемая device-сборка.
После завершения должны остаться только проверки на отдельных физических
iPhone.

Это задача реализации и проверки. Не заканчивай очередным перечнем
рекомендаций или частично зелёным CI. Внешний аудит исключён из объёма и
итогового списка остатка.

Задача включает точечное чтение и изменение необходимых Swift/Rust FFI,
тестов, настроек проекта и CI. Прочитай местный AGENTS.md и актуальные
документы. Сверяй их с кодом: прежние fail-open, отсутствие pending-журнала,
QR и проверки genesis уже частично исправлены — сохраняй сделанное.

Работай с изолированными testnet-кошельками. Реальные средства и
mainnet-данные не затрагивай. Не выводи секреты в логи и артефакты.

#### 1. Зафиксируй объём и необходимые входные данные

Обязательные функции RC: создание, открытие, защита входа, восстановление,
подключение ноды, проверенные балансы и история, send/receive/finalize/post/
confirmation, безопасная отмена, сохранение и продолжение операций,
QR/файл/Share Extension и резервное восстановление.

Встроенная нода, будущая автоматизация через Nostr/Tor, multiwallet и
магазинная публикация не входят в этот этап. Поддерживаемые устройства и
версии iOS зафиксируй явно. Демо не должно подменять обязательные функции.

В начале проверь доступность macOS/Xcode CI, подписи, provisioning и
выбранного канала установки RC. Запроси недостающие обязательные данные
заранее. Отсутствие этих условий не является аппаратной проверкой и не
позволяет объявить задачу завершённой.

#### 2. Подключи общий Rust-модуль десктопа

Используй версионированный модуль, контрактные fixtures и manifest,
подготовленные desktop-задачей. Зафиксируй версию/hash и происхождение
зависимостей.

Подключение должно воспроизводиться в чистом iOS CI без доступа к `C:\DHW`.
Прогони одинаковые контрактные тесты сумм, сетей, состояний и повторов на
обеих платформах.

Общие правила должны реально использоваться сервисом кошелька. SwiftUI,
Keychain, биометрия и жизненный цикл остаются платформенными адаптерами.
Не создавай расходящуюся ручную копию той же бизнес-логики.

#### 3. Заверши устойчивую state machine операций

Расширь существующий pending-журнал до необходимых стадий подготовки,
ответа, финализации, публикации и подтверждения.

Ошибки чтения и записи журнала должны возвращаться вызывающему коду.
Повреждённый или несохранённый журнал нельзя молча считать пустым. До
необратимого действия сохраняй необходимое намерение; после неоднозначного
результата выполняй восстановление состояния.

При сбое post_tx повторно публикуй исходную подписанную транзакцию без
повторной подписи. Проверь остановку приложения и ошибки записи/сети до и
после каждого этапа.

Экспорт должен возвращать данные только указанной операции. Удали fallback
неизвестного txID на pending.latest. Скрытие карточки или нажатие «ответ
передан» не должно удалять единственную копию S2.

Доведи отмену через WalletService, ViewModel и UI; разрешай её только в
безопасных состояниях. Предоставь повторную выдачу S1/S2, продолжение и
повтор публикации после перезапуска.

#### 4. Сделай суммы, комиссии и восстановление достоверными

Получай комиссию и итоговое списание от настоящего backend до подтверждения
пользователем. Убери неподдерживаемые декоративные fee tiers и ETA. Max
должен учитывать реальную комиссию.

Проверь положительность, диапазон, overflow и точность до девяти десятичных
знаков. Исключи молчаливое усечение или переполнение при переводе Decimal в
наногины.

Восстановление должно различать новую операцию, продолжение той же identity
и настоящую ошибку. Не открывай любой существующий кошелёк как успешное
продолжение после произвольной ошибки restore.

Сохраняй необходимость полного scan и прогресс восстановления. Offline, 401
и незавершённый scan должны давать понятное состояние неизвестного/
устаревшего баланса.

Докажи сценарий: прежняя funded testnet-фраза → чистый каталог → прежний
адрес → полный scan → обнаруженные средства → повторное открытие после
перезапуска.

#### 5. Заверши Keychain и жизненный цикл аутентификации

Проверь миграцию legacy и существующих v2-записей. Наличие имени
`wallet-password-v2` не доказывает userPresence: запись могла быть создана с
ослабленной политикой.

Для неоднозначных старых записей обеспечь безопасное усиление защиты тем же
паролем с контрольным чтением. Не удаляй единственную рабочую запись и не
генерируй новый пароль существующему кошельку.

Используй публичные API. Отмена, ошибка миграции, отсутствие кода и две
существующие записи должны обрабатываться явно. Метка защищённости отражает
гарантированную политику.

Ручная блокировка, inactive/background и новый запрос входа должны
инвалидировать прежнее право разблокировки. Поздний результат аутентификации
не открывает заново заблокированное приложение. Повторное использование
контекста допустимо только внутри одного ограниченного процесса входа.

Секретные экраны и seed скрываются при уходе приложения из активного
состояния независимо от настройки обычной автоблокировки. Проверь блокировку
поверх всех sheet. Не обещай гарантированного обнуления всех копий Swift
String.

Добавь детерминированные тесты порядка событий и ошибок через подменяемые
системные адаптеры.

#### 6. Сделай production FFI и подключение ноды обязательными

RC/device target должен требовать настоящий FFI на этапе сборки. Отсутствие
GRINFFI не должно включать Mock. Демо остаётся только явным тестовым
режимом.

Проверь владение FFI-дескрипторами, освобождение памяти, нулевые аргументы,
последовательность вызовов, ошибки и завершение сервиса.

Не включай исходный секретный JSON в ошибки декодирования. Используй
безопасные коды и сведения об операции. Проверь синтетическими секретными
маркерами ошибки, логи, диагностику и CI-артефакты.

Сохрани строгую сеть и существующий NodeProbe. Устрани mainnet-default для
отсутствующего URL в testnet-контексте. Параметры авторизации ноды должны
доходить до настоящих запросов и храниться защищённо.

Применение нового endpoint должно безопасно обновлять backend и исключать
смешение старых callbacks с новыми настройками. Проверь genesis, TLS, timeout,
401, offline и восстановление связи.

#### 7. Закрой настоящую автоматическую приёмку

Раздели demo UI, offline FFI и обязательные online-проверки. Ошибка refresh
может быть ожидаемой только в явно офлайн-наборе.

Выбранный online E2E без обязательных параметров завершается ошибкой
конфигурации. Исправь ранний return/PASS и неверный testnet-порт в helper.
Демонстрационная навигация не заменяет пользовательский цикл с настоящим
backend.

На macOS CI выполни Rust-проверки, сборку XCFramework для simulator/device,
Swift unit-тесты, UI-тесты с настоящим FFI и testnet-контрагентом. Проверь
перевод, подтверждение, отмену, restart, repost, восстановление, неверный
txID и повреждённый/чужой slatepack.

Проверь совместимость desktop ↔ iOS по одинаковым fixtures и реальному обмену
slatepack.

QR, файлы, deep links и Share Extension должны корректно обрабатывать отказ
доступа, отмену, повреждённые данные и слишком большой payload. Импорт не
должен автоматически выполнять денежное действие без предусмотренного
подтверждения.

#### 8. Подготовь device Release RC и аппаратный протокол

Собери именно Release archive приложения для устройства с настоящим FFI и
Share Extension. Подготовь устанавливаемый подписанный RC через согласованный
канал, корректные entitlements, provisioning, версии, иконки, launch screen и
privacy usage descriptions.

Одного simulator build или device XCFramework недостаточно. Материалы подписи
и инфраструктуру нужно решить до завершения задачи.

Зафиксируй source/dependency hashes, Xcode/Rust, CI run, hash артефакта и
проверенные сценарии. Подготовь установку, обновление, резервное
восстановление и безопасную диагностику.

Критерий завершения:

Создай матрицу возможностей с доказательствами и DEVICE_ACCEPTANCE.md.

В остатке разрешены только реальные iPhone-проверки: Face ID/Touch ID и код;
фактический Keychain при обновлении/перезагрузке; системные прерывания;
snapshot переключателя приложений; камера и освещение; Share Extension из
сторонних приложений; мобильная сеть/LAN; реальные экраны, Dynamic Type,
VoiceOver, память, нагрев и батарея.

Код обработки этих событий и автоматические проверки уже должны быть готовы.
Для каждого аппаратного сценария укажи модель/OS, сборку/hash, предусловия,
действия, ожидаемый результат, доказательства и безопасное восстановление
стенда.

Не оставляй в этом списке реализацию отмены, исправление restore,
неподготовленный archive, настройку CI или отсутствующий сертификат. Это
внутренние незавершённые обязательства.

После последней правки повтори затронутые и полные обязательные проверки.
Отчёт должен содержать PASS / FAIL / NOT VERIFIED с доказательствами.
READY_FOR_EXTERNAL_DEVICE_TESTS присваивай только после выполнения всех
внутренних критериев.

### Входные данные, которых не хватает (на 2026-10-29)

- Apple Developer Program: Team ID, подписывающий сертификат (distribution или
  development, `.p12` + пароль) и provisioning profile, либо ключ App Store
  Connect (`.p8`, key id, issuer id) для автоматической подписи/TestFlight.
- Зарегистрированные в профиле устройства (UDID) для Ad Hoc/OTA **или**
  решение использовать TestFlight (нужен App Store Connect record).
- Выбранный канал установки RC: **решено 2026-10-29** — бесплатный Apple ID
  (Personal Team) через Xcode на Mac пользователя; CI готовит неподписанный
  Release archive + device XCFramework, подпись и установка выполняются
  локально (7 дней, до 3 приложений). TestFlight/Ad Hoc требуют платного
  Apple Developer Program и в этом RC не используются.
- Целевые устройства и минимальная версия iOS (предложение: iPhone с Face ID
  или Touch ID, iOS 17.0+; сборка проверяется на iPhone 16 Pro).

Без этих материалов подписанный device archive невозможен, а значит
`READY_FOR_EXTERNAL_DEVICE_TESTS` не может быть присвоен: отсутствие подписи —
внутреннее незавершённое обязательство, а не аппаратная проверка.
