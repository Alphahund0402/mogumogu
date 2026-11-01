#requires -Version 5.1
<#
Resource measurement of the tray owner (F-04, F-25; PROJEKTPLAN §12.1).

Measures the *built* release executable with a separate, throw-away data
directory: tray idle before the first window, the open dashboard, and the
state after N open/close cycles, and the CPU time of full-window redraws (a
dialog dims the whole window; the software renderer draws every pixel on the
CPU). Results are written as JSON and Markdown. These are measurements of
this machine only, never general promises.

  .\scripts\measure.ps1 [-Cycles 100] [-IdleSeconds 60] [-Redraws 20] [-Mode Demo|Local] [-Out docs\messungen]
#>
[CmdletBinding()]
param(
    [int]$Cycles = 100,
    [int]$IdleSeconds = 60,
    [int]$Redraws = 20,
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
$MeasurementRoot = [IO.Path]::GetFullPath((Join-Path $Root 'target\measurements'))
New-Item -ItemType Directory -Force -Path $MeasurementRoot | Out-Null
$Data = Join-Path $MeasurementRoot ("data-" + [Guid]::NewGuid().ToString('N'))
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
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MeasureWindow {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
'@
# Per-monitor aware: without it Windows reports scaled-down sizes, and the
# software renderer's cost follows physical pixels.
[MeasureWindow]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
# A key press posted to this window only; nothing else on the desktop sees it.
function Send-Key([IntPtr]$Window, [int]$Key) {
    [MeasureWindow]::PostMessage($Window, 0x100, [IntPtr]$Key, [IntPtr]0) | Out-Null
    [MeasureWindow]::PostMessage($Window, 0x101, [IntPtr]$Key, [IntPtr]0xC0000001) | Out-Null
}
function Invoke-Cli([string[]]$Arguments) {
    & $Cli --data-dir $Data @ModeArgs --no-start @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "CLI fehlgeschlagen: $Arguments" }
}

$Process = Start-Process -FilePath $Exe -ArgumentList (@('--tray', '--data-dir', ('"' + $Data + '"')) + $ModeArgs) -WindowStyle Hidden -PassThru
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
    # F1 opens the help dialog (the window dims, the card lifts in), Esc closes
    # it: about 25 frames of the whole window per cycle.
    $Process.Refresh()
    $Window = $Process.MainWindowHandle
    $RedrawCpuMs = $null; $Pixels = $null
    if ($Window -ne [IntPtr]::Zero -and $Redraws -gt 0) {
        $Rect = New-Object MeasureWindow+RECT
        [MeasureWindow]::GetClientRect($Window, [ref]$Rect) | Out-Null
        $Pixels = "$($Rect.Right - $Rect.Left) x $($Rect.Bottom - $Rect.Top) Pixel bei $([math]::Round(100 * [MeasureWindow]::GetDpiForWindow($Window) / 96)) %"
        $CpuStart = $Process.TotalProcessorTime.TotalMilliseconds
        for ($i = 0; $i -lt $Redraws; $i++) {
            Send-Key $Window 0x70; Start-Sleep -Milliseconds 450
            Send-Key $Window 0x1B; Start-Sleep -Milliseconds 450
        }
        $Process.Refresh()
        $RedrawCpuMs = [math]::Round(($Process.TotalProcessorTime.TotalMilliseconds - $CpuStart) / $Redraws, 1)
    }
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
        window_pixels     = $Pixels
        redraw_cpu_ms_per_dialog_cycle = $RedrawCpuMs
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
    if ($RedrawCpuMs) { $Lines += "Ganzes Fenster neu zeichnen ($Pixels): $RedrawCpuMs ms CPU je Dialogzyklus (Öffnen und Schließen, $Redraws Zyklen)." }
    $Lines | Set-Content -Encoding UTF8 (Join-Path $Out "messung-$Stamp.md")
    $Report | Format-List
} finally {
    if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force }
    # Only this run's generated fixture may be removed, never a supplied path.
    $ResolvedData = [IO.Path]::GetFullPath($Data)
    $ExpectedPrefix = $MeasurementRoot.TrimEnd('\') + '\'
    if (-not $ResolvedData.StartsWith($ExpectedPrefix, [StringComparison]::OrdinalIgnoreCase) -or
        (Split-Path -Parent $ResolvedData) -ne $MeasurementRoot -or
        (Split-Path -Leaf $ResolvedData) -notmatch '^data-[a-f0-9]{32}$') {
        throw 'Messdatenpfad liegt außerhalb der erzeugten Testwurzel; wird nicht entfernt.'
    }
    if (Test-Path -LiteralPath $ResolvedData) {
        $DataItem = Get-Item -LiteralPath $ResolvedData -Force
        if (($DataItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Messdatenwurzel wurde durch einen Link ersetzt; wird nicht entfernt.'
        }
        Remove-Item -LiteralPath $ResolvedData -Recurse -Force
    }
}
