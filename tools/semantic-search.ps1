# Semantic code search over crates/ via local LM Studio embeddings.
# Usage: pwsh -File tools/semantic-search.ps1 -Rebuild
#        pwsh -File tools/semantic-search.ps1 [-Ru] "<query>" [-TopK 8]
param(
    [Parameter(Position = 0)][string]$Query,
    [switch]$Rebuild,
    [switch]$Ru,
    [int]$TopK = 8,
    [string]$Python = $env:FLINT_PYTHON,
    [string]$Endpoint = "http://127.0.0.1:1234",
    [string]$IndexDir = $env:FLINT_SEMANTIC_INDEX
)
$ErrorActionPreference = "Stop"

if (-not $Python) {
    $cmd = Get-Command python -ErrorAction SilentlyContinue
    if (-not $cmd) { throw "python не найден: задай -Python <путь> или переменную FLINT_PYTHON" }
    $Python = $cmd.Source
}

if (-not $IndexDir) { $IndexDir = Join-Path (Split-Path -Parent $PSScriptRoot) ".tmp\semantic" }
New-Item -ItemType Directory -Force -Path $IndexDir | Out-Null

$env:FLINT_SEMANTIC_INDEX = $IndexDir
$env:LMSTUDIO_URL = $Endpoint
$env:PYTHONIOENCODING = "utf-8"
$env:PYTHONDONTWRITEBYTECODE = "1"
[Console]::OutputEncoding = [Text.Encoding]::UTF8

& $Python -c "import numpy" 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "у $Python нет numpy: задай -Python <интерпретатор с numpy> или FLINT_PYTHON" }

if ($Rebuild) {
    & $Python (Join-Path $PSScriptRoot "semantic\index.py")
    exit $LASTEXITCODE
}

if (-not $Query) { throw "нужен запрос или -Rebuild" }

$pyArgs = @((Join-Path $PSScriptRoot "semantic\query.py"))
if ($Ru) { $pyArgs += "--ru" }
$pyArgs += @($Query, "$TopK")
& $Python @pyArgs
exit $LASTEXITCODE
