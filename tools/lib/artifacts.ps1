# Общие правила путей и версий для воспроизводимых артефактов Flint.
function Get-FlintVersion {
    param([string]$Manifest, [string]$Section = 'workspace.package', [string]$Expected = '')
    $text = Get-Content -LiteralPath $Manifest -Raw
    $sectionPattern = '(?ms)^\[' + [regex]::Escape($Section) + '\]\s*\r?\n(?<body>.*?)(?=^\[|\z)'
    $sectionMatch = [regex]::Match($text, $sectionPattern)
    $versionMatch = [regex]::Match($sectionMatch.Groups['body'].Value, '(?m)^version\s*=\s*"([0-9A-Za-z.+-]+)"')
    if (-not $versionMatch.Success) { throw "Не найдена версия [$Section] в $Manifest" }
    $value = $versionMatch.Groups[1].Value
    if ($Expected -and $Expected -ne $value) { throw "Версия $Expected не совпадает с Cargo.toml ($value)" }
    return $value
}

function Assert-FlintContainedPath {
    param([string]$Root, [string]$Path)
    $base = [IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $full = [IO.Path]::GetFullPath($Path)
    if (-not $full.StartsWith($base + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Путь находится вне рабочего корня: $full" }
    $probe = $full
    while ($probe -and $probe.Length -ge $base.Length) {
        if (Test-Path -LiteralPath $probe) {
            if ((Get-Item -LiteralPath $probe -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Переход по ссылке/reparse point запрещён: $probe" }
        }
        $probe = Split-Path -Parent $probe
    }
    return $full
}

function Assert-FlintNoLocalData {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($Path)
    while ($pending.Count) {
        $current = $pending.Pop()
        foreach ($entry in Get-ChildItem -LiteralPath $current -Force) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Обнаружена ссылка: $($entry.FullName)" }
            if ($entry.Name -match '^(data|\.wallet-data|\.flint|wallet_data|wallets|mainnet|testnet|\.node|fixtures|\.secrets|\.git)$' -or $entry.Name -match '^(wallet\.seed(?:\..*)?|wallet\.db|data\.mdb|messenger\.db|data-root\.json|\.api_secret)$') { throw "Обнаружены локальные данные; автоматическая операция запрещена: $($entry.FullName)" }
            if ($entry.PSIsContainer) { $pending.Push($entry.FullName) }
        }
    }
}

function Assert-FlintNoReparseTree {
    param([string]$Root, [string]$Path)
    $path = Assert-FlintContainedPath -Root $Root -Path $Path
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($path)
    while ($pending.Count) {
        $entry = Get-Item -LiteralPath $pending.Pop() -Force
        if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Ссылка в дереве исходников: $($entry.FullName)" }
        if ($entry.PSIsContainer) { foreach ($child in Get-ChildItem -LiteralPath $entry.FullName -Force) { $pending.Push($child.FullName) } }
    }
}

function Get-FlintArtifactContext {
    param([string]$Root, [string]$BuildId = '', [string]$OutputDir = '', [switch]$RequireExisting)
    $version = Get-FlintVersion -Manifest (Join-Path $Root 'Cargo.toml')
    $dist = Join-Path $Root 'dist'
    if ($OutputDir) {
        $path = if ([IO.Path]::IsPathRooted($OutputDir)) { $OutputDir } else { Join-Path $Root $OutputDir }
        $path = Assert-FlintContainedPath -Root $Root -Path $path
        if ((Split-Path -Parent $path) -ne $dist -or (Split-Path -Leaf $path) -notlike "$version-*") { throw "Артефакты разрешены только в dist/$version-<build-id>" }
        $outputBuildId = (Split-Path -Leaf $path).Substring($version.Length + 1)
        if ($BuildId -and $BuildId -ne $outputBuildId) { throw 'BuildId не совпадает с OutputDir' }
        $BuildId = $outputBuildId
    } else {
        if (-not $BuildId) { $BuildId = (Get-Date).ToUniversalTime().ToString('yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8) }
        $path = Assert-FlintContainedPath -Root $Root -Path (Join-Path $dist "$version-$BuildId")
    }
    if ($BuildId -notmatch '^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$' -or $BuildId.Contains('..')) { throw 'Недопустимый BuildId' }
    $marker = Join-Path $path '.flint-artifacts.json'
    if (Test-Path -LiteralPath $path) {
        if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) { throw "Каталог не помечен как generated artifacts: $path" }
        [void](Assert-FlintContainedPath -Root $Root -Path $marker)
        $identity = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
        if ($identity.kind -ne 'flint-artifacts' -or $identity.version -ne $version -or $identity.build_id -ne $BuildId) { throw "Неверный маркер артефактов: $path" }
        Assert-FlintNoLocalData -Path $path
        $buildInfo = Join-Path $path 'BUILD-INFO.json'
        if ((Test-Path -LiteralPath $buildInfo -PathType Leaf) -and (Get-Content -LiteralPath $buildInfo -Raw | ConvertFrom-Json).state -eq 'release') { throw "Релиз уже финализирован: $path. Выберите новый BuildId." }
    } else {
        if ($RequireExisting) { throw "Каталог артефактов отсутствует: $path" }
        New-Item -ItemType Directory -Path $path -Force | Out-Null
        @{ kind='flint-artifacts'; version=$version; build_id=$BuildId } | ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding UTF8
    }
    return [pscustomobject]@{ Root=$Root; Version=$version; BuildId=$BuildId; Path=$path }
}

function Write-FlintArtifactHashes {
    param([string]$Path)
    Assert-FlintNoLocalData -Path $Path
    $lines = foreach ($file in Get-ChildItem -LiteralPath $Path -File -Recurse -Force | Where-Object { $_.Name -ne 'SHA256SUMS' } | Sort-Object FullName) {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), [IO.Path]::GetRelativePath($Path, $file.FullName).Replace('\','/')
    }
    Set-Content -LiteralPath (Join-Path $Path 'SHA256SUMS') -Value $lines -Encoding ascii
}

function Invoke-FlintIsolatedSelftest {
    param([string]$Root, [string]$Binary, [string]$ReportPath = '')
    $work = Assert-FlintContainedPath -Root $Root -Path (Join-Path $Root ('.tmp/flint-selftest-' + [guid]::NewGuid().ToString('N')))
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    Push-Location $work
    try {
        & $Binary | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "flint-selftest завершился с кодом $LASTEXITCODE" }
        if ($ReportPath) { Copy-Item -LiteralPath (Join-Path $work 'selftest.json') -Destination $ReportPath }
    } finally {
        Pop-Location
        $work = Assert-FlintContainedPath -Root $Root -Path $work
        Remove-Item -LiteralPath $work -Recurse -Force
    }
}
