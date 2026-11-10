@echo off
rem Double-click to open the installer window. With arguments, such as
rem -Quiet or -InstallDir <folder>, it installs from the console instead.
if "%~1"=="" (
    start "" powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "%~dp0install.ps1" -Gui
    exit /b 0
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
