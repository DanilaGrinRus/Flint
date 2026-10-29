# Runs RustSec against Cargo.lock with an explicit list of accepted advisories.
# Accepted findings and their rationale: docs/security/RUSTSEC_2026-10-28.md.
# Usage: pwsh -File tools/rustsec.ps1 [-Update] [-Db <path>]
param(
    [switch]$Update,
    [string]$Db = "",
    [string]$Python = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Db) { $Db = Join-Path $root ".tmp\advisory-db" }
$json = Join-Path $root ".tmp\rustsec.json"
$err = Join-Path $root ".tmp\rustsec.err"

$audit = (Get-Command cargo-audit -ErrorAction SilentlyContinue).Source
if (-not $audit) {
    $local = Join-Path $root ".tmp\rustsec-tools\bin\cargo-audit.exe"
    if (Test-Path $local) { $audit = $local }
}
if (-not $audit) {
    throw "cargo-audit not found; install it into .tmp/rustsec-tools (see docs/security/RUSTSEC_2026-10-28.md)"
}

if (-not (Test-Path (Join-Path $Db ".git"))) {
    Write-Host "cloning RustSec advisory-db into $Db"
    git clone --depth 1 https://github.com/RustSec/advisory-db $Db
} elseif ($Update) {
    git -C $Db pull --ff-only
}
$dbCommit = (git -C $Db rev-parse HEAD).Trim()

$accepted = @(
    "RUSTSEC-2026-0387", # crypto-bigint: not built with current features
    "RUSTSEC-2023-0071"  # rsa via arti: external tor.exe, no remote RSA path
)

& $audit audit --file (Join-Path $root "Cargo.lock") --db $Db --no-fetch --json 1> $json 2> $err
$code = $LASTEXITCODE
if (-not (Test-Path $json) -or (Get-Item $json).Length -eq 0) {
    Get-Content $err -ErrorAction SilentlyContinue | Select-Object -Last 20 | Out-String | Write-Host
    throw "cargo-audit produced no JSON (exit $code)"
}
$report = Get-Content $json -Raw | ConvertFrom-Json
$vulnerabilities = @($report.vulnerabilities.list)
$unaccepted = @($vulnerabilities | Where-Object { $accepted -notcontains $_.advisory.id })
$warnings = @($report.warnings.PSObject.Properties | ForEach-Object { @($_.Value) })

Write-Host "RustSec: db $($dbCommit.Substring(0,12)), lock $((Get-FileHash (Join-Path $root 'Cargo.lock') -Algorithm SHA256).Hash.Substring(0,12))"
Write-Host "vulnerabilities: $($vulnerabilities.Count) (accepted $($vulnerabilities.Count - $unaccepted.Count), unaccepted $($unaccepted.Count)); warnings: $($warnings.Count)"

if ($unaccepted.Count -gt 0) {
    $unaccepted | ForEach-Object {
        Write-Host "UNACCEPTED $($_.advisory.id) $($_.package.name) $($_.package.version): $($_.advisory.title)"
    }
    exit 1
}
$vulnerabilities | ForEach-Object {
    Write-Host "accepted $($_.advisory.id) $($_.package.name) $($_.package.version)"
}
exit 0
