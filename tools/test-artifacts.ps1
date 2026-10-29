# Изолированный smoke упаковки исходников и защит уборки. Ни сборок, ни сети.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $projectRoot ('.tmp/flint-tools-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $testRoot 'tools/lib') -Force | Out-Null
foreach ($name in @('source.ps1','package-transfer-contract.ps1','bootstrap.ps1','clean.ps1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $testRoot "tools/$name")
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'lib/artifacts.ps1') -Destination (Join-Path $testRoot 'tools/lib/artifacts.ps1')
function Write-TestFile([string]$Relative, [string]$Value) {
    $path = Join-Path $testRoot $Relative
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    [IO.File]::WriteAllText($path, $Value)
}
function Assert-Test([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
Write-TestFile 'Cargo.toml' "[workspace]`nmembers = [`"crates/transfer-contract`"]`n[workspace.package]`nversion = `"9.8.7`"`n"
Write-TestFile 'Cargo.lock' '# smoke lockfile'
Write-TestFile 'crates/transfer-contract/Cargo.toml' "[package]`nname = `"flint-transfer-contract`"`nversion = `"1.2.3`"`nedition = `"2021`"`n"
Write-TestFile 'crates/transfer-contract/src/lib.rs' '// smoke source'
Write-TestFile 'crates/transfer-contract/fixtures/example.json' '{}'
Write-TestFile 'crates/transfer-contract/README.md' 'Контракт'
Write-TestFile 'lang/catalog.json' '{"static":[],"dynamic":[]}'
Write-TestFile 'resources/native-assets.sha256' '# smoke manifest'
Write-TestFile '.cargo/config.toml' "[net]`noffline = true`n"
Write-TestFile '.github/workflows/smoke.yml' 'name: smoke'
Write-TestFile 'vendor/slint-1.18.1/Cargo.toml' '# smoke corresponding source'
Write-TestFile 'docs/STATUS.md' 'NOT VERIFIED — smoke fixture'
Write-TestFile 'evidence/github-checks.json' '{"smoke":true}'
Write-TestFile 'docs/testnet-funded.txt' 'FAKE TEST CREDENTIAL — DO NOT SHIP'
Write-TestFile 'docs/grinflint-ci-wallet.txt' 'FAKE TEST CREDENTIAL — DO NOT SHIP'
Write-TestFile 'docs/.secrets/marker.txt' 'FAKE TEST SECRET — DO NOT SHIP'
Write-TestFile 'docs/.env' 'FAKE TEST ENV — DO NOT SHIP'
Write-TestFile 'docs/.wallet-data/marker.txt' 'FAKE TEST DATA — DO NOT SHIP'
Write-TestFile 'docs/data/marker.txt' 'FAKE TEST DATA — DO NOT SHIP'
Write-TestFile 'AGENTS.md' 'Тестовые инструкции'
Write-TestFile 'README.md' 'Тестовые исходники'
Write-TestFile 'COPYING' 'GPL-3.0-only'
Write-TestFile 'THIRD_PARTY_NOTICES.md' 'Тест'
Write-TestFile 'dist/data/preserve.txt' 'preserve'
Write-TestFile 'target/preserve.txt' 'preserve'

& (Join-Path $testRoot 'tools/source.ps1') -BuildId 'smoke'
$artifactRoot = Join-Path $testRoot 'dist/9.8.7-smoke'
$sourceZip = Join-Path $artifactRoot 'Flint-9.8.7-source.zip'
$archive = [IO.Compression.ZipFile]::OpenRead($sourceZip)
try {
    $names = @($archive.Entries | ForEach-Object { $_.FullName.Replace('\','/') })
    foreach ($required in @('lang/catalog.json','.cargo/config.toml','.github/workflows/smoke.yml','resources/native-assets.sha256','tools/bootstrap.ps1','AGENTS.md','SOURCE.md','vendor/slint-1.18.1/Cargo.toml','evidence/github-checks.json')) {
        Assert-Test ($required -in $names) "В source archive отсутствует $required"
    }
    Assert-Test (-not @($names | Where-Object { $_ -match '^(dist|target|\.tmp)/' }).Count) 'В архив попали локальные файлы'
    Assert-Test (-not @($names | Where-Object { $_ -match '(^|/)(testnet-funded\.txt|grinflint-ci-wallet\.txt|\.secrets|\.env|\.wallet-data|data)(/|$)' }).Count) 'В архив попали credentials или локальные данные'
} finally { $archive.Dispose() }
$hashBefore = (Get-FileHash -LiteralPath $sourceZip).Hash
$rejected = $false
try { & (Join-Path $testRoot 'tools/source.ps1') -BuildId 'smoke' } catch { $rejected = $true }
Assert-Test $rejected 'Повторная упаковка должна отклонять перезапись существующего архива'
Assert-Test ((Get-FileHash -LiteralPath $sourceZip).Hash -eq $hashBefore) 'Архив был изменён повторной упаковкой'

& (Join-Path $testRoot 'tools/package-transfer-contract.ps1') -BuildId 'smoke'
Assert-Test (Test-Path -LiteralPath (Join-Path $artifactRoot 'flint-transfer-contract-1.2.3.zip')) 'Версия контракта должна браться из его Cargo.toml'
$sourceHashLines = Get-Content -LiteralPath (Join-Path $artifactRoot 'SHA256SUMS')
foreach ($line in $sourceHashLines) {
    Assert-Test ($line -match '^([0-9a-f]{64})  (.+)$') 'Неверный SHA256SUMS'
    $expected = $Matches[1]; $relative = $Matches[2]
    Assert-Test ((Get-FileHash -LiteralPath (Join-Path $artifactRoot $relative)).Hash.ToLowerInvariant() -eq $expected) "Неверный hash $relative"
}
. (Join-Path $testRoot 'tools/lib/artifacts.ps1')
$rejected = $false
try { Get-FlintArtifactContext -Root $testRoot -OutputDir '../outside' | Out-Null } catch { $rejected = $true }
Assert-Test $rejected 'Путь вне рабочего дерева должен отклоняться'
& (Join-Path $testRoot 'tools/clean.ps1') -IncludeArtifacts -IncludeTemp -WhatIf
Assert-Test (Test-Path -LiteralPath (Join-Path $testRoot 'target/preserve.txt')) 'WhatIf изменил target'
Assert-Test (Test-Path -LiteralPath (Join-Path $testRoot 'dist/data/preserve.txt')) 'WhatIf изменил dist/data'
Assert-Test (Test-Path -LiteralPath $sourceZip) 'WhatIf удалил артефакт'
Write-TestFile 'dist/9.8.7-smoke/Flint-slint-preview/data/preserve.txt' 'preserve'
$rejected = $false
try { & (Join-Path $testRoot 'tools/clean.ps1') -IncludeArtifacts -WhatIf } catch { $rejected = $true }
Assert-Test $rejected 'Артефакт с portable data должен блокировать уборку даже в WhatIf'
$configBefore = (Get-FileHash -LiteralPath (Join-Path $testRoot '.cargo/config.toml')).Hash
& (Join-Path $testRoot 'tools/bootstrap.ps1') -RestoreVendor -WhatIf
Assert-Test ((Get-FileHash -LiteralPath (Join-Path $testRoot '.cargo/config.toml')).Hash -eq $configBefore) 'Bootstrap WhatIf изменил offline config'
$finalized = Get-FlintArtifactContext -Root $testRoot -BuildId 'finalized'
Set-Content -LiteralPath (Join-Path $finalized.Path 'BUILD-INFO.json') -Value '{"state":"release"}'
Write-FlintArtifactHashes -Path $finalized.Path
$finalizedHashes = (Get-FileHash -LiteralPath (Join-Path $finalized.Path 'SHA256SUMS')).Hash
$rejected = $false
try { & (Join-Path $testRoot 'tools/package-transfer-contract.ps1') -OutputDir $finalized.Path } catch { $rejected = $true }
Assert-Test $rejected 'Общий контекст должен запрещать дописывать финализированный релиз'
Assert-Test ((Get-FileHash -LiteralPath (Join-Path $finalized.Path 'SHA256SUMS')).Hash -eq $finalizedHashes) 'Были изменены hashes финализированного релиза'
Assert-Test (-not (Test-Path -LiteralPath (Join-Path $finalized.Path 'flint-transfer-contract-1.2.3.zip'))) 'Был дописан финализированный релиз'
$link = Join-Path $testRoot 'crates/transfer-contract/src/linked'
$linkTarget = Join-Path $testRoot 'linked-source'
New-Item -ItemType Directory -Path $linkTarget -Force | Out-Null
$linkCreated = $false
try { New-Item -ItemType Junction -Path $link -Target $linkTarget -ErrorAction Stop | Out-Null; $linkCreated = $true }
catch { Write-Warning 'Reparse negative smoke: NOT VERIFIED — нет возможности создать junction' }
if ($linkCreated) {
    try {
        $rejected = $false
        try { & (Join-Path $testRoot 'tools/package-transfer-contract.ps1') -BuildId 'linked' } catch { $rejected = $true }
        Assert-Test $rejected 'Архивирование контракта должно отклонять junction в исходниках'
        Assert-Test (-not (Test-Path -LiteralPath (Join-Path $testRoot 'dist/9.8.7-linked/flint-transfer-contract-1.2.3.zip'))) 'Был создан архив через junction'
        Write-Host 'Reparse negative smoke: PASS'
    } finally { Remove-Item -LiteralPath $link -Force }
}
Write-Host "Artifact smoke: PASS. Только тестовые файлы: $testRoot"
