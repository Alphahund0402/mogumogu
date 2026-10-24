#requires -Version 5.1
<#
Baut mogumogu.exe und mogumogu-cli.exe neu nach dist\mogumogu und zeigt, wo
sie liegen. Läuft mogumogu aus dist, wird jede Instanz vorher über die eigene
CLI geordnet beendet (gleicher Modus, gleiches Datenverzeichnis) und danach
mit ihren Argumenten wieder im Tray gestartet; ein Prozess wird nie hart
beendet. Installiert nichts und ändert keine Einstellungen.

  .\rebuild.ps1               Release-Build ohne Tests (schnell)
  .\rebuild.ps1 -Test         vorher die Testsuite
  .\rebuild.ps1 -Open         danach den Ausgabeordner im Explorer öffnen
  .\rebuild.ps1 -NoRestart    beendete Instanzen nicht wieder starten
#>
[CmdletBinding()]
param([switch]$Test, [switch]$Open, [switch]$NoRestart)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$OutDir = Join-Path $Root 'dist\mogumogu'
$Exe = Join-Path $OutDir 'mogumogu.exe'
$Cli = Join-Path $OutDir 'mogumogu-cli.exe'

# Instances started from this build's EXE, with the arguments after its path.
function Get-DistOwners {
    @(Get-CimInstance Win32_Process -Filter "Name = 'mogumogu.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -eq $Exe } |
        ForEach-Object {
            $Line = [string]$_.CommandLine
            $Rest = if ($Line.StartsWith('"')) { $Line.Substring($Line.IndexOf('"', 1) + 1) } else { @($Line -split '\s+', 2)[1] }
            [pscustomobject]@{ Id = $_.ProcessId; Arguments = "$Rest".Trim() }
        })
}

# The same mode and data directory for the CLI as for the instance.
function Get-CliFlags([string]$Arguments) {
    $Flags = @()
    if ($Arguments -match '(^|\s)--demo(\s|$)') { $Flags += '--demo' }
    if ($Arguments -match '--data-dir\s+(?:"([^"]+)"|(\S+))') {
        $Flags += '--data-dir'
        $Flags += $(if ($Matches[1]) { $Matches[1] } else { $Matches[2] })
    }
    $Flags
}

$Started = Get-Date
$Restart = @()
$Failed = $false
try {
    foreach ($Owner in Get-DistOwners) {
        Write-Host "mogumogu läuft (PID $($Owner.Id)) und wird für den Build geordnet beendet …" -ForegroundColor Yellow
        $Flags = @(Get-CliFlags $Owner.Arguments)
        & $Cli @Flags --no-start owner stop | Out-Null
        $Restart += $Owner
    }
    $Deadline = (Get-Date).AddSeconds(15)
    while (@(Get-DistOwners).Count -gt 0 -and (Get-Date) -lt $Deadline) { Start-Sleep -Milliseconds 300 }
    $Left = @(Get-DistOwners)
    if ($Left.Count -gt 0) {
        throw "mogumogu aus dist läuft noch (PID $($Left.Id -join ', ')). Bitte über das Tray-Menü beenden und erneut bauen."
    }

    $Build = @{ NoRun = $true }
    if (-not $Test) { $Build.SkipTests = $true }
    & (Join-Path $Root 'build.ps1') @Build
}
catch {
    $Failed = $true
    Write-Host ''
    Write-Host "Build fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
}
finally {
    # Also after a failed build: the previous EXE is then still in place.
    if (-not $NoRestart -and (Test-Path -LiteralPath $Exe)) {
        foreach ($Owner in $Restart) {
            $Arguments = $Owner.Arguments -replace '(^|\s)--show(\s|$)', '$1--tray$2'
            if (-not $Arguments) { $Arguments = '--tray' }
            Start-Process -FilePath $Exe -ArgumentList $Arguments -WorkingDirectory $OutDir | Out-Null
            Write-Host "mogumogu wieder im Tray gestartet ($Arguments)." -ForegroundColor Green
        }
    }
}
if ($Failed) { exit 1 }

$Seconds = [int]((Get-Date) - $Started).TotalSeconds
$Item = Get-Item -LiteralPath $Exe
Write-Host ''
Write-Host ("Fertig in {0} s (Release, {1}). Ergebnis:" -f $Seconds, $(if ($Test) { 'mit Tests' } else { 'ohne Tests' })) -ForegroundColor Green
Write-Output $Exe
Write-Host ("  {0:N1} MB · gebaut {1:g}" -f ($Item.Length / 1MB), $Item.LastWriteTime)
Write-Host "  CLI:    $Cli"
Write-Host "  Ordner: $OutDir"
if ($Open) { Start-Process explorer.exe -ArgumentList "`"$OutDir`"" }
