$ErrorActionPreference = 'Stop'
$root = 'C:\DHW'
$failed = $false
Get-Content (Join-Path $root 'resources\native-assets.sha256') | ForEach-Object {
    if ($_ -notmatch '^([0-9a-f]{64})\s+(.+)$') { throw "bad manifest line" }
    $expected = $Matches[1]
    $relative = $Matches[2]
    $actual = (Get-FileHash (Join-Path $root $relative) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        $failed = $true
        Write-Output "MISMATCH $relative"
    } else {
        Write-Output "OK $relative"
    }
}
if ($failed) { exit 1 }
