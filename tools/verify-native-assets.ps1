$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/artifacts.ps1')
$failed = $false
Get-Content (Join-Path $root 'resources\native-assets.sha256') | ForEach-Object {
    if ($_ -notmatch '^([0-9a-f]{64})\s+(.+)$') { throw "bad manifest line" }
    $expected = $Matches[1]
    $relative = $Matches[2]
    $path = Assert-FlintContainedPath -Root $root -Path (Join-Path $root $relative)
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        $failed = $true
        Write-Output "MISMATCH $relative"
    } else {
        Write-Output "OK $relative"
    }
}
if ($failed) { throw 'Не совпали контрольные суммы native assets' }
