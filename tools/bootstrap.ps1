# По умолчанию — только offline-проверка. Сеть/сборка доступны по явным флагам.
[CmdletBinding(SupportsShouldProcess)]
param([switch]$RestoreVendor, [switch]$FetchNode, [switch]$BuildWallet)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$env:TMP = Join-Path $root '.tmp'
$env:TEMP = $env:TMP
Push-Location $root
try {
    if ($RestoreVendor) {
        if (-not $PSCmdlet.ShouldProcess((Join-Path $root 'vendor'), 'Восстановить locked зависимости через сеть; сохранить прежний vendor')) { return }
        $id = [guid]::NewGuid().ToString('N')
        $work = Assert-FlintContainedPath -Root $root -Path (Join-Path $root ".tmp/flint-bootstrap-$id")
        New-Item -ItemType Directory -Path $work -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $work '.flint-ephemeral') -Value 'vendor-staging' -Encoding ascii
        $stagedVendor = Join-Path $work 'vendor'
        # Без --respect-source-config cargo vendor игнорирует replace-with.
        # Override касается только этого процесса; offline config рабочего дерева не меняется.
        cargo --config net.offline=false vendor --locked --versioned-dirs $stagedVendor
        if ($LASTEXITCODE -ne 0) { throw "cargo vendor не удался; прежний vendor/config сохранены. Staging: $work" }
        $vendor = Assert-FlintContainedPath -Root $root -Path (Join-Path $root 'vendor')
        $backup = Assert-FlintContainedPath -Root $root -Path (Join-Path $root ".cleanup-rollback/vendor-before-$id")
        if (Test-Path -LiteralPath $vendor) {
            New-Item -ItemType Directory -Path (Split-Path -Parent $backup) -Force | Out-Null
            Move-Item -LiteralPath $vendor -Destination $backup
        }
        try { Move-Item -LiteralPath $stagedVendor -Destination $vendor }
        catch {
            if ((Test-Path -LiteralPath $backup) -and -not (Test-Path -LiteralPath $vendor)) { Move-Item -LiteralPath $backup -Destination $vendor }
            throw
        }
        Write-Host "Полный vendor восстановлен. Прежняя копия, если была: $backup"
    }
    if ($FetchNode -and $PSCmdlet.ShouldProcess('resources/node', 'Скачать закреплённую testnet-совместимую ноду и проверить SHA256')) { & (Join-Path $PSScriptRoot 'fetch-node.ps1') }
    if ($BuildWallet -and $PSCmdlet.ShouldProcess('resources/wallet', 'Проверить закреплённый fork и собрать grin-wallet offline')) { & (Join-Path $PSScriptRoot 'fetch-wallet.ps1') }
    cargo metadata --format-version 1 --locked --offline | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Offline-зависимости не готовы. Для восстановления с сетью: tools/bootstrap.ps1 -RestoreVendor' }
    $missing = @()
    foreach ($line in Get-Content -LiteralPath (Join-Path $root 'resources/native-assets.sha256')) {
        if ($line -notmatch '^([0-9a-f]{64})\s+(.+)$') { throw 'Неверный native-assets manifest' }
        $expected = $Matches[1]; $relative = $Matches[2]
        $path = Assert-FlintContainedPath -Root $root -Path (Join-Path $root $relative)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { $missing += $relative; continue }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw "Native asset не совпал с манифестом: $relative" }
    }
    if ($missing.Count) { Write-Warning ('Нужны native assets: ' + ($missing -join ', ') + '. Tor и VC runtime автоматически не скачиваются.') }
    Write-Host 'Offline Cargo metadata: PASS; проверены имеющиеся pinned native assets.'
} finally { Pop-Location }
