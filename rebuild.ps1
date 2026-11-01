#requires -Version 5.1
<#
Baut mogumogu.exe und mogumogu-cli.exe neu nach dist\mogumogu und zeigt, wo
sie liegen. Läuft mogumogu aus dist, wird jede Instanz vorher über die eigene
CLI geordnet beendet (gleicher Modus, gleiches Datenverzeichnis) und danach
mit ihren Argumenten wieder im Tray gestartet; ein Prozess wird nie hart
beendet. Installiert nichts und ändert keine Einstellungen.

Ohne installiertes Rust baut es im Docker-Container (nur Docker Desktop nötig):
die GNU-Toolchain kompiliert dort die Windows-Programme, die danach wie gewohnt
nativ unter Windows laufen. Ohne Rust und ohne Docker, etwa auf schwächeren
Rechnern, lädt es den fertigen Build statt zu bauen (scripts\download.ps1):
den CI-Build dieses Commits, sonst das neueste Release.

  .\rebuild.ps1               Release-Build ohne Tests (schnell)
  .\rebuild.ps1 -Docker       im Container bauen, auch wenn Rust installiert ist
  .\rebuild.ps1 -Download     nicht bauen, sondern den fertigen Build laden (CI-Build dieses Commits, sonst Release)
  .\rebuild.ps1 -Fast         inkrementell, opt-level 1; für schnelle Iteration (auch mit -Docker)
  .\rebuild.ps1 -Test         vorher die Testsuite (im Container: die Kernprüfungen)
  .\rebuild.ps1 -Open         danach den Ausgabeordner im Explorer öffnen
  .\rebuild.ps1 -NoRestart    beendete Instanzen nicht wieder starten
#>
[CmdletBinding()]
param([switch]$Test, [switch]$Open, [switch]$NoRestart, [switch]$Docker, [switch]$Fast, [switch]$Download)
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

$Configuration = if ($Fast) { 'Fast' } else { 'Release' }
$CargoProfile = $Configuration.ToLowerInvariant()
$Started = Get-Date
$Restart = @()
$Failed = $false
$HasCargo = [bool](Get-Command cargo -ErrorAction SilentlyContinue)
$HasDocker = [bool](Get-Command docker -ErrorAction SilentlyContinue)
# Without any toolchain there is nothing to build with: load the finished build.
$UseDownload = $Download -or (-not $Docker -and -not $HasCargo -and -not $HasDocker)
$UseDocker = -not $UseDownload -and ($Docker -or -not $HasCargo)
$Loaded = $null
$Staging = Join-Path ([IO.Path]::GetTempPath()) ('mogumogu-staging-' + [Guid]::NewGuid().ToString('N'))
try {
    if ($UseDownload) {
        if (-not $Download) { Write-Host 'Weder Rust noch Docker gefunden: mogumogu wird geladen statt gebaut.' -ForegroundColor Yellow }
        if ($Fast -or $Test) { Write-Host '-Fast und -Test gelten nur für einen Build; geladen wird der geprüfte Build.' -ForegroundColor Yellow }
        if ((Get-Command git -ErrorAction SilentlyContinue) -and @(& git -C $Root status --porcelain --untracked-files=no).Count -gt 0) {
            Write-Host 'Lokale Änderungen sind im geladenen Build nicht enthalten; dafür bauen oder erst committen und pushen.' -ForegroundColor Yellow
        }
        # Load first: a running instance is only stopped once there is something to put in its place.
        $Loaded = & (Join-Path $Root 'scripts\download.ps1') -Destination $Staging | Select-Object -Last 1
    }
    elseif ($UseDocker) {
        if (-not $HasDocker) { throw 'Docker nicht gefunden. Docker Desktop installieren oder mit -Download den fertigen Build laden.' }
        & docker info --format '{{.OSType}}' *> $null
        if ($LASTEXITCODE -ne 0) { throw 'Docker läuft nicht. Docker Desktop starten, oder mit -Download den fertigen Build laden.' }
        Write-Host 'mogumogu · Build im Docker-Container (GNU-Toolchain)' -ForegroundColor Cyan
    }
    foreach ($Owner in Get-DistOwners) {
        Write-Host "mogumogu läuft (PID $($Owner.Id)) und wird zum Ersetzen geordnet beendet …" -ForegroundColor Yellow
        $Flags = @(Get-CliFlags $Owner.Arguments)
        & $Cli @Flags --no-start owner stop | Out-Null
        $Restart += $Owner
    }
    $Deadline = (Get-Date).AddSeconds(15)
    while (@(Get-DistOwners).Count -gt 0 -and (Get-Date) -lt $Deadline) { Start-Sleep -Milliseconds 300 }
    $Left = @(Get-DistOwners)
    if ($Left.Count -gt 0) {
        throw "mogumogu aus dist läuft noch (PID $($Left.Id -join ', ')). Bitte über das Tray-Menü beenden und erneut versuchen."
    }

    if ($UseDownload) {
        New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
        Get-ChildItem -LiteralPath $Staging -File | Copy-Item -Destination $OutDir -Force
    }
    elseif ($UseDocker) {
        if ($Test) {
            & docker build --target validated $Root
            if ($LASTEXITCODE -ne 0) { throw "Kernprüfungen im Container fehlgeschlagen (Exit $LASTEXITCODE)." }
        }
        & docker build --target windows-exe --build-arg "BUILD_PROFILE=$CargoProfile" --output "type=local,dest=$OutDir" $Root
        if ($LASTEXITCODE -ne 0) { throw "docker build fehlgeschlagen (Exit $LASTEXITCODE)." }
    }
    else {
        $Build = @{ NoRun = $true; Configuration = $Configuration }
        if (-not $Test) { $Build.SkipTests = $true }
        & (Join-Path $Root 'build.ps1') @Build
    }
}
catch {
    $Failed = $true
    Write-Host ''
    Write-Host "$(if ($UseDownload) { 'Laden' } else { 'Build' }) fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
}
finally {
    Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
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
if ($UseDownload) {
    Write-Host ("Fertig in {0} s, geladen: {1}. Ergebnis:" -f $Seconds, $Loaded) -ForegroundColor Green
}
else {
    $Way = if ($UseDocker) { 'Docker' } else { 'lokal' }
    Write-Host ("Fertig in {0} s ({1}, {2}, {3}). Ergebnis:" -f $Seconds, $Configuration, $Way, $(if ($Test) { 'mit Tests' } else { 'ohne Tests' })) -ForegroundColor Green
}
Write-Output $Exe
Write-Host ("  {0:N1} MB · Stand {1:g}" -f ($Item.Length / 1MB), $Item.LastWriteTime)
Write-Host "  CLI:    $Cli"
Write-Host "  Ordner: $OutDir"
if ($Open) { Start-Process explorer.exe -ArgumentList "`"$OutDir`"" }
