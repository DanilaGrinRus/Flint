# Third-party notices

Flint itself is licensed under **GPL-3.0-only**; see `COPYING`.

This file is a summary, not a replacement for the upstream license texts. Exact
versions are pinned in `Cargo.lock`; the release SBOM lists the dependency
inventory.

## Grin / Mimblewimble

- `grin`, `grin-wallet`, `grin_core`, `grin_keychain`, `grin_util`,
  `grin_wallet_*` — Apache License 2.0.
- Upstream: <https://github.com/mimblewimble/grin>,
  <https://github.com/mimblewimble/grin-wallet>.
- F-04 receive recovery: поддерживаемый исходник `third_party/grin-wallet`,
  база `v5.5.1` (`f097811ee953fc81be4e70435d6ef845b6b2e8c9`), commit
  `11046cd197f8296e6cd9313502a9f2cbce6e262a`; submodule `grin` —
  `dbaff34f7751ee23316fbbfe5806b47c546776ca`. Embedded и RPC собираются из
  этого источника. Патч и SHA-256 исходников находятся рядом в `third_party/`;
  уведомления Apache-2.0 сохранены в исходных файлах.

## Desktop shell and Rust libraries

- `arboard 3.7.0` — MIT OR Apache-2.0; clipboard support in the Slint shell.
- `qrcode 0.14.1` — MIT OR Apache-2.0; QR rendering in the Slint shell.
- Rust crates listed in `Cargo.lock` — their respective licenses (mostly
  MIT/Apache-2.0/BSD); the SBOM and crate metadata are authoritative.

## Tor

- `tor.exe` and Tor Project components — see
  <https://www.torproject.org/about/legal/>.
- `lyrebird` — Tor Project pluggable transport; see the Tor Project
  distribution for its license text.

## Slint

Slint 1.18.1 is the production UI (`crates/slint-ui`). Its crate metadata is:

`GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`

Flint uses the **GPL-3.0-only** option. The source archive includes
`vendor/slint*` and `vendor/i-slint*`; the Slint binary ships with the same
corresponding source (or a GPL-compliant source offer).

## Microsoft Visual C++ Runtime

The portable preview carries the Microsoft Visual C++ 2015–2022 runtime file
`vcruntime140.dll` next to `Flint.exe` and `resources\wallet\grin-wallet.exe`.
It is redistributed under the Microsoft Visual Studio license terms:
<https://visualstudio.microsoft.com/license-terms/>. The file is unmodified.
The pinned copy is `resources/runtime/vcruntime140.dll`; its SHA-256 and the
hashes of the other native binaries are in `resources/native-assets.sha256`.

