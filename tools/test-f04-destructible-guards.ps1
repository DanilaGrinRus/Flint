# Безопасные negative-тесты F-04: нода, кошельки и аварийные действия не нужны.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $PSScriptRoot "f04-destructible.ps1"
$engine = Join-Path $PSHOME "powershell.exe"
if (-not (Test-Path $engine -PathType Leaf)) { $engine = (Get-Process -Id $PID).Path }
$base = Join-Path $root (".tmp/f04-guard-tests-" + [Guid]::NewGuid().ToString("N"))
$stand = Join-Path $base "stand"
$a = Join-Path $stand "sender"
$b = Join-Path $stand "receiver"
$outside = Join-Path $base "outside"
New-Item -ItemType Directory -Force $a, $b, $outside | Out-Null
$script:Passed = 0

function Assert-Reject([string]$Name, [string]$Expected, [string[]]$Arguments) {
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $output = & $engine -NoProfile -File $target @Arguments 2>&1 | Out-String
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $previousPreference
    if ($exitCode -eq 0 -or $output -notmatch [regex]::Escape($Expected)) {
        throw "Guard $Name не отклонил сценарий ожидаемым образом: $output"
    }
    $script:Passed++
    Write-Output "PASS $Name"
}

try {
    $common = @(
        "-Action", "Preflight", "-Backend", "embedded", "-Scenario", "upstream",
        "-StandRoot", $stand, "-DataA", $a, "-DataB", $b, "-DestB", "tgrin1guard"
    )
    Assert-Reject "unmarked-stand" ".destructible-f04-stand" $common

    Set-Content (Join-Path $stand ".destructible-f04-stand") -Value "WRONG" -NoNewline
    Assert-Reject "wrong-stand-marker" ".destructible-f04-stand" $common

    Set-Content (Join-Path $stand ".destructible-f04-stand") -Value "F04-DESTRUCTIBLE-TESTNET-VM" -NoNewline
    Assert-Reject "unmarked-wallets" ".crash-test-ok" $common

    foreach ($dir in @($a, $b)) {
        Set-Content (Join-Path $dir ".crash-test-ok") -Value "testnet" -NoNewline
        Set-Content (Join-Path $dir ".destructible-f04-ok") -Value "testnet" -NoNewline
    }
    $outsideArgs = @(
        "-Action", "Preflight", "-Backend", "embedded", "-Scenario", "upstream",
        "-StandRoot", $stand, "-DataA", $outside, "-DataB", $b, "-DestB", "tgrin1guard"
    )
    Assert-Reject "outside-stand" "StandRoot" $outsideArgs

    $mainnet = Join-Path $stand "mainnet-copy"
    New-Item -ItemType Directory -Force $mainnet | Out-Null
    $mainnetArgs = $common.Clone()
    $mainnetArgs[9] = $mainnet
    Assert-Reject "mainnet-path" "Mainnet" $mainnetArgs

    $remoteArgs = $common + @("-NodeUrl", "http://192.0.2.1:13413")
    Assert-Reject "remote-node" "loopback" $remoteArgs

    $addressArgs = $common.Clone()
    $addressArgs[11] = "grin1notestnet"
    Assert-Reject "mainnet-address" "testnet" $addressArgs

    Assert-Reject "power-cut-not-attested" "PowerCutConfirmed" @(
        "-Action", "ResumePowerLoss", "-StateFile", (Join-Path $stand "missing.json"),
        "-PassA", "disposable-a", "-PassB", "disposable-b"
    )

    Write-Output "F-04 guard tests passed: $script:Passed"
} finally {
    if (Test-Path $base) { Remove-Item $base -Recurse -Force }
}
