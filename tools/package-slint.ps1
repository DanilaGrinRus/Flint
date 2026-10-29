# Portable и corresponding source пишутся только в новый dist/<version>-<build-id>.
param([switch]$SkipBuild, [string]$BuildId = '', [string]$OutputDir = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$context = Get-FlintArtifactContext -Root $root -BuildId $BuildId -OutputDir $OutputDir
$version = $context.Version
$dir = Join-Path $context.Path 'Flint-slint-preview'
if (Test-Path -LiteralPath $dir) { throw "Portable уже существует: $dir. Выберите новый BuildId." }
Push-Location $root
try {
    $env:TMP = Join-Path $root '.tmp'
    $env:TEMP = $env:TMP
    New-Item -ItemType Directory -Path $env:TMP -Force | Out-Null
    if (-not $SkipBuild) {
        cargo build --release -p flint-slint --locked --offline
        if ($LASTEXITCODE -ne 0) { throw 'Сборка flint-slint не удалась' }
        cargo build --release -p flint-runtime --bin flint-selftest --locked --offline
        if ($LASTEXITCODE -ne 0) { throw 'Сборка flint-selftest не удалась' }
    }
    & (Join-Path $PSScriptRoot 'verify-native-assets.ps1')
    $assets = @{
        'target/release/flint-slint.exe' = 'Flint.exe'
        'target/release/flint-selftest.exe' = 'flint-selftest.exe'
        'resources/node/grin.exe' = 'resources/node/grin.exe'
        'resources/wallet/grin-wallet.exe' = 'resources/wallet/grin-wallet.exe'
        'resources/runtime/vcruntime140.dll' = 'vcruntime140.dll'
        'resources/tor/expert/tor/tor.exe' = 'resources/tor/expert/tor/tor.exe'
        'resources/tor/expert/tor/pluggable_transports/lyrebird.exe' = 'resources/tor/expert/tor/pluggable_transports/lyrebird.exe'
        'resources/tor/expert/tor/pluggable_transports/pt_config.json' = 'resources/tor/expert/tor/pluggable_transports/pt_config.json'
        'COPYING' = 'COPYING'
        'THIRD_PARTY_NOTICES.md' = 'THIRD_PARTY_NOTICES.md'
    }
    foreach ($source in $assets.Keys) {
        $destination = Join-Path $dir $assets[$source]
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $root $source) -Destination $destination
    }
    Copy-Item -LiteralPath (Join-Path $root 'resources/runtime/vcruntime140.dll') -Destination (Join-Path $dir 'resources/wallet/vcruntime140.dll')
    Set-Content -LiteralPath (Join-Path $dir 'portable') -Value '' -NoNewline
    @"
Flint $version — portable DEV BUILD, testnet по умолчанию.
Запуск: Flint.exe. Данные кошельков сохраняются рядом с программой в data/.
RPC: Flint.exe --backend=grin-wallet. Для скана и отправки нужна testnet-нода.
flint-selftest.exe выполняет изолированную проверку без пользовательских средств.
Соответствующие исходники, SBOM и SHA256SUMS находятся рядом с portable-пакетом.
"@ | Set-Content -LiteralPath (Join-Path $dir 'README.txt') -Encoding UTF8
    Invoke-FlintIsolatedSelftest -Root $root -Binary (Join-Path $dir 'flint-selftest.exe') -ReportPath (Join-Path $context.Path 'selftest.json')
    $zip = Join-Path $context.Path "Flint-$version-slint-preview.zip"
    [IO.Compression.ZipFile]::CreateFromDirectory($dir, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
    & (Join-Path $PSScriptRoot 'source.ps1') -Version $version -OutputDir $context.Path
    & (Join-Path $PSScriptRoot 'sbom.ps1') -Out (Join-Path $context.Path 'sbom.json')
    & (Join-Path $PSScriptRoot 'verify-sbom.ps1') -Sbom (Join-Path $context.Path 'sbom.json')
    $info = [ordered]@{
        version=$version; build_id=$context.BuildId; state='packaged'; signed=$false
        target='x86_64-pc-windows-msvc'; profile='release'
        generated_at=(Get-Date).ToUniversalTime().ToString('o')
        cargo_lock_sha256=(Get-FileHash -LiteralPath (Join-Path $root 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
        rustc=(rustc -Vv | Out-String).Trim(); cargo=(cargo -V | Out-String).Trim()
        zip=(Split-Path -Leaf $zip); source_zip="Flint-$version-source.zip"
    }
    $info | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $context.Path 'BUILD-INFO.json') -Encoding UTF8
    Write-FlintArtifactHashes -Path $context.Path
    Write-Host "Артефакты: $($context.Path)"
} finally { Pop-Location }
