# Downloads the official Grin node (Rust) into resources/node and verifies the
# published SHA256. Run once; the app then manages the node itself.
param(
    [string]$Version = "5.5.2",
    [string]$Sha256 = "7c11350effb869a6e3ed9d7e32aa2ad5ef6db4707ca659b4ac61659345d652cc"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "resources\node"
$zip = Join-Path $dir "grin-v$Version-win-x86_64.zip"
$url = "https://github.com/mimblewimble/grin/releases/download/v$Version/grin-v$Version-win-x86_64.zip"

New-Item -ItemType Directory -Force $dir | Out-Null

if (-not (Test-Path $zip)) {
    Write-Host "downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
}

$actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $Sha256.ToLower()) {
    Remove-Item $zip -Force
    throw "SHA256 mismatch: got $actual, expected $Sha256"
}

Expand-Archive -Path $zip -DestinationPath $dir -Force
& (Join-Path $dir "grin.exe") --version
Write-Host "node ready: $(Join-Path $dir 'grin.exe')"
