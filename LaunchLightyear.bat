@echo off
cd /d "%~dp0"
where pythonw.exe >nul 2>nul
if %ERRORLEVEL% EQU 0 (
    start "" /B pythonw.exe "%~dp0main.py"
) else (
    start "" /B python.exe "%~dp0main.py"
)
exit
