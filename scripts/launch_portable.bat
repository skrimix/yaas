@echo off
setlocal

set "BASEDIR=%~dp0"
cd /d "%BASEDIR%"

start "" "%BASEDIR%yaas.exe" --portable

endlocal

