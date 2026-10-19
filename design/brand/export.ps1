# Artwork preparation only; not part of the application build or runtime.
# Uses the SVG/image dependency already cached by a native Debug build.
$ErrorActionPreference = 'Stop'
$BrandProject = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$BrandSource = Join-Path $BrandProject 'design\variants\manga\01-mochi-cloud.png'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $BrandSource).Hash -ne '59A2F8CBD92029CF12E8FAAF5BA218286A8B6313B8380283A8A275BE4643F6FE') {
    throw 'The approved Mochi source changed. Review the source and crop coordinates before exporting.'
}
$BrandDeps = Join-Path $BrandProject 'target\debug\deps'
$BrandRenderer = Get-ChildItem -LiteralPath $BrandDeps -Filter 'libresvg-*.rlib' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $BrandRenderer) { throw 'Build the native app in Debug first to cache its image renderer.' }
$BrandExporter = Join-Path $BrandProject 'target\export-brand.exe'
& rustc (Join-Path $PSScriptRoot 'export.rs') --edition 2024 --extern "resvg=$($BrandRenderer.FullName)" -L "dependency=$BrandDeps" -o $BrandExporter
if ($LASTEXITCODE -ne 0) { throw 'Brand exporter compilation failed.' }
& $BrandExporter $BrandProject
if ($LASTEXITCODE -ne 0) { throw 'Brand export failed.' }
