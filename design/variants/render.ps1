# Preview tooling only. Uses Slint's cached SVG renderer; no new dependency.
$ErrorActionPreference = 'Stop'
$VariantRoot = $PSScriptRoot
$ProjectRoot = (Resolve-Path (Join-Path $VariantRoot '..\..')).Path
$VariantDeps = Join-Path $ProjectRoot 'target\debug\deps'
$VariantRenderer = Join-Path $ProjectRoot 'target\design-variant-render.exe'
$VariantResvg = Get-ChildItem -LiteralPath $VariantDeps -Filter 'libresvg-*.rlib' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $VariantResvg) { throw 'First build the native application in Debug configuration to cache its SVG renderer.' }
& rustc (Join-Path $VariantRoot 'render.rs') --edition 2024 --extern "resvg=$($VariantResvg.FullName)" -L "dependency=$VariantDeps" -o $VariantRenderer
if ($LASTEXITCODE -ne 0) { throw 'Preview renderer compilation failed.' }
$VariantImages = @(Get-ChildItem -LiteralPath $VariantRoot -Filter '*.svg' | Sort-Object Name | ForEach-Object FullName)
& $VariantRenderer @VariantImages
if ($LASTEXITCODE -ne 0) { throw 'Preview rendering failed.' }
