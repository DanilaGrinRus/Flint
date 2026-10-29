# Собирает grin-wallet из закреплённого F-04 fork в общем offline workspace.
# Исходники проверяются по Git (если он есть) и всегда по SHA-256 манифесту,
# поэтому source-архив без .git собирается по тем же правилам.
param(
    [string]$Target = "resources/wallet"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$fork = Join-Path $root "third_party\grin-wallet"
$pin = Join-Path $root "third_party\GRIN_WALLET_FORK.toml"
$manifest = Join-Path $root "third_party\grin-wallet-sources.sha256"
$dir = if ([System.IO.Path]::IsPathRooted($Target)) { $Target } else { Join-Path $root $Target }
$tmp = Join-Path $root ".tmp"
New-Item -ItemType Directory -Force $dir, $tmp | Out-Null
$env:TMP = $tmp
$env:TEMP = $tmp
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
$cargo = if ($cargo) { $cargo.Source } else { Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe" }
if (-not (Test-Path $cargo)) { throw "cargo не найден: $cargo" }

function Pin-Value([string]$Name) {
    $match = Select-String -Path $pin -Pattern "^$Name\s*=\s*`"([^`"]+)`"" | Select-Object -First 1
    if (-not $match) { throw "в GRIN_WALLET_FORK.toml нет $Name" }
    return $match.Matches[0].Groups[1].Value
}

$commit = Pin-Value "commit"
$grinCommit = Pin-Value "grin_commit"
if ((Test-Path (Join-Path $fork ".git"))) {
    $actual = (& git -C $fork rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -ne $commit) {
        throw "grin-wallet commit не совпадает: ожидался $commit, получен $actual"
    }
    $actualGrin = (& git -C (Join-Path $fork "grin") rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actualGrin -ne $grinCommit) {
        throw "grin submodule commit не совпадает: ожидался $grinCommit, получен $actualGrin"
    }
    $dirty = (& git -C $fork status --porcelain --untracked-files=all)
    if ($LASTEXITCODE -ne 0 -or $dirty) {
        throw "grin-wallet fork содержит незакреплённые изменения; сборка RPC запрещена"
    }
}

$expected = @{}
Get-Content $manifest | ForEach-Object {
    if ($_ -notmatch '^([0-9a-f]{64})  (.+)$') { throw "неверная строка source manifest: $_" }
    $expected[$Matches[2]] = $Matches[1]
}
if ($expected.Count -eq 0) { throw "пустой source manifest: $manifest" }
$excluded = @(".git", "target", "test_output", ".tmp", "dist", "data", "vendor")
$actualFiles = @{}
Get-ChildItem $fork -Recurse -File | ForEach-Object {
    $relative = $_.FullName.Substring($fork.Length).TrimStart('\', '/').Replace('\', '/')
    $parts = $relative.Split('/')
    if ($parts | Where-Object { $excluded -contains $_ }) { return }
    $actualFiles[$relative] = $_.FullName
}
foreach ($path in $expected.Keys) {
    if (-not $actualFiles.ContainsKey($path)) { throw "fork: отсутствует $path" }
    $hash = (Get-FileHash $actualFiles[$path] -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $expected[$path]) { throw "fork: SHA256 не совпал для $path" }
}
foreach ($path in $actualFiles.Keys) {
    if (-not $expected.ContainsKey($path)) { throw "fork: незакреплённый файл $path" }
}
Write-Host "fork source: $commit ($($expected.Count) files), grin $grinCommit"

Push-Location $root
try {
    & $cargo build --release -p grin_wallet --bin grin-wallet --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "сборка grin-wallet из fork не удалась" }
} finally {
    Pop-Location
}
$built = Join-Path $root "target\release\grin-wallet.exe"
if (-not (Test-Path $built)) { throw "сборка не создала $built" }
$exe = Join-Path $dir "grin-wallet.exe"
if ((Test-Path $exe) -and -not (Test-Path (Join-Path $tmp "grin-wallet-prefix.exe"))) {
    Copy-Item $exe (Join-Path $tmp "grin-wallet-prefix.exe")
}
Copy-Item $built $exe -Force
$actualHash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant()
$native = Join-Path $root "resources\native-assets.sha256"
$lines = Get-Content $native | ForEach-Object {
    if ($_ -match '  resources/wallet/grin-wallet\.exe$') {
        "$actualHash  resources/wallet/grin-wallet.exe"
    } else {
        $_
    }
}
Set-Content $native $lines -Encoding ascii
Write-Host "grin-wallet готов: $exe (SHA256 $actualHash)"
