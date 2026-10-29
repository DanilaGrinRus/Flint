# Flint RC install, update, rollback, backup

## Install

1. Verify `SHA256SUMS` against `Flint.exe` and the zip.
2. Unpack `Flint-0.1.69-slint-preview.zip` into a fresh directory.
3. Run `Flint.exe --data <isolated-testnet-root>`; do not point at mainnet.
4. The package is **unsigned**; Windows may show SmartScreen. No signing key
   was available for this RC.

## Update

Keep the previous directory. Unpack the new zip beside it, verify hashes, and
start with the same testnet data root. Do not overwrite the old directory
until the new build opens and shows the expected testnet profile.

## Rollback

Stop Flint, restore the previous full directory, and keep the data root
untouched. Database migrations are forward-only in this RC; restore the
pre-update data backup when rolling back across a migration.

## Backup / restore

Use the in-app wallet backup/seed procedure. A filesystem copy of
`wallet_data` is not a substitute for the seed envelope. Restore into a clean
testnet directory and run a full scan; verify the address and reported
balance before use. Never copy `C:\Flint\data1` (mainnet).

## Included artifacts

- `Flint.exe`, `flint-selftest.exe`, resources, runtime DLLs
- `BUILD-INFO.json`, `SHA256SUMS`, `sbom.json`
- `Flint-0.1.69-source.zip`
- `flint-transfer-contract-1.0.0.zip` + SHA-256 for the separate iOS task
