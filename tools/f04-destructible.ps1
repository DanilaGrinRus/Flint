# F-04: подготовка и проверка восстановления на одноразовой Windows VM.
#
# Скрипт НЕ создаёт/заполняет VHD и НЕ выключает VM. Disk-full и жёсткое
# отключение выполняет оператор по docs/ACCEPTANCE.md между
# отдельными запусками. Обычный exit 9 не принимается как доказательство power-loss.
#
# Новый сценарий:
#   ArmPowerLoss -Scenario upstream|journal
#   PrepareDiskFull -Scenario DF-J|DF-W
# После действия оператора:
#   ResumePowerLoss -StateFile ... -PowerCutConfirmed
#   RunDiskFull -StateFile ...       # пока целевой том заполнен
#   ResumeDiskFull -StateFile ...    # после освобождения места
#
# Пароли и seed в state/evidence не записываются. Для Resume нужны только пароли
# и StateFile; конфигурация и хеши бинарей восстанавливаются из состояния.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Preflight", "ArmPowerLoss", "ResumePowerLoss", "PrepareDiskFull", "RunDiskFull", "ResumeDiskFull")]
    [string]$Action,
    [ValidateSet("embedded", "rpc")][string]$Backend,
    [ValidateSet("upstream", "journal", "DF-J", "DF-W")][string]$Scenario,
    [string]$StandRoot,
    [string]$DataA,
    [string]$DataB,
    [string]$PassA,
    [string]$PassB,
    [string]$DestB,
    [string]$StateFile,
    [long]$Amount = 10000000,
    [string]$NodeUrl = "http://127.0.0.1:13413",
    [string]$NodeSecretFile = ".node/testnet/node/.api_secret",
    [string]$Work = ".tmp/f04-destructible",
    [string]$EmbeddedCli = "target/debug/examples/embedded_cli.exe",
    [string]$RpcCli = "target/debug/examples/wallet_cli.exe",
    [string]$WalletExe = "resources/wallet/grin-wallet.exe",
    [long]$FullThresholdBytes = 1048576,
    [long]$MinFreeAfterBytes = 16777216,
    [switch]$SkipBuild,
    [switch]$PowerCutConfirmed
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:TMP = Join-Path $root ".tmp"
$env:TEMP = $env:TMP
$env:FLINT_MIN_CONF = "1"
Remove-Item Env:FLINT_CRASH_AFTER_RECEIVE -ErrorAction SilentlyContinue
Remove-Item Env:FLINT_CRASH_AFTER_SEND -ErrorAction SilentlyContinue
Remove-Item Env:FLINT_CRASH_AFTER_FINALIZE -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $env:TMP | Out-Null
$script:OwnedWallets = [System.Collections.Generic.HashSet[int]]::new()
$script:OwnedCli = $null
$script:LeavePausedTree = $false
$script:CliSequence = 0
$script:State = $null
$script:RunDir = $null
$script:NodeTip = $null

function Resolve-FromRoot([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
    return [System.IO.Path]::GetFullPath((Join-Path $root $Path))
}

function Resolve-ExistingFile([string]$Path, [string]$Label) {
    $full = Resolve-FromRoot $Path
    if (-not (Test-Path $full -PathType Leaf)) { throw "$Label не найден: $full" }
    return $full
}

function Assert-UnderStand([string]$Path, [string]$Label) {
    $full = [System.IO.Path]::GetFullPath($Path)
    $prefix = $script:StandRoot.TrimEnd('\') + '\'
    if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label должен находиться строго внутри одноразового StandRoot: $full"
    }
    return $full
}

function Get-Sha256Text([string]$Text) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text)))).Replace("-", "") }
    finally { $sha.Dispose() }
}

function Get-BinaryHashes {
    $hashes = [ordered]@{ cli = (Get-FileHash $script:Cli -Algorithm SHA256).Hash }
    if ($Backend -eq "rpc") { $hashes.wallet = (Get-FileHash $script:WalletExe -Algorithm SHA256).Hash }
    return $hashes
}

function Write-TextDurable([string]$Path, [string]$Text) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    # Power loss is initiated only after this function returns. A direct complete
    # write plus Flush(true) is intentional: a temp rename without a durable
    # directory flush would not prove that recovery input survives the cut.
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
}

function Write-State([string]$Path) {
    $full = Assert-UnderStand $Path "StateFile"
    $parent = Split-Path -Parent $full
    New-Item -ItemType Directory -Force $parent | Out-Null
    $json = $script:State | ConvertTo-Json -Depth 100
    if ($json -match '(?i)mnemonic|seed_phrase|password|pass_a|pass_b|owner_api_secret') {
        throw "Внутренний отказ: state содержит поле, похожее на секрет"
    }
    Write-TextDurable $full $json
    $script:StateFile = $full
}

function Read-State([string]$Path) {
    $full = Resolve-FromRoot $Path
    if (-not (Test-Path $full -PathType Leaf)) { throw "StateFile не найден: $full" }
    $script:State = Get-Content $full -Raw | ConvertFrom-Json
    if ($script:State.schema -ne "flint-f04-destructible-v1") { throw "Неизвестная схема StateFile" }
    $script:StateFile = $full
}

function Get-VolumeRoot([string]$Path) {
    $item = Get-Item $Path -Force -ErrorAction Stop
    $effective = $item.FullName
    if ($item.LinkType -and $item.Target) { $effective = [string]$item.Target[0] }
    $effective = [IO.Path]::GetFullPath($effective)
    return [IO.Path]::GetPathRoot($effective).TrimEnd('\')
}

function Get-WalletHomeB {
    if ($Backend -eq "embedded") { return Join-Path $DataB "testnet/wallet" }
    return $DataB
}

function Get-JournalB {
    if ($Backend -eq "embedded") { return Join-Path $DataB "testnet/pending" }
    return Join-Path $DataB "rpc-pending"
}

function Assert-VolumeTopology {
    if ($Scenario -notin @("DF-J", "DF-W")) { return }
    $wallet = Get-WalletHomeB
    $journal = Get-JournalB
    if (-not (Test-Path $wallet -PathType Container)) { throw "Нет wallet home получателя: $wallet" }
    if (-not (Test-Path $journal -PathType Container)) { throw "Нет journal dir получателя: $journal" }
    $system = [IO.Path]::GetPathRoot($env:SystemRoot).TrimEnd('\')
    $aVolume = Get-VolumeRoot $DataA
    $bVolume = Get-VolumeRoot $DataB
    $evidenceVolume = Get-VolumeRoot $script:StandRoot
    if ($Scenario -eq "DF-J") {
        $link = Get-Item $journal -Force
        if (-not $link.LinkType -or -not $link.Target) {
            throw "DF-J требует отдельный junction/symlink журнала на одноразовый VHD: $journal"
        }
        $jVolume = Get-VolumeRoot $journal
        $wVolume = Get-VolumeRoot $wallet
        if ($jVolume -eq $wVolume -or $jVolume -eq $aVolume) {
            throw "DF-J: том журнала должен отличаться от томов wallet B и sender A"
        }
        if ($jVolume -eq $system) { throw "DF-J запрещает заполнять системный том" }
        if ($jVolume -eq $evidenceVolume) { throw "DF-J: state/evidence должны быть вне заполняемого тома" }
        $script:FullTarget = $journal
    } else {
        if ($bVolume -eq $system) { throw "DF-W запрещает заполнять системный том" }
        if ($bVolume -eq $aVolume) { throw "DF-W: sender A должен быть на другом томе" }
        if ($bVolume -eq $evidenceVolume) { throw "DF-W: state/evidence должны быть вне заполняемого тома" }
        $script:FullTarget = $wallet
    }
}

function Test-NodeAndStand {
    if (-not $script:StandRoot) { throw "Нужен -StandRoot" }
    $script:StandRoot = Resolve-FromRoot $script:StandRoot
    $standMarker = Join-Path $script:StandRoot ".destructible-f04-stand"
    if (-not (Test-Path $standMarker -PathType Leaf)) { throw "Нет маркера одноразовой VM: $standMarker" }
    if ((Get-Content $standMarker -Raw).Trim() -cne "F04-DESTRUCTIBLE-TESTNET-VM") {
        throw "Неверное содержимое .destructible-f04-stand"
    }
    foreach ($dir in @($DataA, $DataB)) {
        if ($dir -match 'mainnet') { throw "Mainnet-путь запрещён: $dir" }
        if (-not (Test-Path $dir -PathType Container)) { throw "Каталог кошелька не найден: $dir" }
        foreach ($marker in @(".crash-test-ok", ".destructible-f04-ok")) {
            if (-not (Test-Path (Join-Path $dir $marker) -PathType Leaf)) { throw "Нет маркера $marker в $dir" }
        }
    }
    if ($DataA -eq $DataB) { throw "DataA и DataB должны быть разными кошельками" }
    if ($DestB -and $DestB -notmatch '^tgrin1') { throw "DestB должен быть testnet-адресом" }
    if ($Amount -le 0) { throw "Amount должен быть положительным" }
    if ($FullThresholdBytes -lt 0 -or $MinFreeAfterBytes -le $FullThresholdBytes) {
        throw "Некорректные пороги свободного места"
    }
    $nodeUri = [Uri]$NodeUrl
    if ($nodeUri.Scheme -ne "http" -or -not @("127.0.0.1", "localhost", "::1").Contains($nodeUri.Host)) {
        throw "Разрешён только plain HTTP к loopback testnet-ноде"
    }
    $secretPath = Resolve-ExistingFile $NodeSecretFile "Секрет owner API ноды"
    $secret = (Get-Content $secretPath -Raw).Trim()
    if (-not $secret) { throw "Секрет owner API ноды пуст" }
    $basic = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("grin:$secret"))
    $response = Invoke-RestMethod -Uri ($NodeUrl.TrimEnd('/') + "/v2/owner") -Method Post `
        -Headers @{ Authorization = "Basic $basic" } -ContentType "application/json" `
        -Body '{"jsonrpc":"2.0","method":"get_status","params":[],"id":1}' -TimeoutSec 5
    $status = $response.result.Ok
    if (-not $status -or $status.chain -ne "test" -or $status.sync_status -ne "no_sync") {
        throw "Нода не готова: требуется chain=test и sync_status=no_sync"
    }
    if ([uint64]$status.tip.height -eq 0) { throw "Нода вернула нулевую высоту" }
    $script:NodeTip = [uint64]$status.tip.height
    Assert-VolumeTopology
}

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
        if ($proc -and $proc.ProcessName -eq "grin-wallet") { [void]$script:OwnedWallets.Add($pidValue) }
    }
}

function Stop-OwnedWallets {
    foreach ($pidValue in $script:OwnedWallets) {
        & cmd.exe /d /c "taskkill /PID $pidValue /T /F >nul 2>&1" | Out-Null
    }
    $script:OwnedWallets.Clear()
}

function Set-SideEnvironment([string]$Side) {
    if ($Side -eq "A") {
        $env:FLINT_DATA_DIR = $DataA
        $env:FLINT_WALLET_PASS = $PassA
    } elseif ($Side -eq "B") {
        $env:FLINT_DATA_DIR = $DataB
        $env:FLINT_WALLET_PASS = $PassB
    } else { throw "Неизвестная сторона: $Side" }
}

function Get-CliArgs([string]$Side, [string[]]$CommandArgs) {
    if ($Backend -eq "embedded") { return $CommandArgs }
    $data = if ($Side -eq "A") { $DataA } else { $DataB }
    $password = if ($Side -eq "A") { $PassA } else { $PassB }
    return @($CommandArgs[0], $data, $password) + @($CommandArgs | Select-Object -Skip 1)
}

function Start-Cli([string]$Side, [string[]]$CommandArgs) {
    Set-SideEnvironment $Side
    $script:CliSequence++
    $outFile = Join-Path $script:RunDir ("cli-{0:D3}.out" -f $script:CliSequence)
    $errFile = Join-Path $script:RunDir ("cli-{0:D3}.err" -f $script:CliSequence)
    $process = Start-Process -FilePath $script:Cli -ArgumentList (Get-CliArgs $Side $CommandArgs) -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    $processHandle = $process.Handle
    Start-Sleep -Milliseconds 500
    Register-WalletChildren $process.Id
    return [pscustomobject]@{ Process = $process; OutFile = $outFile; ErrFile = $errFile }
}

function Read-CliResult($Started) {
    $process = $Started.Process
    $process.WaitForExit()
    $process.Refresh()
    return [pscustomobject]@{
        Code = $process.ExitCode
        Out = (Get-Content $Started.OutFile -Raw -ErrorAction SilentlyContinue)
        Err = (Get-Content $Started.ErrFile -Raw -ErrorAction SilentlyContinue)
    }
}

function Invoke-Wallet([string]$Side, [string[]]$CommandArgs) {
    $started = Start-Cli $Side $CommandArgs
    $deadline = (Get-Date).AddSeconds(180)
    while (-not $started.Process.HasExited) {
        if ((Get-Date) -gt $deadline) {
            & taskkill /PID $started.Process.Id /T /F 2>&1 | Out-Null
            throw "CLI $($CommandArgs[0]) не завершился за 180 с"
        }
        Start-Sleep -Milliseconds 200
        Register-WalletChildren $started.Process.Id
    }
    $result = Read-CliResult $started
    if ($Backend -eq "rpc") { Stop-OwnedWallets }
    return $result
}

function Invoke-Success([string]$Side, [string[]]$CommandArgs) {
    $result = Invoke-Wallet $Side $CommandArgs
    if ($result.Code -ne 0) { throw "$Backend $($CommandArgs[0]) завершился с кодом $($result.Code): $($result.Err)" }
    return $result
}

function Get-SlateId([string]$Text) {
    $match = [regex]::Match($Text, 'SLATE_ID=([0-9a-fA-F-]{36})')
    if (-not $match.Success) { throw "CLI не вернул SLATE_ID" }
    return $match.Groups[1].Value
}

function Get-Confirmation([string]$Text) {
    $match = [regex]::Match($Text, 'CONFIRMATION=(\S+)')
    if (-not $match.Success) { throw "CLI не вернул CONFIRMATION: $Text" }
    return $match.Groups[1].Value
}

function Save-Slatepack([string]$Text, [string]$Path) {
    $lines = @($Text -split "\r?\n" | Where-Object { $_ -like "BEGINSLATEPACK*" })
    if ($lines.Count -ne 1) { throw "Ожидался один Slatepack, получено: $($lines.Count)" }
    Write-TextDurable $Path $lines[0].Trim()
}

function Get-SavedResponse([string]$Id) {
    $relative = if ($Backend -eq "embedded") { "testnet/pending/$Id.json" } else { "rpc-pending/$Id.recv.json" }
    $path = Join-Path $DataB $relative
    if (-not (Test-Path $path -PathType Leaf)) { throw "Нет журнала ответа: $path" }
    $record = Get-Content $path -Raw | ConvertFrom-Json
    $response = if ($Backend -eq "embedded") { $record.slate } else { $record.response }
    if (-not $response) { throw "Журнал не содержит S2" }
    return ($response | ConvertTo-Json -Depth 100 -Compress)
}

function Assert-JournalState([string]$Id, [string]$Expected) {
    $relative = if ($Backend -eq "embedded") { "testnet/pending/$Id.json" } else { "rpc-pending/$Id.recv.json" }
    $record = Get-Content (Join-Path $DataB $relative) -Raw | ConvertFrom-Json
    $actual = if ($Backend -eq "embedded") { [string]$record.state } else { [string]$record.state }
    if ($actual -notmatch $Expected) { throw "Журнал $Id в состоянии $actual, ожидалось $Expected" }
}

function Get-ReceiveActivity {
    $result = Invoke-Success "B" @("activity")
    return @($result.Out -split "\r?\n" | Where-Object { $_ -match "TxReceived" })
}

function Assert-OneNewReceive([string[]]$Before, [string[]]$After) {
    $known = @{}
    foreach ($line in @($Before | Where-Object { $_ })) { $known[$line] = $true }
    $new = @($After | Where-Object { $_ -and -not $known.ContainsKey($_) })
    if ($new.Count -ne 1 -or $After.Count -ne $Before.Count + 1) {
        throw "Ожидалась ровно одна новая запись TxReceived; before=$($Before.Count), after=$($After.Count)"
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
    throw "Недостаточно spendable у отправителя"
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

function New-RunState {
    $now = (Get-Date).ToUniversalTime().ToString("o")
    $runName = (Get-Date -Format "yyyyMMdd-HHmmss") + "-$Backend-$Scenario-" + [Guid]::NewGuid().ToString("N").Substring(0, 8)
    $script:RunDir = Assert-UnderStand (Join-Path (Resolve-FromRoot $Work) $runName) "RunDir"
    New-Item -ItemType Directory -Force $script:RunDir | Out-Null
    $script:State = [ordered]@{
        schema = "flint-f04-destructible-v1"
        status = "prepared"
        action = $Action
        backend = $Backend
        scenario = $Scenario
        created_utc = $now
        stand_root = $script:StandRoot
        data_a = $DataA
        data_b = $DataB
        dest_b = $DestB
        amount = $Amount
        node_url = $NodeUrl
        node_secret_file = (Resolve-FromRoot $NodeSecretFile)
        node_tip = $script:NodeTip
        work = $Work
        embedded_cli = $script:EmbeddedCli
        rpc_cli = $script:RpcCli
        wallet_exe = if ($Backend -eq "rpc") { $script:WalletExe } else { $null }
        binary_hashes = Get-BinaryHashes
        full_target = $script:FullTarget
        full_threshold_bytes = $FullThresholdBytes
        min_free_after_bytes = $MinFreeAfterBytes
        run_dir = $script:RunDir
        boot_before = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString("o")
    }
}

function Restore-RunState {
    $config = $script:State
    $script:Backend = [string]$config.backend
    $script:Scenario = [string]$config.scenario
    $script:StandRoot = [string]$config.stand_root
    $script:DataA = Assert-UnderStand ([string]$config.data_a) "DataA"
    $script:DataB = Assert-UnderStand ([string]$config.data_b) "DataB"
    $script:DestB = [string]$config.dest_b
    $script:Amount = [long]$config.amount
    $script:NodeUrl = [string]$config.node_url
    $script:NodeSecretFile = [string]$config.node_secret_file
    $script:Work = [string]$config.work
    $script:EmbeddedCli = [string]$config.embedded_cli
    $script:RpcCli = [string]$config.rpc_cli
    $script:WalletExe = [string]$config.wallet_exe
    $script:FullThresholdBytes = [long]$config.full_threshold_bytes
    $script:MinFreeAfterBytes = [long]$config.min_free_after_bytes
    $script:FullTarget = [string]$config.full_target
    $script:RunDir = Assert-UnderStand ([string]$config.run_dir) "RunDir"
    $script:StateFile = Assert-UnderStand $script:StateFile "StateFile"
    $script:Cli = Resolve-ExistingFile $(if ($Backend -eq "embedded") { $script:EmbeddedCli } else { $script:RpcCli }) "CLI"
    if ($Backend -eq "rpc") {
        $script:WalletExe = Resolve-ExistingFile $script:WalletExe "grin-wallet"
        $env:FLINT_WALLET_EXE = $script:WalletExe
    }
    $actual = Get-BinaryHashes
    if ($actual.cli -ne $config.binary_hashes.cli) { throw "CLI изменился после подготовки; recovery должен использовать тот же binary" }
    if ($Backend -eq "rpc" -and $actual.wallet -ne $config.binary_hashes.wallet) {
        throw "grin-wallet изменился после подготовки; recovery должен использовать тот же binary"
    }
    $script:CliSequence = 100
}

function Initialize-NewRun {
    foreach ($name in @("Backend", "Scenario", "StandRoot", "DataA", "DataB", "DestB")) {
        if ([string]::IsNullOrWhiteSpace((Get-Variable $name -ValueOnly))) { throw "Нужен -$name" }
    }
    if ($Action -ne "Preflight") {
        if ([string]::IsNullOrWhiteSpace($PassA) -or [string]::IsNullOrWhiteSpace($PassB)) { throw "Нужны -PassA и -PassB" }
    }
    if (($Action -eq "ArmPowerLoss") -xor ($Scenario -in @("upstream", "journal"))) {
        if ($Action -ne "Preflight") { throw "ArmPowerLoss требует upstream/journal; PrepareDiskFull требует DF-J/DF-W" }
    }
    $script:DataA = Resolve-FromRoot $DataA
    $script:DataB = Resolve-FromRoot $DataB
    $script:StandRoot = Resolve-FromRoot $StandRoot
    $script:DataA = Assert-UnderStand $DataA "DataA"
    $script:DataB = Assert-UnderStand $DataB "DataB"
    Test-NodeAndStand
    if ($Action -eq "Preflight") {
        Write-Output "PREFLIGHT OK: backend=$Backend scenario=$Scenario tip=$($script:NodeTip); аварийные действия не выполнялись"
        return
    }
    if (-not $SkipBuild) {
        Push-Location $root
        try {
            & cargo build -p flint-core --examples --features crash-injection --locked --offline
            if ($LASTEXITCODE -ne 0) { throw "Сборка crash CLI завершилась с кодом $LASTEXITCODE" }
        } finally { Pop-Location }
    }
    $script:EmbeddedCli = Resolve-ExistingFile $EmbeddedCli "Embedded CLI"
    $script:RpcCli = Resolve-ExistingFile $RpcCli "RPC CLI"
    $script:Cli = if ($Backend -eq "embedded") { $script:EmbeddedCli } else { $script:RpcCli }
    if ($Backend -eq "rpc") {
        $script:WalletExe = Resolve-ExistingFile $WalletExe "grin-wallet"
        $env:FLINT_WALLET_EXE = $script:WalletExe
    }
    New-RunState
}

function Test-FullTarget {
    $target = [string]$script:State.full_target
    if (-not (Test-Path $target -PathType Container)) { throw "FullTarget не найден: $target" }
    $drive = [IO.DriveInfo]::new((Get-VolumeRoot $target))
    if ($drive.AvailableFreeSpace -gt $FullThresholdBytes) {
        throw "Целевой том не заполнен: свободно $($drive.AvailableFreeSpace) байт"
    }
    $probe = Join-Path $target ".f04-full-probe-$PID-$([Guid]::NewGuid().ToString('N')).tmp"
    $failed = $false
    try {
        $stream = [IO.File]::Open($probe, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $stream.SetLength(2097152)
            $stream.Flush($true)
        } finally { $stream.Dispose() }
    } catch { $failed = $true }
    finally { Remove-Item $probe -Force -ErrorAction SilentlyContinue }
    if (-not $failed) { throw "Контрольная запись 2 MiB неожиданно удалась: disk-full не доказан" }
    Write-Output "DISK FULL OBSERVED: target=$target free=$($drive.AvailableFreeSpace); write probe failed"
}

function Test-FreeTarget {
    $target = [string]$script:State.full_target
    $drive = [IO.DriveInfo]::new((Get-VolumeRoot $target))
    if ($drive.AvailableFreeSpace -lt $MinFreeAfterBytes) {
        throw "Недостаточно свободного места после recovery preparation: $($drive.AvailableFreeSpace)"
    }
    $probe = Join-Path $target ".f04-free-probe-$PID-$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        $stream = [IO.File]::Open($probe, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.SetLength(1048576); $stream.Flush($true) } finally { $stream.Dispose() }
    } finally { Remove-Item $probe -Force -ErrorAction SilentlyContinue }
}

function Complete-Recovery {
    $id = [string]$script:State.slate_id
    $slateFile = [string]$script:State.slate_file
    $retry = Invoke-Success "B" @("receive", $slateFile)
    if ((Get-SlateId $retry.Out) -ne $id) { throw "Retry изменил slate id" }
    $responseFile = Join-Path $script:RunDir "slate2.txt"
    Save-Slatepack $retry.Out $responseFile
    $saved = Get-SavedResponse $id
    if ($script:State.saved_response -and $saved -cne [string]$script:State.saved_response) {
        throw "S2 после аварии отличается от S2, сохранённого до аварии"
    }
    Assert-JournalState $id "ResponseReceived|response_received|received"
    $again = Invoke-Success "B" @("receive", $slateFile)
    if ((Get-SlateId $again.Out) -ne $id) { throw "Повторный receive изменил slate id" }
    if ((Get-SavedResponse $id) -cne $saved) { throw "Повторный receive вернул другой S2" }
    $after = Get-ReceiveActivity
    Assert-OneNewReceive @($script:State.before_activity) $after
    $finalize = Invoke-Success "A" @("finalize", $id, $responseFile, [string]$script:State.confirmation)
    if ($finalize.Out -notmatch "posted") { throw "Finalize не подтвердил post" }
    if ($Backend -eq "embedded") {
        $stored = Invoke-Success "A" @("kernel", $id)
        $kernels = @([regex]::Matches($stored.Out, 'KERNEL_EXCESS=([0-9a-fA-F]{64})'))
        $signatures = @([regex]::Matches($stored.Out, 'KERNEL_EXCESS_SIG=([0-9a-fA-F]+)'))
        if ($kernels.Count -ne 1 -or $signatures.Count -ne 1) {
            throw "Embedded stored transaction не вернул ровно один kernel/signature"
        }
        $excess = $kernels[0].Groups[1].Value
        if ($excess -notmatch '[1-9a-fA-F]' -or $signatures[0].Groups[1].Value -notmatch '[1-9a-fA-F]') {
            throw "Embedded stored transaction имеет пустой kernel/signature"
        }
    } else {
        $stored = Invoke-Success "A" @("stored", $id)
        $transaction = $stored.Out.Trim() | ConvertFrom-Json
        $excess = [string]$transaction.body.kernels[0].excess
        if ($excess -notmatch '[1-9a-fA-F]' -or [string]$transaction.body.kernels[0].excess_sig -notmatch '[1-9a-fA-F]') {
            throw "Stored transaction имеет пустой kernel/signature"
        }
    }
    $confirmed = Wait-ChainKernel $excess
    $script:State.status = "passed"
    $script:State.completed_utc = (Get-Date).ToUniversalTime().ToString("o")
    $script:State.response_sha256 = Get-Sha256Text $saved
    $script:State.receive_count_before = @($script:State.before_activity).Count
    $script:State.receive_count_after = $after.Count
    $script:State.kernel_excess = $excess
    $script:State.kernel_height = [uint64]$confirmed.height
    $script:State.balance_a_after = (Invoke-Success "A" @("balance")).Out.Trim()
    $script:State.balance_b_after = (Invoke-Success "B" @("balance")).Out.Trim()
    Write-State $script:StateFile
    Write-Output "PASS: backend=$Backend scenario=$Scenario same_S2=true one_TxReceived=true kernel_height=$($confirmed.height)"
    Write-Output "Evidence state: $($script:StateFile)"
}

try {
    $resuming = $Action -in @("ResumePowerLoss", "RunDiskFull", "ResumeDiskFull")
    if ($resuming) {
        if (-not $StateFile -or [string]::IsNullOrWhiteSpace($PassA) -or [string]::IsNullOrWhiteSpace($PassB)) {
            throw "Для $Action нужны -StateFile, -PassA и -PassB"
        }
        if ($Action -eq "ResumePowerLoss" -and -not $PowerCutConfirmed) {
            throw "Нужен явный -PowerCutConfirmed после жёсткого отключения VM с хоста"
        }
        Read-State $StateFile
        Restore-RunState
        Test-NodeAndStand
    } else {
        Initialize-NewRun
        if ($Action -eq "Preflight") { return }
    }

    if ($Action -in @("ArmPowerLoss", "PrepareDiskFull")) {
        Wait-Spendable
        $before = Get-ReceiveActivity
        $send = Invoke-Success "A" @("send", $DestB, "$Amount")
        $id = Get-SlateId $send.Out
        $script:State.confirmation = Get-Confirmation $send.Out
        $slateFile = Join-Path $script:RunDir "slate1.txt"
        Save-Slatepack $send.Out $slateFile
        $script:State.slate_id = $id
        $script:State.slate_file = $slateFile
        $script:State.before_activity = $before
        Write-State (Join-Path $script:RunDir "state.json")
    }

    if ($Action -eq "ArmPowerLoss") {
        $env:FLINT_PAUSE_AFTER_RECEIVE = $Scenario
        $env:FLINT_PAUSE_EVIDENCE_DIR = $script:StandRoot
        $started = Start-Cli "B" @("receive", [string]$script:State.slate_file)
        $script:OwnedCli = $started.Process
        $marker = Join-Path $script:StandRoot ("receive-$Scenario-$($started.Process.Id)-paused.json")
        $deadline = (Get-Date).AddSeconds(180)
        while (-not (Test-Path $marker -PathType Leaf)) {
            if ($started.Process.HasExited) {
                $result = Read-CliResult $started
                throw "Pause-hook завершился до маркера, код $($result.Code): $($result.Err)"
            }
            if ((Get-Date) -gt $deadline) { throw "Pause-маркер не появился за 180 с" }
            Start-Sleep -Milliseconds 200
        }
        Register-WalletChildren $started.Process.Id
        $markerText = Get-Content $marker -Raw
        if ($markerText -notmatch '"kind": "f04_receive_pause"') { throw "Неверный pause-маркер" }
        if ($Scenario -eq "journal") {
            $script:State.saved_response = Get-SavedResponse $script:State.slate_id
            Assert-JournalState $script:State.slate_id "ResponseReceived|response_received|received"
        } else {
            Assert-JournalState $script:State.slate_id "ReceiveIntent|receive_intent|intent"
        }
        $script:State.status = "ready_for_power_cut"
        $script:State.pause_marker = $marker
        $script:State.pause_marker_sha256 = (Get-FileHash $marker -Algorithm SHA256).Hash
        $script:State.ready_for_power_cut_utc = (Get-Date).ToUniversalTime().ToString("o")
        Write-State $script:StateFile
        $script:LeavePausedTree = $true
        Write-Output "READY FOR OPERATOR POWER CUT: $($script:StateFile)"
        Write-Output "Не выключайте изнутри Windows. Жёстко выключите одноразовую VM с хоста по runbook."
    } elseif ($Action -eq "ResumePowerLoss") {
        if (-not $PowerCutConfirmed) { throw "Нужен явный -PowerCutConfirmed после жёсткого отключения VM с хоста" }
        if ($script:State.status -ne "ready_for_power_cut") { throw "State не находится в ready_for_power_cut" }
        $bootNow = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString("o")
        if ($bootNow -eq $script:State.boot_before) { throw "VM не перезагружалась после ArmPowerLoss" }
        $marker = [string]$script:State.pause_marker
        if (-not (Test-Path $marker -PathType Leaf)) { throw "Pause-маркер потерян после reboot" }
        if ((Get-FileHash $marker -Algorithm SHA256).Hash -ne $script:State.pause_marker_sha256) { throw "Pause-маркер изменился" }
        $script:State.power_cut_attested = $true
        $script:State.boot_after = $bootNow
        Complete-Recovery
    } elseif ($Action -eq "PrepareDiskFull") {
        if ($Scenario -eq "DF-J") {
            # Without a durable intent, filling the journal would fail before
            # receive_tx and would not exercise the post-commit response gap.
            $env:FLINT_CRASH_AFTER_RECEIVE = "intent"
            try {
                $armed = Invoke-Wallet "B" @("receive", [string]$script:State.slate_file)
            } finally {
                Remove-Item Env:FLINT_CRASH_AFTER_RECEIVE -ErrorAction SilentlyContinue
            }
            if ($armed.Code -ne 9) { throw "DF-J intent failpoint: ожидался exit 9 до wallet mutation" }
            Assert-JournalState $script:State.slate_id "ReceiveIntent|receive_intent|intent"
            $afterIntent = Get-ReceiveActivity
            $differences = @(Compare-Object @($script:State.before_activity) @($afterIntent))
            if ($afterIntent.Count -ne @($script:State.before_activity).Count -or $differences.Count -ne 0) {
                throw "DF-J intent failpoint создал TxReceived до disk-full"
            }
            $script:State.intent_armed_before_disk_full = $true
        }
        $script:State.status = "awaiting_disk_full"
        Write-State $script:StateFile
        Write-Output "READY FOR OPERATOR DISK FULL: $($script:StateFile)"
        Write-Output "Заполните только FullTarget=$($script:State.full_target) по runbook, затем запустите RunDiskFull."
    } elseif ($Action -eq "RunDiskFull") {
        if ($script:State.status -notin @("awaiting_disk_full", "full_failure_observed")) { throw "Неверное состояние для RunDiskFull" }
        if ($Scenario -eq "DF-J" -and $script:State.intent_armed_before_disk_full -ne $true) {
            throw "DF-J не имеет доказанного durable intent до заполнения журнала"
        }
        Test-FullTarget
        $failed = Invoke-Wallet "B" @("receive", [string]$script:State.slate_file)
        if ($failed.Code -eq 0) { throw "Receive неожиданно успешен на заполненном томе" }
        if ($failed.Code -eq 9) { throw "Exit 9 здесь не ожидался и не заменяет disk-full" }
        if ($Scenario -eq "DF-J") {
            Assert-JournalState $script:State.slate_id "ReceiveIntent|receive_intent|intent"
            Assert-OneNewReceive @($script:State.before_activity) (Get-ReceiveActivity)
        }
        $script:State.status = "full_failure_observed"
        $script:State.full_failure_exit_code = $failed.Code
        $script:State.full_failure_stderr_sha256 = Get-Sha256Text ([string]$failed.Err)
        $script:State.full_failure_utc = (Get-Date).ToUniversalTime().ToString("o")
        Write-State $script:StateFile
        Write-Output "DISK-FULL FAILURE RECORDED. Освободите место только на FullTarget, затем ResumeDiskFull."
    } elseif ($Action -eq "ResumeDiskFull") {
        if ($script:State.status -ne "full_failure_observed") { throw "Сначала нужен подтверждённый RunDiskFull" }
        Test-FreeTarget
        Complete-Recovery
    }
} finally {
    if (-not $script:LeavePausedTree) {
        if ($script:OwnedCli -and -not $script:OwnedCli.HasExited) {
            & taskkill /PID $script:OwnedCli.Id /T /F 2>&1 | Out-Null
        }
        Stop-OwnedWallets
    }
    Remove-Item Env:FLINT_PAUSE_AFTER_RECEIVE -ErrorAction SilentlyContinue
    Remove-Item Env:FLINT_PAUSE_EVIDENCE_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:FLINT_WALLET_PASS -ErrorAction SilentlyContinue
}

