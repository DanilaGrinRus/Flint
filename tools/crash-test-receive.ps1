# F-04/C: process-death tests for embedded and RPC receive recovery.
#
# Requires a ready local testnet node, two funded disposable wallets and a
# `.crash-test-ok` marker in BOTH data roots. The script never creates wallets,
# funds them, picks passwords, or accepts mainnet-looking directories.
#
#   pwsh -File tools/crash-test-receive.ps1 -Backend embedded `
#     -DataA .tmp/f04-a -DataB .tmp/f04-b -PassA ... -PassB ... -DestB tgrin1...
#   pwsh -File tools/crash-test-receive.ps1 -Backend rpc ...
#
# Each backend runs two independent S1 transactions:
#   upstream: die after patched receive_tx, before Flint response journal;
#   journal:  die after Flint response journal, before Slatepack armoring.
# Recovery compares the saved S2 JSON, not encrypted Slatepack armor bytes.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet("embedded", "rpc")][string]$Backend,
    [Parameter(Mandatory = $true)][string]$DataA,
    [Parameter(Mandatory = $true)][string]$DataB,
    [Parameter(Mandatory = $true)][string]$PassA,
    [Parameter(Mandatory = $true)][string]$PassB,
    [Parameter(Mandatory = $true)][string]$DestB,
    [long]$Amount = 10000000,
    [string]$NodeUrl = "http://127.0.0.1:13413",
    [string]$NodeSecretFile = ".node/testnet/node/.api_secret",
    [string]$Work = ".tmp/crash-receive",
    [string]$EmbeddedCli = "target/debug/examples/embedded_cli.exe",
    [string]$RpcCli = "target/debug/examples/wallet_cli.exe",
    [string]$WalletExe = "resources/wallet/grin-wallet.exe",
    [switch]$SkipBuild,
    [switch]$PreflightOnly,
    [switch]$DiskFullOnly
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:TMP = Join-Path $root ".tmp"
$env:TEMP = $env:TMP
$env:FLINT_MIN_CONF = "1"
New-Item -ItemType Directory -Force $env:TMP | Out-Null

function Resolve-FromRoot([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
    return [System.IO.Path]::GetFullPath((Join-Path $root $Path))
}

function Resolve-ExistingFile([string]$Path, [string]$Label) {
    $full = Resolve-FromRoot $Path
    if (-not (Test-Path $full -PathType Leaf)) { throw "$Label не найден: $full" }
    return $full
}

$DataA = Resolve-FromRoot $DataA
$DataB = Resolve-FromRoot $DataB
if ($DataA -eq $DataB) { throw "DataA и DataB должны быть разными одноразовыми кошельками" }
if ($Amount -le 0) { throw "Amount должен быть положительным числом наногрин" }
if ([string]::IsNullOrWhiteSpace($PassA) -or [string]::IsNullOrWhiteSpace($PassB)) {
    throw "Пароли одноразовых кошельков не должны быть пустыми"
}
if ($DataA -match 'mainnet' -or $DataB -match 'mainnet' -or $DestB -notmatch '^tgrin1') {
    throw "Допускаются только testnet-каталоги и testnet-адрес получателя"
}
foreach ($dir in @($DataA, $DataB)) {
    if (-not (Test-Path $dir -PathType Container)) { throw "Каталог кошелька не найден: $dir" }
    if (-not (Test-Path (Join-Path $dir ".crash-test-ok") -PathType Leaf)) {
        throw "Нет маркера .crash-test-ok: $dir. Проверьте одноразовый testnet-кошелёк и добавьте маркер явно"
    }
}

$nodeUri = [Uri]$NodeUrl
if ($nodeUri.Scheme -ne "http" -or -not @("127.0.0.1", "localhost", "::1").Contains($nodeUri.Host)) {
    throw "Crash-suite разрешает только plain HTTP к loopback-ноде; для остальных случаев нужен отдельный проверенный HTTPS-стенд"
}
$secretPath = Resolve-ExistingFile $NodeSecretFile "Секрет owner API ноды"
$secret = (Get-Content $secretPath -Raw).Trim()
if (-not $secret) { throw "Секрет owner API пуст: $secretPath" }
$basic = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("grin:$secret"))
$headers = @{ Authorization = "Basic $basic" }
$body = '{"jsonrpc":"2.0","method":"get_status","params":[],"id":1}'
$statusResponse = Invoke-RestMethod -Uri ($NodeUrl.TrimEnd('/') + "/v2/owner") -Method Post `
    -Headers $headers -Body $body -ContentType "application/json" -TimeoutSec 5
$status = $statusResponse.result.Ok
if (-not $status) { throw "Нода не вернула result.Ok из get_status" }
if ($status.chain -ne "test") { throw "Нода не testnet: chain=$($status.chain)" }
if ($status.sync_status -ne "no_sync") { throw "Нода не синхронизирована: sync_status=$($status.sync_status)" }
$tip = [uint64]$status.tip.height
if ($tip -eq 0) { throw "Нода вернула нулевую высоту" }
Write-Output "Preflight: chain=test, sync_status=no_sync, tip=$tip, backend=$Backend"
if ($PreflightOnly) { return }

if ($Backend -eq "rpc") {
    $script:WalletExe = Resolve-ExistingFile $WalletExe "grin-wallet из Flint fork"
    $env:FLINT_WALLET_EXE = $script:WalletExe
}

if (-not $SkipBuild) {
    Push-Location $root
    try {
        & cargo build -p flint-core --examples --features crash-injection --locked --offline
        if ($LASTEXITCODE -ne 0) { throw "Сборка crash CLI завершилась с кодом $LASTEXITCODE" }
    } finally { Pop-Location }
}
$script:Cli = if ($Backend -eq "embedded") {
    Resolve-ExistingFile $EmbeddedCli "Embedded CLI"
} else {
    Resolve-ExistingFile $RpcCli "RPC CLI"
}

$runDir = Join-Path (Resolve-FromRoot $Work) ((Get-Date -Format "yyyyMMdd-HHmmss") + "-$Backend-" + [Guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force $runDir | Out-Null
$script:OwnedWallets = [System.Collections.Generic.HashSet[int]]::new()
$script:CliSequence = 0

function Get-DescendantPids([int]$ParentPid) {
    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$ParentPid" -ErrorAction SilentlyContinue
    foreach ($child in $children) {
        $child.ProcessId
        Get-DescendantPids $child.ProcessId
    }
}

function Register-WalletChildren([int]$ParentPid) {
    if ($Backend -ne "rpc") { return }
    foreach ($pidValue in @(Get-DescendantPids $ParentPid)) {
        $proc = Get-Process -Id $pidValue -ErrorAction SilentlyContinue
        if ($proc -and $proc.ProcessName -eq "grin-wallet") {
            [void]$script:OwnedWallets.Add($pidValue)
        }
    }
}

function Stop-OwnedWallets {
    foreach ($pidValue in $script:OwnedWallets) {
        # The CLI may already have reaped its child; an absent PID is not a suite error.
        & cmd.exe /d /c "taskkill /PID $pidValue /T /F >nul 2>&1" | Out-Null
    }
    $script:OwnedWallets.Clear()
}

function Run-Cli([string[]]$CliArgs) {
    $script:CliSequence++
    $outFile = Join-Path $runDir ("cli-{0:D3}.out" -f $script:CliSequence)
    $errFile = Join-Path $runDir ("cli-{0:D3}.err" -f $script:CliSequence)
    # Do not use -Wait: after failpoint exit an RPC child may inherit the pipes.
    $process = Start-Process -FilePath $script:Cli -ArgumentList $CliArgs -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    # Materialize the handle while the process is alive; otherwise PowerShell 5.1
    # can lose ExitCode for short-lived processes.
    $processHandle = $process.Handle
    Start-Sleep -Milliseconds 500
    Register-WalletChildren $process.Id
    $deadline = (Get-Date).AddSeconds(180)
    while (-not $process.HasExited) {
        if ((Get-Date) -gt $deadline) {
            try { & taskkill /PID $process.Id /T /F 2>&1 | Out-Null } catch {}
            Stop-OwnedWallets
            throw "CLI $($CliArgs[0]) не завершился за 180 с"
        }
        Start-Sleep -Milliseconds 200
        Register-WalletChildren $process.Id
    }
    Register-WalletChildren $process.Id
    $process.WaitForExit()
    $process.Refresh()
    return [pscustomobject]@{
        Code = $process.ExitCode
        Out = (Get-Content $outFile -Raw -ErrorAction SilentlyContinue)
        Err = (Get-Content $errFile -Raw -ErrorAction SilentlyContinue)
    }
}

function Invoke-Wallet([string]$Side, [string[]]$CommandArgs) {
    if ($Side -eq "A") {
        $env:FLINT_DATA_DIR = $DataA
        $env:FLINT_WALLET_PASS = $PassA
    } elseif ($Side -eq "B") {
        $env:FLINT_DATA_DIR = $DataB
        $env:FLINT_WALLET_PASS = $PassB
    } else {
        throw "Неизвестная сторона: $Side"
    }
    $cliArgs = $CommandArgs
    if ($Backend -eq "rpc") {
        $data = if ($Side -eq "A") { $DataA } else { $DataB }
        $password = if ($Side -eq "A") { $PassA } else { $PassB }
        $cliArgs = @($CommandArgs[0], $data, $password) + @($CommandArgs | Select-Object -Skip 1)
    }
    $result = Run-Cli $cliArgs
    if ($Backend -eq "rpc") { Stop-OwnedWallets }
    return $result
}

function Invoke-Success([string]$Side, [string[]]$CommandArgs) {
    $result = Invoke-Wallet $Side $CommandArgs
    if ($result.Code -ne 0) {
        throw "$Backend $($CommandArgs[0]) завершился с кодом $($result.Code): $($result.Err)"
    }
    return $result
}

function Get-SlateId([string]$Text) {
    $match = [regex]::Match($Text, 'SLATE_ID=([0-9a-fA-F-]{36})')
    if (-not $match.Success) { throw "CLI не вернул SLATE_ID: $Text" }
    return $match.Groups[1].Value
}

function Save-Slatepack([string]$Text, [string]$Path) {
    $lines = @($Text -split "\r?\n" | Where-Object { $_ -like "BEGINSLATEPACK*" })
    if ($lines.Count -ne 1) { throw "Ожидался один Slatepack в stdout, получено: $($lines.Count)" }
    Set-Content $Path -Value $lines[0].Trim() -NoNewline
}

function Get-SavedResponse([string]$Id) {
    $relative = if ($Backend -eq "embedded") {
        "testnet/pending/$Id.json"
    } else {
        "rpc-pending/$Id.recv.json"
    }
    $path = Join-Path $DataB $relative
    if (-not (Test-Path $path -PathType Leaf)) { throw "Нет журнала ответа: $path" }
    $record = Get-Content $path -Raw | ConvertFrom-Json
    $response = if ($Backend -eq "embedded") { $record.slate } else { $record.response }
    if (-not $response) { throw "Журнал не содержит S2: $path" }
    return ($response | ConvertTo-Json -Depth 100 -Compress)
}

function Get-ReceiveActivity {
    $result = Invoke-Success "B" @("activity")
    return @($result.Out -split "\r?\n" | Where-Object { $_ -match "TxReceived" })
}

function Assert-OneNewReceive([string[]]$Before, [string[]]$After) {
    $beforeLines = @($Before | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $afterLines = @($After | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $known = @{}
    foreach ($line in $beforeLines) { $known[$line] = $true }
    $new = @($afterLines | Where-Object { -not $known.ContainsKey($_) })
    if ($new.Count -ne 1 -or $afterLines.Count -ne $beforeLines.Count + 1) {
        throw "Ожидалась ровно одна новая запись TxReceived; before=$($beforeLines.Count), after=$($afterLines.Count)"
    }
}

function Assert-SinglePending([string]$Id, [string]$ExpectedState) {
    $result = Invoke-Success "B" @("pending")
    $lines = @($result.Out -split "\r?\n" | Where-Object { $_ -match [regex]::Escape($Id) })
    if ($lines.Count -ne 1 -or $lines[0] -notmatch $ExpectedState) {
        throw "Ожидалась одна запись $Id в состоянии ${ExpectedState}: $($result.Out)"
    }
}

function Wait-Spendable {
    $deadline = (Get-Date).AddSeconds(180)
    $required = $Amount + 200000000
    while ((Get-Date) -lt $deadline) {
        $balance = Invoke-Success "A" @("balance")
        $match = [regex]::Match($balance.Out, 'spendable=(\d+)')
        if ($match.Success -and [long]$match.Groups[1].Value -ge $required) { return }
        Start-Sleep -Seconds 3
    }
    throw "У отправителя не появилось достаточно spendable после предыдущей транзакции"
}

function Invoke-ReceiveCrash([string]$Stage, [string]$SlateFile) {
    $env:FLINT_CRASH_AFTER_RECEIVE = $Stage
    try {
        $result = Invoke-Wallet "B" @("receive", $SlateFile)
    } finally {
        Remove-Item Env:FLINT_CRASH_AFTER_RECEIVE -ErrorAction SilentlyContinue
    }
    if ($result.Code -ne 9) {
        throw "Failpoint ${Stage}: ожидался exit 9, получен $($result.Code). Отсутствие crash-injection не является pass: $($result.Err)"
    }
}

function Get-ChainKernel([string]$Excess) {
    $body = @{ jsonrpc = "2.0"; method = "get_kernel"; params = @($Excess, $null, $null); id = 1 } |
        ConvertTo-Json -Depth 10 -Compress
    $response = Invoke-RestMethod -Method Post -Uri ($NodeUrl.TrimEnd('/') + "/v2/foreign") `
        -ContentType "application/json" -Body $body -TimeoutSec 15
    if ($response.error) { throw "get_kernel failed: $($response.error | ConvertTo-Json -Compress)" }
    return $response.result.Ok
}

function Wait-ChainKernel([string]$Excess) {
    $deadline = (Get-Date).AddSeconds(180)
    while ((Get-Date) -lt $deadline) {
        $confirmed = Get-ChainKernel $Excess
        if ($confirmed) { return $confirmed }
        Start-Sleep -Seconds 10
    }
    throw "Kernel не найден в цепи за 180 с: $Excess"
}

function Get-ResponseSha256([string]$Text) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text)))).Replace("-", "") }
    finally { $sha.Dispose() }
}

function Invoke-DiskFullSimulation {
    Wait-Spendable
    $before = Get-ReceiveActivity
    $send = Invoke-Success "A" @("send", $DestB, "$Amount")
    $id = Get-SlateId $send.Out
    $slateFile = Join-Path $runDir "disk-full-slate1.txt"
    Save-Slatepack $send.Out $slateFile

    $env:FLINT_SIMULATE_RECEIVE_DISK_FULL = "1"
    try {
        $failed = Invoke-Wallet "B" @("receive", $slateFile)
    } finally {
        Remove-Item Env:FLINT_SIMULATE_RECEIVE_DISK_FULL -ErrorAction SilentlyContinue
    }
    if ($failed.Code -eq 0 -or $failed.Code -eq 9 -or $failed.Err -notmatch "112") {
        throw "Симуляция disk-full не достигла response journal с ERROR_DISK_FULL: code=$($failed.Code); $($failed.Err)"
    }
    Assert-SinglePending $id "ReceiveIntent|receive_intent|intent"
    Assert-OneNewReceive $before (Get-ReceiveActivity)

    $retry = Invoke-Success "B" @("receive", $slateFile)
    if ((Get-SlateId $retry.Out) -ne $id) { throw "Retry после disk-full изменил slate id" }
    $responseFile = Join-Path $runDir "disk-full-slate2.txt"
    Save-Slatepack $retry.Out $responseFile
    $saved = Get-SavedResponse $id
    Assert-SinglePending $id "ResponseReceived|response_received|received"

    $again = Invoke-Success "B" @("receive", $slateFile)
    if ((Get-SlateId $again.Out) -ne $id) { throw "Повторный receive изменил slate id" }
    if ((Get-SavedResponse $id) -cne $saved) { throw "Повторный receive вернул другой S2" }
    $after = Get-ReceiveActivity
    Assert-OneNewReceive $before $after

    $finalize = Invoke-Success "A" @("finalize", $id, $responseFile)
    if ($finalize.Out -notmatch "posted") { throw "Finalize не подтвердил post: $($finalize.Out)" }
    if ($Backend -eq "embedded") {
        $stored = Invoke-Success "A" @("kernel", $id)
        $kernels = @([regex]::Matches($stored.Out, 'KERNEL_EXCESS=([0-9a-fA-F]{66})'))
        $signatures = @([regex]::Matches($stored.Out, 'KERNEL_EXCESS_SIG=([0-9a-fA-F]+)'))
        if ($kernels.Count -ne 1 -or $signatures.Count -ne 1) { throw "Нет полного embedded kernel/signature" }
        $excess = $kernels[0].Groups[1].Value
        $signature = $signatures[0].Groups[1].Value
    } else {
        $stored = Invoke-Success "A" @("stored", $id)
        $transaction = $stored.Out.Trim() | ConvertFrom-Json
        $excess = [string]$transaction.body.kernels[0].excess
        $signature = [string]$transaction.body.kernels[0].excess_sig
    }
    if ($excess -notmatch '[1-9a-fA-F]' -or $signature -notmatch '[1-9a-fA-F]') {
        throw "Пустой kernel/signature после simulated disk-full"
    }
    $confirmed = Wait-ChainKernel $excess
    $evidence = [pscustomobject]@{
        kind = "simulated_disk_full_not_physical"
        backend = $Backend
        slate_id = $id
        error = "ERROR_DISK_FULL_112_after_upstream_before_response_journal"
        response_sha256 = Get-ResponseSha256 $saved
        receive_count_before = $before.Count
        receive_count_after = $after.Count
        kernel_excess = $excess
        kernel_height = [uint64]$confirmed.height
    }
    Set-Content (Join-Path $runDir "disk-full-simulation.json") -Value ($evidence | ConvertTo-Json -Depth 10)
    Write-Output "[$Backend/simulated-disk-full] PASS: commit состоялся, journal вернул 112, recovery тот же S2, один TxReceived, kernel height=$($confirmed.height)"
    Write-Output "Это fault injection, не реальное заполнение диска и не power-loss."
}

try {
    function Invoke-Scenario([string]$Stage) {
        Wait-Spendable
        $before = Get-ReceiveActivity
        $send = Invoke-Success "A" @("send", $DestB, "$Amount")
        $id = Get-SlateId $send.Out
        $slateFile = Join-Path $runDir "$Stage-slate1.txt"
        Save-Slatepack $send.Out $slateFile

        Invoke-ReceiveCrash $Stage $slateFile
        $beforeRetry = $null
        if ($Stage -eq "journal") {
            $beforeRetry = Get-SavedResponse $id
            Assert-SinglePending $id "ResponseReceived|response_received|received"
        } else {
            Assert-SinglePending $id "ReceiveIntent|receive_intent|intent"
        }

        $retry = Invoke-Success "B" @("receive", $slateFile)
        if ((Get-SlateId $retry.Out) -ne $id) { throw "Retry изменил slate id" }
        $responseFile = Join-Path $runDir "$Stage-slate2.txt"
        Save-Slatepack $retry.Out $responseFile
        $saved = Get-SavedResponse $id
        if ($Stage -eq "journal" -and $saved -cne $beforeRetry) {
            throw "S2 в журнале изменился после crash=$Stage"
        }
        Assert-SinglePending $id "ResponseReceived|response_received|received"

        $again = Invoke-Success "B" @("receive", $slateFile)
        if ((Get-SlateId $again.Out) -ne $id) { throw "Повторный receive изменил slate id" }
        if ((Get-SavedResponse $id) -cne $saved) { throw "Повторный receive вернул другой S2" }
        Assert-SinglePending $id "ResponseReceived|response_received|received"
        Assert-OneNewReceive $before (Get-ReceiveActivity)

        $finalize = Invoke-Success "A" @("finalize", $id, $responseFile)
        if ($finalize.Out -notmatch "posted") { throw "Finalize не подтвердил post: $($finalize.Out)" }
        Write-Output "[$Backend/$Stage] PASS: тот же S2, один журнал, один TxReceived, finalize/post выполнен"
    }

    if ($DiskFullOnly) {
        Invoke-DiskFullSimulation
        Write-Output "== F-04 simulated disk-full passed: $Backend =="
    } else {
        Invoke-Scenario "upstream"
        Invoke-Scenario "journal"
        Write-Output "== F-04/C receive crash-suite passed: $Backend =="
    }
    Write-Output "Evidence: $runDir"
    Write-Output "Process kill/fault injection не заменяют физический disk-full/power-loss."
} finally {
    Stop-OwnedWallets
    Remove-Item Env:FLINT_CRASH_AFTER_RECEIVE -ErrorAction SilentlyContinue
    Remove-Item Env:FLINT_SIMULATE_RECEIVE_DISK_FULL -ErrorAction SilentlyContinue
    Remove-Item Env:FLINT_WALLET_PASS -ErrorAction SilentlyContinue
}
