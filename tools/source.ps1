# Builds a source archive next to the release binaries.
#
# vendor/, resources/ and target/ are deliberately excluded: they are large,
# platform-specific or fetched by tools/fetch-*.ps1. SOURCE.md in the archive
# explains how to reconstruct a working tree.
param(
    [string]$Version = "",
    [string]$OutputDir = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Version) {
    $Version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
}
$dist = if ($OutputDir) { $OutputDir } else { Join-Path $root "dist" }
$stage = Join-Path $root ".tmp\source-$Version"
$zip = Join-Path $dist "Flint-$Version-source.zip"
New-Item -ItemType Directory -Force $dist | Out-Null
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null

function Copy-ForkTree([string]$Source, [string]$Destination) {
    New-Item -ItemType Directory -Force $Destination | Out-Null
    foreach ($entry in Get-ChildItem $Source -Force) {
        if ($entry.PSIsContainer) {
            if (@(".git", "target", "test_output", ".tmp", "dist", "data", "vendor") -contains $entry.Name) {
                continue
            }
            Copy-ForkTree $entry.FullName (Join-Path $Destination $entry.Name)
        } else {
            Copy-Item $entry.FullName (Join-Path $Destination $entry.Name) -Force
        }
    }
}

$items = @(
    "Cargo.toml",
    "Cargo.lock",
    "COPYING",
    "THIRD_PARTY_NOTICES.md",
    "README.md",
    "AGENTS.md",
    "DESIGN.md",
    "crates",
    "tools",
    "docs",
    ".github",
    ".cargo",
    "third_party"
)
foreach ($item in $items) {
    $src = Join-Path $root $item
    if (-not (Test-Path $src)) { continue }
    $dst = Join-Path $stage $item
    New-Item -ItemType Directory -Force (Split-Path $dst -Parent) | Out-Null
    if ($item -eq "third_party") {
        Copy-ForkTree $src $dst
    } else {
        Copy-Item $src $dst -Recurse -Force
    }
}

# Generated / local-only trees inside otherwise-source directories.
foreach ($generated in @()) {
    $path = Join-Path $stage $generated
    if (Test-Path $path) { Remove-Item $path -Recurse -Force }
}
Get-ChildItem $stage -Recurse -File -Include "*.log","selftest.json" -ErrorAction SilentlyContinue |
    Remove-Item -Force -ErrorAction SilentlyContinue

# GPL corresponding source for the Slint dependency. The rest of vendor/ is
# fetched from Cargo.lock; these crates are the UI toolkit itself.
$slintVendor = Join-Path $stage "vendor"
New-Item -ItemType Directory -Force $slintVendor | Out-Null
Get-ChildItem (Join-Path $root "vendor") -Directory |
    Where-Object { $_.Name -match '^(slint|i-slint)' } |
    ForEach-Object { Copy-Item $_.FullName (Join-Path $slintVendor $_.Name) -Recurse -Force }

@"
Flint $Version — source archive
================================

This archive contains the Rust workspace, Slint UI, tools and documentation used
to build the $Version release. Flint is licensed under GPL-3.0-only; see COPYING
and THIRD_PARTY_NOTICES.md.

`third_party/grin-wallet` contains the exact F-04 fork and grin submodule;
`.cargo/config.toml` keeps Cargo offline. `GRIN_WALLET_FORK.toml` records both
commits and `grin-wallet-sources.sha256` authenticates every included fork file.
The Flint patch is also included as `third_party/grin-wallet-flint.patch`.

`vendor/` includes the exact Slint source used by this archive (GPLv3
corresponding source). Other vendored dependencies are intentionally excluded:

- vendor/ (rest)    — run `cargo vendor --locked vendor` with network access;
- resources/node    — run `pwsh -File tools/fetch-node.ps1`;
- resources/wallet  — run `pwsh -File tools/fetch-wallet.ps1` (builds the pinned
                       fork; it does not download an upstream binary);
- resources/tor     — Tor expert bundle (tor.exe + lyrebird.exe);
- target/, dist/, data/, .tmp/, .node/ — build outputs/local data.

When distributing a Slint binary, include this archive (with `vendor/slint*`)
or provide an equivalent GPL-compliant source offer.

Build:
    cargo build --release -p flint-slint --locked --offline
    cargo build --release -p flint-runtime --bin flint-selftest --locked --offline
    pwsh -File tools/fetch-wallet.ps1
    flint-selftest.exe

The source version is defined once in Cargo.toml (workspace.package.version).
"@ | Set-Content (Join-Path $stage "SOURCE.md") -Encoding UTF8

if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -CompressionLevel Optimal
$size = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "source: $zip ($size MB)"
