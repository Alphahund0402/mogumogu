#requires -Version 5.1
<#
Builds both executables, runs the test suite and starts the owner in the tray.
Does not install prerequisites, set autostart or change machine settings.
#>
[CmdletBinding()]
param(
    [ValidateSet('Release','Debug')][string]$Configuration = 'Release',
    [ValidateSet('Demo','Local')][string]$Mode = 'Demo',
    [switch]$ShowDashboard,
    [switch]$NoRun,
    [switch]$SkipTests
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$Target = 'x86_64-pc-windows-msvc'
$OutDir = Join-Path $Root 'dist\mogumogu'
$TargetDir = Join-Path $Root 'target'

function Invoke-Cargo([string[]]$CargoArguments) {
    & cargo @CargoArguments
    if ($LASTEXITCODE -ne 0) { throw "cargo $($CargoArguments[0]) fehlgeschlagen (Exit $LASTEXITCODE). Die Anwendung wurde nicht gestartet." }
}

if ($env:OS -ne 'Windows_NT') { throw 'Windows 10/11 x64 ist erforderlich.' }
foreach ($Tool in @('cargo', 'rustc', 'rustup')) {
    if (-not (Get-Command $Tool -ErrorAction SilentlyContinue)) {
        throw 'Rust fehlt. Bitte rustup mit der MSVC-Toolchain installieren (siehe README.md).'
    }
}
Push-Location -LiteralPath $Root
try {
    Write-Host 'mogumogu · Windows-Build' -ForegroundColor Cyan
    & rustc --version
    if ($LASTEXITCODE -ne 0) { throw 'Die in rust-toolchain.toml festgelegte Toolchain ist nicht verfügbar (rustup toolchain install).' }
    $Installed = @(& rustup target list --installed)
    if ($Installed -notcontains $Target) { throw "Ziel fehlt: rustup target add $Target (plus MSVC C++ Build Tools und Windows SDK)." }

    if (-not $SkipTests) {
        # Core, contracts, Windows read boundary, IPC and end-to-end pilot.
        Invoke-Cargo @('test', '--locked', '--no-default-features', '--features', 'network', '--target', $Target, '--target-dir', $TargetDir)
    }
    $Build = @('build', '--locked', '--bins', '--target', $Target, '--target-dir', $TargetDir)
    $Folder = 'debug'
    if ($Configuration -eq 'Release') { $Build += '--release'; $Folder = 'release' }
    Invoke-Cargo $Build

    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
    $BinDir = Join-Path $TargetDir "$Target\$Folder"
    foreach ($File in @('mogumogu.exe', 'mogumogu-cli.exe')) {
        $Source = Join-Path $BinDir $File
        if (-not (Test-Path -LiteralPath $Source)) { throw "Build-Artefakt fehlt: $Source" }
        try { Copy-Item -LiteralPath $Source -Destination (Join-Path $OutDir $File) -Force }
        catch { throw 'Die EXE ist in Benutzung. Laufende Instanz über das Tray-Menü beenden und erneut bauen.' }
    }
    foreach ($File in @('LICENSE', 'README.md', 'THIRD_PARTY.md', 'Cargo.lock')) {
        Copy-Item -LiteralPath (Join-Path $Root $File) -Destination $OutDir -Force
    }
    Write-Host "Build bereit: $OutDir" -ForegroundColor Green

    if (-not $NoRun) {
        $Arguments = @('--tray')
        if ($ShowDashboard) { $Arguments = @('--show') }
        if ($Mode -eq 'Demo') { $Arguments += '--demo' }
        $Process = Start-Process -FilePath (Join-Path $OutDir 'mogumogu.exe') -ArgumentList $Arguments -WorkingDirectory $OutDir -PassThru
        Start-Sleep -Milliseconds 1500
        $Process.Refresh()
        if ($Process.HasExited -and $Process.ExitCode -ne 0) {
            throw "mogumogu wurde beim Start beendet (Exit $($Process.ExitCode)). Läuft bereits eine Instanz im Tray?"
        }
        Write-Host 'Gestartet. Das Katzensymbol liegt im Windows-Tray (ggf. im Überlaufbereich).' -ForegroundColor Green
        Write-Host 'Linksklick: Dashboard · Rechtsklick: Menü · Fenster schließen hält den Tray-Prozess aktiv.'
    }
}
finally { Pop-Location }
