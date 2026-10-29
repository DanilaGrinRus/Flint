# Run the live (ignored) suites against a real testnet node.
#
# These tests move real testnet coins and take minutes; they are NOT part of
# `cargo test` and there is no CI provider configured in this project. Run this
# manually before a release.
#
# Prerequisites:
#   * synced testnet node (tools/run-testnet-node.ps1 start);
#   * two embedded wallet dirs: funded sender (A) and receiver (B);
#   * same password for both (default below).
#
# Usage:
#   pwsh -File tools/run-live-suite.ps1 -A .tmp/emb-dst -B .tmp/emb-recv

param(
    [Parameter(Mandatory = $true)][string]$A,
    [Parameter(Mandatory = $true)][string]$B,
    [string]$Pass = "live-testnet-scenario",
    [int]$TimeoutSecs = 300
)

$ErrorActionPreference = "Stop"
$env:TMP = (Resolve-Path ".").Path + "\.tmp"
$env:TEMP = $env:TMP
$env:FLINT_LIVE_TESTNET = "1"
$env:FLINT_LIVE_A = (Resolve-Path $A).Path
$env:FLINT_LIVE_B = (Resolve-Path $B).Path
$env:FLINT_LIVE_PASS = $Pass
$env:FLINT_LIVE_TIMEOUT_SECS = "$TimeoutSecs"

Write-Output "live lifecycle: send -> receive -> confirm -> cancel"
cargo test -p flint-core --test live_testnet -- --ignored --nocapture
$code = $LASTEXITCODE

Remove-Item Env:FLINT_LIVE_TESTNET, Env:FLINT_LIVE_A, Env:FLINT_LIVE_B,
    Env:FLINT_LIVE_PASS, Env:FLINT_LIVE_TIMEOUT_SECS -ErrorAction SilentlyContinue
exit $code
