@echo off
rem Doppelklick: startet mogumogu und oeffnet das Dashboard. Fehlt das Programm,
rem wird vorher das neueste Release geladen (SHA-256 geprueft, nicht signiert).
rem Die ExecutionPolicy gilt nur fuer diesen PowerShell-Prozess.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" -ShowDashboard %*
if errorlevel 1 (
  echo.
  echo Start fehlgeschlagen. Bitte die Meldung oben lesen.
  pause
  exit /b 1
)
