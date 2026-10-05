#requires -Version 5.1
<#
Creates a versioned release ZIP from a clean, tested build (F-27):
executables, license texts, Cargo.lock, dependency/license inventory,
source commit and SHA-256 checksums. Signing is a separate, later decision.

  .\scripts\release.ps1 [-AllowDirty]
#>
[CmdletBinding()]
param([switch]$AllowDirty)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $Root
try {
    $Dirty = @(& git status --porcelain)
    if ($Dirty.Count -gt 0 -and -not $AllowDirty) { throw 'Arbeitskopie hat Änderungen: Release nur aus einem sauberen, committeten Stand.' }
    $Commit = (& git rev-parse HEAD).Trim()
    $Version = ((Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value)
    & (Join-Path $Root 'build.ps1') -NoRun
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) { throw 'Build fehlgeschlagen.' }

    $Stage = Join-Path $Root "dist\mogumogu-$Version"
    Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
    Copy-Item -LiteralPath (Join-Path $Root 'dist\mogumogu') -Destination $Stage -Recurse

    # Dependency and license inventory from the locked graph (not an SBOM standard).
    $Metadata = & cargo metadata --locked --format-version 1 | ConvertFrom-Json
    $Packages = $Metadata.packages | Sort-Object name, version | ForEach-Object {
        [pscustomobject]@{ name = $_.name; version = $_.version; license = $_.license; source = $_.source }
    }
    $Packages | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $Stage 'dependencies.json')
    $Packages | Group-Object license | Sort-Object Count -Descending |
        ForEach-Object { "{0,5}  {1}" -f $_.Count, $_.Name } | Set-Content -Encoding UTF8 (Join-Path $Stage 'licenses.txt')
    @(
        "mogumogu $Version",
        "Quellstand: $Commit",
        "Rust: $(& rustc --version)",
        "Erstellt: $((Get-Date).ToString('s'))",
        'Quellcode: GPL-3.0-only. Der zugehörige Quellstand ist der oben genannte Commit.'
    ) | Set-Content -Encoding UTF8 (Join-Path $Stage 'BUILDINFO.txt')

    $Sums = Get-ChildItem -LiteralPath $Stage -File | Sort-Object Name | ForEach-Object {
        "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLower(), $_.Name
    }
    $Sums | Set-Content -Encoding ASCII (Join-Path $Stage 'SHA256SUMS')
    $Zip = "$Stage.zip"
    Remove-Item -LiteralPath $Zip -Force -ErrorAction SilentlyContinue
    Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath $Zip
    "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 -LiteralPath $Zip).Hash.ToLower(), (Split-Path -Leaf $Zip) |
        Set-Content -Encoding ASCII "$Zip.sha256"
    Write-Host "Release-Paket: $Zip" -ForegroundColor Green
}
finally { Pop-Location }
