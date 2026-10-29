# Hidden, detach-safe launcher for the testnet node.
#
# Called through WMI by run-testnet-node.ps1 so the node:
#   * is not part of the caller's job object (survives a killed shell), and
#   * never shows a console window.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Config,
    [Parameter(Mandatory = $true)][string]$WorkDir,
    [Parameter(Mandatory = $true)][string]$Log,
    [Parameter(Mandatory = $true)][string]$ErrLog,
    [Parameter(Mandatory = $true)][string]$PidFile
)

$ErrorActionPreference = "Stop"
$proc = Start-Process -FilePath $Exe `
    -ArgumentList @("--testnet", "--no-tui", "server", "-c", $Config, "run") `
    -WorkingDirectory $WorkDir `
    -WindowStyle Hidden `
    -RedirectStandardOutput $Log `
    -RedirectStandardError $ErrLog `
    -PassThru
Set-Content -Path $PidFile -Value $proc.Id
