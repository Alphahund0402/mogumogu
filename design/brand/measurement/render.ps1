param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$Repository = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$SavedUi = $env:MOGU_UI_MODULE
Push-Location $Repository
try {
    if (-not $SkipBuild) {
        & cargo build --locked --offline --bins
        if ($LASTEXITCODE -ne 0) { throw 'Native Debug build failed.' }
    }
    $Output = Join-Path $Repository 'target/stroke-qa'
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $Deps = Join-Path $Repository 'target/debug/deps'
    $Slint = Get-ChildItem -LiteralPath $Deps -Filter 'libslint-*.rlib' |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $Ui = Get-ChildItem -LiteralPath 'target/debug/build' -Directory -Filter 'mogumogu-*' |
        ForEach-Object { Get-ChildItem -LiteralPath (Join-Path $_.FullName 'out') -Filter 'app.rs' -ErrorAction SilentlyContinue } |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $Slint -or -not $Ui) { throw 'Build with the default desktop feature first.' }
    $env:MOGU_UI_MODULE = $Ui.FullName
    $CargoCache = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $NativeArgs = @()
    Get-ChildItem -Path (Join-Path $CargoCache 'registry/src/*/windows_x86_64_msvc-*/lib') -Directory |
        ForEach-Object { $NativeArgs += @('-L', "native=$($_.FullName)") }
    & rustc (Join-Path $PSScriptRoot 'render.rs') --edition 2024 -A dead_code `
        --extern "slint=$($Slint.FullName)" -L "dependency=$Deps" @NativeArgs `
        -o (Join-Path $Output 'render.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Measurement fixture compilation failed.' }
    & (Join-Path $Output 'render.exe') $Output
    if ($LASTEXITCODE -ne 0) { throw 'Native measurement render failed.' }
    $SourceFiles = @('assets/brand/mascots-v2.png', 'build.rs', 'Cargo.lock',
        'design/brand/measurement/render.rs') + @(Get-ChildItem -LiteralPath 'ui' -Recurse -File -Filter '*.slint' |
        ForEach-Object { $_.FullName.Substring($Repository.Length + 1).Replace('\', '/') })
    $SourceHashes = [ordered]@{}
    foreach ($File in $SourceFiles) {
        $SourceHashes[$File] = (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $FrameHashes = [ordered]@{}
    Get-ChildItem -LiteralPath $Output -File -Filter '*.ppm' | ForEach-Object {
        $FrameHashes[$_.Name] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    [ordered]@{ sources = $SourceHashes; frames = $FrameHashes;
        generated_ui_sha256 = (Get-FileHash -LiteralPath $Ui.FullName -Algorithm SHA256).Hash.ToLowerInvariant() } |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $Output 'source-manifest.json') -Encoding UTF8
} finally {
    $env:MOGU_UI_MODULE = $SavedUi
    Pop-Location
}
