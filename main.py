"""
Lightyear Launcher — Entry Point
Minimalist flat frosted glass edition.
"""

from __future__ import annotations

import ctypes
import os
import sys

from PySide6.QtCore import Qt
from PySide6.QtGui import QFont, QFontDatabase, QGuiApplication
from PySide6.QtWidgets import QApplication

from lightyear.core import FONTS_DIR
from lightyear.main_window import LightyearMainWindow
from lightyear.ui_components import make_circular_logo_icon


def hide_terminal_window() -> None:
    """Hides the Windows console/terminal window if one is attached to this process."""
    if os.name == "nt":
        try:
            hwnd = ctypes.windll.kernel32.GetConsoleWindow()
            if hwnd:
                ctypes.windll.user32.ShowWindow(hwnd, 0)  # SW_HIDE = 0
        except Exception:
            pass


def set_windows_app_user_model_id() -> None:
    """
    Sets an explicit AppUserModelID on Windows so the taskbar uses Lightyear's
    circular logo icon instead of grouping under python.exe / pythonw.exe.
    """
    if os.name == "nt":
        try:
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(
                "Lightyear.MinecraftLauncher.1"
            )
        except Exception:
            pass


def load_custom_fonts() -> tuple[str, str]:
    """
    Loads the bundled Playfair Display and Lato TTF fonts into QFontDatabase
    and returns `(playfair_family_name, lato_family_name)`.
    """
    playfair_family = "Playfair Display"
    lato_family = "Lato"

    if FONTS_DIR.exists():
        pf_path = FONTS_DIR / "PlayfairDisplay.ttf"
        if pf_path.exists():
            fid = QFontDatabase.addApplicationFont(str(pf_path))
            if fid != -1:
                fams = QFontDatabase.applicationFontFamilies(fid)
                if fams:
                    playfair_family = fams[0]

        for lato_file in ("Lato-Regular.ttf", "Lato-Bold.ttf", "Lato-Black.ttf"):
            lp = FONTS_DIR / lato_file
            if lp.exists():
                fid = QFontDatabase.addApplicationFont(str(lp))
                if fid != -1:
                    fams = QFontDatabase.applicationFontFamilies(fid)
                    if fams:
                        lato_family = fams[0]

    return (playfair_family, lato_family)


def main() -> int:
    hide_terminal_window()
    set_windows_app_user_model_id()

    QGuiApplication.setHighDpiScaleFactorRoundingPolicy(
        Qt.HighDpiScaleFactorRoundingPolicy.PassThrough
    )
    app = QApplication(sys.argv)
    app.setApplicationName("lightyear")
    app.setOrganizationName("Lightyear")
    app.setWindowIcon(make_circular_logo_icon())

    playfair_family, lato_family = load_custom_fonts()
    app.setFont(QFont(lato_family, 10))

    window = LightyearMainWindow(playfair_family=playfair_family, lato_family=lato_family)

    # Center window on primary screen in strict windowed mode
    screen = app.primaryScreen()
    if screen is not None:
        avail = screen.availableGeometry()
        x = avail.x() + (avail.width() - window.width()) // 2
        y = avail.y() + (avail.height() - window.height()) // 2
        window.move(x, y)

    window.showNormal()
    window.raise_()
    window.activateWindow()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
