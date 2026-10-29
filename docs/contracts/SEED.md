# Seed envelope и миграция

Действующая реализация — `crates/core/src/seed/{envelope,store,legacy}.rs`.
Контракт перенесён из исторического ARGON2ID-дизайна без смены формата/crypto.
Политика — [SECURITY.md](../SECURITY.md). Testnet-приёмка не разрешает real funds.

## Источник истины

`wallet.seed.v2` — encrypted envelope, а не upstream wallet.seed под паролем
пользователя. Расширение v2 не меняет внутреннюю `version: 1` формата flint-seed.
На unlock SeedStore создаёт временный upstream wallet.seed под новым случайным
паролем, открывает backend, сверяет адрес и удаляет только принадлежащий ему
transient. Пароль пользователя не отправляется в слабый upstream PBKDF2-100 путь.
Кошелёк без envelope обрабатывается как legacy, без тихой миграции.

## Wire contract v1

| Поле | Требование |
|---|---|
| format / version | flint-seed / 1, validate до расшифровки |
| network | testnet или mainnet, должна совпасть с выбранным контекстом |
| kdf | argon2id, version=19, m_kib/t/p и 16-байтная base64 salt |
| cipher | chacha20poly1305, 12-байтный base64 nonce |
| seed_len | 16/20/24/28/32 байта upstream entropy |
| address | Slatepack address, сверяется с реально открытым кошельком |
| ciphertext | AEAD ciphertext/tag, base64 |

AAD (порядок/байты фиксированы): `flint-seed-v1\0`, network UTF-8, NUL,
четыре u32 LE m_kib/t/p/19, salt(16), nonce(12), seed_len u32 LE, address UTF-8.
`format`, строки algorithm и top-level version отдельно проверяет validate;
они не добавляются задним числом в AAD без версии/миграции.

Plaintext: JSON с base64 entropy `seed` и optional `legacy` — точными прежними
байтами wallet.seed для разрешённого отката. Это entropy, не слова mnemonic.
seed_len должен совпасть. Прежние bytes возвращаются побайтно, пока пароль
не менялся; после смены пароля действует текущая переупаковка.

Запись: m=65536 КиБ, t=3, p=4, output=32 bytes. При чтении пределы
19456≤m≤262144, 1≤t≤10, 1≤p≤8 проверяются **до выделения памяти/KDF**.
Salt/nonce новые на каждую запись, источник — системная CSPRNG. Пароль — UTF-8
как введён, без нормализации; держится в Zeroizing там, где это возможно.
Неверный пароль и AEAD corruption не различаются; структурные JSON/base64/size
ошибки возможны до KDF. Чужой уже существовавший envelope автоматически не удаляется.

## Миграция и cleanup

Миграция legacy выполняется явным действием и со согласием на слабые backup-копии.
До удаления seed: новый envelope записан, прочитан, расшифрован и фактический
address индекса 0 совпал. При ошибке исходный пригодный wallet.seed не должен
оказаться скрыт непроверенным envelope. Записи приватны и продвигаются атомарно.

`.wallet.seed.transient` помечает только временный seed, созданный этим механизмом.
Непомеченный legacy не удаляется при lock/exit. `.wallet.seed.bak.cleanup`
фиксирует согласие на удаление слабых .bak*: без marker копии не очищаются.
Backup-копии убираются в согласованном порядке до исходного seed, чтобы crash
не оставлял их бесконтрольно. Stale cleanup не удаляет восстановленный legacy.
При repair повреждённый envelope сохраняется/quarantine с явной диагностикой;
содержимое исходного кошелька не сбрасывается.

Смена пароля envelope переупаковывает его с новыми salt/nonce, без смены seed
и без вызова upstream change_password для этого формата. Отключение envelope,
repair, migration и delete сериализуются с init/open/restore/lock/switch/password.
Mainnet-политика не обходится repair/disable или «успешной миграцией».

## Восстановление и проверки

Использовать поддерживаемую upstream фразу и новый чистый root; проверять тот же
адрес и полный scan. Seed восстанавливает средства, но не парольное шифрование
старой истории messenger. Неверный пароль/ошибка диска не создают новый кошелёк
поверх старого. Созданный самой операцией непроверенный temp не считается backup.

Regression-проверки закрепляют envelope roundtrip/tamper/limits, address binding,
случайность salt/nonce, пароль/repair, migration/retry/rollback и crash M0…M5
(`crates/core/tests/migration_crash.rs`, crash-injection). Process abort не
моделирует кэш диска и физическое отключение; остаются D-04/D-05.

Пределы прежнего migration review R1…R7 сохраняются как контекст: конкурирующий
внешний writer; durability удаления при power-loss; заголовки вне AAD; локальный
TOCTOU; сверка адреса; согласие cleanup; сила legacy-пароля. Это не новые доказанные
дефекты текущего RC. Проверяются реальные гарантии используемого кода/ФС;
изменение crypto-формата требует ADR, версии и совместимых тест-векторов.
