# -*- mode: python ; coding: utf-8 -*-
"""
PyInstaller build spec for Lightyear.

Usage:
    pyinstaller lightyear.spec                      # single-file  dist/Lightyear.exe
    set LIGHTYEAR_ONEDIR=1 && pyinstaller lightyear.spec   # portable folder dist/Lightyear/
"""
import os

from PyInstaller.utils.hooks import collect_data_files

ONEDIR = os.environ.get("LIGHTYEAR_ONEDIR") == "1"

datas = [
    ("assets", "assets"),
    ("icons", "icons"),
]
datas += collect_data_files("minecraft_launcher_lib")

# Trim Qt modules Lightyear never uses to keep the binary small.
excludes = [
    "tkinter",
    "PySide6.Qt3DAnimation", "PySide6.Qt3DCore", "PySide6.Qt3DExtras",
    "PySide6.Qt3DInput", "PySide6.Qt3DLogic", "PySide6.Qt3DRender",
    "PySide6.QtBluetooth", "PySide6.QtCharts", "PySide6.QtDataVisualization",
    "PySide6.QtGraphs", "PySide6.QtLocation", "PySide6.QtMultimedia",
    "PySide6.QtMultimediaWidgets", "PySide6.QtNfc", "PySide6.QtPdf",
    "PySide6.QtPdfWidgets", "PySide6.QtPositioning", "PySide6.QtQml",
    "PySide6.QtQuick", "PySide6.QtQuick3D", "PySide6.QtQuickWidgets",
    "PySide6.QtRemoteObjects", "PySide6.QtSensors", "PySide6.QtSerialPort",
    "PySide6.QtSpatialAudio", "PySide6.QtSql", "PySide6.QtTest",
    "PySide6.QtTextToSpeech", "PySide6.QtWebChannel", "PySide6.QtWebEngineCore",
    "PySide6.QtWebEngineWidgets", "PySide6.QtWebSockets", "PySide6.QtWebView",
]

a = Analysis(
    ["main.py"],
    pathex=[],
    binaries=[],
    datas=datas,
    hiddenimports=[],
    hookspath=[],
    runtime_hooks=[],
    excludes=excludes,
    noarchive=False,
)
pyz = PYZ(a.pure)

common = dict(
    name="Lightyear",
    icon="assets/logo_circle.ico",
    console=False,
    debug=False,
    strip=False,
    upx=False,
)

if ONEDIR:
    exe = EXE(pyz, a.scripts, [], exclude_binaries=True, **common)
    coll = COLLECT(exe, a.binaries, a.datas, strip=False, upx=False, name="Lightyear")
else:
    exe = EXE(pyz, a.scripts, a.binaries, a.datas, [], runtime_tmpdir=None, **common)
