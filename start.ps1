#requires -Version 5.1
# Starts the owner process. No autostart, no service. A missing program is
# loaded first: the newest release, SHA-256 checked.
[CmdletBinding()]
param([ValidateSet('Demo','Local')][string]$Mode = 'Local', [switch]$ShowDashboard)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows ist erforderlich.' }
$Exe = Join-Path $PSScriptRoot 'dist\mogumogu\mogumogu.exe'
if (-not (Test-Path -LiteralPath $Exe)) {
    Write-Host 'mogumogu fehlt noch: Das neueste Release wird von GitHub geladen.' -ForegroundColor Yellow
    try { & (Join-Path $PSScriptRoot 'scripts\download.ps1') -Release | Out-Null }
    catch { throw "$($_.Exception.Message) Alternativ selbst bauen: rebuild.cmd" }
}
$Arguments = @('--tray')
if ($ShowDashboard) { $Arguments = @('--show') }
if ($Mode -eq 'Demo') { $Arguments += '--demo' }
Start-Process -FilePath $Exe -ArgumentList $Arguments -WorkingDirectory (Split-Path -Parent $Exe)
