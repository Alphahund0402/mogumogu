#requires -Version 5.1
# Starts an already built owner process. No autostart, no service.
[CmdletBinding()]
param([ValidateSet('Demo','Local')][string]$Mode = 'Demo', [switch]$ShowDashboard)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows ist erforderlich.' }
$Exe = Join-Path $PSScriptRoot 'dist\mogumogu\mogumogu.exe'
if (-not (Test-Path -LiteralPath $Exe)) { throw 'Zuerst bauen: .\build.ps1 -NoRun' }
$Arguments = @('--tray')
if ($ShowDashboard) { $Arguments = @('--show') }
if ($Mode -eq 'Demo') { $Arguments += '--demo' }
Start-Process -FilePath $Exe -ArgumentList $Arguments -WorkingDirectory (Split-Path -Parent $Exe)
