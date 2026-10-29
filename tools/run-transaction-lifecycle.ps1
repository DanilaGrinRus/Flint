# Runbook for the ignored runtime transaction lifecycle on disposable testnet wallets.
#
# Full mode moves testnet coins: send -> receive -> finalize -> post -> confirm
# -> cancel a separate operation -> restart both wallets and re-check balances.
# Scan-only mode is a separate test and never claims a full transfer.
#
#   pwsh -File tools/run-transaction-lifecycle.ps1 `
#     -DataA .tmp/live-a -DataB .tmp/live-b `
#     -PassA <password> -PassB <password> -Mode full
#
# Both roots must already exist, be marked with .crash-test-ok, and hold funded
# disposable wallets. Passwords are never printed or written by this script.

param(
    [Parameter(Mandatory = $true)][string]$DataA,
    [Parameter(Mandatory = $true)][string]$DataB,
    [Parameter(Mandatory = $true)][string]$PassA,
    [Parameter(Mandatory = $true)][string]$PassB,
    [ValidateSet("full", "scan-only")][string]$Mode = "full",
    [ValidateSet("both", "embedded", "rpc")][string]$Backend = "both",
    [string]$NodeUrl = "http://127.0.0.1:13413",
    [string]$NodeSecretFile = ".node/testnet/node/.api_secret",
    [int]$TimeoutSecs = 300
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:TMP = Join-Path $root ".tmp"
$env:TEMP = $env:TMP
New-Item -ItemType Directory -Force $env:TMP | Out-Null

function Fail([string]$message) {
    Write-Host "FAIL: $message" -ForegroundColor Red
    exit 1
}
if ($TimeoutSecs -lt 30 -or $TimeoutSecs -gt 1800) {
    Fail "TimeoutSecs must be between 30 and 1800"
}
$a = [IO.Path]::GetFullPath((Join-Path $root $DataA))
$b = [IO.Path]::GetFullPath((Join-Path $root $DataB))
if ($a -eq $b) { Fail "sender and receiver roots must differ" }
foreach ($dir in @($a, $b)) {
    if (-not (Test-Path $dir)) { Fail "wallet root does not exist: $dir" }
    if (-not (Test-Path (Join-Path $dir ".crash-test-ok"))) {
        Fail "wallet root is not marked with .crash-test-ok: $dir"
    }
    if ($dir.ToLowerInvariant().Contains("mainnet")) {
        Fail "mainnet-labelled wallet root is forbidden: $dir"
    }
}
$headers = @{}
if (Test-Path $NodeSecretFile) {
    $secret = (Get-Content $NodeSecretFile -Raw).Trim()
    if ($secret) {
        $basic = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("grin:$secret"))
        $headers["Authorization"] = "Basic $basic"
    }
}
try {
    $status = (Invoke-RestMethod -Uri "$NodeUrl/v2/owner" -Method Post `
        -Headers $headers `
        -Body '{"jsonrpc":"2.0","method":"get_status","params":[],"id":1}' `
        -ContentType "application/json" -TimeoutSec 8).result.Ok
} catch {
    Fail "testnet node is not reachable at $NodeUrl"
}
if ($null -eq $status) { Fail "testnet node returned an empty status" }
if ($status.chain -ne "test") { Fail "node chain is '$($status.chain)', expected testnet" }
if ($status.sync_status -ne "no_sync") { Fail "node is not synced: $($status.sync_status)" }

$env:FLINT_LIVE_TESTNET = "1"
$env:FLINT_LIVE_A = $a
$env:FLINT_LIVE_B = $b
$env:FLINT_LIVE_PASS = $PassA
$env:FLINT_LIVE_PASS_B = $PassB
$env:FLINT_LIVE_TIMEOUT_SECS = "$TimeoutSecs"
$env:FLINT_NODE_URL = $NodeUrl
if ($Mode -eq "scan-only") { $env:FLINT_LIVE_SCAN_ONLY = "1" } else {
    Remove-Item Env:FLINT_LIVE_SCAN_ONLY -ErrorAction SilentlyContinue
}
$filter = if ($Mode -eq "scan-only") { "scan_only_live" } else { "full_transaction_lifecycle" }
if ($Backend -eq "embedded") { $filter = if ($Mode -eq "scan-only") { "embedded_scan_only_live" } else { "embedded_full_transaction_lifecycle" } }
if ($Backend -eq "rpc") { $filter = if ($Mode -eq "scan-only") { "rpc_scan_only_live" } else { "rpc_full_transaction_lifecycle" } }
$started = Get-Date
try {
    Write-Host "mode=$Mode backend=$Backend timeout=${TimeoutSecs}s roots verified; passwords are not echoed"
    & cargo test -p flint-runtime --test transaction_lifecycle --locked --offline `
        -- --ignored --test-threads=1 --nocapture $filter
    $code = $LASTEXITCODE
} finally {
    $elapsed = [int]((Get-Date) - $started).TotalSeconds
    Remove-Item Env:FLINT_LIVE_TESTNET, Env:FLINT_LIVE_A, Env:FLINT_LIVE_B,
        Env:FLINT_LIVE_PASS, Env:FLINT_LIVE_PASS_B, Env:FLINT_LIVE_TIMEOUT_SECS,
        Env:FLINT_LIVE_SCAN_ONLY, Env:FLINT_NODE_URL -ErrorAction SilentlyContinue
}
Write-Host "elapsed=${elapsed}s mode=$Mode"
exit $code
