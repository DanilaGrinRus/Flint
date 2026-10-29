# Crash tests for the RPC wallet backend, on a live testnet node.
#
# Simulates two process deaths at journal boundaries and checks that the next
# process resumes without signing twice:
#   1. after send_prepare (journal "locked", slate not delivered yet)
#   2. after finalize_tx (journal "finalized_not_posted", tx never published)
#
# Requires: running testnet node, two funded wallets, wallet_cli built in debug.
# Usage:
#   pwsh -File tools/crash-test-rpc.ps1 -DataA .tmp/e2e-a -DataB .tmp/e2e-b `
#        -PassA e2e-pass-123 -PassB e2e-pass-123 -Dest tgrin1... -Amount 10000000

param(
    [Parameter(Mandatory = $true)][string]$DataA,
    [Parameter(Mandatory = $true)][string]$DataB,
    [Parameter(Mandatory = $true)][string]$PassA,
    [Parameter(Mandatory = $true)][string]$PassB,
    [Parameter(Mandatory = $true)][string]$Dest,
    [long]$Amount = 10000000,
    [string]$Cli = "target/debug/examples/wallet_cli.exe",
    [string]$Work = ".tmp/crash-test",
    [string]$NodeUrl = "http://127.0.0.1:13413"
)

$ErrorActionPreference = "Stop"
$env:TMP = (Resolve-Path ".").Path + "\.tmp"
$env:TEMP = $env:TMP
$env:FLINT_MIN_CONF = "1"
$env:FLINT_NODE_URL = $NodeUrl
New-Item -ItemType Directory -Force $Work | Out-Null
$cli = (Resolve-Path $Cli).Path
$DataA = (Resolve-Path $DataA).Path
$DataB = (Resolve-Path $DataB).Path

if ($DataA -match 'mainnet' -or $DataB -match 'mainnet') {
    throw "crash-test-rpc refuses mainnet data directories"
}
foreach ($dir in @($DataA, $DataB)) {
    if (-not (Test-Path (Join-Path $dir ".crash-test-ok"))) {
        throw "refusing unmarked data directory (create .crash-test-ok after verifying testnet): $dir"
    }
}

# Only processes started by this script may be killed. Tracking by name would
# terminate an unrelated wallet (audit F-11).
$script:OwnedWallets = [System.Collections.Generic.HashSet[int]]::new()

function Get-DescendantPids([int]$ParentPid) {
    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$ParentPid" -ErrorAction SilentlyContinue
    foreach ($child in $children) {
        $child.ProcessId
        Get-DescendantPids $child.ProcessId
    }
}

function Register-WalletChildren([int]$ParentPid) {
    foreach ($pidValue in @(Get-DescendantPids $ParentPid)) {
        $proc = Get-Process -Id $pidValue -ErrorAction SilentlyContinue
        if ($proc -and $proc.ProcessName -eq 'grin-wallet') {
            [void]$script:OwnedWallets.Add($pidValue)
        }
    }
}

function Stop-Wallets {
    foreach ($pidValue in $script:OwnedWallets) {
        & cmd.exe /d /c "taskkill /PID $pidValue /T /F >nul 2>&1" | Out-Null
    }
    $script:OwnedWallets.Clear()
}

# Start-Process waits for the CLI itself, not for inherited pipes; a crash hook
# leaves the wallet child alive, so `& $cli` would hang on the pipe.
function Run-Cli([string[]]$CliArgs) {
    $outFile = Join-Path $Work "cli.out"
    $errFile = Join-Path $Work "cli.err"
    # No -Wait: PowerShell waits for the whole process tree, and a crash hook
    # deliberately leaves the wallet child alive.
    $process = Start-Process -FilePath $cli -ArgumentList $CliArgs -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    # Keep ExitCode available for short-lived processes under PowerShell 5.1.
    $processHandle = $process.Handle
    Start-Sleep -Milliseconds 500
    Register-WalletChildren $process.Id
    $deadline = (Get-Date).AddSeconds(180)
    while (-not $process.HasExited) {
        if ((Get-Date) -gt $deadline) {
            try { taskkill /PID $process.Id /T /F 2>&1 | Out-Null } catch {}
            throw "wallet_cli $($CliArgs[0]) did not exit in 180s"
        }
        Start-Sleep -Milliseconds 200
    }
    $code = $process.ExitCode
    $out = Get-Content $outFile -Raw -ErrorAction SilentlyContinue
    $err = Get-Content $errFile -Raw -ErrorAction SilentlyContinue
    [pscustomobject]@{ Code = $code; Out = $out; Err = $err }
}

function Balance([string]$dir, [string]$pass) {
    Stop-Wallets
    $result = Run-Cli @("balance", $dir, $pass)
    Stop-Wallets
    return ($result.Out -split "`n" | Where-Object { $_ -like "spendable=*" } | Select-Object -Last 1)
}

Write-Output "== crash test: RPC backend =="
$before = Balance $DataA $PassA
Write-Output "A before: $before"

# --- 1. die right after send_prepare -----------------------------------------
$env:FLINT_CRASH_AFTER_SEND = "1"
$send = Run-Cli @("send", $DataA, $PassA, $Dest, "$Amount")
Remove-Item Env:FLINT_CRASH_AFTER_SEND
if ($send.Code -ne 9) { throw "expected crash exit 9 after send, got $($send.Code): $($send.Err)" }
$id = ($send.Out -split "`n" | Where-Object { $_ -like "SLATE_ID=*" } | Select-Object -First 1) -replace "SLATE_ID=([^ ]+).*", '$1'
$slate = ($send.Out -split "`n" | Where-Object { $_ -like "BEGINSLATEPACK*" } | Select-Object -First 1)
if (-not $id -or -not $slate) { throw "send produced no slate before crash: $($send.Out)" }
Set-Content "$Work/slate1.txt" -Value $slate -NoNewline
Stop-Wallets

$pending = (Run-Cli @("pending", $DataA, $PassA)).Out
Stop-Wallets
if ($pending -notmatch [regex]::Escape($id) -or $pending -notmatch "state=locked") {
    throw "journal after crash does not show locked slate: $pending"
}
Write-Output "1) crash after send_prepare: journal locked, slate preserved"

# --- receiver answers, sender "dies" after finalize ---------------------------
Stop-Wallets
$recv = Run-Cli @("receive", $DataB, $PassB, "$Work/slate1.txt")
Stop-Wallets
$response = ($recv.Out -split "`n" | Where-Object { $_ -like "BEGINSLATEPACK*" } | Select-Object -First 1)
if (-not $response) { throw "receiver produced no response: $($recv.Err)" }
Set-Content "$Work/slate2.txt" -Value $response -NoNewline

$env:FLINT_CRASH_AFTER_FINALIZE = "1"
$finalize = Run-Cli @("finalize", $DataA, $PassA, $id, "$Work/slate2.txt")
Remove-Item Env:FLINT_CRASH_AFTER_FINALIZE
if ($finalize.Code -ne 9) { throw "expected crash exit 9 after finalize, got $($finalize.Code): $($finalize.Err)" }
Stop-Wallets

$pending = (Run-Cli @("pending", $DataA, $PassA)).Out
Stop-Wallets
if ($pending -notmatch "state=finalized_not_posted") {
    throw "journal after crash does not show finalized_not_posted: $pending"
}
Write-Output "2) crash after finalize: journal finalized_not_posted, tx not published"

# --- next process publishes the stored transaction, no second signing --------
Stop-Wallets
$resume = Run-Cli @("finalize", $DataA, $PassA, $id, "$Work/slate2.txt")
Stop-Wallets
if ($resume.Code -ne 0) { throw "resume after crash failed: $($resume.Err)" }
$pending = (Run-Cli @("pending", $DataA, $PassA)).Out
Stop-Wallets
if ($pending -notmatch "state=posted") {
    throw "resumed transaction is not posted: $pending"
}
Write-Output "3) resume in a new process: posted from stored tx (no re-sign)"

# --- second finalize must be a no-op -----------------------------------------
$again = Run-Cli @("finalize", $DataA, $PassA, $id, "$Work/slate2.txt")
Stop-Wallets
if ($again.Code -ne 0) { throw "second finalize returned error: $($again.Err)" }
Write-Output "4) repeated finalize is idempotent"

# A JSON-RPC Ok alone is not evidence that the exact signed transaction entered
# the chain. Read the binary-backed Transaction and require its kernel on-chain.
$stored = Run-Cli @("stored", $DataA, $PassA, $id)
Stop-Wallets
if ($stored.Code -ne 0 -or -not $stored.Out.Trim()) {
    throw "cannot read stored transaction: $($stored.Err)"
}
$transaction = $stored.Out.Trim() | ConvertFrom-Json
$kernel = $transaction.body.kernels[0]
if (-not $kernel -or $kernel.excess -notmatch '[1-9a-f]' -or $kernel.excess_sig -notmatch '[1-9a-f]') {
    throw "stored transaction has an empty kernel/signature; refusing chain PASS"
}

function Get-ChainKernel([string]$Excess) {
    $body = @{
        jsonrpc = "2.0"
        method = "get_kernel"
        params = @($Excess, $null, $null)
        id = 1
    } | ConvertTo-Json -Depth 10 -Compress
    $response = Invoke-RestMethod -Method Post -Uri "$NodeUrl/v2/foreign" `
        -ContentType "application/json" -Body $body -TimeoutSec 15
    if ($response.error) { throw "get_kernel failed: $($response.error | ConvertTo-Json -Compress)" }
    return $response.result.Ok
}

$confirmed = Get-ChainKernel $kernel.excess
$deadline = (Get-Date).AddSeconds(180)
while (-not $confirmed -and (Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 10
    $confirmed = Get-ChainKernel $kernel.excess
}
if (-not $confirmed) {
    throw "posted transaction kernel was not found on chain in 180s: $id"
}
Write-Output "5) stored tx kernel confirmed on chain: height=$($confirmed.height)"

Write-Output "== crash test passed =="
Write-Output "A after (may lag until wallet refresh): $(Balance $DataA $PassA)"
