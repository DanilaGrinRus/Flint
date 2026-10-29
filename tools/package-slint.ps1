# Build and package the Slint-only preview.
# Usage: pwsh -File tools/package-slint.ps1 [-SkipBuild]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $env:TMP = Join-Path $root ".tmp"
    $env:TEMP = $env:TMP
    if (-not $SkipBuild) {
        cargo build --release -p flint-slint
        if ($LASTEXITCODE -ne 0) { throw "flint-slint release build failed" }
        cargo build --release -p flint-runtime --bin flint-selftest
        if ($LASTEXITCODE -ne 0) { throw "flint-selftest release build failed" }
    }

    $exe = Join-Path $root "target\release\flint-slint.exe"
    if (-not (Test-Path $exe)) { throw "missing $exe" }
    $node = Join-Path $root "resources\node\grin.exe"
    if (-not (Test-Path $node)) { throw "missing node binary: $node" }
    $wallet = Join-Path $root "resources\wallet\grin-wallet.exe"
    if (-not (Test-Path $wallet)) {
        throw "missing RPC wallet: $wallet — run tools/fetch-wallet.ps1 first"
    }

    $manifest = Join-Path $root "resources\native-assets.sha256"
    if (-not (Test-Path $manifest)) { throw "missing native asset manifest: $manifest" }
    foreach ($line in Get-Content $manifest) {
        if ($line -notmatch '^([0-9a-f]{64})\s+(.+)$') { throw "bad native asset pin: $line" }
        $expected = $matches[1]
        $rel = $matches[2]
        $path = Join-Path $root $rel
        if (-not (Test-Path $path)) { throw "missing pinned native asset: $path" }
        $actual = (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $expected) { throw "native asset hash mismatch: $rel" }
    }

    $dir = Join-Path $root "dist\Flint-slint-preview"
    if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
    New-Item -ItemType Directory -Force (Join-Path $dir "resources\node") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $dir "resources\wallet") | Out-Null

    Copy-Item $exe (Join-Path $dir "Flint.exe")
    Copy-Item $node (Join-Path $dir "resources\node\grin.exe")
    Copy-Item $wallet (Join-Path $dir "resources\wallet\grin-wallet.exe")

    # Flint, selftest and grin-wallet import VCRUNTIME140.dll. Keep the runtime
    # next to each executable so the portable preview starts without a
    # machine-wide VC++ Redistributable install. UCRT api-ms-win-crt-* is an OS
    # component on supported Windows 10/11. The pinned copy in resources/runtime
    # is used instead of the machine's System32 file so the packaged bytes are
    # reproducible and covered by resources/native-assets.sha256.
    $vcruntime = Join-Path $root "resources\runtime\vcruntime140.dll"
    if (-not (Test-Path $vcruntime)) { throw "missing pinned VC runtime: $vcruntime" }
    Copy-Item $vcruntime (Join-Path $dir "vcruntime140.dll")
    Copy-Item $vcruntime (Join-Path $dir "resources\wallet\vcruntime140.dll")

    # Tor is optional, but a preview that advertises "Приём по Tor" must ship
    # tor.exe and the unified lyrebird client; otherwise the settings button can
    # only report a missing binary.
    $torRoot = Join-Path $root "resources\tor\expert\tor"
    foreach ($rel in @("tor.exe", "pluggable_transports\lyrebird.exe", "pluggable_transports\pt_config.json")) {
        $src = Join-Path $torRoot $rel
        if (-not (Test-Path $src)) { throw "missing Tor file: $src" }
        $dst = Join-Path $dir ("resources\tor\expert\tor\" + $rel)
        New-Item -ItemType Directory -Force (Split-Path $dst -Parent) | Out-Null
        Copy-Item $src $dst
    }
    Copy-Item (Join-Path $root "COPYING") $dir
    Copy-Item (Join-Path $root "THIRD_PARTY_NOTICES.md") $dir
    Set-Content (Join-Path $dir "portable") "" -NoNewline

    $selftest = Join-Path $root "target\release\flint-selftest.exe"
    if (Test-Path $selftest) { Copy-Item $selftest (Join-Path $dir "flint-selftest.exe") }

    @'
Flint Slint preview (0.1.69)
DEV BUILD, not audited, testnet only.

Slint-only shell. Wallet, send/receive, history, swaps, settings,
QR, clipboard, node controls, seed security, the RPC backend, Tor and messenger
(send/inbox/history/contacts/blocked) are available.

Run Flint.exe. Data is kept next to the executable (portable).
RPC mode: run `Flint.exe --backend=grin-wallet` (or set FLINT_BACKEND=grin-wallet);
the bundled `resources\wallet\grin-wallet.exe` is used. A testnet node is still
required for scanning, sending and balance refresh.
`flint-selftest.exe` runs the headless wallet/Janus/node check.
The package carries Microsoft vcruntime140.dll next to Flint/selftest and
resources\wallet so no machine-wide VC++ Redistributable is required.
'@ | Set-Content (Join-Path $dir "README.txt") -Encoding UTF8

    $zip = Join-Path $root "dist\Flint-0.1.69-slint-preview.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $dir "*") -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  dist/Flint-0.1.69-slint-preview.zip" | Set-Content (Join-Path $root "dist\Flint-slint-preview.sha256") -Encoding ascii

    # Canonical latest portable build, kept separate from dist build output.
    $lastver = Join-Path $root "lastver"
    if (Test-Path $lastver) { Remove-Item $lastver -Recurse -Force }
    New-Item -ItemType Directory -Force $lastver | Out-Null
    Copy-Item $dir (Join-Path $lastver "Flint-slint-preview") -Recurse
    Copy-Item $zip $lastver
    Copy-Item (Join-Path $root "dist\Flint-slint-preview.sha256") $lastver
    # lastver is the canonical snapshot: sources must travel with the binary.
    & (Join-Path $root "tools\source.ps1") -Version "0.1.69" -OutputDir $lastver
    $sourceZip = Join-Path $lastver "Flint-0.1.69-source.zip"
    $sourceHash = (Get-FileHash $sourceZip -Algorithm SHA256).Hash.ToLowerInvariant()
    $exeHash = (Get-FileHash (Join-Path $dir "Flint.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
    @{
        version = "0.1.69"
        built_at = (Get-Date).ToString("o")
        zip = "Flint-0.1.69-slint-preview.zip"
        zip_sha256 = $hash
        zip_size = (Get-Item $zip).Length
        source_zip = "Flint-0.1.69-source.zip"
        source_sha256 = $sourceHash
        source_size = (Get-Item $sourceZip).Length
        exe_sha256 = $exeHash
        exe_size = (Get-Item (Join-Path $dir "Flint.exe")).Length
        source = "crates + tools/package-slint.ps1"
    } | ConvertTo-Json | Set-Content (Join-Path $lastver "BUILD-INFO.json") -Encoding UTF8

    Write-Host "preview: $zip"
    Write-Host "latest:  $lastver"
    Write-Host "sha256: $hash"
} finally {
    Pop-Location
}
