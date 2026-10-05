@echo off
rem ExecutionPolicy applies to this PowerShell process only, not machine policy.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build.ps1" %*
if errorlevel 1 (
  echo.
  echo Build failed. Read the diagnostic above and README.md.
  pause
  exit /b 1
)
