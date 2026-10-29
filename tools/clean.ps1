# Разрешённая уборка: по умолчанию только build cache. Данные и backups не входят.
[CmdletBinding(SupportsShouldProcess, ConfirmImpact='High')]
param([switch]$IncludeArtifacts, [switch]$IncludeTemp)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$candidates = [Collections.Generic.List[string]]::new()
$candidates.Add((Join-Path $root 'target'))
if ($IncludeArtifacts -and (Test-Path -LiteralPath (Join-Path $root 'dist') -PathType Container)) {
    [void](Assert-FlintContainedPath -Root $root -Path (Join-Path $root 'dist'))
    foreach ($entry in Get-ChildItem -LiteralPath (Join-Path $root 'dist') -Directory -Force) {
        if ($entry.Name -match '^\d+\.\d+\.\d+[-+a-zA-Z0-9._]*-' -and (Test-Path -LiteralPath (Join-Path $entry.FullName '.flint-artifacts.json') -PathType Leaf)) { $candidates.Add($entry.FullName) }
    }
}
if ($IncludeTemp -and (Test-Path -LiteralPath (Join-Path $root '.tmp') -PathType Container)) {
    [void](Assert-FlintContainedPath -Root $root -Path (Join-Path $root '.tmp'))
    foreach ($entry in Get-ChildItem -LiteralPath (Join-Path $root '.tmp') -Directory -Force) {
        if ($entry.Name -match '^flint-(source|bootstrap)-[0-9a-f]{32}$' -and (Test-Path -LiteralPath (Join-Path $entry.FullName '.flint-ephemeral') -PathType Leaf)) { $candidates.Add($entry.FullName) }
    }
}
foreach ($candidate in $candidates) {
    $path = Assert-FlintContainedPath -Root $root -Path $candidate
    if (-not (Test-Path -LiteralPath $path)) { continue }
    $relative = [IO.Path]::GetRelativePath($root, $path).Replace('\','/')
    if ($relative -ne 'target' -and $relative -notmatch '^dist/\d[^/]+$' -and $relative -notmatch '^\.tmp/flint-(source|bootstrap)-[0-9a-f]{32}$') { throw "Путь отсутствует в allowlist: $path" }
    # WhatIf не читает содержимое target. Реальное удаление дополнительно
    # проверяет всю метаинформацию дерева, data/wallet markers и reparse points.
    if ($relative -ne 'target') { Assert-FlintNoLocalData -Path $path }
    if ($PSCmdlet.ShouldProcess($path, 'Удалить только разрешённые generated-файлы')) {
        Assert-FlintNoLocalData -Path $path
        $running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { try { $_.Path -and $_.Path.StartsWith($path + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) } catch { $false } })
        if ($running.Count) { throw "Из каталога запущены процессы; остановите их перед уборкой: $path" }
        Remove-Item -LiteralPath $path -Recurse -Force
    }
}
