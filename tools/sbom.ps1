# Generates a CycloneDX SBOM for the Cargo workspace and pinned native assets.
# Usage: pwsh -File tools/sbom.ps1 [-Out dist/sbom.json]
param(
    [string]$Out = "dist/sbom.json"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root ".tmp"
New-Item -ItemType Directory -Force $tmp | Out-Null
$env:TMP = $tmp
$env:TEMP = $tmp

Push-Location $root
try {
    $meta = cargo metadata --format-version 1 --locked --offline | ConvertFrom-Json -AsHashtable
} finally {
    Pop-Location
}
$version = ($meta["packages"] | Where-Object { $_["name"] -eq "flint-slint" } | Select-Object -First 1)["version"]
if (-not $version) { throw "flint-slint package not found in cargo metadata" }
$pinPath = Join-Path $root "third_party\GRIN_WALLET_FORK.toml"
$pin = Get-Content $pinPath -Raw
$forkCommit = ([regex]::Match($pin, '(?m)^commit = "([0-9a-f]{40})"')).Groups[1].Value
$grinCommit = ([regex]::Match($pin, '(?m)^grin_commit = "([0-9a-f]{40})"')).Groups[1].Value
if (-not $forkCommit -or -not $grinCommit) { throw "F-04 fork commits not found" }
$sourceHash = (Get-FileHash (Join-Path $root "third_party\grin-wallet-sources.sha256") -Algorithm SHA256).Hash.ToLowerInvariant()

$components = foreach ($p in ($meta["packages"] | Sort-Object { $_["name"] }, { $_["version"] })) {
    $licenseText = $p["license"]
    $license = if ($licenseText) { @(@{ expression = $licenseText }) } else { @() }
    $component = [ordered]@{
        type = "library"
        name = $p["name"]
        version = $p["version"]
        purl = "pkg:cargo/$($p["name"])@$($p["version"])"
        licenses = $license
    }
    $manifest = $p["manifest_path"]
    if ($manifest -like "*\third_party\grin-wallet\*" -or $manifest -like "*\crates\grin-wallet-rpc\Cargo.toml") {
        $component["properties"] = @(
            @{ name = "flint:f04_grin_wallet_commit"; value = $forkCommit },
            @{ name = "flint:f04_grin_commit"; value = $grinCommit },
            @{ name = "flint:f04_source_manifest_sha256"; value = $sourceHash }
        )
    }
    $component
}

$nativeManifest = Join-Path $root "resources\native-assets.sha256"
if (-not (Test-Path $nativeManifest)) { throw "missing native asset manifest: $nativeManifest" }
$nativeManifestHash = (Get-FileHash $nativeManifest -Algorithm SHA256).Hash.ToLowerInvariant()
$native = foreach ($line in Get-Content $nativeManifest) {
    if ($line -notmatch '^([0-9a-f]{64})\s+(.+)$') { throw "bad native asset pin: $line" }
    $expected = $matches[1]
    $relative = $matches[2]
    $full = Join-Path $root $relative
    if (-not (Test-Path $full)) { throw "missing pinned native asset: $full" }
    $actual = (Get-FileHash $full -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "native asset hash mismatch: $relative" }
    [ordered]@{
        type = "file"
        name = (Split-Path $relative -Leaf)
        hashes = @(@{ alg = "SHA-256"; content = $expected })
        properties = @(
            @{ name = "flint:path"; value = $relative },
            @{ name = "flint:source_manifest"; value = "resources/native-assets.sha256" }
        )
    }
}

$sbom = [ordered]@{
    bomFormat = "CycloneDX"
    specVersion = "1.5"
    serialNumber = "urn:uuid:$([guid]::NewGuid())"
    version = 1
    metadata = [ordered]@{
        timestamp = (Get-Date).ToUniversalTime().ToString("o")
        component = [ordered]@{
            type = "application"
            name = "Flint"
            version = $version
            purl = "pkg:cargo/flint-slint@$version"
            properties = @(
                @{ name = "flint:f04_grin_wallet_commit"; value = $forkCommit },
                @{ name = "flint:f04_grin_commit"; value = $grinCommit },
                @{ name = "flint:f04_source_manifest_sha256"; value = $sourceHash },
                @{ name = "flint:native_asset_manifest_sha256"; value = $nativeManifestHash }
            )
        }
        tools = @(@{ vendor = "Flint"; name = "tools/sbom.ps1"; version = "1.2" })
    }
    components = @($components) + @($native)
}
$path = Join-Path $root $Out
New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
$json = $sbom | ConvertTo-Json -Depth 12
Set-Content -Path $path -Value $json -Encoding UTF8
Write-Host "SBOM: $path ($($components.Count) cargo + $($native.Count) native components)"
