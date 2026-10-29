# Версионированный source package контракта для iOS CI; версия берётся из его Cargo.toml.
param([string]$Version = '', [string]$BuildId = '', [string]$OutputDir = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$source = Join-Path $root 'crates/transfer-contract'
Assert-FlintNoReparseTree -Root $root -Path $source
$Version = Get-FlintVersion -Manifest (Join-Path $source 'Cargo.toml') -Section 'package' -Expected $Version
$context = Get-FlintArtifactContext -Root $root -BuildId $BuildId -OutputDir $OutputDir
$zip = Join-Path $context.Path "flint-transfer-contract-$Version.zip"
if (Test-Path -LiteralPath $zip) { throw "Архив уже существует: $zip. Выберите новый BuildId." }
Push-Location $source
try { Compress-Archive -LiteralPath @('Cargo.toml','src','fixtures','README.md') -DestinationPath $zip }
finally { Pop-Location }
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -LiteralPath (Join-Path $context.Path "flint-transfer-contract-$Version.sha256") -Value "$hash  flint-transfer-contract-$Version.zip" -Encoding ascii
Write-FlintArtifactHashes -Path $context.Path
Write-Host "package=$zip"
Write-Host "sha256=$hash"
