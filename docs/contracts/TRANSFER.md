# Контракт переводов schema 1

Действующая реализация — [crates/transfer-contract](../../crates/transfer-contract/),
`flint-transfer-contract`, SCHEMA=1, GPL-3.0-only. Desktop использует общий Rust-код
для сумм/стадий/действий; iOS подключается отдельным версионированным source artifact.
Наличие этого crate не подтверждает его интеграцию или сборку iOS.

## Границы

Общие DTO, сети, NanoGrin и правила действий не зависят от UI, Windows paths,
filesystem, сети, secret storage или wallet cryptography. Подпись/криптография
остаются в backend. SwiftUI/Keychain/lifecycle, Slint/ACL/process management —
адаптеры платформ. Каталоги desktop и iOS не объединяются.

Канон wire representation — serde-типы и fixtures самого crate. Суммы
`NanoGrin(u64)` передаются как JSON **целые числа**, не Double; строка десятичного
ввода разбирается отдельно. Старый draft со строковыми amount_nano не является
фактической сериализацией schema 1. Swift/FFI должен декодировать без потери UInt64;
потребитель с неточным JSON Number обязан использовать точный integer parser.

## Суммы и идентичность

1 GRIN = 1 000 000 000 nano. Максимум 9 десятичных разрядов, checked add/sub/mul,
отказ при переполнении/недостатке средств. `Network` различает testnet/mainnet;
допуск mainnet определяется политикой приложения, не этим enum.
`slate_id` непустой, direction = send/receive, schema проверяется явно.

TransferState: schema, slate_id, direction, stage, amount_nano, fee_nano, peer,
next_action, can_resend, can_export_response, can_finalize, can_repost, can_cancel,
last_error, updated_at. Ошибка не содержит seed/пароля/private keys или полного
чувствительного slatepack. Новая несовместимая семантика требует версии.

## Стадии и действия

| Stage | Разрешённые действия |
|---|---|
| prepared | resend, cancel |
| request_sent | resend, wait |
| response_ready | export_response, finalize |
| finalized | repost, wait |
| posted | wait, restore_scan |
| confirmed / cancelled | Денежных действий нет |
| failed | restore_scan, cancel — дополнительная безопасность остаётся в backend |

next_action вычисляется состоянием и direction. Для response_ready у receive —
export_response, у send — finalize. Terminal состояние может возвращать next_action
wait, но capability flags не разрешают повторный денежный эффект.
`Stage::from_wallet_state` отображает backend journal в общий вид; неизвестное
состояние не превращается в confirmed. Общий action gate не заменяет проверку
реального кошелька/журнала/сети, в особенности для cancel и recovery.

request_sent означает транспортную отправку, не подтверждение сети. posted также
не равно confirmed. Повтор сохраняет исходную операцию/envelope id и подпись,
не создаёт новый перевод. Повтор S2 armor может дать другие bytes шифрования,
но сериализованный response Slate должен остаться прежним.

## Восстановление и поставка

После restore в чистый root обязателен полный scan; до его завершения баланс
неизвестен. Ошибку scan нельзя замаскировать нулевым балансом/успешным onboarding.
Прежний draft JSON restore описывал намерение и не является отдельным реализованным
DTO этого crate: конкретный adapter обязан явно передать состояние scan/ошибку.

Контракт поставляется с manifest/version/hash, исходниками и теми же fixtures,
которые выполняются на обеих платформах. Путь зависимости должен работать в чистом
iOS CI без доступа к C:\DHW. Совпадение номера grin-wallet 5.5.1 не гарантирует
совпадение fork/receive recovery. Политика изменений — [ARCHITECTURE.md](../ARCHITECTURE.md),
атомарный приём — [RECEIVE_RECOVERY.md](RECEIVE_RECOVERY.md).

```powershell
cargo test -p flint-transfer-contract --offline
```

Unit/fixtures подтверждают общий контракт, но не выполняют подпись, сетевую
доставку или проверку device UI. В принятом RC исходный контракт имел SHA-256
`ca7ebbc51a73fd1275422b9e5b697028879264d16ec5501e92d0c4395ac6d377`;
новая упаковка получает собственный полный hash.
