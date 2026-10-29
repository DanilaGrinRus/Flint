# Соответствующие исходники релиза. Полный vendor восстанавливается явным bootstrap.
param([string]$Version = '', [string]$BuildId = '', [string]$OutputDir = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$Version = Get-FlintVersion -Manifest (Join-Path $root 'Cargo.toml') -Expected $Version
$context = Get-FlintArtifactContext -Root $root -BuildId $BuildId -OutputDir $OutputDir
$zip = Join-Path $context.Path "Flint-$Version-source.zip"
if (Test-Path -LiteralPath $zip) { throw "Исходный архив уже существует: $zip. Выберите новый BuildId." }
$stage = Assert-FlintContainedPath -Root $root -Path (Join-Path $root ('.tmp/flint-source-' + [guid]::NewGuid().ToString('N')))
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Set-Content -LiteralPath (Join-Path $stage '.flint-ephemeral') -Value 'source-staging' -Encoding ascii
try {

function Copy-FlintSourceTree([string]$Source, [string]$Destination) {
    $item = Get-Item -LiteralPath $Source -Force
    if ($item.Name -match '^\.env(?:\..*)?$|^wallet\.seed(?:\..*)?$|^\.api_secret$' -or $item.Name -in @('testnet-funded.txt','grinflint-ci-wallet.txt')) { return }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Ссылка в исходниках: $Source" }
    if (-not $item.PSIsContainer) {
        if ($item.Name -notmatch '\.log$|^selftest\.json$') { Copy-Item -LiteralPath $Source -Destination $Destination }
        return
    }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    foreach ($entry in Get-ChildItem -LiteralPath $Source -Force) {
        if ($entry.PSIsContainer -and $entry.Name -in @('.git','target','test_output','.tmp','dist','data','vendor','node_modules','.cleanup-rollback','.secrets','.wallet-data','.flint','wallet_data','wallets')) { continue }
        Copy-FlintSourceTree -Source $entry.FullName -Destination (Join-Path $Destination $entry.Name)
    }
}

$items = @('Cargo.toml','Cargo.lock','COPYING','THIRD_PARTY_NOTICES.md','README.md','AGENTS.md',
    'crates','tools','docs','evidence','lang','.github','.cargo','third_party')
foreach ($item in $items) {
    $source = Join-Path $root $item
    if (Test-Path -LiteralPath $source) { Copy-FlintSourceTree -Source $source -Destination (Join-Path $stage $item) }
}
New-Item -ItemType Directory -Path (Join-Path $stage 'resources') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'resources/native-assets.sha256') -Destination (Join-Path $stage 'resources/native-assets.sha256')
$slintSources = @(Get-ChildItem -LiteralPath (Join-Path $root 'vendor') -Directory | Where-Object { $_.Name -match '^(slint|i-slint)(-|$)' })
if (-not $slintSources.Count) { throw 'Нет соответствующих исходников Slint в vendor. Сначала восстановите vendor.' }
foreach ($source in $slintSources) { Copy-FlintSourceTree -Source $source.FullName -Destination (Join-Path $stage "vendor/$($source.Name)") }
@"
Flint $Version — соответствующие исходники (GPL-3.0-only)

В архиве: workspace, закреплённый third_party/grin-wallet вместе с Grin,
каталог lang, инструменты, инструкции и точные исходники Slint.
vendor содержит ТОЛЬКО Slint: прямой offline build без восстановления остальных
зависимостей не поддерживается. Cargo.lock и .cargo/config.toml сохранены.

Подготовка чистой копии (сеть нужна только по явному запросу):
    pwsh -File tools/bootstrap.ps1 -RestoreVendor
    pwsh -File tools/bootstrap.ps1 -FetchNode -BuildWallet
    pwsh -File tools/ci.ps1
    pwsh -File tools/release.ps1

bootstrap без флагов только проверяет зависимости и не скачивает их.
Tor expert bundle (tor.exe, lyrebird.exe, pt_config.json) и vcruntime140.dll
нужно предоставить отдельно и проверить по resources/native-assets.sha256.
Автоматическое скачивание Tor/VC runtime здесь не реализовано.
Данные кошельков, ноды, build cache и локальные secrets в архив не входят.
Пакет бинарников распространяется вместе с этим соответствующим source archive.
"@ | Set-Content -LiteralPath (Join-Path $stage 'SOURCE.md') -Encoding UTF8

# ZipFile сохраняет скрытые .cargo/.github; Compress-Archive может их пропустить.
Remove-Item -LiteralPath (Join-Path $stage '.flint-ephemeral')
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
Write-FlintArtifactHashes -Path $context.Path
Write-Host "source: $zip"
} finally {
    # Удаляется только созданная этим запуском копия исходников.
    $stage = Assert-FlintContainedPath -Root $root -Path $stage
    Remove-Item -LiteralPath $stage -Recurse -Force
}
