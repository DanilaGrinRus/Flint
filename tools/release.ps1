# Release pipeline for the Slint-only portable Windows build.
# Usage:
#   pwsh -File tools/release.ps1
#   pwsh -File tools/release.ps1 -SkipTests
#   pwsh -File tools/release.ps1 -CertificateThumbprint <sha1>
param(
    [string]$Version = "0.1.69",
    [switch]$SkipTests,
    [switch]$SkipPackage,
    [string]$CertificateThumbprint = "",
    [switch]$RequireSigned
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root ".tmp"
New-Item -ItemType Directory -Force $tmp | Out-Null
$env:TMP = $tmp
$env:TEMP = $tmp
Push-Location $root
try {
    $cargoVersion = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
    if ($cargoVersion -ne $Version) {
        throw "версия Cargo.toml ($cargoVersion) не совпадает с -Version ($Version)"
    }

    # Форматируются только исходники Flint, не закреплённый grin fork.
    cargo fmt -p flint-core -p flint-janus -p flint-messenger -p flint-runtime -p flint-slint -- --check
    if ($LASTEXITCODE -ne 0) { throw "cargo fmt --check failed" }
    if (-not $SkipTests) {
        cargo test --workspace --locked --offline
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
    }
    cargo build --release -p flint-slint --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "Slint release build failed" }
    cargo build --release -p flint-runtime --bin flint-selftest --locked --offline
    if ($LASTEXITCODE -ne 0) { throw "selftest release build failed" }

    $selftest = Join-Path $root "target\release\flint-selftest.exe"
    & $selftest | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "selftest exit code $LASTEXITCODE" }

    pwsh -File (Join-Path $root "tools\check-node-version.ps1") -Expected "5.5.2"
    if ($LASTEXITCODE -ne 0) { throw "node version check failed" }
    # RPC обязан быть собран из того же F-04 fork, а не взят из кэша/релиза upstream.
    pwsh -File (Join-Path $root "tools\fetch-wallet.ps1")
    if ($LASTEXITCODE -ne 0) { throw "pinned grin-wallet build failed" }
    pwsh -File (Join-Path $root "tools\sbom.ps1") -Out "dist\sbom.json"
    if ($LASTEXITCODE -ne 0) { throw "SBOM generation failed" }

    if (-not $SkipPackage) {
        pwsh -File (Join-Path $root "tools\package-slint.ps1")
        if ($LASTEXITCODE -ne 0) { throw "package-slint.ps1 failed" }
        pwsh -File (Join-Path $root "tools\source.ps1") -Version $Version
        if ($LASTEXITCODE -ne 0) { throw "source.ps1 failed" }
    }

    $previewExe = Join-Path $root "dist\Flint-slint-preview\Flint.exe"
    $singleExe = Join-Path $root "dist\Flint-$Version-single.exe"
    Copy-Item $previewExe $singleExe -Force
    $zip = Join-Path $root "dist\Flint-$Version-slint-preview.zip"
    $signed = $false
    if ($CertificateThumbprint) {
        $signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
        if (-not $signtool) {
            $signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
                Sort-Object FullName -Descending | Select-Object -First 1
        }
        if (-not $signtool) { throw "signtool.exe не найден; подпись невозможна" }
        $toolPath = if ($signtool.Source) { $signtool.Source } else { $signtool.FullName }
        foreach ($target in @($previewExe, $singleExe)) {
            & $toolPath sign /sha1 $CertificateThumbprint /fd sha256 /tr http://timestamp.digicert.com /td sha256 $target
            if ($LASTEXITCODE -ne 0) { throw "signtool failed for $target" }
        }
        Remove-Item $zip -Force -ErrorAction SilentlyContinue
        Compress-Archive -Path (Join-Path $root "dist\Flint-slint-preview\*") -DestinationPath $zip -CompressionLevel Optimal
        $signed = $true
        foreach ($target in @($previewExe, $singleExe)) {
            $sig = Get-AuthenticodeSignature $target
            if ($sig.Status -ne "Valid") {
                throw "Authenticode не подтверждён для $target : $($sig.Status) $($sig.StatusMessage)"
            }
        }
        Write-Host "signed: $CertificateThumbprint"
    } else {
        Write-Warning "артефакты UNSIGNED: сертификат не передан (-CertificateThumbprint)"
    }
    if ($RequireSigned -and -not $signed) {
        throw "запрошена подпись (-RequireSigned), но сертификат не передан или подпись не подтверждена"
    }

    $lockHash = (Get-FileHash (Join-Path $root "Cargo.lock") -Algorithm SHA256).Hash.ToLowerInvariant()
    $rustcVersion = (rustc -Vv | Out-String).Trim()
    $cargoVersionString = (cargo -V | Out-String).Trim()
    $buildInfo = [ordered]@{
        version = $Version
        target = "x86_64-pc-windows-msvc"
        profile = "release"
        features = @()
        cargo = $cargoVersionString
        rustc = $rustcVersion
        cargo_lock_sha256 = $lockHash
        signed = $signed
        generated_at = (Get-Date).ToUniversalTime().ToString("o")
    }
    $buildInfo | ConvertTo-Json -Depth 5 | Set-Content -Path (Join-Path $root "dist\build-info.json") -Encoding UTF8

    $files = @(
        $previewExe,
        $zip,
        $singleExe,
        (Join-Path $root "dist\Flint-$Version-source.zip"),
        (Join-Path $root "dist\sbom.json"),
        (Join-Path $root "dist\build-info.json"),
        (Join-Path $root "dist\COPYING"),
        (Join-Path $root "dist\THIRD_PARTY_NOTICES.md")
    ) | Where-Object { Test-Path $_ }
    $lines = foreach ($f in $files) {
        $hash = (Get-FileHash $f -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $([IO.Path]::GetRelativePath($root, $f).Replace('\','/'))"
    }
    Set-Content -Path (Join-Path $root "dist\SHA256SUMS") -Value $lines -Encoding ascii
    Write-Host "release: $Version signed=$signed"
    Get-Content (Join-Path $root "dist\SHA256SUMS")
} finally {
    Pop-Location
}
