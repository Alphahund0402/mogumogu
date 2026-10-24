@echo off
rem Doppelklick: baut dist\mogumogu\mogumogu.exe neu und zeigt den Pfad.
rem Die ExecutionPolicy gilt nur fuer diesen PowerShell-Prozess.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0rebuild.ps1" %*
set "code=%errorlevel%"
rem Aus dem Explorer gestartet: Fenster offen lassen, bis der Pfad gelesen ist.
set "explorer="
for %%A in (%cmdcmdline%) do if /i "%%~A"=="/c" set "explorer=1"
if defined explorer pause
exit /b %code%
