# Приёмка зависимостей — 2026-10-30

База: коммит `0a2bf69` плюс UI-правки рабочего дерева (иконки, кнопки, диалоги).
Дерево собрано offline из `vendor/`; `Cargo.lock` не менялся с момента добавления
`tungstenite`/`rustls` для Nostr-транспорта.

## RustSec

```
pwsh -File tools/rustsec.ps1
RustSec: db 99670cfc0bc5, lock 449647BDF4CE
vulnerabilities: 2 (accepted 2, unaccepted 0); warnings: 17
accepted RUSTSEC-2026-0387 crypto-bigint 0.5.5
accepted RUSTSEC-2023-0071 rsa 0.9.10
```

Новые зависимости (`tungstenite`, `rustls`, `secp256k1`, `hmac`, `chacha20`)
непринятых advisories не добавили. Основания принятия — `docs/SECURITY.md`.

Сырой отчёт: `rustsec.json` (выход `tools/rustsec.ps1`).

## SBOM

```
pwsh -File tools/sbom.ps1 -Out evidence/deps-20261030/sbom.json
SBOM: ... (1155 cargo + 6 native components)

pwsh -File tools/verify-sbom.ps1 -Sbom evidence/deps-20261030/sbom.json
SBOM verified: 1155 cargo packages, 6 native assets, 1161 components total
```

Это снимок текущего дерева, а не SBOM поставки: хэши и SBOM в
`dist/0.1.69-cleanup-20261029/` относятся к прошлой сборке и новой поставкой
не перекрываются.
