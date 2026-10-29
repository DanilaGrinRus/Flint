# Проверка, сборка и финализация одного version/build-id комплекта.
param([string]$Version = '', [string]$BuildId = '', [switch]$SkipTests, [switch]$SkipPackage,
    [string]$CertificateThumbprint = '', [switch]$RequireSigned)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$Version = Get-FlintVersion -Manifest (Join-Path $root 'Cargo.toml') -Expected $Version
if ($RequireSigned -and -not $CertificateThumbprint) { throw 'RequireSigned требует CertificateThumbprint' }
if ($SkipPackage -and -not $BuildId) { throw 'SkipPackage требует явный BuildId ранее подготовленного комплекта' }
$context = Get-FlintArtifactContext -Root $root -BuildId $BuildId -RequireExisting:$SkipPackage
$infoPath = Join-Path $context.Path 'BUILD-INFO.json'
if ((Test-Path -LiteralPath $infoPath) -and (Get-Content -LiteralPath $infoPath -Raw | ConvertFrom-Json).state -eq 'release') { throw 'Релиз уже финализирован; выберите новый BuildId' }
$env:TMP = Join-Path $root '.tmp'
$env:TEMP = $env:TMP
New-Item -ItemType Directory -Path $env:TMP -Force | Out-Null
Push-Location $root
try {
    if (-not $SkipPackage) {
        & (Join-Path $PSScriptRoot 'i18n-gen.ps1') -Check
        & (Join-Path $PSScriptRoot 'i18n-check.ps1')
        cargo fmt -p flint-core -p flint-janus -p flint-messenger -p flint-runtime -p flint-slint -p flint-transfer-contract -- --check
        if ($LASTEXITCODE -ne 0) { throw 'Проверка форматирования не удалась' }
        if (-not $SkipTests) {
            cargo test --workspace --locked --offline
            if ($LASTEXITCODE -ne 0) { throw 'Тесты не прошли' }
        }
        & (Join-Path $PSScriptRoot 'check-node-version.ps1') -Expected '5.5.2'
        & (Join-Path $PSScriptRoot 'fetch-wallet.ps1')
        & (Join-Path $PSScriptRoot 'package-slint.ps1') -OutputDir $context.Path
    }
    foreach ($required in @('BUILD-INFO.json','sbom.json','selftest.json',"Flint-$Version-source.zip",'Flint-slint-preview/Flint.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $context.Path $required) -PathType Leaf)) { throw "Неполный комплект: $required" }
    }
    & (Join-Path $PSScriptRoot 'verify-sbom.ps1') -Sbom (Join-Path $context.Path 'sbom.json')
    $previewExe = Join-Path $context.Path 'Flint-slint-preview/Flint.exe'
    $signed = $false
    if ($CertificateThumbprint) {
        $sign = Get-Command signtool.exe -ErrorAction SilentlyContinue
        if (-not $sign) { $sign = Get-ChildItem 'C:/Program Files (x86)/Windows Kits/10/bin/*/x64/signtool.exe' -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1 }
        if (-not $sign) { throw 'signtool.exe не найден' }
        $toolPath = if ($sign.Source) { $sign.Source } else { $sign.FullName }
        & $toolPath sign /sha1 $CertificateThumbprint /fd sha256 /tr http://timestamp.digicert.com /td sha256 $previewExe
        if ($LASTEXITCODE -ne 0) { throw 'Подпись не удалась' }
        if ((Get-AuthenticodeSignature -LiteralPath $previewExe).Status -ne 'Valid') { throw 'Authenticode не подтверждён' }
        $signed = $true
        $zip = Assert-FlintContainedPath -Root $root -Path (Join-Path $context.Path "Flint-$Version-slint-preview.zip")
        Remove-Item -LiteralPath $zip
        [IO.Compression.ZipFile]::CreateFromDirectory((Split-Path -Parent $previewExe), $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
    } else { Write-Warning 'Артефакты не подписаны: сертификат не передан' }
    $info = Get-Content -LiteralPath $infoPath -Raw | ConvertFrom-Json
    $info.state = 'release'
    $info.signed = $signed
    $info | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $infoPath -Encoding UTF8
    Write-FlintArtifactHashes -Path $context.Path
    Write-Host "Релиз: $($context.Path); signed=$signed"
} finally { Pop-Location }
