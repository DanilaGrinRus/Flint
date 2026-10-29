# RC acceptance matrix

Build under test: `lastver/Flint-0.1.69-keyboard` (portable), pending final
repackaging after the latest source changes. Evidence paths are repository-local;
device evidence is recorded in `DEVICE_ACCEPTANCE.md`.

| Function | Backend | Verification | Result | Evidence |
|---|---|---|---|---|
| create/open/lock wallet | embedded | automated + manual | PASS | `cargo test --workspace`; manual onboarding screenshots |
| create/open/lock wallet | RPC | automated | PASS | `rpc_lifecycle` |
| restore funded testnet | embedded | live, clean dir | PASS | WORK_LOG 68: 1397.39 GRIN, full scan |
| profiles/switch | embedded/RPC | tests + manual | PASS | multiwallet/RPC lifecycle |
| node connect/status | embedded/RPC | manual + tests | PASS | UI node card; node tests |
| balances/history | embedded/RPC | tests + manual | PASS | balance split tests; screenshots |
| send/receive/finalize/post/confirm | embedded | live testnet | PASS | transaction_lifecycle, 1 pass |
| send/receive/finalize/post/confirm | RPC | live testnet | PASS | RPC lifecycle 309.62 s |
| cancel/restart recovery | embedded/RPC | live testnet | PASS | transaction_lifecycle assertions |
| pending export response | embedded | manual | PASS | clipboard 1902 chars, armor |
| pending export response | RPC | live pending test | PASS | `rpc_pending_export_live`: S1/S2 armor, same slate_id, finalize, confirmation |
| resend S1 request | both | live pending test | PASS | S1 export is byte-identical and repeated without new signature |
| messenger contacts/block/history | embedded | tests + manual | PASS | UI-тесты, screenshots, WORK_LOG |
| messenger durable inbox/outbox | embedded | fault/restart drill | PASS | WORK_LOG 66 queue cleared after kill |
| messenger replay after ACK loss | embedded | OS kill after attach | PASS | `.tmp/replay-kill`: reply_sent persisted, restart queue cleared, no second S2 |
| transfer contract v1 | platform-neutral | unit + fixtures | PASS | 3 tests; `flint-transfer-contract-1.0.0.zip` SHA `ca7ebbc5…` |
| localization source/generator | both | CI check | PASS | `i18n-gen -Check`, 215 static entries |
| muted contrast | UI | automated ratio + manual | PASS | 5.25:1; 125% screenshot |
| 150% scaling | UI | manual | PASS | window 1920×1290; onboarding/create/restore screenshots |
| Tor direct live | RPC | live suite | PASS | direct_onion 1, tor_hidden_service 2, tor_listener_gateway 1 |
| GitHub live jobs | both | dispatch preflight | NOT VERIFIED | jobs defined, not run in GitHub |
| dependencies/advisories | both | RustSec/native assets | NOT VERIFIED | final run pending |
| portable package/SBOM/source | both | build + launch outside tree | PASS | zip `9d913509…`, exe `91db1181…`; launch outside tree PASS |
| device-only checks | both | physical Windows devices | DEVICE ONLY | `DEVICE_ACCEPTANCE.md` |

## Status

**NOT READY_FOR_EXTERNAL_DEVICE_TESTS** until all non-device NOT VERIFIED rows
above become PASS and the package is rebuilt after the last change.
