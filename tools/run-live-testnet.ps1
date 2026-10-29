# Live testnet scenario runner (send -> receive -> finalize -> post -> cancel).
#
#   pwsh -File tools/run-live-testnet.ps1 -DataA .live/a -DataB .live/b
#
# Refuses to do anything unless the node proves it is ready:
#   * its own get_status says no_sync with a non-zero tip;
#   * its chain is "test" (testnet), never mainnet;
#   * both data directories do not exist yet.
#
# The heavy lifting is crates/core/tests/live_testnet.rs (ignored by default).
# Funding is not automated: the sender wallet must already hold testnet coins,
# otherwise the scenario stops with a clear message before spending anything.
param(
    [string]$DataA = ".live/sender",
    [string]$DataB = ".live/receiver",
    [string]$NodeUrl = "http://127.0.0.1:13413"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Fail($message) {
    Write-Host "ОТКАЗ: $message" -ForegroundColor Red
    exit 1
}

# --- preflight: the node must prove itself ready and on testnet -----------------
$status = $null
try {
    $body = '{"jsonrpc":"2.0","method":"get_status","params":[],"id":1}'
    $status = (Invoke-RestMethod -Uri "$NodeUrl/v2/owner" -Method Post -Body $body `
        -ContentType "application/json" -TimeoutSec 5).result.Ok
} catch {
    Fail "нода не отвечает на get_status ($NodeUrl) — сценарий ничего не создаёт"
}
if ($null -eq $status) { Fail "нода вернула пустой статус" }

if ($status.chain -ne "test") {
    Fail ("сеть ноды '" + $status.chain + "', а сценарий разрешён только для testnet")
}
if ($status.sync_status -ne "no_sync") {
    Fail ("нода не готова: sync_status=" + $status.sync_status +
        ", current=" + $status.sync_info.current_height + ", highest=" + $status.sync_info.highest_height)
}
if (-not $status.tip -or [int]$status.tip.height -le 0) {
    Fail "нода сообщает no_sync, но tip пустой — тестировать нечего"
}

# --- preflight: fresh directories only ------------------------------------------
foreach ($dir in @($DataA, $DataB)) {
    if (Test-Path $dir) { Fail "каталог $dir уже существует — сценарий создаёт только новые кошельки" }
}

Write-Host ("нода готова: высота {0}, соединений {1}" -f $status.tip.height, $status.connections)

# --- run ------------------------------------------------------------------------
$env:FLINT_LIVE_TESTNET = "1"
$env:FLINT_LIVE_A = (Join-Path $root $DataA)
$env:FLINT_LIVE_B = (Join-Path $root $DataB)
$env:FLINT_NODE_URL = $NodeUrl

cargo test -p flint-core --test live_testnet -- --ignored --nocapture
exit $LASTEXITCODE
