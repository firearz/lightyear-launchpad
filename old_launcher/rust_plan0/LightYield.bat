@echo off
title Light Yield Launcher
cd /d "%~dp0"
if exist "%~dp0lightyield.exe" (
    "%~dp0lightyield.exe" %*
) else if exist "%~dp0dist\lightyield.exe" (
    "%~dp0dist\lightyield.exe" %*
) else (
    cargo run --release -p lightyield-cli -- %*
)
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [Light Yield Launcher exited with code %ERRORLEVEL%]
    pause
)
