# Android — отложенный план

Статус: предложение, не реализованный/собранный Android-кошелёк.
Первая цель — проверяемый testnet APK: create/restore, адрес/баланс/история,
ручной Slatepack, безопасное продолжение/отмена и backup. Janus, встроенная
нода, Tor/relay-автоматизация и публичный mainnet-релиз не входят в MVP.

Предлагаемая схема: Kotlin/Compose → ViewModel/WalletService → тонкий JNI →
Rust wallet/domain → внешняя Grin-нода. Android Keystore/биометрия/lifecycle,
QR/файлы/разрешения — адаптеры платформы, Windows NodeManager/ACL/exe не переносятся.
Суммы — целые nano с checked arithmetic. JNI owns handles/memory явно, вызовы
сериализованы, Rust panic и секреты не проходят в JSON/error на границе.

До UI нужно согласовать границы общего ядра, лицензию и source pin/patch,
Rust/JDK/Gradle/SDK/NDK, ABI/min SDK/target SDK и канал доставки. Общий код
Flint GPL-3.0-only не копируется под несовместимой декларацией лицензии.
Отдельный Android workspace не включается в desktop Cargo случайно.

Этапы и проверяемые результаты:

1. Arm64-v8a + x86_64 JNI прототип, read-only операция и одинаковые
   [fixtures](../../contracts/TRANSFER.md) в чистом CI без C:\DHW.
2. Create/open/restore, защищённый seed container, Keystore lifecycle,
   password/biometry, lock/background; сбой ключа не создаёт новый wallet поверх старого.
3. Нода/сеть, full restore scan, проверенные балансы; до scan баланс неизвестен.
4. Полная state machine send/receive/finalize/post/confirmation,
   [F-04 recovery](../../contracts/RECEIVE_RECOVERY.md), crash/retry без второй подписи.
5. QR/импорт/экспорт с валидацией недоверенного входа и безопасным backup.
6. Release/JNI ABI/упаковка/лицензии; независимые физические Android-проверки.

У каждого этапа — отдельная спецификация поведения/ошибок и критерии приёмки;
прототип UI не считается реализацией денежных функций. Общие ограничения —
[SECURITY.md](../../SECURITY.md); состояние desktop — [STATUS.md](../../STATUS.md).
