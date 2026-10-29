# Verifies the bundled node binary's reported version and records its SHA-256.
# Usage: pwsh -File tools/check-node-version.ps1 [-Expected 5.5.2]
param(
    [string]$Expected = "5.5.2"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $root "resources\node\grin.exe"
if (-not (Test-Path $bin)) { throw "нет $bin" }
$out = & $bin --version 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { throw "grin.exe --version завершился с кодом $LASTEXITCODE`n$out" }
if ($out -notmatch "grin\s+$([regex]::Escape($Expected))") {
    throw "ожидался grin $Expected, получено:`n$out"
}
$hash = (Get-FileHash $bin -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "node: grin $Expected, sha256=$hash, path=$bin"
