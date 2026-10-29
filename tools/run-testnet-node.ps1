# Long-lived testnet node for the phase-1+ gates (two wallets, crash tests).
#
#   pwsh -File tools/run-testnet-node.ps1 start      # detached, syncs in background
#   pwsh -File tools/run-testnet-node.ps1 status
#   pwsh -File tools/run-testnet-node.ps1 stop
#
# Lives in .node/testnet (gitignored) with its own ports and catalogs, so it does
# not touch the wallet's data or a mainnet node on 3413.
#
# Started through Start-Process -WindowStyle Hidden on purpose:
#   * a plain child of this shell dies together with the command that spawned it;
#   * cmd.exe /c via WMI left a visible console window behind, which is why this
#     uses Start-Process with redirected streams instead.
param(
    [ValidateSet("start", "stop", "status")]
    [string]$Action = "status",
    [string]$DataDir = ".node/testnet"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = if ([System.IO.Path]::IsPathRooted($DataDir)) { $DataDir } else { Join-Path $root $DataDir }
$nodeDir = Join-Path $dir "node"
$grin = Join-Path $root "resources\node\grin.exe"
$config = Join-Path $nodeDir "grin-server.toml"
$log = Join-Path $nodeDir "node.log"
$logErr = Join-Path $nodeDir "node.err.log"
$pidFile = Join-Path $dir "node.pid"

function Get-NodePid {
    if (-not (Test-Path $pidFile)) { return $null }
    $raw = Get-Content $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $raw) { return $null }
    return [int]$raw
}

function Get-NodeProcess {
    $id = Get-NodePid
    if (-not $id) { return $null }
    return Get-Process -Id $id -ErrorAction SilentlyContinue
}

function Get-Height {
    try {
        $body = '{"jsonrpc":"2.0","method":"get_tip","params":[],"id":1}'
        $r = Invoke-RestMethod -Uri "http://127.0.0.1:13413/v2/foreign" -Method Post `
            -Body $body -ContentType "application/json" -TimeoutSec 3
        # grin v2 wraps results as {"result":{"Ok":{...}},"error":null}
        if ($r.result.Ok) { return $r.result.Ok.height }
        if ($r.result.height) { return $r.result.height }
    } catch { return $null }
}

switch ($Action) {
    "start" {
        if (-not (Test-Path $grin)) { throw "нет $grin — сначала: tools\fetch-node.ps1" }
        if (Get-NodeProcess) { Write-Host "уже запущена (pid $(Get-NodePid))"; break }

        New-Item -ItemType Directory -Force $nodeDir | Out-Null
        if (-not (Test-Path $config)) {
            Push-Location $nodeDir
            try { & $grin --testnet server config | Out-Null } finally { Pop-Location }
            if (-not (Test-Path $config)) { throw "grin не создал $config" }
            # Warning hides peer/sync lines the UI parses.
            (Get-Content $config -Raw).Replace('stdout_log_level = "Warning"', 'stdout_log_level = "Info"') |
                Set-Content $config -NoNewline
        }

        # Launched through WMI + a hidden helper so the node is not part of this
        # shell's job object and never shows a console window.
        $helper = Join-Path $PSScriptRoot "_start-node-hidden.ps1"
        Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
        $wmiCmd = "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$helper`" " +
            "-Exe `"$grin`" -Config `"$config`" -WorkDir `"$nodeDir`" " +
            "-Log `"$log`" -ErrLog `"$logErr`" -PidFile `"$pidFile`""
        $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
            CommandLine      = $wmiCmd
            CurrentDirectory = $nodeDir
        }
        if ($r.ReturnValue -ne 0) { throw "не удалось запустить ноду (Win32_Process.Create = $($r.ReturnValue))" }

        $deadline = (Get-Date).AddSeconds(60)
        while ((Get-Date) -lt $deadline) {
            if (Get-Height) { break }
            Start-Sleep -Milliseconds 500
        }
        $h = Get-Height
        $nodeId = Get-NodePid
        if ($h) { Write-Host "нода запущена (pid $nodeId), высота: $h" }
        else { Write-Host "процесс запущен (pid $nodeId), RPC ещё молчит — идёт первичная синхронизация; лог: $log" }
    }
    "stop" {
        $id = Get-NodePid
        if (-not $id) { Write-Host "не запущена"; break }
        & taskkill /PID $id /T /F 2>&1 | Out-Null
        Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
        Write-Host "остановлена"
    }
    "status" {
        $p = Get-NodeProcess
        if (-not $p) { Write-Host "не запущена"; break }
        $id = Get-NodePid
        $h = Get-Height
        $size = 0
        $chain = Join-Path $nodeDir "chain_data"
        if (Test-Path $chain) {
            $size = [math]::Round(((Get-ChildItem $chain -Recurse -File -ErrorAction SilentlyContinue |
                Measure-Object Length -Sum).Sum / 1GB), 2)
        }
        $batches = (Select-String -Path $log -Pattern 'Received 32 block headers' -ErrorAction SilentlyContinue | Measure-Object).Count
        Write-Host ("pid {0}, RPC: {1}, chain_data: {2} ГБ, батчей заголовков: {3}" -f `
            $id, $(if ($h) { "высота $h" } else { "молчит" }), $size, $batches)
    }
}
