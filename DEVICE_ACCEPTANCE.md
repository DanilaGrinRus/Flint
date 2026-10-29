# DEVICE_ACCEPTANCE — physical Windows devices only

Build/hash: fill from the final `BUILD-INFO.json`; do not run against mainnet.
Preconditions: isolated testnet data, disposable funded wallets, second
non-admin Windows account, supported NTFS/exFAT-mounted test volume, physical
DPI monitor(s), NVDA and IME available. Preserve logs/screenshots outside the
portable tree; reset by deleting only `%TEMP%\flint-device-*` and the disposable
testnet roots.

| ID | Check | Steps | Expected | Evidence |
|---|---|---|---|---|
| D-01 | clean install | unpack portable RC on a clean machine, launch, create testnet wallet | app starts, no missing DLL/resource, testnet label | screenshot + BUILD-INFO hash |
| D-02 | second account | run as another local account | independent data, no access to first account secrets | screenshot + directory ACL listing |
| D-03 | filesystem | run data root on supported NTFS and exFAT test volume | startup/open/scan works; no silent data loss | log + screenshots |
| D-04 | physical disk-full | on disposable stand fill volume, perform create/scan/save | clear error, no corrupt wallet; recovery after freeing space | error screenshot + restored test |
| D-05 | power-loss | kill power during disposable save/scan | restart offers recovery; no false confirmed balance | photos/logs + restart evidence |
| D-06 | real displays/DPI | 100%, 125%, 150%, mixed-DPI monitors | no clipped actions; dialogs readable | screenshots per display |
| D-07 | NVDA | launch, unlock, send/receive dialogs, pending | controls announced, no trap | NVDA transcript |
| D-08 | IME | RU/EN IME in password/address/amount fields | no lost characters, no unintended submit | screenshots/video |
| D-09 | clipboard | copy address/slatepack; export response | exact armor/address; no seed/password | clipboard capture + hash |
| D-10 | independent backup/restore | export backup, restore into clean testnet dir on another device | same address, full scan, expected funds | before/after balances + screenshots |

Unsafe reset: do not delete user wallets; only disposable `%TEMP%\flint-device-*`
and explicitly marked testnet roots. Mainnet and fixtures are out of scope.
