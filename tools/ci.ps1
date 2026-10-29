# Local CI equivalent: format, check, tests, Slint build and headless selftest.
# Usage: pwsh -File tools/ci.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root ".tmp"
New-Item -ItemType Directory -Force $tmp | Out-Null
$env:TMP = $tmp
$env:TEMP = $tmp
Push-Location $root
try {
    pwsh -File (Join-Path $PSScriptRoot "i18n-gen.ps1") -Check
    if ($LASTEXITCODE -ne 0) { throw "i18n source is stale" }
    pwsh -File (Join-Path $PSScriptRoot "i18n-check.ps1")
    if ($LASTEXITCODE -ne 0) { throw "i18n catalog check failed" }
    # Не форматируем закреплённые path-исходники grin: их SHA-256 неизменен.
    cargo fmt -p flint-core -p flint-janus -p flint-messenger -p flint-runtime -p flint-slint -- --check
    if ($LASTEXITCODE -ne 0) { throw "cargo fmt --check failed" }
    cargo check --workspace --all-targets --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "cargo check failed" }
    cargo test --workspace --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
    cargo test --workspace --doc --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "doctests failed" }
    # Cover rustdoc as part of the full gate; the previous E0463 failure that
    # appeared only in a rustdoc invocation must not be hidden by test-only CI.
    cargo doc --workspace --no-deps --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "rustdoc failed" }
    cargo build -p flint-slint --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "Slint debug build failed" }
    cargo run -p flint-runtime --bin flint-selftest --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "selftest failed" }
    Write-Host "CI: ok"
} finally {
    Pop-Location
}
