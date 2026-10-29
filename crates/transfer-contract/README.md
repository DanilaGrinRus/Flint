# Flint transfer contract v1

Platform-neutral money/stage/action rules for desktop and iOS. No UI, Windows,
network, filesystem or secret storage. Source of this crate is standalone and
does not depend on `C:\DHW`.

## Provenance

- Contract semantics: `docs/architecture/IOS_SHARED_CONTRACT.md`, schema 1.
- Desktop integration: `crates/slint-ui` pending actions/next-action gating.
- Crypto and wallet signing stay in upstream `grin-wallet` 5.5.1/5.5.2 fork;
  this crate performs no cryptography.
- License: GPL-3.0-only (same as Flint).

## Build

`cargo test -p flint-transfer-contract --offline`

For iOS, vendor the generated `flint-transfer-contract-1.zip` source package
and add it as a path dependency inside the iOS FFI workspace. Do not reference
`C:\DHW`.
