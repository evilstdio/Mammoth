@echo off
rem Runs install.ps1 from this folder. The setup program starts this file.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
