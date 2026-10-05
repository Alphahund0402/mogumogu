#requires -Version 5.1
<#
Resource measurement of the tray owner (F-04, F-25; PROJEKTPLAN §12.1).

Measures the *built* release executable with a separate, throw-away data
directory: tray idle before the first window, the open dashboard, and the
state after N open/close cycles. Results are written as JSON and Markdown.
These are measurements of this machine only, never general promises.

  .\scripts\measure.ps1 [-Cycles 100] [-IdleSeconds 60] [-Mode Demo|Local] [-Out docs\messungen]
#>
[CmdletBinding()]
param(
    [int]$Cycles = 100,
    [int]$IdleSeconds = 60,
    [ValidateSet('Demo','Local')][string]$Mode = 'Demo',
    [string]$Out = ''
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Out) { $Out = Join-Path $Root 'docs\messungen' }
$Exe = Join-Path $Root 'dist\mogumogu\mogumogu.exe'
$Cli = Join-Path $Root 'dist\mogumogu\mogumogu-cli.exe'
if (-not (Test-Path -LiteralPath $Exe)) { throw 'Zuerst bauen: .\build.ps1 -NoRun' }
$Data = Join-Path ([IO.Path]::GetTempPath()) ("mogumogu-measure-" + [Guid]::NewGuid().ToString('N'))
$ModeArgs = @(); if ($Mode -eq 'Demo') { $ModeArgs = @('--demo') }

function Sample([Diagnostics.Process]$Process, [string]$Label) {
    $Process.Refresh()
    [pscustomobject]@{
        phase          = $Label
        private_mib    = [math]::Round($Process.PrivateMemorySize64 / 1MB, 1)
        working_set_mib = [math]::Round($Process.WorkingSet64 / 1MB, 1)
        handles        = $Process.HandleCount
        threads        = $Process.Threads.Count
        cpu_seconds    = [math]::Round($Process.TotalProcessorTime.TotalSeconds, 3)
    }
}
function Invoke-Cli([string[]]$Arguments) {
    & $Cli --data-dir $Data @ModeArgs --no-start @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "CLI fehlgeschlagen: $Arguments" }
}

$Process = Start-Process -FilePath $Exe -ArgumentList (@('--tray', '--data-dir', $Data) + $ModeArgs) -PassThru
try {
    Start-Sleep -Seconds 5
    $Samples = @(Sample $Process 'tray_settled')
    $CpuBefore = $Process.TotalProcessorTime.TotalSeconds
    Start-Sleep -Seconds $IdleSeconds
    $Process.Refresh()
    $IdleCpuPercent = [math]::Round(100 * ($Process.TotalProcessorTime.TotalSeconds - $CpuBefore) / $IdleSeconds, 3)
    $Samples += Sample $Process 'tray_idle'
    Invoke-Cli @('dashboard'); Start-Sleep -Seconds 2
    $Samples += Sample $Process 'dashboard_open'
    $Open = New-Object Collections.Generic.List[double]
    for ($i = 1; $i -le $Cycles; $i++) {
        Invoke-Cli @('dashboard', 'hide'); Start-Sleep -Milliseconds 150
        $Watch = [Diagnostics.Stopwatch]::StartNew()
        Invoke-Cli @('dashboard'); $Open.Add($Watch.Elapsed.TotalMilliseconds)
        Start-Sleep -Milliseconds 150
        if ($i -eq 10 -or $i -eq 50) {
            Invoke-Cli @('dashboard', 'hide'); Start-Sleep -Seconds 3
            $Samples += Sample $Process "tray_after_${i}_cycles"
        }
    }
    Invoke-Cli @('dashboard', 'hide'); Start-Sleep -Seconds 3
    $Samples += Sample $Process "tray_after_${Cycles}_cycles"
    Invoke-Cli @('owner', 'stop')
    $Sorted = $Open | Sort-Object
    $P95 = if ($Sorted.Count) { [math]::Round($Sorted[[math]::Min($Sorted.Count - 1, [int][math]::Ceiling(0.95 * $Sorted.Count) - 1)], 1) } else { $null }
    $Os = Get-CimInstance Win32_OperatingSystem
    $Cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    $Report = [pscustomobject]@{
        measured_at       = (Get-Date).ToString('s')
        windows           = "$($Os.Caption) $($Os.Version) (Build $($Os.BuildNumber))"
        cpu               = $Cpu
        memory_gib        = [math]::Round($Os.TotalVisibleMemorySize / 1MB, 1)
        build             = 'release (dist\mogumogu)'
        mode              = $Mode
        cycles            = $Cycles
        idle_seconds      = $IdleSeconds
        idle_cpu_percent_of_one_core = $IdleCpuPercent
        show_request_p95_ms = $P95
        note              = 'show_request misst die IPC-Anfrage bis zur Bestätigung, nicht das vollständige Zeichnen.'
        samples           = $Samples
    }
    New-Item -ItemType Directory -Force -Path $Out | Out-Null
    $Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $Report | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $Out "messung-$Stamp.json")
    $Lines = @("# Messung $Stamp", '', "Windows: $($Report.windows) · CPU: $Cpu · RAM: $($Report.memory_gib) GiB · Modus: $Mode", '',
        '| Phase | Private MiB | Working Set MiB | Handles | Threads | CPU s |', '|---|---|---|---|---|---|')
    foreach ($S in $Samples) { $Lines += "| $($S.phase) | $($S.private_mib) | $($S.working_set_mib) | $($S.handles) | $($S.threads) | $($S.cpu_seconds) |" }
    $Lines += '', "Leerlauf-CPU über $IdleSeconds s: $IdleCpuPercent % eines Kerns · Anzeige-Anfrage p95: $P95 ms ($Cycles Zyklen)."
    $Lines | Set-Content -Encoding UTF8 (Join-Path $Out "messung-$Stamp.md")
    $Report | Format-List
} finally {
    if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force }
    Remove-Item -LiteralPath $Data -Recurse -Force -ErrorAction SilentlyContinue
}
