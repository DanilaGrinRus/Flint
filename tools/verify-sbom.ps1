# Verifies that a generated SBOM covers every Cargo package and pinned native asset.
# Usage: pwsh -File tools/verify-sbom.ps1 [-Sbom dist/sbom.json]
param(
    [string]$Sbom = "dist/sbom.json"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$path = if ([IO.Path]::IsPathRooted($Sbom)) { $Sbom } else { Join-Path $root $Sbom }
if (-not (Test-Path $path)) { throw "SBOM not found: $path" }
$data = Get-Content $path -Raw | ConvertFrom-Json
if ($data.bomFormat -ne "CycloneDX") { throw "not a CycloneDX document" }
if ($data.specVersion -notin @("1.4", "1.5", "1.6")) { throw "unsupported CycloneDX version: $($data.specVersion)" }
$components = @($data.components)
if ($components.Count -eq 0) { throw "SBOM has no components" }

$byPath = @{}
foreach ($component in $components) {
    $pin = @($component.properties | Where-Object { $_.name -eq "flint:path" }) | Select-Object -First 1
    if ($pin) { $byPath[$pin.value] = $component }
}

$nativeManifest = Join-Path $root "resources\native-assets.sha256"
$nativeCount = 0
foreach ($line in Get-Content $nativeManifest) {
    if ($line -notmatch '^([0-9a-f]{64})\s+(.+)$') { throw "bad native asset pin: $line" }
    $expected = $matches[1]
    $relative = $matches[2]
    if (-not $byPath.ContainsKey($relative)) { throw "native asset missing from SBOM: $relative" }
    $hash = @($byPath[$relative].hashes | Where-Object { $_.alg -eq "SHA-256" }) | Select-Object -First 1
    if (-not $hash) { throw "native asset has no SHA-256 hash: $relative" }
    if ($hash.content -ne $expected) { throw "native asset hash differs from manifest: $relative" }
    $nativeCount++
}

Push-Location $root
try {
    $meta = cargo metadata --format-version 1 --locked --offline | ConvertFrom-Json -AsHashtable
} finally {
    Pop-Location
}
$cargo = @{}
foreach ($package in $meta["packages"]) { $cargo["$($package["name"])@$($package["version"])"] = $true }
$sbomCargo = @{}
foreach ($component in $components) {
    if ($component.purl -like "pkg:cargo/*") { $sbomCargo["$($component.name)@$($component.version)"] = $true }
}
$missing = @($cargo.Keys | Where-Object { -not $sbomCargo.ContainsKey($_) } | Sort-Object)
if ($missing.Count -gt 0) { throw "Cargo packages missing from SBOM: $($missing -join ', ')" }

$f04 = @($components | Where-Object {
    @($_.properties) | Where-Object { $_.name -eq "flint:f04_grin_wallet_commit" }
})
if ($f04.Count -eq 0) { throw "F-04 fork identity is missing from SBOM" }

Write-Host "SBOM verified: $($cargo.Count) cargo packages, $nativeCount native assets, $($components.Count) components total"
