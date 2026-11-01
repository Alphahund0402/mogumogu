#requires -Version 5.1
<#
Lädt fertige Windows-Programme, statt sie zu bauen: für Rechner, auf denen
Rust oder Docker zu schwer sind. Zuerst der CI-Build genau dieses Commits
(GitHub Actions; braucht die angemeldete GitHub-CLI `gh`), sonst das neueste
Release (ohne Anmeldung, SHA-256 geprüft). Beide sind nicht signiert.
Geschrieben wird nur nach -Destination. Das Skript verbindet sich mit GitHub;
mogumogu selbst stellt keine Netzwerkverbindungen her.

  .\scripts\download.ps1                   CI-Build des aktuellen Commits, sonst das neueste Release
  .\scripts\download.ps1 -Commit 1a2b3c4   nur der CI-Build dieses Commits
  .\scripts\download.ps1 -Release          nur das neueste Release

Eine laufende Instanz aus dem Zielordner vorher beenden; rebuild.ps1 -Download
erledigt das selbst.
#>
[CmdletBinding()]
param(
    [string]$Destination = '',
    [string]$Commit = '',
    [switch]$Release,
    [string]$Repository = ''
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Invoke-WebRequest in Windows PowerShell is many times slower with a progress bar.
$ProgressPreference = 'SilentlyContinue'
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Destination) { $Destination = Join-Path $Root 'dist\mogumogu' }

# Native tools write diagnostics to stderr; in Windows PowerShell that must not
# stop the script, only the exit code counts.
function Invoke-Native([scriptblock]$Command) {
    $Saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $Output = & $Command 2>$null
        [pscustomobject]@{ Ok = ($LASTEXITCODE -eq 0); Text = ("$Output").Trim() }
    }
    finally { $ErrorActionPreference = $Saved }
}

$HasGit = [bool](Get-Command git -ErrorAction SilentlyContinue)
if (-not $Repository) {
    $Origin = if ($HasGit) { (Invoke-Native { git -C $Root remote get-url origin }).Text } else { '' }
    $Repository = if ($Origin -match 'github\.com[:/]([^/]+/[^/]+?)(\.git)?$') { $Matches[1] } else { 'Alphahund0402/mogumogu' }
}
$Work = Join-Path ([IO.Path]::GetTempPath()) ('mogumogu-download-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Work | Out-Null
$Folder = $null
$Source = $null
$BuiltFrom = $null
$Reasons = @()

# The commit a build was made from, as its BUILDINFO.txt names it.
function Get-BuiltFrom([string]$Path) {
    $Info = Join-Path $Path 'BUILDINFO.txt'
    if (-not (Test-Path -LiteralPath $Info)) { return $null }
    $Line = Select-String -LiteralPath $Info -Pattern '^Quellstand: ([0-9a-f]{40})' | Select-Object -First 1
    if ($Line) { $Line.Matches[0].Groups[1].Value } else { $null }
}

function Get-CiBuild([string]$Sha) {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { return 'Die GitHub-CLI fehlt (winget install GitHub.cli).' }
    if (-not (Invoke-Native { gh auth status }).Ok) { return 'Die GitHub-CLI ist nicht angemeldet (gh auth login).' }
    $Run = Invoke-Native { gh run list --repo $Repository --workflow Windows --commit $Sha --status success --limit 1 --json databaseId --jq '.[0].databaseId' }
    if (-not $Run.Ok -or -not $Run.Text) { return "Kein erfolgreicher CI-Build für $($Sha.Substring(0, 7)): Commit gepusht und Windows-Workflow grün?" }
    $Target = Join-Path $Work 'ci'
    if (-not (Invoke-Native { gh run download $Run.Text --repo $Repository --name mogumogu-windows --dir $Target }).Ok) {
        return "CI-Lauf $($Run.Text) ließ sich nicht laden; Artefakte bleiben 90 Tage."
    }
    $Exe = Get-ChildItem -LiteralPath $Target -Recurse -Filter 'mogumogu.exe' | Select-Object -First 1
    if (-not $Exe) { return "CI-Lauf $($Run.Text) enthält kein mogumogu.exe." }
    $From = Get-BuiltFrom $Exe.DirectoryName
    if ($From -and $From -ne $Sha) { return "CI-Lauf $($Run.Text) nennt den Quellstand $From statt $Sha." }
    $script:Folder = $Exe.DirectoryName
    $script:Source = "CI-Build des Commits $($Sha.Substring(0, 7)) (GitHub-Actions-Lauf $($Run.Text))"
    $script:BuiltFrom = $Sha
    return $null
}

function Get-ReleaseBuild {
    # Windows PowerShell may still offer older TLS versions first.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $Headers = @{ 'User-Agent' = 'mogumogu-download'; 'Accept' = 'application/vnd.github+json' }
    try { $Latest = Invoke-RestMethod -UseBasicParsing -Headers $Headers -Uri "https://api.github.com/repos/$Repository/releases/latest" }
    catch { return "Kein Release unter github.com/$Repository/releases." }
    $Zip = @($Latest.assets | Where-Object { $_.name -like 'mogumogu-*.zip' }) | Select-Object -First 1
    if (-not $Zip) { return "Release $($Latest.tag_name) enthält kein Programmpaket." }
    $Sum = @($Latest.assets | Where-Object { $_.name -eq "$($Zip.name).sha256" }) | Select-Object -First 1
    if (-not $Sum) { return "Release $($Latest.tag_name) enthält keine Prüfsumme; es wird nicht verwendet." }
    $ZipFile = Join-Path $Work $Zip.name
    $SumFile = Join-Path $Work $Sum.name
    Invoke-WebRequest -UseBasicParsing -Headers $Headers -Uri $Zip.browser_download_url -OutFile $ZipFile
    Invoke-WebRequest -UseBasicParsing -Headers $Headers -Uri $Sum.browser_download_url -OutFile $SumFile
    $Expected = ((Get-Content -LiteralPath $SumFile -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $ZipFile).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) { throw "Prüfsumme von $($Zip.name) stimmt nicht; nichts wurde ersetzt." }
    $Target = Join-Path $Work 'release'
    Expand-Archive -LiteralPath $ZipFile -DestinationPath $Target
    if (-not (Test-Path -LiteralPath (Join-Path $Target 'mogumogu.exe'))) { return "$($Zip.name) enthält kein mogumogu.exe." }
    $script:Folder = $Target
    $script:Source = "Release $($Latest.tag_name) (SHA-256 geprüft)"
    $script:BuiltFrom = Get-BuiltFrom $Target
    return $null
}

try {
    if (-not $Release) {
        $Sha = $Commit
        if ($HasGit) {
            $Rev = if ($Sha) { $Sha } else { 'HEAD' }
            $Resolved = Invoke-Native { git -C $Root rev-parse --verify "$Rev^{commit}" }
            if ($Resolved.Ok) { $Sha = $Resolved.Text }
        }
        if ($Sha -match '^[0-9a-f]{40}$') {
            $Why = Get-CiBuild $Sha
            if ($Why) { $Reasons += $Why }
        }
        else { $Reasons += 'Kein vollständiger Commit bekannt.' }
        if ($Commit -and -not $Folder) { throw "Kein CI-Build für $Commit. $($Reasons -join ' ')" }
    }
    if (-not $Folder) {
        if ($Reasons.Count -gt 0) { Write-Host "Kein passender CI-Build: $($Reasons -join ' ') Lade das neueste Release." -ForegroundColor Yellow }
        $Why = Get-ReleaseBuild
        if ($Why) { throw "Nichts zu laden. $(@($Reasons + $Why) -join ' ')" }
    }
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    try { Get-ChildItem -LiteralPath $Folder -File | Copy-Item -Destination $Destination -Force }
    catch { throw "Konnte nicht nach $Destination schreiben. Läuft mogumogu von dort? Über das Tray beenden oder rebuild.ps1 -Download verwenden. ($($_.Exception.Message))" }
    $Head = if ($HasGit) { (Invoke-Native { git -C $Root rev-parse HEAD }).Text } else { '' }
    if ($BuiltFrom -and $Head -and $BuiltFrom -ne $Head) {
        Write-Host "Hinweis: Der Build stammt von $($BuiltFrom.Substring(0, 7)), die Arbeitskopie steht auf $($Head.Substring(0, 7))." -ForegroundColor Yellow
    }
    Write-Host "Geladen: $Source" -ForegroundColor Green
    Write-Output $Source
}
finally {
    Remove-Item -LiteralPath $Work -Recurse -Force -ErrorAction SilentlyContinue
}
