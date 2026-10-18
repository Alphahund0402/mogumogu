param([switch]$SkipBuild)
# Native screen renders with synthetic data (see render.rs). Output: target/screens.
$ErrorActionPreference = 'Stop'
$Repository = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$SavedUi = $env:MOGU_UI_MODULE
$SavedVersion = $env:CARGO_PKG_VERSION
Push-Location $Repository
try {
    if (-not $SkipBuild) {
        & cargo build --locked --offline --bins
        if ($LASTEXITCODE -ne 0) { throw 'Native Debug build failed.' }
    }
    $Output = Join-Path $Repository 'target/screens'
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $Deps = Join-Path $Repository 'target/debug/deps'
    $Newest = { param($Filter) Get-ChildItem -LiteralPath $Deps -Filter $Filter | Sort-Object LastWriteTime -Descending | Select-Object -First 1 }
    $Slint = & $Newest 'libslint-*.rlib'
    $Core = & $Newest 'libmogumogu-*.rlib'
    $Json = & $Newest 'libserde_json-*.rlib'
    $Ui = Get-ChildItem -LiteralPath 'target/debug/build' -Directory -Filter 'mogumogu-*' |
        ForEach-Object { Get-ChildItem -LiteralPath (Join-Path $_.FullName 'out') -Filter 'app.rs' -ErrorAction SilentlyContinue } |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $Slint -or -not $Core -or -not $Json -or -not $Ui) { throw 'Build with the default desktop feature first.' }
    $env:MOGU_UI_MODULE = $Ui.FullName
    $env:CARGO_PKG_VERSION = ((Select-String -LiteralPath 'Cargo.toml' -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value)
    $CargoCache = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $NativeArgs = @()
    Get-ChildItem -Path (Join-Path $CargoCache 'registry/src/*/windows_x86_64_msvc-*/lib') -Directory |
        ForEach-Object { $NativeArgs += @('-L', "native=$($_.FullName)") }
    & rustc (Join-Path $PSScriptRoot 'render.rs') --edition 2024 -A dead_code `
        --extern "slint=$($Slint.FullName)" --extern "mogumogu=$($Core.FullName)" --extern "serde_json=$($Json.FullName)" -L "dependency=$Deps" @NativeArgs `
        -o (Join-Path $Output 'render.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Screen renderer compilation failed.' }
    & (Join-Path $Output 'render.exe') $Output
    if ($LASTEXITCODE -ne 0) { throw 'Native screen render failed.' }
    if (Get-Command py -ErrorAction SilentlyContinue) { & py -3 (Join-Path $PSScriptRoot 'to_png.py') $Output }
    else { & python (Join-Path $PSScriptRoot 'to_png.py') $Output }
    if ($LASTEXITCODE -ne 0) { throw 'PNG conversion failed.' }
} finally {
    $env:MOGU_UI_MODULE = $SavedUi
    $env:CARGO_PKG_VERSION = $SavedVersion
    Pop-Location
}
