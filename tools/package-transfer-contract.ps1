# Build the standalone, versioned transfer-contract source package for iOS CI.
param(
    [string]$Version = "1.0.0",
    [string]$OutputDir = "lastver"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root "crates/transfer-contract"
$out = Join-Path $root $OutputDir
New-Item -ItemType Directory -Force $out | Out-Null
$zip = Join-Path $out "flint-transfer-contract-$Version.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Push-Location $source
try {
    Compress-Archive -Path @("Cargo.toml", "src", "fixtures", "README.md") -DestinationPath $zip -Force
} finally { Pop-Location }
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content (Join-Path $out "flint-transfer-contract-$Version.sha256") "$hash  flint-transfer-contract-$Version.zip" -Encoding ASCII
Write-Host "package=$zip"
Write-Host "sha256=$hash"
