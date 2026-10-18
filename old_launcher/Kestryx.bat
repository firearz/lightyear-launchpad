@echo off
title Kestryx Launcher
cd /d "%~dp0"
if exist "dist\kestryx.exe" (
    start "" "dist\kestryx.exe"
) else if exist "kestryx.exe" (
    start "" "kestryx.exe"
) else (
    echo [ERROR] kestryx.exe not found in this folder or dist\
    pause
)
