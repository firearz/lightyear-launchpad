"""
Android 17 Flat Frosted Glass + Apple Minimalist UI Components for Lightyear Launcher.
Zero outlines, zero drop shadows, flat translucent frosted glass, and custom vector iconography.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Callable

from PIL import Image, ImageFilter
from PySide6.QtCore import (
    QEvent,
    QPoint,
    QPointF,
    QRect,
    QRectF,
    QSize,
    Qt,
    QTimer,
    QUrl,
    Signal,
)
from PySide6.QtGui import (
    QColor,
    QCursor,
    QDesktopServices,
    QFont,
    QIcon,
    QImage,
    QPainter,
    QPainterPath,
    QPen,
    QPixmap,
    QPolygonF,
)
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QDialog,
    QFileDialog,
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSpinBox,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from .core import (
    ASSETS_DIR,
    PROJECT_ROOT,
    InstalledGameVersion,
    InstanceConfig,
    LauncherStore,
    VersionInfo,
    check_java_compatibility,
    decide_download_strategy,
    format_last_played,
    get_java_download_url,
    get_logo_path,
    inspect_java_major,
    sanitize_username,
)


# ---------------------------------------------------------------------------
# Backdrop Manager (High-Radius Gaussian-Blurred Flat Frosted Glass Engine)
# ---------------------------------------------------------------------------

class BackdropManager:
    """
    Holds both the sharp sky/cloudscape background (`backgroudn1.jpg`) and a
    creamy Gaussian-blurred version (`radius=48` at screen scale) scaled to the
    current window geometry so any `FrostedGlassPanel` can sample its exact region.
    """

    def __init__(self, bg_path: Path) -> None:
        self.bg_path = bg_path
        self._sharp_qimage: QImage | None = None
        self._blurred_qimage: QImage | None = None

        self._scaled_sharp: QPixmap | None = None
        self._scaled_blurred: QPixmap | None = None
        self._last_size: QSize = QSize(0, 0)

        self._load_images()

    def _load_images(self) -> None:
        if self.bg_path.exists():
            pil_img = Image.open(self.bg_path).convert("RGBA")
        else:
            pil_img = Image.new("RGBA", (1920, 1080), (14, 28, 52, 255))

        # Pre-scale to 1280x800 before blurring so radius=48 produces a deep,
        # creamy Android 17 / visionOS frosted glass blur at window resolution.
        work_img = pil_img.resize((1280, 800), Image.Resampling.LANCZOS)
        blurred_img = work_img.filter(ImageFilter.GaussianBlur(radius=48))

        self._sharp_qimage = self._pil_to_qimage(work_img)
        self._blurred_qimage = self._pil_to_qimage(blurred_img)

    @staticmethod
    def _pil_to_qimage(pil_img: Image.Image) -> QImage:
        data = pil_img.tobytes("raw", "RGBA")
        qimg = QImage(data, pil_img.width, pil_img.height, pil_img.width * 4, QImage.Format.Format_RGBA8888)
        return qimg.copy()

    def ensure_scaled(self, target_size: QSize) -> None:
        if target_size.width() <= 0 or target_size.height() <= 0:
            return
        if self._last_size == target_size and self._scaled_sharp and self._scaled_blurred:
            return

        self._last_size = QSize(target_size)
        if self._sharp_qimage and self._blurred_qimage:
            sharp_pm = QPixmap.fromImage(self._sharp_qimage).scaled(
                target_size,
                Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                Qt.TransformationMode.SmoothTransformation,
            )
            blur_pm = QPixmap.fromImage(self._blurred_qimage).scaled(
                target_size,
                Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                Qt.TransformationMode.SmoothTransformation,
            )
            x_off = max(0, (sharp_pm.width() - target_size.width()) // 2)
            y_off = max(0, (sharp_pm.height() - target_size.height()) // 2)
            crop_rect = QRect(x_off, y_off, target_size.width(), target_size.height())
            self._scaled_sharp = sharp_pm.copy(crop_rect)
            self._scaled_blurred = blur_pm.copy(crop_rect)

    def get_sharp_pixmap(self, target_size: QSize) -> QPixmap | None:
        self.ensure_scaled(target_size)
        return self._scaled_sharp

    def get_blurred_pixmap(self, target_size: QSize) -> QPixmap | None:
        self.ensure_scaled(target_size)
        return self._scaled_blurred


# ---------------------------------------------------------------------------
# Flat Frosted Glass Surface (Android 17 Style — Zero Outline, Zero Shadow)
# ---------------------------------------------------------------------------

class FrostedGlassPanel(QFrame):
    """
    Pure flat frosted glass surface inspired by Android 17 & Apple visionOS:
    - Samples the creamy Gaussian-blurred backdrop directly behind its geometry
    - Applies the shared base atmospheric dim + a pure flat milky frost glaze
    - Strictly NO border outlines and NO drop shadows
    """

    def __init__(
        self,
        backdrop: BackdropManager,
        radius: int = 18,
        tint_color: QColor = QColor(255, 255, 255, 26),
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.backdrop = backdrop
        self.radius = radius
        self.tint_color = tint_color
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFrameShape(QFrame.Shape.NoFrame)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        painter.setPen(Qt.PenStyle.NoPen)

        rect = QRectF(self.rect())
        path = QPainterPath()
        path.addRoundedRect(rect, self.radius, self.radius)

        painter.save()
        painter.setClipPath(path)

        win = self.window()
        if win is not None and self.backdrop is not None:
            blurred_pm = self.backdrop.get_blurred_pixmap(win.size())
            if blurred_pm is not None:
                top_left_in_win = self.mapTo(win, QPoint(0, 0))
                src_rect = QRect(top_left_in_win, self.size())
                painter.drawPixmap(self.rect(), blurred_pm, src_rect)

        # Base atmospheric tone matching the window background + pure flat milky frost
        painter.fillPath(path, QColor(8, 18, 36, 95))
        painter.fillPath(path, self.tint_color)
        painter.restore()


# ---------------------------------------------------------------------------
# Vector Icon Renderer (Apple-Minimal Icons & Borderless Instance Icons)
# ---------------------------------------------------------------------------

PRESET_ICONS: list[tuple[str, str]] = [
    ("grass", "Grass"),
    ("diamond_sword", "Diamond"),
    ("crafting_table", "Crafting"),
    ("command_block", "Command"),
    ("ender_pearl", "Ender"),
    ("redstone", "Redstone"),
    ("beacon", "Beacon"),
    ("furnace", "Furnace"),
    ("cherry_blossom", "Cherry"),
    ("axolotl", "Axolotl"),
]

ICON_FOLDER_ALIASES: dict[str, tuple[str, ...]] = {
    "grass": ("grass",),
    "diamond_sword": ("diamond_sword", "sword", "diamond sword"),
    "crafting_table": ("crafting_table", "crafting", "crafting table"),
    "command_block": ("command_block", "command block", "command"),
    "ender_pearl": ("ender_pearl", "ender pearl", "ender"),
    "redstone": ("redstone", "ruby redstone", "ruby_redstone"),
    "beacon": ("beacon",),
    "furnace": ("furnace", "furnance"),
    "cherry_blossom": ("cherry_blossom", "cherry blossum", "cherry blossom", "cherry"),
    "axolotl": ("axolotl",),
}

_ICON_PIXMAP_CACHE: dict[tuple[str, int], QPixmap] = {}


def make_circular_logo_pixmap(size: int = 34) -> QPixmap:
    """Renders `logo.png` clipped into a pure borderless anti-aliased circle."""
    pm = QPixmap(size, size)
    pm.fill(Qt.GlobalColor.transparent)

    logo_path = get_logo_path()
    src = QPixmap(str(logo_path)) if logo_path.exists() else QPixmap()

    painter = QPainter(pm)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
    painter.setPen(Qt.PenStyle.NoPen)

    clip = QPainterPath()
    clip.addEllipse(0.0, 0.0, float(size), float(size))
    painter.setClipPath(clip)

    if not src.isNull():
        scaled = src.scaled(
            size,
            size,
            Qt.AspectRatioMode.KeepAspectRatioByExpanding,
            Qt.TransformationMode.SmoothTransformation,
        )
        x_off = (scaled.width() - size) // 2
        y_off = (scaled.height() - size) // 2
        painter.drawPixmap(-x_off, -y_off, scaled)
    else:
        painter.fillPath(clip, QColor(255, 255, 255, 40))

    painter.end()
    return pm


def ensure_circular_logo_ico() -> Path | None:
    """
    Generates a multi-resolution circular `.ico` file from `logo.png` in `assets/logo_circle.ico`
    so Windows Taskbar and Win32 `WM_SETICON` display the circular Lightyear icon cleanly.
    """
    logo_path = get_logo_path()
    ico_target = ASSETS_DIR / "logo_circle.ico"
    try:
        from PIL import ImageDraw

        if ico_target.exists() and logo_path.exists():
            if ico_target.stat().st_mtime >= logo_path.stat().st_mtime and ico_target.stat().st_size > 0:
                return ico_target

        if not logo_path.exists():
            return None

        src_img = Image.open(logo_path).convert("RGBA")
        side = min(src_img.width, src_img.height)
        left = (src_img.width - side) // 2
        top = (src_img.height - side) // 2
        square_img = src_img.crop((left, top, left + side, top + side)).resize(
            (256, 256), Image.Resampling.LANCZOS
        )

        # Supersampled circular alpha mask for crisp anti-aliased edges
        mask_hi = Image.new("L", (1024, 1024), 0)
        draw = ImageDraw.Draw(mask_hi)
        draw.ellipse((0, 0, 1023, 1023), fill=255)
        mask = mask_hi.resize((256, 256), Image.Resampling.LANCZOS)

        orig_r, orig_g, orig_b, orig_a = square_img.split()
        combined_alpha = Image.composite(orig_a, Image.new("L", (256, 256), 0), mask)
        circ_img = Image.merge("RGBA", (orig_r, orig_g, orig_b, combined_alpha))

        ico_target.parent.mkdir(parents=True, exist_ok=True)
        circ_img.save(
            ico_target,
            format="ICO",
            sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
        )
        return ico_target
    except Exception:
        return None


def make_circular_logo_icon() -> QIcon:
    """Builds a multi-size QIcon of the circular Lightyear logo for window & taskbar use."""
    ico_path = ensure_circular_logo_ico()
    icon = QIcon(str(ico_path)) if (ico_path and ico_path.exists()) else QIcon()
    for sz in (16, 24, 32, 48, 64, 128, 256):
        icon.addPixmap(make_circular_logo_pixmap(sz))
    return icon


def _draw_pixel_sprite(icon_key: str) -> QPixmap:
    """Renders a 16x16 cute aesthetic pixel-art square sprite for preset instance icons."""
    grid_size = 16
    img = QImage(grid_size, grid_size, QImage.Format.Format_ARGB32)
    img.fill(Qt.GlobalColor.transparent)

    def fill_rect(x0: int, y0: int, w: int, h: int, hex_col: str) -> None:
        c = QColor(hex_col)
        for yy in range(max(0, y0), min(grid_size, y0 + h)):
            for xx in range(max(0, x0), min(grid_size, x0 + w)):
                img.setPixelColor(xx, yy, c)

    def set_px(x: int, y: int, hex_col: str) -> None:
        if 0 <= x < grid_size and 0 <= y < grid_size:
            img.setPixelColor(x, y, QColor(hex_col))

    if icon_key == "grass":
        # Cute pastel meadow grass block with pixel overhang and tiny flower
        fill_rect(0, 0, 16, 16, "#8D5B4C")
        fill_rect(0, 5, 16, 11, "#7A4B3A")
        for x, y in ((2, 8), (5, 11), (9, 7), (12, 12), (7, 13), (13, 9), (3, 13)):
            set_px(x, y, "#633B2C")
        for x, y in ((4, 9), (10, 10), (6, 7), (11, 13)):
            set_px(x, y, "#9E6B59")
        fill_rect(0, 0, 16, 5, "#74C66B")
        fill_rect(0, 0, 16, 2, "#8FE084")
        # Pixelated grass fringe overhang
        for x, h in enumerate((2, 3, 1, 2, 3, 2, 1, 3, 2, 1, 3, 2, 1, 2, 3, 1)):
            fill_rect(x, 5, 1, h, "#74C66B")
            set_px(x, 5 + h, "#5AA654")
        # Cute tiny pixel daisy on top left
        set_px(3, 1, "#FFF59D")
        set_px(2, 1, "#FFFFFF")
        set_px(4, 1, "#FFFFFF")
        set_px(3, 2, "#FFFFFF")

    elif icon_key == "diamond_sword":
        # Dreamy twilight sky square with sparkling cyan pixel sword
        fill_rect(0, 0, 16, 16, "#1E2942")
        fill_rect(0, 0, 16, 8, "#253454")
        # Sparkle pixels
        for x, y in ((3, 3), (12, 4), (4, 12), (13, 11)):
            set_px(x, y, "#7DD3FC")
        # Diagonal pixel sword blade
        for i in range(8):
            bx = 6 + i
            by = 9 - i
            set_px(bx, by, "#67E8F9")
            set_px(bx - 1, by, "#22D3EE")
            set_px(bx, by + 1, "#06B6D4")
        set_px(13, 2, "#ECFEFF")
        # Guard & hilt
        for x, y in ((4, 8), (5, 9), (6, 10), (7, 11)):
            set_px(x, y, "#38BDF8")
        set_px(4, 11, "#A16207")
        set_px(3, 12, "#CA8A04")
        set_px(2, 13, "#FDE047")

    elif icon_key == "crafting_table":
        # Warm cozy oak crafting table with 3x3 pixel grid top
        fill_rect(0, 0, 16, 16, "#B47B4E")
        fill_rect(0, 0, 16, 6, "#D69E6B")
        fill_rect(2, 1, 12, 4, "#8C5835")
        for gx in (4, 8, 11):
            for gy in (1, 3):
                set_px(gx, gy, "#F3C697")
                set_px(gx + 1, gy, "#E2AF7E")
        fill_rect(0, 6, 16, 1, "#5C3820")
        fill_rect(2, 8, 5, 6, "#8C5835")
        fill_rect(9, 8, 5, 6, "#8C5835")
        fill_rect(3, 9, 3, 4, "#6E4224")
        fill_rect(10, 9, 3, 4, "#6E4224")

    elif icon_key == "command_block":
        # Pastel terracotta command block with glowing pixel core
        fill_rect(0, 0, 16, 16, "#C87A5B")
        fill_rect(1, 1, 14, 14, "#B5674A")
        fill_rect(3, 3, 10, 10, "#7E4433")
        fill_rect(5, 5, 6, 6, "#4A2A22")
        # Glowing indicator pixels
        fill_rect(6, 6, 4, 4, "#FCD34D")
        set_px(7, 7, "#FFFBEB")
        set_px(8, 8, "#F59E0B")
        for x, y in ((2, 7), (2, 8), (13, 7), (13, 8), (7, 2), (8, 2), (7, 13), (8, 13)):
            set_px(x, y, "#E7A386")

    elif icon_key == "ender_pearl":
        # Deep mystic chorus purple background with glossy pixel teal pearl
        fill_rect(0, 0, 16, 16, "#1E1B38")
        fill_rect(3, 3, 10, 10, "#0F766E")
        fill_rect(4, 2, 8, 12, "#0F766E")
        fill_rect(2, 4, 12, 8, "#0F766E")
        fill_rect(4, 4, 8, 8, "#14B8A6")
        fill_rect(5, 5, 6, 6, "#2DD4BF")
        fill_rect(5, 4, 3, 2, "#99F6E4")
        set_px(5, 4, "#F0FDFA")

    elif icon_key == "redstone":
        # Dark slate stone with glowing ruby pixel crystal heart
        fill_rect(0, 0, 16, 16, "#2B2635")
        fill_rect(4, 4, 8, 8, "#DC2626")
        fill_rect(5, 3, 6, 10, "#EF4444")
        fill_rect(3, 5, 10, 6, "#EF4444")
        fill_rect(5, 5, 6, 6, "#F87171")
        fill_rect(6, 5, 2, 2, "#FECACA")
        for x, y in ((2, 3), (13, 4), (3, 12), (12, 13)):
            set_px(x, y, "#FCA5A5")

    elif icon_key == "beacon":
        # Obsidian base + radiant starlight cyan pixel crystal
        fill_rect(0, 0, 16, 16, "#162A40")
        fill_rect(1, 1, 14, 14, "#224263")
        fill_rect(2, 12, 12, 3, "#312E43")
        fill_rect(4, 4, 8, 8, "#38BDF8")
        fill_rect(5, 5, 6, 6, "#7DD3FC")
        fill_rect(6, 6, 4, 4, "#E0F2FE")
        set_px(7, 7, "#FFFFFF")
        set_px(8, 8, "#FFFFFF")

    else:
        # Cozy lit stone furnace with warm pixel fire glow
        fill_rect(0, 0, 16, 16, "#64748B")
        fill_rect(1, 1, 14, 14, "#475569")
        fill_rect(3, 3, 10, 4, "#334155")
        fill_rect(4, 9, 8, 5, "#1E293B")
        fill_rect(5, 10, 6, 4, "#F97316")
        fill_rect(6, 11, 4, 3, "#FACC15")
        fill_rect(7, 12, 2, 2, "#FEF08A")

    return QPixmap.fromImage(img)


def _resolve_icon_asset_path(icon_key: str) -> Path | None:
    """
    Resolves `icon_key` from custom file paths, `assets/icons/`, or the top-level `icons/` folder.
    Prioritizes pre-fitted RGBA icons in `assets/icons/<key>.png`.
    """
    if os.path.isfile(icon_key):
        return Path(icon_key)

    project_root = PROJECT_ROOT
    assets_icons_dir = ASSETS_DIR / "icons"
    user_icons_dir = project_root / "icons"

    processed_png = assets_icons_dir / f"{icon_key}.png"
    if processed_png.is_file():
        return processed_png

    aliases = ICON_FOLDER_ALIASES.get(icon_key, (icon_key,))
    if assets_icons_dir.is_dir():
        for alias in aliases:
            for ext in (".png", ".webp", ".jpg", ".jpeg", ".ico"):
                p = assets_icons_dir / f"{alias}{ext}"
                if p.is_file():
                    return p

    if user_icons_dir.is_dir():
        for alias in aliases:
            for ext in (".png", ".jpg", ".jpeg", ".webp", ".ico"):
                candidate = user_icons_dir / f"{alias}{ext}"
                if candidate.is_file():
                    return candidate

    return None


def render_instance_icon(icon_key: str, size: int = 30) -> QPixmap:
    """
    Renders a cute aesthetic pixelated square icon for an instance without clipping any edges.
    Automatically loads from `assets/icons/` or `icons/` (or custom file paths) with caching,
    and falls back to the built-in 16x16 pixel-art square sprite.
    """
    cache_key = (icon_key, size)
    cached = _ICON_PIXMAP_CACHE.get(cache_key)
    if cached is not None:
        return cached

    pm = QPixmap(size, size)
    pm.fill(Qt.GlobalColor.transparent)

    painter = QPainter(pm)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    painter.setPen(Qt.PenStyle.NoPen)

    cpath = _resolve_icon_asset_path(icon_key)
    if cpath is not None:
        custom_pm = QPixmap(str(cpath))
        if not custom_pm.isNull():
            # Only apply rounded-rect clip to opaque external images; pre-shaped RGBA icons in assets/icons/
            # already have their full isometric / pixel-squircle silhouette and padding intact.
            if not custom_pm.hasAlphaChannel():
                radius = max(3.0, size * 0.18)
                rect = QRectF(0.0, 0.0, float(size), float(size))
                path = QPainterPath()
                path.addRoundedRect(rect, radius, radius)
                painter.setClipPath(path)

            transform_mode = (
                Qt.TransformationMode.FastTransformation
                if custom_pm.width() <= 64
                else Qt.TransformationMode.SmoothTransformation
            )
            painter.setRenderHint(
                QPainter.RenderHint.SmoothPixmapTransform,
                custom_pm.width() > 64,
            )
            scaled = custom_pm.scaled(
                size,
                size,
                Qt.AspectRatioMode.KeepAspectRatio,
                transform_mode,
            )
            x_off = (size - scaled.width()) // 2
            y_off = (size - scaled.height()) // 2
            painter.drawPixmap(x_off, y_off, scaled)
            painter.end()
            _ICON_PIXMAP_CACHE[cache_key] = pm
            return pm

    # Built-in 16x16 cute pixelated square sprite scaled with nearest-neighbor (FastTransformation)
    radius = max(3.0, size * 0.18)
    rect = QRectF(0.0, 0.0, float(size), float(size))
    path = QPainterPath()
    path.addRoundedRect(rect, radius, radius)
    painter.setClipPath(path)
    painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
    pixel_pm = _draw_pixel_sprite(icon_key)
    scaled_px = pixel_pm.scaled(
        size,
        size,
        Qt.AspectRatioMode.IgnoreAspectRatio,
        Qt.TransformationMode.FastTransformation,
    )
    painter.drawPixmap(0, 0, scaled_px)
    painter.end()
    _ICON_PIXMAP_CACHE[cache_key] = pm
    return pm


def make_ui_icon(
    kind: str,
    size: int = 18,
    color: QColor = QColor(255, 255, 255, 235),
    bg_color: QColor | None = None,
) -> QPixmap:
    """
    Renders crisp Apple-style vector icons (`play`, `folder`, `plus`, `plus_tile`,
    `chevron_updown`, `check`, `mods`, `resourcepacks`, `shaderpacks`, `delete`,
    `vanilla`, `fabric`, `forge`, `quilt`, `search`, `close`, `help`).
    """
    pm = QPixmap(size, size)
    pm.fill(Qt.GlobalColor.transparent)

    painter = QPainter(pm)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)

    if bg_color is not None:
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(bg_color)
        r = size * 0.25
        painter.drawRoundedRect(QRectF(0, 0, size, size), r, r)

    pen = QPen(color, max(1.4, size * 0.095))
    pen.setCapStyle(Qt.PenCapStyle.RoundCap)
    pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
    painter.setPen(pen)
    painter.setBrush(Qt.BrushStyle.NoBrush)

    s = float(size)
    cx, cy = s / 2.0, s / 2.0

    if kind == "play":
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(color)
        path = QPainterPath()
        path.moveTo(s * 0.32, s * 0.22)
        path.lineTo(s * 0.76, cy)
        path.lineTo(s * 0.32, s * 0.78)
        path.closeSubpath()
        painter.drawPath(path)

    elif kind == "folder":
        path = QPainterPath()
        path.moveTo(s * 0.16, s * 0.30)
        path.lineTo(s * 0.40, s * 0.30)
        path.lineTo(s * 0.48, s * 0.39)
        path.lineTo(s * 0.84, s * 0.39)
        path.lineTo(s * 0.84, s * 0.74)
        path.lineTo(s * 0.16, s * 0.74)
        path.closeSubpath()
        painter.drawPath(path)

    elif kind in ("plus", "plus_tile"):
        margin = s * 0.28 if kind == "plus_tile" else s * 0.24
        painter.drawLine(QPointF(cx, margin), QPointF(cx, s - margin))
        painter.drawLine(QPointF(margin, cy), QPointF(s - margin, cy))

    elif kind == "chevron_updown":
        w = s * 0.20
        painter.drawPolyline(
            QPolygonF([QPointF(cx - w, cy - s * 0.10), QPointF(cx, cy - s * 0.26), QPointF(cx + w, cy - s * 0.10)])
        )
        painter.drawPolyline(
            QPolygonF([QPointF(cx - w, cy + s * 0.10), QPointF(cx, cy + s * 0.26), QPointF(cx + w, cy + s * 0.10)])
        )

    elif kind == "check":
        painter.drawPolyline(
            QPolygonF([QPointF(s * 0.22, cy + s * 0.02), QPointF(s * 0.42, s * 0.72), QPointF(s * 0.78, s * 0.28)])
        )

    elif kind == "mods":
        painter.drawRoundedRect(QRectF(s * 0.20, s * 0.20, s * 0.60, s * 0.60), s * 0.14, s * 0.14)
        painter.drawLine(QPointF(cx, s * 0.20), QPointF(cx, s * 0.80))
        painter.drawLine(QPointF(s * 0.20, cy), QPointF(s * 0.80, cy))

    elif kind == "resourcepacks":
        painter.drawRoundedRect(QRectF(s * 0.18, s * 0.24, s * 0.56, s * 0.54), s * 0.10, s * 0.10)
        painter.drawEllipse(QPointF(s * 0.36, s * 0.40), s * 0.06, s * 0.06)
        painter.drawPolyline(
            QPolygonF([QPointF(s * 0.24, s * 0.70), QPointF(s * 0.44, s * 0.52), QPointF(s * 0.68, s * 0.70)])
        )

    elif kind == "shaderpacks":
        painter.drawEllipse(QPointF(cx, cy), s * 0.18, s * 0.18)
        for dx, dy in ((0, -1), (0, 1), (-1, 0), (1, 0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)):
            painter.drawLine(
                QPointF(cx + dx * s * 0.27, cy + dy * s * 0.27),
                QPointF(cx + dx * s * 0.37, cy + dy * s * 0.37),
            )

    elif kind == "delete":
        painter.drawLine(QPointF(s * 0.22, s * 0.30), QPointF(s * 0.78, s * 0.30))
        painter.drawLine(QPointF(s * 0.40, s * 0.22), QPointF(s * 0.60, s * 0.22))
        painter.drawRoundedRect(QRectF(s * 0.28, s * 0.30, s * 0.44, s * 0.50), s * 0.06, s * 0.06)

    elif kind == "vanilla":
        painter.drawRoundedRect(QRectF(s * 0.24, s * 0.24, s * 0.52, s * 0.52), s * 0.12, s * 0.12)

    elif kind == "fabric":
        painter.drawRoundedRect(QRectF(s * 0.24, s * 0.20, s * 0.52, s * 0.60), s * 0.10, s * 0.10)
        painter.drawLine(QPointF(s * 0.36, s * 0.38), QPointF(s * 0.64, s * 0.38))
        painter.drawLine(QPointF(s * 0.36, s * 0.54), QPointF(s * 0.64, s * 0.54))

    elif kind == "forge":
        painter.drawLine(QPointF(s * 0.22, s * 0.34), QPointF(s * 0.78, s * 0.34))
        painter.drawRoundedRect(QRectF(s * 0.30, s * 0.34, s * 0.40, s * 0.22), 2, 2)
        painter.drawLine(QPointF(s * 0.24, s * 0.72), QPointF(s * 0.76, s * 0.72))

    elif kind == "quilt":
        poly = QPainterPath()
        poly.moveTo(cx, s * 0.18)
        poly.lineTo(s * 0.82, cy)
        poly.lineTo(cx, s * 0.82)
        poly.lineTo(s * 0.18, cy)
        poly.closeSubpath()
        painter.drawPath(poly)

    elif kind == "search":
        r = s * 0.23
        c_pt = QPointF(s * 0.44, s * 0.44)
        painter.drawEllipse(c_pt, r, r)
        painter.drawLine(QPointF(s * 0.61, s * 0.61), QPointF(s * 0.80, s * 0.80))

    elif kind == "close":
        m = s * 0.28
        painter.drawLine(QPointF(m, m), QPointF(s - m, s - m))
        painter.drawLine(QPointF(s - m, m), QPointF(m, s - m))

    elif kind == "help":
        painter.drawEllipse(QPointF(cx, cy), s * 0.34, s * 0.34)
        painter.drawLine(QPointF(cx, s * 0.36), QPointF(cx, s * 0.54))
        painter.drawPoint(QPointF(cx, s * 0.67))

    elif kind in ("rename", "edit"):
        painter.drawLine(QPointF(s * 0.25, s * 0.75), QPointF(s * 0.35, s * 0.75))
        painter.drawLine(QPointF(s * 0.25, s * 0.75), QPointF(s * 0.25, s * 0.65))
        painter.drawLine(QPointF(s * 0.25, s * 0.65), QPointF(s * 0.65, s * 0.25))
        painter.drawLine(QPointF(s * 0.35, s * 0.75), QPointF(s * 0.75, s * 0.35))
        painter.drawLine(QPointF(s * 0.65, s * 0.25), QPointF(s * 0.75, s * 0.35))

    elif kind == "duplicate":
        painter.drawRoundedRect(QRectF(s * 0.30, s * 0.18, s * 0.50, s * 0.50), 2, 2)
        painter.drawRoundedRect(QRectF(s * 0.18, s * 0.30, s * 0.50, s * 0.50), 2, 2)

    elif kind == "settings":
        painter.drawLine(QPointF(s * 0.20, s * 0.35), QPointF(s * 0.80, s * 0.35))
        painter.drawLine(QPointF(s * 0.20, s * 0.65), QPointF(s * 0.80, s * 0.65))
        painter.drawRoundedRect(QRectF(s * 0.35, s * 0.25, s * 0.14, s * 0.20), 1, 1)
        painter.drawRoundedRect(QRectF(s * 0.55, s * 0.55, s * 0.14, s * 0.20), 1, 1)

    elif kind == "repair":
        painter.drawArc(QRectF(s * 0.20, s * 0.20, s * 0.60, s * 0.60), 45 * 16, 270 * 16)
        painter.drawPolyline(
            QPolygonF([QPointF(s * 0.68, s * 0.18), QPointF(s * 0.80, s * 0.32), QPointF(s * 0.64, s * 0.36)])
        )

    painter.end()
    return pm


# ---------------------------------------------------------------------------
# Smooth Mouse-Wheel Scroll Area (For Dropdown Popups & Scrollable Pages)
# ---------------------------------------------------------------------------

class SmoothWheelScrollArea(QScrollArea):
    """
    QScrollArea with guaranteed mouse-wheel scrolling even inside frameless
    Qt.WindowType.Popup dialogs and over child QPushButton rows on Windows.
    """

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWidgetResizable(True)
        self.setFrameShape(QFrame.Shape.NoFrame)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        self.setStyleSheet(
            """
            QScrollArea {
                background: transparent;
                border: none;
            }
            QScrollBar:vertical {
                background: transparent;
                width: 5px;
                margin: 2px 0px 2px 0px;
            }
            QScrollBar::handle:vertical {
                background: rgba(255, 255, 255, 0.26);
                min-height: 24px;
                border-radius: 2px;
            }
            QScrollBar::handle:vertical:hover {
                background: rgba(255, 255, 255, 0.44);
            }
            QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
                height: 0px;
                background: none;
                border: none;
            }
            QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical {
                background: transparent;
            }
            """
        )
        self.viewport().installEventFilter(self)

    def watch_widget(self, widget: QWidget) -> None:
        """Installs the wheel event filter on a popup or child row button so wheel events always scroll."""
        widget.installEventFilter(self)

    def handle_wheel(self, event) -> bool:
        bar = self.verticalScrollBar()
        if bar is None:
            return False
        pixel_y = event.pixelDelta().y() if hasattr(event, "pixelDelta") else 0
        angle_y = event.angleDelta().y() if hasattr(event, "angleDelta") else 0
        if pixel_y != 0:
            step = pixel_y
        elif angle_y != 0:
            step = int(angle_y * 0.65)
        else:
            return False
        if step != 0:
            bar.setValue(bar.value() - step)
            event.accept()
            return True
        return False

    def wheelEvent(self, event) -> None:
        if not self.handle_wheel(event):
            super().wheelEvent(event)

    def eventFilter(self, watched, event) -> bool:
        if event.type() == QEvent.Type.Wheel:
            if self.handle_wheel(event):
                return True
        return super().eventFilter(watched, event)


# ---------------------------------------------------------------------------
# Apple-Style Flat Frosted Popover Menu (For Dropdowns & Right-Click Context)
# ---------------------------------------------------------------------------

class ApplePopoverMenu(QDialog):
    """
    Borderless, shadowless, flat frosted glass popover menu inspired by macOS / iOS / Android 17.
    Supports icons, primary/secondary labels, optional trailing icon, checkmark, destructive actions,
    and smooth mouse-wheel scrolling when many items are present.
    """

    def __init__(
        self,
        lato_family: str,
        min_width: int = 215,
        parent: QWidget | None = None,
        trigger_button: QWidget | None = None,
        max_height: int = 270,
    ) -> None:
        super().__init__(parent)
        self.lato_family = lato_family
        self.trigger_button = trigger_button
        self.closed_by_trigger: bool = False
        self._max_height = max_height
        self._content_height = 10
        self.setWindowFlags(
            Qt.WindowType.Popup
            | Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.NoDropShadowWindowHint
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setMinimumWidth(min_width)

        outer_layout = QVBoxLayout(self)
        outer_layout.setContentsMargins(5, 5, 5, 5)
        outer_layout.setSpacing(0)

        self._scroll = SmoothWheelScrollArea(self)
        self._scroll.watch_widget(self)

        self._container = QWidget()
        self._container.setStyleSheet("background: transparent;")
        self._scroll.watch_widget(self._container)

        self._layout = QVBoxLayout(self._container)
        self._layout.setContentsMargins(0, 0, 0, 0)
        self._layout.setSpacing(2)

        self._scroll.setWidget(self._container)
        outer_layout.addWidget(self._scroll)

    def _sync_popup_height(self, add_px: int) -> None:
        self._content_height += add_px
        target_h = min(self._max_height, self._content_height)
        self.setFixedHeight(target_h)

    def wheelEvent(self, event) -> None:
        if not self._scroll.handle_wheel(event):
            super().wheelEvent(event)

    def mousePressEvent(self, event) -> None:
        if self.trigger_button is not None:
            gpos = event.globalPosition().toPoint() if hasattr(event, "globalPosition") else QCursor.pos()
            btn_rect = QRect(self.trigger_button.mapToGlobal(QPoint(0, 0)), self.trigger_button.size())
            if btn_rect.contains(gpos):
                self.closed_by_trigger = True
        super().mousePressEvent(event)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        painter.setPen(Qt.PenStyle.NoPen)

        path = QPainterPath()
        path.addRoundedRect(QRectF(self.rect()), 13.0, 13.0)

        painter.save()
        painter.setClipPath(path)

        win = self.parentWidget().window() if self.parentWidget() else None
        if win is not None and hasattr(win, "backdrop"):
            blurred_pm = win.backdrop.get_blurred_pixmap(win.size())
            if blurred_pm is not None:
                top_left_in_win = win.mapFromGlobal(self.mapToGlobal(QPoint(0, 0)))
                src_rect = QRect(top_left_in_win, self.size())
                painter.drawPixmap(self.rect(), blurred_pm, src_rect)

        painter.fillPath(path, QColor(14, 26, 46, 205))
        painter.fillPath(path, QColor(255, 255, 255, 24))
        painter.restore()

    def add_item(
        self,
        icon_pm: QPixmap,
        title: str,
        subtitle: str = "",
        trailing_icon_pm: QPixmap | None = None,
        checked: bool = False,
        destructive: bool = False,
        on_click: Callable[[], None] | None = None,
    ) -> None:
        btn = QPushButton()
        btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        row_h = 34 if not subtitle else 40
        btn.setFixedHeight(row_h)
        hover_bg = "rgba(239, 68, 68, 0.22)" if destructive else "rgba(255, 255, 255, 0.13)"
        active_bg = "rgba(255, 255, 255, 0.07)" if checked else "transparent"
        btn.setStyleSheet(
            f"""
            QPushButton {{
                background-color: {active_bg};
                border: none;
                border-radius: 8px;
                text-align: left;
            }}
            QPushButton:hover {{
                background-color: {hover_bg};
            }}
            """
        )

        row = QHBoxLayout(btn)
        row.setContentsMargins(9, 0, 9, 0)
        row.setSpacing(9)

        ic_lbl = QLabel()
        ic_lbl.setPixmap(icon_pm)
        ic_lbl.setFixedSize(icon_pm.width(), icon_pm.height())
        ic_lbl.setStyleSheet("background: transparent; border: none;")
        ic_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        row.addWidget(ic_lbl)

        text_col = QVBoxLayout()
        text_col.setContentsMargins(0, 0, 0, 0)
        text_col.setSpacing(0)

        t_color = "#FF6B6B" if destructive else "#FFFFFF"
        t_lbl = QLabel(title)
        t_lbl.setFont(QFont(self.lato_family, 9, QFont.Weight.Bold if not destructive else QFont.Weight.Medium))
        t_lbl.setStyleSheet(f"color: {t_color}; background: transparent; border: none;")
        t_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        text_col.addWidget(t_lbl)

        if subtitle:
            s_lbl = QLabel(subtitle)
            s_lbl.setFont(QFont(self.lato_family, 8, QFont.Weight.Normal))
            s_lbl.setStyleSheet("color: rgba(255, 255, 255, 0.52); background: transparent; border: none;")
            s_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
            text_col.addWidget(s_lbl)

        row.addLayout(text_col, 1)

        if trailing_icon_pm is not None:
            tr_lbl = QLabel()
            tr_lbl.setPixmap(trailing_icon_pm)
            tr_lbl.setFixedSize(trailing_icon_pm.width(), trailing_icon_pm.height())
            tr_lbl.setStyleSheet("background: transparent; border: none;")
            tr_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
            row.addWidget(tr_lbl)

        if checked:
            chk_lbl = QLabel()
            chk_lbl.setPixmap(make_ui_icon("check", 14, QColor(255, 255, 255, 230)))
            chk_lbl.setFixedSize(14, 14)
            chk_lbl.setStyleSheet("background: transparent; border: none;")
            chk_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
            row.addWidget(chk_lbl)

        def _trigger() -> None:
            self.accept()
            if on_click is not None:
                QTimer.singleShot(0, on_click)

        btn.clicked.connect(_trigger)
        self._scroll.watch_widget(btn)
        self._layout.addWidget(btn)
        self._sync_popup_height(row_h + 2)

    def add_separator(self) -> None:
        sep = QFrame()
        sep.setFixedHeight(1)
        sep.setStyleSheet("background-color: rgba(255, 255, 255, 0.08); border: none; margin: 3px 8px;")
        self._scroll.watch_widget(sep)
        self._layout.addWidget(sep)
        self._sync_popup_height(9)


# ---------------------------------------------------------------------------
# 1st Component (Top): Minimal Flat Frosted Navigation Bar
# ---------------------------------------------------------------------------

class TopNavBar(FrostedGlassPanel):
    """
    Minimal top bar:
    - Left: "lightyear" in Playfair Display (no circular logo in navbar)
    - Center: "Home", "Installed", and "Help" page navigation pills
    - Right: "hello! username" (double-click to edit inline; clicking anywhere outside saves & quits edit mode)
    """

    username_changed = Signal(str)
    page_changed = Signal(str)

    def __init__(
        self,
        backdrop: BackdropManager,
        initial_username: str,
        playfair_family: str,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            backdrop=backdrop,
            radius=18,
            tint_color=QColor(255, 255, 255, 26),
            parent=parent,
        )
        self.setFixedHeight(50)
        self._username = initial_username
        self._lato_family = lato_family
        self._active_page: str = "home"

        layout = QHBoxLayout(self)
        layout.setContentsMargins(16, 6, 16, 6)
        layout.setSpacing(10)

        self.brand_label = QLabel("lightyear")
        brand_font = QFont(playfair_family, 17, QFont.Weight.Bold)
        brand_font.setLetterSpacing(QFont.SpacingType.AbsoluteSpacing, 0.4)
        self.brand_label.setFont(brand_font)
        self.brand_label.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.brand_label.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        layout.addWidget(self.brand_label)

        layout.addSpacing(8)

        # Navigation pills: Home | Installed | Help
        self.nav_pill_container = QFrame()
        self.nav_pill_container.setFixedHeight(32)
        self.nav_pill_container.setStyleSheet(
            """
            QFrame {
                background-color: rgba(255, 255, 255, 0.08);
                border: none;
                border-radius: 11px;
            }
            """
        )
        nav_pill_layout = QHBoxLayout(self.nav_pill_container)
        nav_pill_layout.setContentsMargins(3, 3, 3, 3)
        nav_pill_layout.setSpacing(3)

        self.home_nav_btn = QPushButton("Home")
        self.home_nav_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.home_nav_btn.setFixedHeight(26)
        self.home_nav_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.home_nav_btn.clicked.connect(lambda: self.set_active_page("home"))

        self.installed_nav_btn = QPushButton("Installed")
        self.installed_nav_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.installed_nav_btn.setFixedHeight(26)
        self.installed_nav_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.installed_nav_btn.clicked.connect(lambda: self.set_active_page("installed"))

        self.help_nav_btn = QPushButton("Help")
        self.help_nav_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.help_nav_btn.setFixedHeight(26)
        self.help_nav_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.help_nav_btn.clicked.connect(lambda: self.set_active_page("help"))

        nav_pill_layout.addWidget(self.home_nav_btn)
        nav_pill_layout.addWidget(self.installed_nav_btn)
        nav_pill_layout.addWidget(self.help_nav_btn)
        layout.addWidget(self.nav_pill_container)

        self._update_nav_styles()

        layout.addStretch(1)

        # Right-most: "hello! username" (double-click to edit)
        self.user_stack = QStackedWidget()
        self.user_stack.setFixedHeight(32)
        self.user_stack.setSizePolicy(QSizePolicy.Policy.Maximum, QSizePolicy.Policy.Fixed)

        self.greeting_pill = QFrame()
        self.greeting_pill.setObjectName("greetingPill")
        self.greeting_pill.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.greeting_pill.setToolTip("Double-click to edit username")
        self.greeting_pill.setStyleSheet(
            """
            #greetingPill {
                background-color: rgba(255, 255, 255, 0.10);
                border: none;
                border-radius: 11px;
            }
            #greetingPill:hover {
                background-color: rgba(255, 255, 255, 0.17);
            }
            """
        )
        pill_layout = QHBoxLayout(self.greeting_pill)
        pill_layout.setContentsMargins(12, 0, 12, 0)
        pill_layout.setSpacing(5)

        hello_font = QFont(lato_family, 9, QFont.Weight.Normal)
        user_font = QFont(lato_family, 9, QFont.Weight.Bold)

        self.hello_prefix = QLabel("hello!")
        self.hello_prefix.setFont(hello_font)
        self.hello_prefix.setStyleSheet("color: rgba(255, 255, 255, 0.65); border: none; background: transparent;")

        self.username_label = QLabel(self._username)
        self.username_label.setFont(user_font)
        self.username_label.setStyleSheet("color: #FFFFFF; border: none; background: transparent;")

        pill_layout.addWidget(self.hello_prefix)
        pill_layout.addWidget(self.username_label)

        self.edit_container = QFrame()
        self.edit_container.setObjectName("editContainer")
        self.edit_container.setStyleSheet(
            """
            #editContainer {
                background-color: rgba(255, 255, 255, 0.16);
                border: none;
                border-radius: 11px;
            }
            """
        )
        edit_layout = QHBoxLayout(self.edit_container)
        edit_layout.setContentsMargins(12, 0, 10, 0)
        edit_layout.setSpacing(5)

        edit_prefix = QLabel("hello!")
        edit_prefix.setFont(hello_font)
        edit_prefix.setStyleSheet("color: rgba(255, 255, 255, 0.65); border: none; background: transparent;")

        self.username_input = QLineEdit(self._username)
        self.username_input.setFont(user_font)
        self.username_input.setMaxLength(16)
        self.username_input.setFixedWidth(105)
        self.username_input.setStyleSheet(
            """
            QLineEdit {
                color: #FFFFFF;
                background: transparent;
                border: none;
                selection-background-color: rgba(255, 255, 255, 0.25);
            }
            """
        )
        self.username_input.returnPressed.connect(self._commit_username_edit)
        self.username_input.editingFinished.connect(self._commit_username_edit)

        edit_layout.addWidget(edit_prefix)
        edit_layout.addWidget(self.username_input)

        self.user_stack.addWidget(self.greeting_pill)
        self.user_stack.addWidget(self.edit_container)
        layout.addWidget(self.user_stack)

        self.brand_label.installEventFilter(self)
        self.greeting_pill.installEventFilter(self)
        self.hello_prefix.installEventFilter(self)
        self.username_label.installEventFilter(self)
        self.username_input.installEventFilter(self)

        app = QApplication.instance()
        if app is not None:
            app.installEventFilter(self)

    def set_active_page(self, page_key: str) -> None:
        if page_key not in ("home", "installed", "help"):
            return
        self._active_page = page_key
        self._update_nav_styles()
        self.page_changed.emit(page_key)

    def _update_nav_styles(self) -> None:
        active_style = """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.22);
                color: #FFFFFF;
                border: none;
                border-radius: 8px;
                padding: 0 14px;
            }
        """
        inactive_style = """
            QPushButton {
                background-color: transparent;
                color: rgba(255, 255, 255, 0.62);
                border: none;
                border-radius: 8px;
                padding: 0 14px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.10);
                color: #FFFFFF;
            }
        """
        self.home_nav_btn.setStyleSheet(active_style if self._active_page == "home" else inactive_style)
        self.installed_nav_btn.setStyleSheet(active_style if self._active_page == "installed" else inactive_style)
        self.help_nav_btn.setStyleSheet(active_style if self._active_page == "help" else inactive_style)

    def eventFilter(self, watched, event) -> bool:
        if watched is self.brand_label and event.type() == QEvent.Type.MouseButtonPress:
            self.set_active_page("home")
            return True

        if watched in (self.greeting_pill, self.hello_prefix, self.username_label):
            if event.type() == QEvent.Type.MouseButtonDblClick:
                self.start_editing_username()
                return True

        if self.user_stack.currentIndex() == 1:
            etype = event.type()
            if watched is self.username_input and etype == QEvent.Type.KeyPress:
                if event.key() == Qt.Key.Key_Escape:
                    self._commit_username_edit()
                    return True
            elif etype in (
                QEvent.Type.MouseButtonPress,
                QEvent.Type.MouseButtonDblClick,
                QEvent.Type.ContextMenu,
            ):
                gpos = event.globalPosition().toPoint() if hasattr(event, "globalPosition") else QCursor.pos()
                edit_rect = QRect(self.edit_container.mapToGlobal(QPoint(0, 0)), self.edit_container.size())
                if not edit_rect.contains(gpos):
                    self._commit_username_edit()
            elif etype in (QEvent.Type.WindowDeactivate, QEvent.Type.ApplicationDeactivate):
                self._commit_username_edit()

        return super().eventFilter(watched, event)

    def start_editing_username(self) -> None:
        self.username_input.setText(self._username)
        self.user_stack.setCurrentIndex(1)
        self.username_input.setFocus()
        self.username_input.selectAll()

    def _commit_username_edit(self) -> None:
        if self.user_stack.currentIndex() != 1:
            return
        new_name = sanitize_username(self.username_input.text())
        self._username = new_name
        self.username_label.setText(self._username)
        self.user_stack.setCurrentIndex(0)
        self.username_input.clearFocus()
        self.username_changed.emit(self._username)


# ---------------------------------------------------------------------------
# 2nd Component: Minimal Flat Frosted Hero Section + Apple Dropdown with Icons
# ---------------------------------------------------------------------------

NEW_INSTALLATION_SENTINEL = "__NEW_INSTALLATION__"


class HeroSection(FrostedGlassPanel):
    """
    Minimal flat frosted Hero rectangle:
    - "Ready to Play?" headline in Lato
    - Apple-style Dropdown with icons + borderless Play / Install button
    """

    play_requested = Signal(str)
    install_requested = Signal()
    selection_changed = Signal(str)

    def __init__(
        self,
        backdrop: BackdropManager,
        store: LauncherStore,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            backdrop=backdrop,
            radius=20,
            tint_color=QColor(255, 255, 255, 26),
            parent=parent,
        )
        self.store = store
        self.lato_family = lato_family
        self._selected_id: str = NEW_INSTALLATION_SENTINEL
        self._is_busy: bool = False
        self._suppress_dropdown_press: bool = False

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(32, 22, 32, 22)
        main_layout.setSpacing(16)

        main_layout.addStretch(1)

        self.title_label = QLabel("Ready to Play?")
        title_font = QFont(lato_family, 26, QFont.Weight.Black)
        title_font.setLetterSpacing(QFont.SpacingType.AbsoluteSpacing, -0.3)
        self.title_label.setFont(title_font)
        self.title_label.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        main_layout.addWidget(self.title_label)

        bar_layout = QHBoxLayout()
        bar_layout.setContentsMargins(0, 0, 0, 0)
        bar_layout.setSpacing(10)

        # Apple-style Dropdown Trigger Button with Icon + Version/Loader + Dual Chevron
        self.dropdown_btn = QPushButton()
        self.dropdown_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.dropdown_btn.setFixedHeight(44)
        self.dropdown_btn.setFixedWidth(255)
        self.dropdown_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.12);
                border: none;
                border-radius: 12px;
                color: #FFFFFF;
                text-align: left;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.18);
            }
            QPushButton:pressed {
                background-color: rgba(255, 255, 255, 0.24);
            }
            """
        )
        btn_inner = QHBoxLayout(self.dropdown_btn)
        btn_inner.setContentsMargins(10, 0, 10, 0)
        btn_inner.setSpacing(9)

        self.dd_icon_label = QLabel()
        self.dd_icon_label.setFixedSize(26, 26)
        self.dd_icon_label.setStyleSheet("background: transparent; border: none;")
        self.dd_icon_label.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        btn_inner.addWidget(self.dd_icon_label)

        text_col = QVBoxLayout()
        text_col.setContentsMargins(0, 5, 0, 5)
        text_col.setSpacing(0)

        self.dd_title_label = QLabel("Select Installation")
        self.dd_title_label.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        self.dd_title_label.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        self.dd_title_label.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)

        self.dd_meta_label = QLabel("Vanilla")
        self.dd_meta_label.setFont(QFont(lato_family, 8, QFont.Weight.Normal))
        self.dd_meta_label.setStyleSheet("color: rgba(255, 255, 255, 0.62); background: transparent; border: none;")
        self.dd_meta_label.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)

        text_col.addWidget(self.dd_title_label)
        text_col.addWidget(self.dd_meta_label)
        btn_inner.addLayout(text_col, 1)

        self.dd_chevron = QLabel()
        self.dd_chevron.setPixmap(make_ui_icon("chevron_updown", 15, QColor(255, 255, 255, 175)))
        self.dd_chevron.setFixedSize(15, 15)
        self.dd_chevron.setStyleSheet("background: transparent; border: none;")
        self.dd_chevron.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        btn_inner.addWidget(self.dd_chevron)

        self.dropdown_btn.installEventFilter(self)
        self.dropdown_btn.clicked.connect(self._open_dropdown_menu)
        bar_layout.addWidget(self.dropdown_btn)

        # Dynamic Play / Install Primary Action Button
        self.action_btn = QPushButton("Play")
        self.action_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.action_btn.setFixedHeight(44)
        self.action_btn.setMinimumWidth(132)
        self.action_btn.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        self.action_btn.setIconSize(QSize(14, 14))
        self.action_btn.clicked.connect(self._on_action_clicked)
        bar_layout.addWidget(self.action_btn)

        bar_layout.addStretch(1)
        main_layout.addLayout(bar_layout)
        main_layout.addStretch(1)

        self.refresh_from_store()

    def eventFilter(self, watched, event) -> bool:
        if watched is self.dropdown_btn and event.type() == QEvent.Type.MouseButtonPress:
            if self._suppress_dropdown_press:
                self._suppress_dropdown_press = False
                return True
        return super().eventFilter(watched, event)

    def _clear_dropdown_suppress(self) -> None:
        self._suppress_dropdown_press = False

    def set_busy(self, busy: bool, custom_label: str | None = None) -> None:
        self._is_busy = busy
        self.dropdown_btn.setEnabled(not busy)
        self.action_btn.setEnabled(not busy)
        if busy and custom_label:
            self.action_btn.setIcon(QIcon())
            self.action_btn.setText(custom_label)
        else:
            self._update_display()

    def refresh_from_store(self, select_id: str | None = None) -> None:
        if select_id is not None:
            self._selected_id = select_id
        else:
            last = self.store.get_last_played_instance()
            self._selected_id = last.id if last else NEW_INSTALLATION_SENTINEL
        self._update_display()

    def select_instance(self, instance_id: str) -> None:
        self._selected_id = instance_id
        if instance_id != NEW_INSTALLATION_SENTINEL:
            self.store.last_played_instance_id = instance_id
            self.store.save()
        self._update_display()
        self.selection_changed.emit(self._selected_id)

    def _update_display(self) -> None:
        inst = (
            self.store.get_instance(self._selected_id)
            if self._selected_id != NEW_INSTALLATION_SENTINEL
            else None
        )

        btn_style = """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.92);
                color: #0C1626;
                border: none;
                border-radius: 12px;
                padding: 0px 22px;
            }
            QPushButton:hover {
                background-color: #FFFFFF;
            }
            QPushButton:pressed {
                background-color: rgba(255, 255, 255, 0.78);
            }
            QPushButton:disabled {
                background-color: rgba(255, 255, 255, 0.25);
                color: rgba(255, 255, 255, 0.55);
            }
        """

        if inst is not None:
            self.dd_icon_label.setPixmap(render_instance_icon(inst.icon_key, 26))
            self.dd_title_label.setText(inst.name or inst.mc_version)
            v_jar = self.store.root / "versions" / inst.mc_version / f"{inst.mc_version}.jar"
            is_dl = v_jar.exists() and inst.ready
            dl_tag = "  ·  Downloaded" if is_dl else ("  ·  Needs Repair" if not inst.ready else "")
            self.dd_meta_label.setText(f"{inst.mc_version} {inst.loader}{dl_tag}")
            self.action_btn.setIcon(QIcon(make_ui_icon("play", 14, QColor(12, 22, 38, 255))))
            self.action_btn.setText("  Play" if inst.ready else "  Repair & Play")
            self.action_btn.setStyleSheet(btn_style)
        else:
            self.dd_icon_label.setPixmap(
                make_ui_icon("plus_tile", 26, QColor(255, 255, 255, 230), QColor(255, 255, 255, 32))
            )
            self.dd_title_label.setText("New Installation")
            self.dd_meta_label.setText("Create instance")
            self.action_btn.setIcon(QIcon(make_ui_icon("plus", 14, QColor(12, 22, 38, 255))))
            self.action_btn.setText("  Install")
            self.action_btn.setStyleSheet(btn_style)

    def _open_dropdown_menu(self) -> None:
        popup = ApplePopoverMenu(
            self.lato_family,
            min_width=self.dropdown_btn.width(),
            parent=self,
            trigger_button=self.dropdown_btn,
        )

        ordered = sorted(
            self.store.instances,
            key=lambda x: (x.id == self.store.last_played_instance_id, x.last_played, x.created_at),
            reverse=True,
        )

        for inst in ordered:
            iid = inst.id
            v_jar = self.store.root / "versions" / inst.mc_version / f"{inst.mc_version}.jar"
            is_dl = v_jar.exists() and inst.ready
            dl_suffix = "  ·  Downloaded" if is_dl else ("  ·  Needs Repair" if not inst.ready else "")
            popup.add_item(
                icon_pm=render_instance_icon(inst.icon_key, 24),
                title=inst.name or inst.mc_version,
                subtitle=f"{inst.mc_version} {inst.loader}{dl_suffix}",
                checked=(iid == self._selected_id),
                on_click=lambda target_id=iid: self.select_instance(target_id),
            )

        if ordered:
            popup.add_separator()

        popup.add_item(
            icon_pm=make_ui_icon("plus_tile", 24, QColor(255, 255, 255, 235), QColor(255, 255, 255, 28)),
            title="New Installation",
            subtitle="Add version & mod loader",
            checked=(self._selected_id == NEW_INSTALLATION_SENTINEL),
            on_click=lambda: self.select_instance(NEW_INSTALLATION_SENTINEL),
        )

        pos = self.dropdown_btn.mapToGlobal(QPoint(0, self.dropdown_btn.height() + 5))
        popup.move(pos)
        popup.exec()
        if popup.closed_by_trigger:
            self._suppress_dropdown_press = True
            QTimer.singleShot(50, self._clear_dropdown_suppress)

    def _on_action_clicked(self) -> None:
        if self._is_busy:
            return
        if self._selected_id == NEW_INSTALLATION_SENTINEL or not self.store.get_instance(self._selected_id):
            self.install_requested.emit()
        else:
            self.play_requested.emit(self._selected_id)


# ---------------------------------------------------------------------------
# 3rd Component (Below Hero): Compact Apple-Minimal Instance Blocks
# ---------------------------------------------------------------------------

class InstanceCard(FrostedGlassPanel):
    """
    Compact, rectangular, slightly curved flat frosted glass instance block.
    Contains ONLY:
    - Icon
    - Version and Mod Loader
    - Folder button and Play button
    - Right-click context menu: Add Resource Packs, Add Mods, Add Shaderpacks, Delete
    (No selection state, no borders, no shadows.)
    """

    play_clicked = Signal(str)
    open_folder_clicked = Signal(str)
    rename_clicked = Signal(str)
    duplicate_clicked = Signal(str)
    settings_clicked = Signal(str)
    repair_clicked = Signal(str)
    add_resource_packs_clicked = Signal(str)
    add_mods_clicked = Signal(str)
    add_shaderpacks_clicked = Signal(str)
    delete_clicked = Signal(str)

    def __init__(
        self,
        backdrop: BackdropManager,
        instance: InstanceConfig,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            backdrop=backdrop,
            radius=12,
            tint_color=QColor(255, 255, 255, 26),
            parent=parent,
        )
        self.instance = instance
        self.lato_family = lato_family
        self.setFixedHeight(52)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(10, 0, 10, 0)
        layout.setSpacing(10)

        # 1. Icon
        icon_lbl = QLabel()
        icon_lbl.setPixmap(render_instance_icon(instance.icon_key, 30))
        icon_lbl.setFixedSize(30, 30)
        icon_lbl.setStyleSheet("background: transparent; border: none;")
        layout.addWidget(icon_lbl)

        # 2. Version and Mod Loader + Last Played tracking
        meta_col = QVBoxLayout()
        meta_col.setContentsMargins(0, 7, 0, 7)
        meta_col.setSpacing(0)

        title_text = instance.name if instance.name else instance.mc_version
        title_lbl = QLabel(title_text)
        title_lbl.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        title_lbl.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")

        meta_parts: list[str] = []
        if instance.name and instance.name != instance.mc_version:
            meta_parts.append(instance.mc_version)
        meta_parts.append(instance.loader)
        if not instance.ready:
            meta_parts.append("Needs Repair")
        else:
            meta_parts.append(format_last_played(instance.last_played))

        meta_sub = " · ".join(meta_parts)
        meta_lbl = QLabel(meta_sub)
        meta_lbl.setFont(QFont(lato_family, 8, QFont.Weight.Normal))
        color_str = "#FBBF24" if not instance.ready else "rgba(255, 255, 255, 0.62)"
        meta_lbl.setStyleSheet(f"color: {color_str}; background: transparent; border: none;")

        meta_col.addWidget(title_lbl)
        meta_col.addWidget(meta_lbl)
        layout.addLayout(meta_col, 1)

        # Tooltip for instance details
        self.setToolTip(
            f"{instance.name}\n"
            f"Minecraft {instance.mc_version} ({instance.loader})\n"
            f"{format_last_played(instance.last_played)}\n"
            f"Directory: {instance.resolved_game_dir()}"
        )

        # 3. Folder Button
        folder_btn = QPushButton()
        folder_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        folder_btn.setToolTip("Open instance folder")
        folder_btn.setFixedSize(30, 30)
        folder_btn.setIcon(QIcon(make_ui_icon("folder", 15, QColor(255, 255, 255, 220))))
        folder_btn.setIconSize(QSize(15, 15))
        folder_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.10);
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.20);
            }
            QPushButton:pressed {
                background-color: rgba(255, 255, 255, 0.26);
            }
            """
        )
        folder_btn.clicked.connect(lambda checked=False, iid=self.instance.id: self.open_folder_clicked.emit(iid))
        layout.addWidget(folder_btn)

        # 4. Play Button
        play_btn = QPushButton()
        play_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        play_btn.setToolTip("Play" if instance.ready else "Repair & Play")
        play_btn.setFixedSize(30, 30)
        play_btn.setIcon(QIcon(make_ui_icon("play", 14, QColor(255, 255, 255, 245))))
        play_btn.setIconSize(QSize(14, 14))
        play_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.16);
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.28);
            }
            QPushButton:pressed {
                background-color: rgba(255, 255, 255, 0.34);
            }
            """
        )
        play_btn.clicked.connect(lambda checked=False, iid=self.instance.id: self.play_clicked.emit(iid))
        layout.addWidget(play_btn)

    def enterEvent(self, event) -> None:
        self.tint_color = QColor(255, 255, 255, 42)
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event) -> None:
        self.tint_color = QColor(255, 255, 255, 26)
        self.update()
        super().leaveEvent(event)

    def contextMenuEvent(self, event) -> None:
        menu = ApplePopoverMenu(self.lato_family, min_width=205, parent=self)
        iid = self.instance.id

        menu.add_item(
            icon_pm=make_ui_icon("folder", 16, QColor(255, 255, 255, 220)),
            title="Open Folder",
            on_click=lambda: self.open_folder_clicked.emit(iid),
        )
        menu.add_item(
            icon_pm=make_ui_icon("rename", 16, QColor(255, 255, 255, 220)),
            title="Rename Instance",
            on_click=lambda: self.rename_clicked.emit(iid),
        )
        menu.add_item(
            icon_pm=make_ui_icon("duplicate", 16, QColor(255, 255, 255, 220)),
            title="Duplicate Instance",
            on_click=lambda: self.duplicate_clicked.emit(iid),
        )
        menu.add_item(
            icon_pm=make_ui_icon("settings", 16, QColor(255, 255, 255, 220)),
            title="Edit Settings...",
            on_click=lambda: self.settings_clicked.emit(iid),
        )
        if not self.instance.ready:
            menu.add_item(
                icon_pm=make_ui_icon("repair", 16, QColor(251, 191, 36, 230)),
                title="Repair Installation",
                on_click=lambda: self.repair_clicked.emit(iid),
            )
        menu.add_separator()
        menu.add_item(
            icon_pm=make_ui_icon("resourcepacks", 16, QColor(255, 255, 255, 215)),
            title="Add Resource Packs",
            on_click=lambda: self.add_resource_packs_clicked.emit(iid),
        )
        menu.add_item(
            icon_pm=make_ui_icon("mods", 16, QColor(255, 255, 255, 215)),
            title="Add Mods",
            on_click=lambda: self.add_mods_clicked.emit(iid),
        )
        menu.add_item(
            icon_pm=make_ui_icon("shaderpacks", 16, QColor(255, 255, 255, 215)),
            title="Add Shaderpacks",
            on_click=lambda: self.add_shaderpacks_clicked.emit(iid),
        )
        menu.add_separator()
        menu.add_item(
            icon_pm=make_ui_icon("delete", 16, QColor(255, 107, 107, 235)),
            title="Delete",
            destructive=True,
            on_click=lambda: self.delete_clicked.emit(iid),
        )

        menu.move(event.globalPos())
        menu.exec()


class InstallationsSection(QWidget):
    """
    Displays all instance blocks in a compact multi-column layout without scrollbars.
    No selection state, no redundant headers or extra cards.
    """

    instance_play = Signal(str)
    instance_open_folder = Signal(str)
    instance_rename = Signal(str)
    instance_duplicate = Signal(str)
    instance_settings = Signal(str)
    instance_repair = Signal(str)
    instance_add_resource_packs = Signal(str)
    instance_add_mods = Signal(str)
    instance_add_shaderpacks = Signal(str)
    instance_delete = Signal(str)

    def __init__(
        self,
        backdrop: BackdropManager,
        store: LauncherStore,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.backdrop = backdrop
        self.store = store
        self.lato_family = lato_family

        self.grid_layout = QGridLayout(self)
        self.grid_layout.setContentsMargins(0, 0, 0, 0)
        self.grid_layout.setHorizontalSpacing(10)
        self.grid_layout.setVerticalSpacing(10)

    def refresh_cards(self) -> None:
        while self.grid_layout.count():
            item = self.grid_layout.takeAt(0)
            w = item.widget()
            if w is not None:
                w.deleteLater()

        ordered = sorted(
            self.store.instances,
            key=lambda x: (x.id == self.store.last_played_instance_id, x.last_played, x.created_at),
            reverse=True,
        )

        cols = 3
        for c in range(cols):
            self.grid_layout.setColumnStretch(c, 1)

        for idx, inst in enumerate(ordered):
            card = InstanceCard(
                backdrop=self.backdrop,
                instance=inst,
                lato_family=self.lato_family,
                parent=self,
            )
            card.play_clicked.connect(self.instance_play.emit)
            card.open_folder_clicked.connect(self.instance_open_folder.emit)
            card.rename_clicked.connect(self.instance_rename.emit)
            card.duplicate_clicked.connect(self.instance_duplicate.emit)
            card.settings_clicked.connect(self.instance_settings.emit)
            card.repair_clicked.connect(self.instance_repair.emit)
            card.add_resource_packs_clicked.connect(self.instance_add_resource_packs.emit)
            card.add_mods_clicked.connect(self.instance_add_mods.emit)
            card.add_shaderpacks_clicked.connect(self.instance_add_shaderpacks.emit)
            card.delete_clicked.connect(self.instance_delete.emit)
            self.grid_layout.addWidget(card, idx // cols, idx % cols)

        total_rows = (max(0, len(ordered) - 1) // cols) + 1
        self.grid_layout.setRowStretch(total_rows, 1)


# ---------------------------------------------------------------------------
# New Installation Popup Dialog (Flat Minimal Frosted Glass + Apple Dropdown)
# ---------------------------------------------------------------------------

class NewInstallationDialog(QDialog):
    """
    Minimal flat frosted modal opened when clicking 'Install':
    - Icon chooser
    - Installation Name
    - Mod Loader selector with icons
    - Apple-style Version Dropdown with icons + 'All versions' toggle
    """

    def __init__(
        self,
        store: LauncherStore,
        versions: list[VersionInfo],
        playfair_family: str,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.store = store
        from .core import load_cached_versions_fast
        self.all_versions = versions if versions else load_cached_versions_fast(store.root)
        self.lato_family = lato_family

        self.selected_icon_key: str = "grass"
        self.selected_loader: str = "Vanilla"
        self.selected_version: str = "1.21.4"
        self._user_edited_name: bool = False
        self._updating_name_programmatically: bool = False
        self._filtered_versions: list[VersionInfo] = []
        self._suppress_version_press: bool = False

        self.setWindowTitle("New Installation")
        self.setWindowFlags(
            Qt.WindowType.Dialog
            | Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.NoDropShadowWindowHint
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setModal(True)
        self.setFixedSize(480, 410)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 22)
        root.setSpacing(13)

        hdr_row = QHBoxLayout()
        title = QLabel("New Installation")
        title.setFont(QFont(lato_family, 15, QFont.Weight.Black))
        title.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        hdr_row.addWidget(title)
        hdr_row.addStretch(1)
        root.addLayout(hdr_row)

        # 1. Icon Chooser (10 preset icons + custom image button)
        icon_row = QHBoxLayout()
        icon_row.setSpacing(4)
        self.icon_buttons: dict[str, QPushButton] = {}
        for key, label in PRESET_ICONS:
            btn = QPushButton()
            btn.setToolTip(label)
            btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            btn.setFixedSize(35, 35)
            btn.setIcon(QIcon(render_instance_icon(key, 27)))
            btn.setIconSize(QSize(27, 27))
            btn.clicked.connect(lambda checked=False, k=key: self._select_icon(k))
            self.icon_buttons[key] = btn
            icon_row.addWidget(btn)

        custom_ic_btn = QPushButton()
        custom_ic_btn.setToolTip("Custom icon image")
        custom_ic_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        custom_ic_btn.setFixedSize(35, 35)
        custom_ic_btn.setIcon(QIcon(make_ui_icon("plus", 15, QColor(255, 255, 255, 200))))
        custom_ic_btn.setIconSize(QSize(15, 15))
        custom_ic_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.06);
                border: none;
                border-radius: 10px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.14);
            }
            """
        )
        custom_ic_btn.clicked.connect(self._pick_custom_icon)
        icon_row.addWidget(custom_ic_btn)
        icon_row.addStretch(1)
        root.addLayout(icon_row)

        # 2. Name Input
        self.name_input = QLineEdit()
        self.name_input.setFixedHeight(38)
        self.name_input.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        self.name_input.setPlaceholderText("Instance name")
        self.name_input.setStyleSheet(
            """
            QLineEdit {
                background-color: rgba(255, 255, 255, 0.08);
                border: none;
                border-radius: 11px;
                padding: 0 13px;
                color: #FFFFFF;
            }
            QLineEdit:focus {
                background-color: rgba(255, 255, 255, 0.14);
            }
            """
        )
        self.name_input.textEdited.connect(self._on_name_user_edited)
        root.addWidget(self.name_input)

        # 3. Mod Loader Selector with Icons
        loader_row = QHBoxLayout()
        loader_row.setSpacing(8)
        self.loader_buttons: dict[str, QPushButton] = {}
        for loader_name in ("Vanilla", "Fabric", "Forge", "Quilt"):
            l_btn = QPushButton(f"  {loader_name}")
            l_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            l_btn.setFixedHeight(36)
            l_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
            l_btn.setIcon(QIcon(make_ui_icon(loader_name.lower(), 14, QColor(255, 255, 255, 220))))
            l_btn.setIconSize(QSize(14, 14))
            l_btn.clicked.connect(lambda checked=False, ln=loader_name: self._select_loader(ln))
            self.loader_buttons[loader_name] = l_btn
            loader_row.addWidget(l_btn)
        root.addLayout(loader_row)

        # 4. Version Selector Header + All versions Checkbox
        ver_hdr = QHBoxLayout()
        ver_lbl = QLabel("Version")
        ver_lbl.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        ver_lbl.setStyleSheet("color: rgba(255, 255, 255, 0.56); background: transparent; border: none;")
        ver_hdr.addWidget(ver_lbl)
        ver_hdr.addStretch(1)

        self.show_all_checkbox = QCheckBox("All versions")
        self.show_all_checkbox.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.show_all_checkbox.setFont(QFont(lato_family, 9, QFont.Weight.Medium))
        self.show_all_checkbox.setStyleSheet(
            """
            QCheckBox {
                color: rgba(255, 255, 255, 0.65);
                spacing: 6px;
                background: transparent;
            }
            QCheckBox::indicator {
                width: 14px;
                height: 14px;
                border-radius: 4px;
                border: none;
                background: rgba(255, 255, 255, 0.12);
            }
            QCheckBox::indicator:checked {
                background-color: rgba(255, 255, 255, 0.88);
            }
            """
        )
        self.show_all_checkbox.toggled.connect(self._populate_versions)
        ver_hdr.addWidget(self.show_all_checkbox)
        root.addLayout(ver_hdr)

        # Apple-style Version Dropdown Button with Icon
        self.version_btn = QPushButton()
        self.version_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.version_btn.setFixedHeight(40)
        self.version_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.08);
                border: none;
                border-radius: 11px;
                color: #FFFFFF;
                text-align: left;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.14);
            }
            """
        )
        v_btn_layout = QHBoxLayout(self.version_btn)
        v_btn_layout.setContentsMargins(11, 0, 11, 0)
        v_btn_layout.setSpacing(9)

        self.v_btn_icon = QLabel()
        self.v_btn_icon.setPixmap(render_instance_icon("grass", 20))
        self.v_btn_icon.setFixedSize(20, 20)
        self.v_btn_icon.setStyleSheet("background: transparent; border: none;")
        self.v_btn_icon.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        v_btn_layout.addWidget(self.v_btn_icon)

        self.v_btn_text = QLabel("1.21.4")
        self.v_btn_text.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        self.v_btn_text.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        self.v_btn_text.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        v_btn_layout.addWidget(self.v_btn_text, 1)

        v_chev = QLabel()
        v_chev.setPixmap(make_ui_icon("chevron_updown", 14, QColor(255, 255, 255, 165)))
        v_chev.setFixedSize(14, 14)
        v_chev.setStyleSheet("background: transparent; border: none;")
        v_chev.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        v_btn_layout.addWidget(v_chev)

        self.version_btn.installEventFilter(self)
        self.version_btn.clicked.connect(self._open_version_picker)
        root.addWidget(self.version_btn)

        self.decision_label = QLabel("")
        self.decision_label.setFont(QFont(lato_family, 8))
        self.decision_label.setStyleSheet("color: rgba(255, 255, 255, 0.44); background: transparent; border: none;")
        root.addWidget(self.decision_label)

        root.addStretch(1)

        # Bottom Action Row
        btn_row = QHBoxLayout()
        btn_row.setSpacing(10)
        btn_row.addStretch(1)

        cancel_btn = QPushButton("Cancel")
        cancel_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        cancel_btn.setFixedHeight(38)
        cancel_btn.setMinimumWidth(94)
        cancel_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        cancel_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.08);
                color: rgba(255, 255, 255, 0.80);
                border: none;
                border-radius: 11px;
                padding: 0 16px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.15);
            }
            """
        )
        cancel_btn.clicked.connect(self.reject)
        btn_row.addWidget(cancel_btn)

        self.create_btn = QPushButton("Install")
        self.create_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.create_btn.setFixedHeight(38)
        self.create_btn.setMinimumWidth(120)
        self.create_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.create_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.92);
                color: #0C1626;
                border: none;
                border-radius: 11px;
                padding: 0 20px;
            }
            QPushButton:hover {
                background-color: #FFFFFF;
            }
            """
        )
        self.create_btn.clicked.connect(self.accept)
        btn_row.addWidget(self.create_btn)

        root.addLayout(btn_row)

        self._select_icon("grass")
        self._select_loader("Vanilla")
        self._populate_versions(False)

    def eventFilter(self, watched, event) -> bool:
        if watched is self.version_btn and event.type() == QEvent.Type.MouseButtonPress:
            if self._suppress_version_press:
                self._suppress_version_press = False
                return True
        return super().eventFilter(watched, event)

    def _clear_version_suppress(self) -> None:
        self._suppress_version_press = False

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        painter.setPen(Qt.PenStyle.NoPen)

        path = QPainterPath()
        path.addRoundedRect(QRectF(self.rect()), 18.0, 18.0)

        painter.save()
        painter.setClipPath(path)

        win = self.parentWidget().window() if self.parentWidget() else None
        if win is not None and hasattr(win, "backdrop"):
            blurred_pm = win.backdrop.get_blurred_pixmap(win.size())
            if blurred_pm is not None:
                top_left_in_win = win.mapFromGlobal(self.mapToGlobal(QPoint(0, 0)))
                src_rect = QRect(top_left_in_win, self.size())
                painter.drawPixmap(self.rect(), blurred_pm, src_rect)

        painter.fillPath(path, QColor(14, 26, 46, 210))
        painter.fillPath(path, QColor(255, 255, 255, 24))
        painter.restore()

    def _select_icon(self, key: str) -> None:
        self.selected_icon_key = key
        self.v_btn_icon.setPixmap(render_instance_icon(key, 20))
        for k, btn in self.icon_buttons.items():
            bg = "rgba(255, 255, 255, 0.22)" if k == key else "rgba(255, 255, 255, 0.06)"
            btn.setStyleSheet(
                f"""
                QPushButton {{
                    background-color: {bg};
                    border: none;
                    border-radius: 10px;
                }}
                QPushButton:hover {{
                    background-color: rgba(255, 255, 255, 0.16);
                }}
                """
            )

    def _pick_custom_icon(self) -> None:
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            "Choose Instance Icon",
            "",
            "Images (*.png *.jpg *.jpeg *.webp *.ico)",
        )
        if file_path:
            self._select_icon(file_path)

    def _select_loader(self, loader_name: str) -> None:
        self.selected_loader = loader_name
        for name, btn in self.loader_buttons.items():
            if name == loader_name:
                btn.setStyleSheet(
                    """
                    QPushButton {
                        background-color: rgba(255, 255, 255, 0.22);
                        color: #FFFFFF;
                        border: none;
                        border-radius: 10px;
                    }
                    """
                )
            else:
                btn.setStyleSheet(
                    """
                    QPushButton {
                        background-color: rgba(255, 255, 255, 0.07);
                        color: rgba(255, 255, 255, 0.65);
                        border: none;
                        border-radius: 10px;
                    }
                    QPushButton:hover {
                        background-color: rgba(255, 255, 255, 0.13);
                    }
                    """
                )
        self._sync_default_name_and_decision()

    def _populate_versions(self, show_all: bool) -> None:
        self._filtered_versions = [
            v for v in self.all_versions if show_all or v.vtype == "release"
        ]
        if not any(v.id == self.selected_version for v in self._filtered_versions) and self._filtered_versions:
            self.selected_version = self._filtered_versions[0].id
        self._update_version_button()
        self._sync_default_name_and_decision()

    def _update_version_button(self) -> None:
        v_jar = self.store.root / "versions" / self.selected_version / f"{self.selected_version}.jar"
        downloaded = "  ·  Downloaded" if v_jar.exists() else ""
        self.v_btn_text.setText(f"{self.selected_version}{downloaded}")

    def _open_version_picker(self) -> None:
        popup = QDialog(self)
        popup.setWindowFlags(
            Qt.WindowType.Popup
            | Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.NoDropShadowWindowHint
        )
        popup.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        popup.setFixedSize(self.version_btn.width(), 240)
        closed_by_trigger = {"value": False}

        orig_mp = popup.mousePressEvent

        def _popup_mouse_press(ev) -> None:
            gpos = ev.globalPosition().toPoint() if hasattr(ev, "globalPosition") else QCursor.pos()
            btn_rect = QRect(self.version_btn.mapToGlobal(QPoint(0, 0)), self.version_btn.size())
            if btn_rect.contains(gpos):
                closed_by_trigger["value"] = True
            orig_mp(ev)

        popup.mousePressEvent = _popup_mouse_press  # type: ignore[method-assign]

        def _paint_popup(ev) -> None:
            p = QPainter(popup)
            p.setRenderHint(QPainter.RenderHint.Antialiasing, True)
            p.setPen(Qt.PenStyle.NoPen)
            path = QPainterPath()
            path.addRoundedRect(QRectF(popup.rect()), 13.0, 13.0)
            p.fillPath(path, QColor(14, 25, 44, 242))

        popup.paintEvent = _paint_popup  # type: ignore[method-assign]

        v_lay = QVBoxLayout(popup)
        v_lay.setContentsMargins(5, 5, 5, 5)

        scroll = SmoothWheelScrollArea(popup)
        scroll.watch_widget(popup)

        def _popup_wheel(ev) -> None:
            scroll.handle_wheel(ev)

        popup.wheelEvent = _popup_wheel  # type: ignore[method-assign]

        list_container = QWidget()
        list_container.setStyleSheet("background: transparent;")
        scroll.watch_widget(list_container)
        list_lay = QVBoxLayout(list_container)
        list_lay.setContentsMargins(0, 0, 2, 0)
        list_lay.setSpacing(2)

        selected_row_btn: QPushButton | None = None

        for v in self._filtered_versions:
            row_btn = QPushButton()
            row_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            row_btn.setFixedHeight(32)
            is_sel = v.id == self.selected_version
            bg = "rgba(255, 255, 255, 0.12)" if is_sel else "transparent"
            row_btn.setStyleSheet(
                f"""
                QPushButton {{
                    background-color: {bg};
                    border: none;
                    border-radius: 8px;
                    text-align: left;
                }}
                QPushButton:hover {{
                    background-color: rgba(255, 255, 255, 0.16);
                }}
                """
            )
            rl = QHBoxLayout(row_btn)
            rl.setContentsMargins(9, 0, 9, 0)
            rl.setSpacing(8)

            ic_lbl = QLabel()
            ic_key = "grass" if v.vtype == "release" else "command_block"
            ic_lbl.setPixmap(render_instance_icon(ic_key, 18))
            ic_lbl.setFixedSize(18, 18)
            ic_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
            rl.addWidget(ic_lbl)

            t_lbl = QLabel(v.id)
            t_lbl.setFont(QFont(self.lato_family, 9, QFont.Weight.Bold))
            t_lbl.setStyleSheet("color: #FFFFFF; background: transparent;")
            t_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
            rl.addWidget(t_lbl, 1)

            is_dl = (self.store.root / "versions" / v.id / f"{v.id}.jar").exists()
            if is_dl:
                dl_lbl = QLabel("Downloaded")
                dl_lbl.setFont(QFont(self.lato_family, 8, QFont.Weight.Bold))
                dl_lbl.setStyleSheet(
                    "color: #6EE7B7; background-color: rgba(110, 231, 183, 0.16); "
                    "border-radius: 4px; padding: 2px 6px; border: none;"
                )
                dl_lbl.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
                rl.addWidget(dl_lbl)

            if is_sel:
                chk = QLabel()
                chk.setPixmap(make_ui_icon("check", 13, QColor(255, 255, 255, 230)))
                chk.setFixedSize(13, 13)
                chk.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
                rl.addWidget(chk)
                selected_row_btn = row_btn

            vid = v.id

            def _pick(checked: bool = False, chosen: str = vid) -> None:
                self.selected_version = chosen
                self._update_version_button()
                self._sync_default_name_and_decision()
                popup.accept()

            row_btn.clicked.connect(_pick)
            scroll.watch_widget(row_btn)
            list_lay.addWidget(row_btn)

        list_lay.addStretch(1)
        scroll.setWidget(list_container)
        v_lay.addWidget(scroll)

        pos = self.version_btn.mapToGlobal(QPoint(0, self.version_btn.height() + 4))
        popup.move(pos)
        if selected_row_btn is not None:
            QTimer.singleShot(0, lambda: scroll.ensureWidgetVisible(selected_row_btn, 0, 60))
        popup.exec()
        if closed_by_trigger["value"]:
            self._suppress_version_press = True
            QTimer.singleShot(50, self._clear_version_suppress)

    def _on_name_user_edited(self, text: str) -> None:
        if not self._updating_name_programmatically:
            self._user_edited_name = bool(text.strip())

    def _sync_default_name_and_decision(self) -> None:
        if not self._user_edited_name:
            self._updating_name_programmatically = True
            self.name_input.setText(f"{self.selected_version} {self.selected_loader}")
            self._updating_name_programmatically = False

        decision = decide_download_strategy(self.selected_version, self.selected_loader, self.store.root)
        self.decision_label.setText(f"{decision.situation} — {decision.action}")

    def get_result_data(self) -> tuple[str, str, str, str]:
        name = self.name_input.text().strip() or f"{self.selected_version} {self.selected_loader}"
        return (name, self.selected_version, self.selected_loader, self.selected_icon_key)


# ---------------------------------------------------------------------------
# Delete Confirmation Modal Dialog (Flat Minimal Frosted Glass)
# ---------------------------------------------------------------------------

class DeleteConfirmationDialog(QDialog):
    """
    Minimal flat frosted confirmation dialog used for the two-step instance deletion confirmation.
    """

    def __init__(
        self,
        title_text: str,
        message_text: str,
        confirm_text: str,
        lato_family: str,
        parent: QWidget | None = None,
        step_badge: str = "",
    ) -> None:
        super().__init__(parent)
        self.lato_family = lato_family

        self.setWindowTitle(title_text)
        self.setWindowFlags(
            Qt.WindowType.Dialog
            | Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.NoDropShadowWindowHint
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setModal(True)
        self.setFixedSize(410, 195)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 20, 24, 20)
        root.setSpacing(10)

        hdr_row = QHBoxLayout()
        hdr_row.setSpacing(10)

        ic_lbl = QLabel()
        ic_lbl.setPixmap(make_ui_icon("delete", 20, QColor(255, 107, 107, 240)))
        ic_lbl.setFixedSize(20, 20)
        ic_lbl.setStyleSheet("background: transparent; border: none;")
        hdr_row.addWidget(ic_lbl)

        title = QLabel(title_text)
        title.setFont(QFont(lato_family, 13, QFont.Weight.Black))
        title.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        hdr_row.addWidget(title)
        hdr_row.addStretch(1)

        if step_badge:
            badge_lbl = QLabel(step_badge)
            badge_lbl.setFont(QFont(lato_family, 8, QFont.Weight.Bold))
            badge_lbl.setStyleSheet(
                """
                QLabel {
                    background-color: rgba(239, 68, 68, 0.22);
                    color: #FCA5A5;
                    border: none;
                    border-radius: 7px;
                    padding: 3px 8px;
                }
                """
            )
            hdr_row.addWidget(badge_lbl)

        root.addLayout(hdr_row)

        msg = QLabel(message_text)
        msg.setWordWrap(True)
        msg.setFont(QFont(lato_family, 10, QFont.Weight.Normal))
        msg.setStyleSheet("color: rgba(255, 255, 255, 0.78); background: transparent; border: none;")
        root.addWidget(msg)

        root.addStretch(1)

        btn_row = QHBoxLayout()
        btn_row.setSpacing(10)
        btn_row.addStretch(1)

        cancel_btn = QPushButton("Cancel")
        cancel_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        cancel_btn.setFixedHeight(36)
        cancel_btn.setMinimumWidth(92)
        cancel_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        cancel_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.08);
                color: rgba(255, 255, 255, 0.85);
                border: none;
                border-radius: 10px;
                padding: 0 16px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.16);
            }
            """
        )
        cancel_btn.clicked.connect(self.reject)
        btn_row.addWidget(cancel_btn)

        confirm_btn = QPushButton(confirm_text)
        confirm_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        confirm_btn.setFixedHeight(36)
        confirm_btn.setMinimumWidth(116)
        confirm_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        confirm_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(239, 68, 68, 0.88);
                color: #FFFFFF;
                border: none;
                border-radius: 10px;
                padding: 0 18px;
            }
            QPushButton:hover {
                background-color: rgba(248, 113, 113, 0.98);
            }
            """
        )
        confirm_btn.clicked.connect(self.accept)
        btn_row.addWidget(confirm_btn)

        root.addLayout(btn_row)

    def showEvent(self, event) -> None:
        super().showEvent(event)
        parent = self.parentWidget()
        if parent is not None:
            win = parent.window()
            if win is not None:
                geo = win.geometry()
                x = geo.x() + (geo.width() - self.width()) // 2
                y = geo.y() + (geo.height() - self.height()) // 2
                self.move(x, y)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)
        painter.setPen(Qt.PenStyle.NoPen)

        path = QPainterPath()
        path.addRoundedRect(QRectF(self.rect()), 18.0, 18.0)

        painter.save()
        painter.setClipPath(path)

        win = self.parentWidget().window() if self.parentWidget() else None
        if win is not None and hasattr(win, "backdrop"):
            blurred_pm = win.backdrop.get_blurred_pixmap(win.size())
            if blurred_pm is not None:
                top_left_in_win = win.mapFromGlobal(self.mapToGlobal(QPoint(0, 0)))
                src_rect = QRect(top_left_in_win, self.size())
                painter.drawPixmap(self.rect(), blurred_pm, src_rect)

        painter.fillPath(path, QColor(14, 26, 46, 220))
        painter.fillPath(path, QColor(255, 255, 255, 24))
        painter.restore()


# ---------------------------------------------------------------------------
# Rename Instance Dialog
# ---------------------------------------------------------------------------

class RenameInstanceDialog(QDialog):
    """Minimal flat frosted modal dialog for renaming an instance."""

    def __init__(self, current_name: str, lato_family: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.lato_family = lato_family
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.Dialog)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFixedSize(380, 180)

        root = QVBoxLayout(self)
        root.setContentsMargins(22, 20, 22, 20)
        root.setSpacing(12)

        hdr_row = QHBoxLayout()
        hdr_row.setSpacing(10)
        ic_lbl = QLabel()
        ic_lbl.setPixmap(make_ui_icon("rename", 20, QColor(255, 255, 255, 230)))
        ic_lbl.setFixedSize(20, 20)
        hdr_row.addWidget(ic_lbl)

        title = QLabel("Rename Installation")
        title.setFont(QFont(lato_family, 12, QFont.Weight.Bold))
        title.setStyleSheet("color: #FFFFFF; background: transparent;")
        hdr_row.addWidget(title, 1)
        root.addLayout(hdr_row)

        self.name_input = QLineEdit(current_name)
        self.name_input.setFont(QFont(lato_family, 10))
        self.name_input.setFixedHeight(36)
        self.name_input.setStyleSheet(
            """
            QLineEdit {
                background-color: rgba(255, 255, 255, 0.08);
                color: #FFFFFF;
                border: 1px solid rgba(255, 255, 255, 0.16);
                border-radius: 8px;
                padding: 0 10px;
            }
            QLineEdit:focus {
                border: 1px solid rgba(255, 255, 255, 0.45);
                background-color: rgba(255, 255, 255, 0.12);
            }
            """
        )
        self.name_input.selectAll()
        root.addWidget(self.name_input)

        root.addStretch(1)

        btn_row = QHBoxLayout()
        btn_row.setSpacing(10)
        btn_row.addStretch(1)

        cancel_btn = QPushButton("Cancel")
        cancel_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        cancel_btn.setFixedHeight(34)
        cancel_btn.setMinimumWidth(85)
        cancel_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        cancel_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.08);
                color: rgba(255, 255, 255, 0.85);
                border: none;
                border-radius: 8px;
                padding: 0 14px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.16);
            }
            """
        )
        cancel_btn.clicked.connect(self.reject)
        btn_row.addWidget(cancel_btn)

        save_btn = QPushButton("Save")
        save_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        save_btn.setFixedHeight(34)
        save_btn.setMinimumWidth(85)
        save_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        save_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.92);
                color: #0C1626;
                border: none;
                border-radius: 8px;
                padding: 0 16px;
            }
            QPushButton:hover {
                background-color: #FFFFFF;
            }
            """
        )
        save_btn.clicked.connect(self.accept)
        btn_row.addWidget(save_btn)

        root.addLayout(btn_row)

    def showEvent(self, event) -> None:
        super().showEvent(event)
        parent = self.parentWidget()
        if parent is not None:
            win = parent.window()
            if win is not None:
                geo = win.geometry()
                x = geo.x() + (geo.width() - self.width()) // 2
                y = geo.y() + (geo.height() - self.height()) // 2
                self.move(x, y)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        path = QPainterPath()
        path.addRoundedRect(QRectF(self.rect()), 16.0, 16.0)
        painter.fillPath(path, QColor(14, 26, 46, 235))
        painter.fillPath(path, QColor(255, 255, 255, 18))

    def get_new_name(self) -> str:
        return self.name_input.text().strip()


# ---------------------------------------------------------------------------
# Edit Instance Settings & Advanced Runtime Configuration Dialog
# ---------------------------------------------------------------------------

class InstanceSettingsDialog(QDialog):
    """
    Flat frosted glass modal dialog for editing an instance's settings:
    - Instance Name & Icon
    - Advanced Java Runtime Configuration (Auto vs Custom Java Path)
    - RAM Allocation (Min MB / Max GB Override)
    - Custom JVM Arguments
    - Game Directory display & Open Folder shortcut
    """

    def __init__(
        self,
        store: LauncherStore,
        instance: InstanceConfig,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.store = store
        self.instance = instance
        self.lato_family = lato_family
        self.selected_icon_key = instance.icon_key

        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.Dialog)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFixedSize(520, 560)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 22)
        root.setSpacing(14)

        # Header: Icon + Title + Version Subtitle
        hdr_row = QHBoxLayout()
        hdr_row.setSpacing(10)
        ic_lbl = QLabel()
        ic_lbl.setPixmap(make_ui_icon("settings", 22, QColor(255, 255, 255, 235)))
        ic_lbl.setFixedSize(22, 22)
        hdr_row.addWidget(ic_lbl)

        title_col = QVBoxLayout()
        title_col.setSpacing(1)
        title = QLabel(f"Settings — {instance.name}")
        title.setFont(QFont(lato_family, 13, QFont.Weight.Bold))
        title.setStyleSheet("color: #FFFFFF; background: transparent;")
        sub = QLabel(f"Minecraft {instance.mc_version} • {instance.loader} • {format_last_played(instance.last_played)}")
        sub.setFont(QFont(lato_family, 8))
        sub.setStyleSheet("color: rgba(255, 255, 255, 0.58); background: transparent;")
        title_col.addWidget(title)
        title_col.addWidget(sub)
        hdr_row.addLayout(title_col, 1)
        root.addLayout(hdr_row)

        scroll = SmoothWheelScrollArea(self)
        scroll.watch_widget(self)
        content_w = QWidget()
        content_w.setStyleSheet("background: transparent;")
        scroll.watch_widget(content_w)
        lay = QVBoxLayout(content_w)
        lay.setContentsMargins(0, 0, 4, 0)
        lay.setSpacing(14)

        # 1. Instance Name
        lbl_name = QLabel("Installation Name")
        lbl_name.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        lbl_name.setStyleSheet("color: rgba(255, 255, 255, 0.75); background: transparent;")
        lay.addWidget(lbl_name)

        self.name_input = QLineEdit(instance.name)
        self.name_input.setFont(QFont(lato_family, 10))
        self.name_input.setFixedHeight(34)
        self.name_input.setStyleSheet(
            """
            QLineEdit {
                background-color: rgba(255, 255, 255, 0.08);
                color: #FFFFFF;
                border: 1px solid rgba(255, 255, 255, 0.16);
                border-radius: 8px;
                padding: 0 10px;
            }
            QLineEdit:focus {
                border: 1px solid rgba(255, 255, 255, 0.45);
                background-color: rgba(255, 255, 255, 0.12);
            }
            """
        )
        lay.addWidget(self.name_input)

        # 2. Icon Selection
        lbl_icon = QLabel("Instance Icon")
        lbl_icon.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        lbl_icon.setStyleSheet("color: rgba(255, 255, 255, 0.75); background: transparent;")
        lay.addWidget(lbl_icon)

        icon_row = QHBoxLayout()
        icon_row.setSpacing(8)
        self.icon_buttons: dict[str, QPushButton] = {}
        for key, _lbl in PRESET_ICONS:
            btn = QPushButton()
            btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            btn.setFixedSize(36, 36)
            btn.setIcon(QIcon(render_instance_icon(key, 22)))
            btn.setIconSize(QSize(22, 22))
            btn.setCheckable(True)
            btn.setChecked(key == self.selected_icon_key)
            bg = "rgba(255, 255, 255, 0.25)" if key == self.selected_icon_key else "rgba(255, 255, 255, 0.06)"
            btn.setStyleSheet(f"background-color: {bg}; border: none; border-radius: 8px;")
            btn.clicked.connect(lambda chk=False, k=key: self._on_icon_picked(k))
            self.icon_buttons[key] = btn
            icon_row.addWidget(btn)
        icon_row.addStretch(1)
        lay.addLayout(icon_row)

        # 3. Java Runtime Management
        lbl_java = QLabel("Java Runtime Management")
        lbl_java.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        lbl_java.setStyleSheet("color: rgba(255, 255, 255, 0.75); background: transparent;")
        lay.addWidget(lbl_java)

        compat, req_major, det_major, auto_path = check_java_compatibility(instance.mc_version, store.root)
        status_color = "#34D399" if compat else "#F87171"
        status_text = f"Required: Java {req_major}+ (Detected: Java {det_major or 'None'})"
        java_info = QLabel(status_text)
        java_info.setFont(QFont(lato_family, 8))
        java_info.setStyleSheet(f"color: {status_color}; background: transparent;")
        lay.addWidget(java_info)

        self.custom_java_cb = QCheckBox("Use Custom Java Executable")
        self.custom_java_cb.setChecked(bool(instance.java_path))
        self.custom_java_cb.setFont(QFont(lato_family, 9))
        self.custom_java_cb.setStyleSheet("color: #FFFFFF; background: transparent;")
        self.custom_java_cb.toggled.connect(self._toggle_custom_java)
        lay.addWidget(self.custom_java_cb)

        java_row = QHBoxLayout()
        java_row.setSpacing(8)
        self.java_path_input = QLineEdit(instance.java_path or auto_path)
        self.java_path_input.setFont(QFont(lato_family, 9))
        self.java_path_input.setFixedHeight(32)
        self.java_path_input.setEnabled(bool(instance.java_path))
        self.java_path_input.setStyleSheet(
            """
            QLineEdit {
                background-color: rgba(255, 255, 255, 0.08);
                color: #FFFFFF;
                border: 1px solid rgba(255, 255, 255, 0.16);
                border-radius: 8px;
                padding: 0 8px;
            }
            QLineEdit:disabled {
                color: rgba(255, 255, 255, 0.35);
                background-color: rgba(255, 255, 255, 0.03);
            }
            """
        )
        java_row.addWidget(self.java_path_input, 1)

        self.browse_java_btn = QPushButton("Browse...")
        self.browse_java_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.browse_java_btn.setFixedHeight(32)
        self.browse_java_btn.setFixedWidth(80)
        self.browse_java_btn.setEnabled(bool(instance.java_path))
        self.browse_java_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.browse_java_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.12);
                color: #FFFFFF;
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.22);
            }
            QPushButton:disabled {
                background-color: rgba(255, 255, 255, 0.04);
                color: rgba(255, 255, 255, 0.25);
            }
            """
        )
        self.browse_java_btn.clicked.connect(self._browse_java)
        java_row.addWidget(self.browse_java_btn)
        lay.addLayout(java_row)

        # 4. RAM Allocation
        lbl_ram = QLabel("RAM Allocation")
        lbl_ram.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        lbl_ram.setStyleSheet("color: rgba(255, 255, 255, 0.75); background: transparent;")
        lay.addWidget(lbl_ram)

        self.override_ram_cb = QCheckBox("Override Global RAM Allocation")
        self.override_ram_cb.setChecked(instance.max_ram_gb is not None)
        self.override_ram_cb.setFont(QFont(lato_family, 9))
        self.override_ram_cb.setStyleSheet("color: #FFFFFF; background: transparent;")
        self.override_ram_cb.toggled.connect(self._toggle_override_ram)
        lay.addWidget(self.override_ram_cb)

        ram_row = QHBoxLayout()
        ram_row.setSpacing(12)
        ram_lbl = QLabel("Maximum RAM (GB):")
        ram_lbl.setFont(QFont(lato_family, 9))
        ram_lbl.setStyleSheet("color: rgba(255, 255, 255, 0.85); background: transparent;")
        ram_row.addWidget(ram_lbl)

        self.max_ram_spin = QSpinBox()
        self.max_ram_spin.setRange(1, 64)
        self.max_ram_spin.setValue(instance.max_ram_gb or store.max_ram_gb)
        self.max_ram_spin.setEnabled(instance.max_ram_gb is not None)
        self.max_ram_spin.setFixedHeight(30)
        self.max_ram_spin.setFixedWidth(70)
        self.max_ram_spin.setStyleSheet(
            """
            QSpinBox {
                background-color: rgba(255, 255, 255, 0.08);
                color: #FFFFFF;
                border: 1px solid rgba(255, 255, 255, 0.16);
                border-radius: 6px;
                padding-left: 6px;
            }
            QSpinBox:disabled {
                color: rgba(255, 255, 255, 0.35);
            }
            """
        )
        ram_row.addWidget(self.max_ram_spin)
        ram_row.addStretch(1)
        lay.addLayout(ram_row)

        # 5. Custom JVM Arguments
        lbl_jvm = QLabel("Additional JVM Arguments")
        lbl_jvm.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        lbl_jvm.setStyleSheet("color: rgba(255, 255, 255, 0.75); background: transparent;")
        lay.addWidget(lbl_jvm)

        self.jvm_input = QLineEdit(instance.jvm_args)
        self.jvm_input.setPlaceholderText("e.g. -XX:+UseG1GC -Dexample=true")
        self.jvm_input.setFont(QFont(lato_family, 9))
        self.jvm_input.setFixedHeight(32)
        self.jvm_input.setStyleSheet(
            """
            QLineEdit {
                background-color: rgba(255, 255, 255, 0.08);
                color: #FFFFFF;
                border: 1px solid rgba(255, 255, 255, 0.16);
                border-radius: 8px;
                padding: 0 10px;
            }
            QLineEdit:focus {
                border: 1px solid rgba(255, 255, 255, 0.45);
            }
            """
        )
        lay.addWidget(self.jvm_input)

        # 6. Isolated Game Directory Info
        lbl_dir = QLabel("Isolated Game Directory")
        lbl_dir.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        lbl_dir.setStyleSheet("color: rgba(255, 255, 255, 0.75); background: transparent;")
        lay.addWidget(lbl_dir)

        dir_row = QHBoxLayout()
        dir_row.setSpacing(8)
        dir_val = QLabel(str(instance.resolved_game_dir()))
        dir_val.setFont(QFont(lato_family, 8))
        dir_val.setStyleSheet("color: rgba(255, 255, 255, 0.55); background: transparent;")
        dir_val.setWordWrap(True)
        dir_row.addWidget(dir_val, 1)

        open_folder_btn = QPushButton("Open")
        open_folder_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        open_folder_btn.setFixedHeight(28)
        open_folder_btn.setFixedWidth(65)
        open_folder_btn.setFont(QFont(lato_family, 8, QFont.Weight.Bold))
        open_folder_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.10);
                color: #FFFFFF;
                border: none;
                border-radius: 6px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.20);
            }
            """
        )
        open_folder_btn.clicked.connect(lambda: os.startfile(str(instance.resolved_game_dir())) if os.name == "nt" else None)
        dir_row.addWidget(open_folder_btn)
        lay.addLayout(dir_row)

        scroll.setWidget(content_w)
        root.addWidget(scroll, 1)

        # Bottom Buttons
        btn_row = QHBoxLayout()
        btn_row.setSpacing(10)
        btn_row.addStretch(1)

        cancel_btn = QPushButton("Cancel")
        cancel_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        cancel_btn.setFixedHeight(34)
        cancel_btn.setMinimumWidth(85)
        cancel_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        cancel_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.08);
                color: rgba(255, 255, 255, 0.85);
                border: none;
                border-radius: 8px;
                padding: 0 14px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.16);
            }
            """
        )
        cancel_btn.clicked.connect(self.reject)
        btn_row.addWidget(cancel_btn)

        save_btn = QPushButton("Save Settings")
        save_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        save_btn.setFixedHeight(34)
        save_btn.setMinimumWidth(110)
        save_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        save_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.92);
                color: #0C1626;
                border: none;
                border-radius: 8px;
                padding: 0 16px;
            }
            QPushButton:hover {
                background-color: #FFFFFF;
            }
            """
        )
        save_btn.clicked.connect(self._save_settings)
        btn_row.addWidget(save_btn)
        root.addLayout(btn_row)

    def _on_icon_picked(self, key: str) -> None:
        self.selected_icon_key = key
        for k, btn in self.icon_buttons.items():
            btn.setChecked(k == key)
            bg = "rgba(255, 255, 255, 0.25)" if k == key else "rgba(255, 255, 255, 0.06)"
            btn.setStyleSheet(f"background-color: {bg}; border: none; border-radius: 8px;")

    def _toggle_custom_java(self, enabled: bool) -> None:
        self.java_path_input.setEnabled(enabled)
        self.browse_java_btn.setEnabled(enabled)

    def _toggle_override_ram(self, enabled: bool) -> None:
        self.max_ram_spin.setEnabled(enabled)

    def _browse_java(self) -> None:
        selected, _ = QFileDialog.getOpenFileName(
            self,
            "Select Java Executable",
            "C:\\",
            "Java Executable (java.exe javaw.exe);;All Files (*.*)",
        )
        if selected:
            self.java_path_input.setText(selected)

    def _save_settings(self) -> None:
        name = self.name_input.text().strip() or self.instance.name
        max_ram = self.max_ram_spin.value() if self.override_ram_cb.isChecked() else None
        java_p = self.java_path_input.text().strip() if self.custom_java_cb.isChecked() else ""
        jvm = self.jvm_input.text().strip()

        self.store.update_instance_settings(
            instance_id=self.instance.id,
            name=name,
            icon_key=self.selected_icon_key,
            max_ram_gb=max_ram,
            java_path=java_p,
            jvm_args=jvm,
        )
        self.accept()

    def showEvent(self, event) -> None:
        super().showEvent(event)
        parent = self.parentWidget()
        if parent is not None:
            win = parent.window()
            if win is not None:
                geo = win.geometry()
                x = geo.x() + (geo.width() - self.width()) // 2
                y = geo.y() + (geo.height() - self.height()) // 2
                self.move(x, y)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        path = QPainterPath()
        path.addRoundedRect(QRectF(self.rect()), 18.0, 18.0)
        painter.fillPath(path, QColor(14, 26, 46, 240))
        painter.fillPath(path, QColor(255, 255, 255, 18))


# ---------------------------------------------------------------------------
# Java Missing or Incompatible Guidance Dialog
# ---------------------------------------------------------------------------

class JavaMissingOrIncompatibleDialog(QDialog):
    """
    Guides the user when a Minecraft version requires a Java version that is missing or incompatible.
    Provides direct Adoptium download links and a file browser to locate Java on their PC.
    """

    def __init__(
        self,
        mc_version: str,
        req_major: int,
        det_major: int | None,
        current_path: str,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.mc_version = mc_version
        self.req_major = req_major
        self.lato_family = lato_family
        self.selected_java_path: str = ""

        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.Dialog)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setFixedSize(450, 260)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 22)
        root.setSpacing(12)

        hdr_row = QHBoxLayout()
        hdr_row.setSpacing(10)
        ic_lbl = QLabel()
        ic_lbl.setPixmap(make_ui_icon("help", 22, QColor(251, 191, 36, 240)))
        ic_lbl.setFixedSize(22, 22)
        hdr_row.addWidget(ic_lbl)

        title = QLabel(f"Java {req_major} Required")
        title.setFont(QFont(lato_family, 13, QFont.Weight.Bold))
        title.setStyleSheet("color: #FFFFFF; background: transparent;")
        hdr_row.addWidget(title, 1)
        root.addLayout(hdr_row)

        desc = QLabel(
            f"Minecraft <b>{mc_version}</b> was compiled for <b>Java {req_major}</b> "
            f"(bytecode class {44 + req_major}.0).<br><br>"
            f"Detected runtime: <b>Java {det_major or 'Not Found'}</b>.<br>"
            f"Please download the official Java {req_major} runtime or browse for an installed Java executable."
        )
        desc.setWordWrap(True)
        desc.setFont(QFont(lato_family, 9))
        desc.setStyleSheet("color: rgba(255, 255, 255, 0.78); background: transparent;")
        root.addWidget(desc)

        root.addStretch(1)

        btn_row = QHBoxLayout()
        btn_row.setSpacing(8)

        cancel_btn = QPushButton("Cancel")
        cancel_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        cancel_btn.setFixedHeight(34)
        cancel_btn.setMinimumWidth(75)
        cancel_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        cancel_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.08);
                color: rgba(255, 255, 255, 0.85);
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.16);
            }
            """
        )
        cancel_btn.clicked.connect(self.reject)
        btn_row.addWidget(cancel_btn)

        browse_btn = QPushButton("Browse for Java...")
        browse_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        browse_btn.setFixedHeight(34)
        browse_btn.setMinimumWidth(130)
        browse_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        browse_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.14);
                color: #FFFFFF;
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.24);
            }
            """
        )
        browse_btn.clicked.connect(self._on_browse)
        btn_row.addWidget(browse_btn)

        download_btn = QPushButton(f"Download Java {req_major}")
        download_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        download_btn.setFixedHeight(34)
        download_btn.setMinimumWidth(140)
        download_btn.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        download_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.92);
                color: #0C1626;
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: #FFFFFF;
            }
            """
        )
        download_btn.clicked.connect(self._on_download)
        btn_row.addWidget(download_btn)

        root.addLayout(btn_row)

    def _on_download(self) -> None:
        url = get_java_download_url(self.req_major)
        QDesktopServices.openUrl(QUrl(url))

    def _on_browse(self) -> None:
        selected, _ = QFileDialog.getOpenFileName(
            self,
            f"Select Java {self.req_major} Executable",
            "C:\\",
            "Java Executable (java.exe javaw.exe);;All Files (*.*)",
        )
        if selected:
            maj = inspect_java_major(Path(selected))
            if maj and maj >= self.req_major:
                self.selected_java_path = selected
                self.accept()
            else:
                QMessageBox.warning(
                    self,
                    "Incompatible Java",
                    f"The selected executable is Java {maj or 'Unknown'}.\n"
                    f"Minecraft {self.mc_version} requires Java {self.req_major} or higher."
                )

    def showEvent(self, event) -> None:
        super().showEvent(event)
        parent = self.parentWidget()
        if parent is not None:
            win = parent.window()
            if win is not None:
                geo = win.geometry()
                x = geo.x() + (geo.width() - self.width()) // 2
                y = geo.y() + (geo.height() - self.height()) // 2
                self.move(x, y)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        path = QPainterPath()
        path.addRoundedRect(QRectF(self.rect()), 16.0, 16.0)
        painter.fillPath(path, QColor(14, 26, 46, 245))
        painter.fillPath(path, QColor(255, 255, 255, 18))


# ---------------------------------------------------------------------------
# Installed Game Versions Page (Manage & Completely Uninstall Game Versions)
# ---------------------------------------------------------------------------

class InstalledVersionCard(FrostedGlassPanel):
    """
    Compact flat frosted glass card representing an installed Minecraft game version in `versions/`.
    Allows opening the version folder or completely uninstalling the game version from disk.
    """

    open_folder_clicked = Signal(str)
    uninstall_clicked = Signal(str)

    def __init__(
        self,
        backdrop: BackdropManager,
        version_info: InstalledGameVersion,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            backdrop=backdrop,
            radius=12,
            tint_color=QColor(255, 255, 255, 26),
            parent=parent,
        )
        self.version_info = version_info
        self.lato_family = lato_family
        self.setFixedHeight(56)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(10, 0, 10, 0)
        layout.setSpacing(10)

        icon_key = "crafting_table" if version_info.mc_version.startswith(("b", "a")) else "grass"
        icon_lbl = QLabel()
        icon_lbl.setPixmap(render_instance_icon(icon_key, 30))
        icon_lbl.setFixedSize(30, 30)
        icon_lbl.setStyleSheet("background: transparent; border: none;")
        layout.addWidget(icon_lbl)

        meta_col = QVBoxLayout()
        meta_col.setContentsMargins(0, 8, 0, 8)
        meta_col.setSpacing(0)

        ver_lbl = QLabel(version_info.mc_version)
        ver_lbl.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        ver_lbl.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")

        sub_text = f"{version_info.loaders_label}  ·  {version_info.size_label}"
        if version_info.instance_count > 0:
            plural = "s" if version_info.instance_count != 1 else ""
            sub_text += f"  ·  {version_info.instance_count} instance{plural}"
        sub_lbl = QLabel(sub_text)
        sub_lbl.setFont(QFont(lato_family, 8, QFont.Weight.Normal))
        sub_lbl.setStyleSheet("color: rgba(255, 255, 255, 0.62); background: transparent; border: none;")

        meta_col.addWidget(ver_lbl)
        meta_col.addWidget(sub_lbl)
        layout.addLayout(meta_col, 1)

        folder_btn = QPushButton()
        folder_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        folder_btn.setToolTip("Open version folder")
        folder_btn.setFixedSize(30, 30)
        folder_btn.setIcon(QIcon(make_ui_icon("folder", 15, QColor(255, 255, 255, 220))))
        folder_btn.setIconSize(QSize(15, 15))
        folder_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.10);
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.20);
            }
            QPushButton:pressed {
                background-color: rgba(255, 255, 255, 0.26);
            }
            """
        )
        folder_btn.clicked.connect(lambda: self.open_folder_clicked.emit(self.version_info.mc_version))
        layout.addWidget(folder_btn)

        uninstall_btn = QPushButton()
        uninstall_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        uninstall_btn.setToolTip("Uninstall game version completely")
        uninstall_btn.setFixedSize(30, 30)
        uninstall_btn.setIcon(QIcon(make_ui_icon("delete", 15, QColor(255, 107, 107, 240))))
        uninstall_btn.setIconSize(QSize(15, 15))
        uninstall_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(239, 68, 68, 0.16);
                border: none;
                border-radius: 8px;
            }
            QPushButton:hover {
                background-color: rgba(239, 68, 68, 0.30);
            }
            QPushButton:pressed {
                background-color: rgba(239, 68, 68, 0.40);
            }
            """
        )
        uninstall_btn.clicked.connect(lambda: self.uninstall_clicked.emit(self.version_info.mc_version))
        layout.addWidget(uninstall_btn)

    def enterEvent(self, event) -> None:
        self.tint_color = QColor(255, 255, 255, 42)
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event) -> None:
        self.tint_color = QColor(255, 255, 255, 26)
        self.update()
        super().leaveEvent(event)

    def contextMenuEvent(self, event) -> None:
        menu = ApplePopoverMenu(self.lato_family, min_width=195, parent=self)
        mc_ver = self.version_info.mc_version

        menu.add_item(
            icon_pm=make_ui_icon("folder", 16, QColor(255, 255, 255, 215)),
            title="Open Version Folder",
            on_click=lambda: self.open_folder_clicked.emit(mc_ver),
        )
        menu.add_separator()
        menu.add_item(
            icon_pm=make_ui_icon("delete", 16, QColor(255, 107, 107, 235)),
            title="Uninstall Completely",
            destructive=True,
            on_click=lambda: self.uninstall_clicked.emit(mc_ver),
        )

        menu.move(event.globalPos())
        menu.exec()


class InstalledVersionsPage(QWidget):
    """
    Page showing all installed Minecraft game versions in `versions/`.
    Game versions stay installed even if all their instances are deleted,
    and can be completely uninstalled from here.
    """

    version_open_folder = Signal(str)
    version_uninstall = Signal(str)

    def __init__(
        self,
        backdrop: BackdropManager,
        store: LauncherStore,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.backdrop = backdrop
        self.store = store
        self.lato_family = lato_family

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(12)

        # Header Panel
        self.header_panel = FrostedGlassPanel(
            backdrop=backdrop,
            radius=20,
            tint_color=QColor(255, 255, 255, 26),
            parent=self,
        )
        self.header_panel.setFixedHeight(124)

        hdr_layout = QVBoxLayout(self.header_panel)
        hdr_layout.setContentsMargins(32, 20, 32, 20)
        hdr_layout.setSpacing(6)
        hdr_layout.addStretch(1)

        self.title_label = QLabel("Installed Game Versions")
        title_font = QFont(lato_family, 22, QFont.Weight.Black)
        title_font.setLetterSpacing(QFont.SpacingType.AbsoluteSpacing, -0.3)
        self.title_label.setFont(title_font)
        self.title_label.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        hdr_layout.addWidget(self.title_label)

        self.subtitle_label = QLabel(
            "Game versions stay installed even after deleting all their instances. Uninstall a version here to remove it completely."
        )
        self.subtitle_label.setFont(QFont(lato_family, 9, QFont.Weight.Normal))
        self.subtitle_label.setStyleSheet("color: rgba(255, 255, 255, 0.65); background: transparent; border: none;")
        hdr_layout.addWidget(self.subtitle_label)

        hdr_layout.addStretch(1)
        main_layout.addWidget(self.header_panel)

        # Grid Container
        self.grid_widget = QWidget(self)
        self.grid_layout = QGridLayout(self.grid_widget)
        self.grid_layout.setContentsMargins(0, 0, 0, 0)
        self.grid_layout.setHorizontalSpacing(10)
        self.grid_layout.setVerticalSpacing(10)
        main_layout.addWidget(self.grid_widget, 1)

        self.refresh_versions()

    def refresh_versions(self) -> None:
        while self.grid_layout.count():
            item = self.grid_layout.takeAt(0)
            w = item.widget()
            if w is not None:
                w.deleteLater()

        installed = self.store.get_installed_game_versions()
        count = len(installed)
        if count == 0:
            self.subtitle_label.setText("No game versions currently installed on disk.")
            empty_card = FrostedGlassPanel(
                backdrop=self.backdrop,
                radius=14,
                tint_color=QColor(255, 255, 255, 18),
                parent=self.grid_widget,
            )
            empty_card.setFixedHeight(64)
            el = QHBoxLayout(empty_card)
            el.setContentsMargins(20, 0, 20, 0)
            lbl = QLabel("No installed game versions found. Install a version from the Home tab to see it here.")
            lbl.setFont(QFont(self.lato_family, 9, QFont.Weight.Medium))
            lbl.setStyleSheet("color: rgba(255, 255, 255, 0.58); background: transparent; border: none;")
            el.addWidget(lbl)
            self.grid_layout.addWidget(empty_card, 0, 0, 1, 3)
            self.grid_layout.setRowStretch(1, 1)
            return

        plural = "s" if count != 1 else ""
        self.subtitle_label.setText(
            f"{count} game version{plural} installed on disk  ·  Kept even when instances are deleted"
        )

        cols = 3
        for c in range(cols):
            self.grid_layout.setColumnStretch(c, 1)

        for idx, ver_info in enumerate(installed):
            card = InstalledVersionCard(
                backdrop=self.backdrop,
                version_info=ver_info,
                lato_family=self.lato_family,
                parent=self.grid_widget,
            )
            card.open_folder_clicked.connect(self.version_open_folder.emit)
            card.uninstall_clicked.connect(self.version_uninstall.emit)
            self.grid_layout.addWidget(card, idx // cols, idx % cols)

        total_rows = (max(0, count - 1) // cols) + 1
        self.grid_layout.setRowStretch(total_rows, 1)


# ---------------------------------------------------------------------------
# Help & FAQs Page (Comprehensive FAQ Database + Live Search Bar)
# ---------------------------------------------------------------------------

FAQ_DATABASE: list[dict[str, str]] = [
    {
        "category": "Instances",
        "question": "How do I create and launch a new Minecraft installation?",
        "answer": (
            "On the Home tab, open the Hero dropdown and select 'New Installation', then click 'Install'. "
            "Choose a square pixelated icon, enter a custom name (or keep the default), select your mod loader "
            "(Vanilla, Fabric, Forge, or Quilt), and pick any Minecraft release or snapshot from the scrollable version dropdown."
        ),
        "keywords": "create new installation install launch play version snapshot release dropdown hero",
    },
    {
        "category": "Instances",
        "question": "How do I rename, duplicate, or edit settings for an instance?",
        "answer": (
            "Right-click any instance card on the Home tab to open its frosted context menu. "
            "Select 'Rename' to give it a custom name, 'Duplicate' to clone its settings and worlds into an independent new instance, "
            "or 'Edit Settings' to adjust its icon, allocated RAM, custom Java runtime path, and JVM arguments."
        ),
        "keywords": "rename duplicate edit settings ram java jvm args clone instance last played",
    },
    {
        "category": "Instances",
        "question": "What is the difference between deleting an Instance and uninstalling a Game Version?",
        "answer": (
            "Deleting an instance (via right-click -> Delete on the Home tab) only removes that instance's isolated folder "
            "in 'instances/<id>' (its local mods, configs, and saves). The core Minecraft game files in 'versions/' remain "
            "downloaded and intact so you never have to re-download the base game or affect other instances."
        ),
        "keywords": "delete instance vs uninstall game version difference keep game files folder safe shared",
    },
    {
        "category": "Instances",
        "question": "Why does deleting an instance or uninstalling a game version ask for 2 confirmations?",
        "answer": (
            "Lightyear uses a strict 2-step confirmation flow ('Step 1 of 2' followed by 'Step 2 of 2 · Final') before "
            "permanently removing any instance folder or game version from disk to prevent accidental data loss."
        ),
        "keywords": "two 2 confirmations delete dialog safety accidental remove",
    },
    {
        "category": "Game Files",
        "question": "How do I completely uninstall a Minecraft game version from my computer?",
        "answer": (
            "Click the 'Installed' button in the top navigation bar. Every downloaded Minecraft version on disk is listed "
            "there—even if you deleted all of its instances. Click the red trash button (or right-click -> 'Uninstall Completely') "
            "and confirm twice to remove the version and its loader profiles from disk."
        ),
        "keywords": "installed page tab completely uninstall remove game version disk space",
    },
    {
        "category": "Game Files",
        "question": "How does the Smart Download Decision Matrix work?",
        "answer": (
            "Before downloading anything, Lightyear inspects 'versions/<mc_version>/<mc_version>.jar' and loader profiles. "
            "If the base game is already downloaded on disk, Lightyear skips re-downloading the game engine and assets, "
            "reusing existing files to create new Vanilla or modded instances in seconds."
        ),
        "keywords": "smart download decision matrix reuse fast installjar downloaded disk",
    },
    {
        "category": "Game Files",
        "question": "How do download verification, retries, cancellation, and repair work?",
        "answer": (
            "Lightyear verifies the SHA-1 checksum and ZIP/JAR integrity of downloaded files with automatic exponential "
            "backoff retries. Downloads can be cancelled at any time via the 'Cancel' button in the progress bar dock. "
            "If files ever become damaged or interrupted, right-click the card and choose 'Repair Installation' to automatically "
            "recover missing or damaged files without losing your saves or options."
        ),
        "keywords": "download verification repair integrity retry cancellation recover corrupted files",
    },
    {
        "category": "Mods & Shaders",
        "question": "How do I add Mods, Resource Packs, or Shaderpacks to an instance?",
        "answer": (
            "Right-click any instance block on the Home tab to open the frosted context menu, then click "
            "'Add Resource Packs' (.zip), 'Add Mods' (.jar), or 'Add Shaderpacks' (.zip). Selected files are copied "
            "directly into that instance's isolated folder. You can also click the Folder icon on the card at any time."
        ),
        "keywords": "add mods resource packs shaderpacks right click context menu jar zip folder",
    },
    {
        "category": "Mods & Shaders",
        "question": "Which Mod Loaders are supported in Lightyear?",
        "answer": (
            "Lightyear supports Vanilla, Fabric, Forge, and Quilt. Fabric and Quilt loader profiles and Maven libraries "
            "are resolved and downloaded automatically from their official meta APIs during the 5-step installation pipeline."
        ),
        "keywords": "mod loaders supported vanilla fabric forge quilt maven",
    },
    {
        "category": "Account",
        "question": "How do I change my username, and how does saving work?",
        "answer": (
            "Double-click the 'hello! username' pill on the top-right of the navigation bar. Type your desired username "
            "(up to 16 characters, letters/numbers/underscores) and press Enter or click anywhere outside the input box "
            "in the app to automatically save and exit edit mode."
        ),
        "keywords": "change edit username hello pill double click save offline name",
    },
    {
        "category": "Account",
        "question": "How does Offline Mode authentication and player UUID work?",
        "answer": (
            "Lightyear generates a deterministic Offline UUID v3 using the standard 'OfflinePlayer:<username>' algorithm. "
            "This ensures your inventory, advancements, pet ownership, and singleplayer/LAN player data stay consistent "
            "every time you launch with the same username."
        ),
        "keywords": "offline mode uuid v3 authentication player data inventory lan",
    },
    {
        "category": "Instances",
        "question": "How do I customize an instance with cute pixelated square icons?",
        "answer": (
            "In the 'New Installation' dialog or 'Edit Settings', choose from the 8 built-in pixelated square icons (Grass, Diamond, Crafting, "
            "Command, Ender, Redstone, Beacon, Furnace) or pick any custom square image."
        ),
        "keywords": "custom icon pixelated cute aesthetic square icons png assets",
    },
    {
        "category": "Troubleshooting",
        "question": "Which Java runtime version do I need for different Minecraft versions?",
        "answer": (
            "Lightyear inspects the exact bytecode requirement for each version: Java 25+ for Minecraft 26.x, "
            "Java 21 for 1.20.5–1.21, Java 17 for 1.17–1.20.4, and Java 8 for 1.16.5 and older. "
            "If compatible Java is missing, Lightyear displays a guidance dialog with direct Adoptium download links or lets you "
            "browse for any Java executable on your system."
        ),
        "keywords": "java runtime version 25 21 17 8 jdk crash launch error adoptium custom path",
    },
    {
        "category": "Troubleshooting",
        "question": "Why does the Lightyear launcher window disappear when Minecraft starts?",
        "answer": (
            "To keep your desktop clean and conserve system resources while gaming, Lightyear automatically hides its window "
            "once the Minecraft process starts and automatically restores itself to the foreground when you close the game."
        ),
        "keywords": "window hides disappears launch minimize restore background",
    },
    {
        "category": "Game Files",
        "question": "Where are my worlds (saves), screenshots, and game files stored?",
        "answer": (
            "Each instance has its own isolated directory inside 'instances/<instance_id>/' containing 'saves/', 'mods/', "
            "'resourcepacks/', 'shaderpacks/', and 'screenshots/'. Shared engine files live in 'versions/', 'libraries/', "
            "and 'assets/'. Click the Folder button on any card to open its directory directly in Explorer."
        ),
        "keywords": "where worlds saves screenshots files stored directory explorer path",
    },
    {
        "category": "Troubleshooting",
        "question": "Why can't I see snapshots or older Beta/Alpha versions in the version dropdown?",
        "answer": (
            "By default, the New Installation dialog filters the dropdown to stable 'Release' versions for clarity. "
            "Check the 'All versions' box right above the version dropdown to include Snapshots, Beta, and Alpha versions, "
            "then use your mouse scroll wheel to browse the full list."
        ),
        "keywords": "snapshots beta alpha all versions checkbox dropdown scroll wheel",
    },
]


class FaqItemCard(FrostedGlassPanel):
    """
    Flat frosted glass card displaying a single FAQ question, category badge, and detailed answer.
    """

    def __init__(
        self,
        backdrop: BackdropManager,
        category: str,
        question: str,
        answer: str,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            backdrop=backdrop,
            radius=14,
            tint_color=QColor(255, 255, 255, 24),
            parent=parent,
        )
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(5)

        top_row = QHBoxLayout()
        top_row.setSpacing(8)

        q_lbl = QLabel(question)
        q_lbl.setWordWrap(True)
        q_lbl.setFont(QFont(lato_family, 10, QFont.Weight.Bold))
        q_lbl.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        top_row.addWidget(q_lbl, 1)

        cat_lbl = QLabel(category)
        cat_lbl.setFont(QFont(lato_family, 8, QFont.Weight.Bold))
        cat_lbl.setStyleSheet(
            """
            QLabel {
                background-color: rgba(255, 255, 255, 0.11);
                color: rgba(255, 255, 255, 0.78);
                border: none;
                border-radius: 7px;
                padding: 2px 8px;
            }
            """
        )
        top_row.addWidget(cat_lbl)
        layout.addLayout(top_row)

        a_lbl = QLabel(answer)
        a_lbl.setWordWrap(True)
        a_lbl.setFont(QFont(lato_family, 9, QFont.Weight.Normal))
        a_lbl.setStyleSheet("color: rgba(255, 255, 255, 0.68); background: transparent; border: none;")
        layout.addWidget(a_lbl)


class HelpPage(QWidget):
    """
    Help & FAQs page next to 'Installed' in the navbar.
    Features a live search bar, category filter pills, and a smooth mouse-wheel-scrollable list of FAQs.
    """

    def __init__(
        self,
        backdrop: BackdropManager,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.backdrop = backdrop
        self.lato_family = lato_family
        self._active_category: str = "All"

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(12)

        # Top Header + Search Bar Frosted Panel
        self.header_panel = FrostedGlassPanel(
            backdrop=backdrop,
            radius=20,
            tint_color=QColor(255, 255, 255, 26),
            parent=self,
        )
        self.header_panel.setFixedHeight(148)

        hdr_layout = QVBoxLayout(self.header_panel)
        hdr_layout.setContentsMargins(28, 16, 28, 14)
        hdr_layout.setSpacing(8)

        title_row = QHBoxLayout()
        title_row.setSpacing(10)

        self.title_label = QLabel("Help & Frequently Asked Questions")
        title_font = QFont(lato_family, 19, QFont.Weight.Black)
        title_font.setLetterSpacing(QFont.SpacingType.AbsoluteSpacing, -0.3)
        self.title_label.setFont(title_font)
        self.title_label.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        title_row.addWidget(self.title_label)

        title_row.addStretch(1)

        self.count_label = QLabel(f"{len(FAQ_DATABASE)} FAQs")
        self.count_label.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.count_label.setStyleSheet("color: rgba(255, 255, 255, 0.58); background: transparent; border: none;")
        title_row.addWidget(self.count_label)
        hdr_layout.addLayout(title_row)

        # Search Bar Row
        search_frame = QFrame()
        search_frame.setFixedHeight(38)
        search_frame.setStyleSheet(
            """
            QFrame {
                background-color: rgba(255, 255, 255, 0.10);
                border: none;
                border-radius: 11px;
            }
            """
        )
        sf_layout = QHBoxLayout(search_frame)
        sf_layout.setContentsMargins(12, 0, 10, 0)
        sf_layout.setSpacing(8)

        search_ic = QLabel()
        search_ic.setPixmap(make_ui_icon("search", 15, QColor(255, 255, 255, 190)))
        search_ic.setFixedSize(15, 15)
        search_ic.setStyleSheet("background: transparent; border: none;")
        sf_layout.addWidget(search_ic)

        self.search_input = QLineEdit()
        self.search_input.setFont(QFont(lato_family, 9, QFont.Weight.Medium))
        self.search_input.setPlaceholderText(
            "Search any query (e.g. delete instance, uninstall, mods, shaders, username, Java, Fabric)..."
        )
        self.search_input.setStyleSheet(
            """
            QLineEdit {
                background: transparent;
                border: none;
                color: #FFFFFF;
                selection-background-color: rgba(255, 255, 255, 0.25);
            }
            """
        )
        self.search_input.textChanged.connect(self._on_search_changed)
        sf_layout.addWidget(self.search_input, 1)

        self.clear_btn = QPushButton()
        self.clear_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.clear_btn.setToolTip("Clear search")
        self.clear_btn.setFixedSize(22, 22)
        self.clear_btn.setIcon(QIcon(make_ui_icon("close", 12, QColor(255, 255, 255, 200))))
        self.clear_btn.setIconSize(QSize(12, 12))
        self.clear_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.10);
                border: none;
                border-radius: 7px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.20);
            }
            """
        )
        self.clear_btn.clicked.connect(lambda: self.search_input.setText(""))
        self.clear_btn.hide()
        sf_layout.addWidget(self.clear_btn)

        hdr_layout.addWidget(search_frame)

        # Category Filter Pills
        cat_row = QHBoxLayout()
        cat_row.setContentsMargins(0, 0, 0, 0)
        cat_row.setSpacing(6)
        self.category_buttons: dict[str, QPushButton] = {}
        for cat in ("All", "Instances", "Game Files", "Mods & Shaders", "Account", "Troubleshooting"):
            c_btn = QPushButton(cat.replace("&", "&&"))
            c_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            c_btn.setFixedHeight(24)
            c_btn.setFont(QFont(lato_family, 8, QFont.Weight.Bold))
            c_btn.clicked.connect(lambda checked=False, c=cat: self._set_category(c))
            self.category_buttons[cat] = c_btn
            cat_row.addWidget(c_btn)
        cat_row.addStretch(1)
        hdr_layout.addLayout(cat_row)

        main_layout.addWidget(self.header_panel)

        # Scrollable FAQ Cards Area
        self.scroll_area = SmoothWheelScrollArea(self)
        self.scroll_area.watch_widget(self)
        self.faq_container = QWidget()
        self.faq_container.setStyleSheet("background: transparent;")
        self.scroll_area.watch_widget(self.faq_container)

        self.faq_layout = QVBoxLayout(self.faq_container)
        self.faq_layout.setContentsMargins(0, 0, 4, 0)
        self.faq_layout.setSpacing(9)

        self.scroll_area.setWidget(self.faq_container)
        main_layout.addWidget(self.scroll_area, 1)

        self._update_category_styles()
        self.refresh_faqs()

    def _set_category(self, category: str) -> None:
        self._active_category = category
        self._update_category_styles()
        self.refresh_faqs()

    def _update_category_styles(self) -> None:
        active_css = """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.22);
                color: #FFFFFF;
                border: none;
                border-radius: 7px;
                padding: 0 11px;
            }
        """
        inactive_css = """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.07);
                color: rgba(255, 255, 255, 0.62);
                border: none;
                border-radius: 7px;
                padding: 0 11px;
            }
            QPushButton:hover {
                background-color: rgba(255, 255, 255, 0.14);
                color: #FFFFFF;
            }
        """
        for cat, btn in self.category_buttons.items():
            btn.setStyleSheet(active_css if cat == self._active_category else inactive_css)

    def _on_search_changed(self, text: str) -> None:
        self.clear_btn.setVisible(bool(text.strip()))
        self.refresh_faqs()

    def refresh_faqs(self) -> None:
        while self.faq_layout.count():
            item = self.faq_layout.takeAt(0)
            w = item.widget()
            if w is not None:
                w.deleteLater()

        query = self.search_input.text().strip().lower()
        terms = [t for t in query.split() if t]

        matched: list[dict[str, str]] = []
        for entry in FAQ_DATABASE:
            if self._active_category != "All" and entry["category"] != self._active_category:
                continue
            if terms:
                haystack = (
                    f"{entry['question']} {entry['answer']} {entry['category']} {entry.get('keywords', '')}"
                ).lower()
                if not all(term in haystack for term in terms):
                    continue
            matched.append(entry)

        plural = "s" if len(matched) != 1 else ""
        self.count_label.setText(f"{len(matched)} FAQ{plural} shown")

        if not matched:
            empty_card = FrostedGlassPanel(
                backdrop=self.backdrop,
                radius=14,
                tint_color=QColor(255, 255, 255, 18),
                parent=self.faq_container,
            )
            empty_card.setFixedHeight(64)
            el = QHBoxLayout(empty_card)
            el.setContentsMargins(20, 0, 20, 0)
            lbl = QLabel(
                f"No FAQs matched '{self.search_input.text().strip()}'. Try searching for 'mods', 'delete', 'uninstall', or 'Java'."
            )
            lbl.setFont(QFont(self.lato_family, 9, QFont.Weight.Medium))
            lbl.setStyleSheet("color: rgba(255, 255, 255, 0.62); background: transparent; border: none;")
            el.addWidget(lbl)
            self.scroll_area.watch_widget(empty_card)
            self.faq_layout.addWidget(empty_card)
            self.faq_layout.addStretch(1)
            return

        for entry in matched:
            card = FaqItemCard(
                backdrop=self.backdrop,
                category=entry["category"],
                question=entry["question"],
                answer=entry["answer"],
                lato_family=self.lato_family,
                parent=self.faq_container,
            )
            self.scroll_area.watch_widget(card)
            self.faq_layout.addWidget(card)

        self.faq_layout.addStretch(1)


# ---------------------------------------------------------------------------
# Bottom Minimal Progress Bar Dock
# ---------------------------------------------------------------------------

class InstallProgressDock(FrostedGlassPanel):
    """
    Minimal flat frosted progress bar dock shown at the bottom of the window during install/launch.
    """

    cancel_clicked = Signal()

    def __init__(
        self,
        backdrop: BackdropManager,
        lato_family: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            backdrop=backdrop,
            radius=14,
            tint_color=QColor(14, 26, 46, 155),
            parent=parent,
        )
        self.lato_family = lato_family
        self.setFixedHeight(52)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 8, 16, 8)
        layout.setSpacing(5)

        top_row = QHBoxLayout()
        top_row.setSpacing(8)

        self.stage_title = QLabel("Installing...")
        self.stage_title.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.stage_title.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        top_row.addWidget(self.stage_title)

        self.detail_label = QLabel("")
        self.detail_label.setFont(QFont(lato_family, 8))
        self.detail_label.setStyleSheet("color: rgba(255, 255, 255, 0.56); background: transparent; border: none;")
        top_row.addWidget(self.detail_label, 1)

        self.percent_label = QLabel("0%")
        self.percent_label.setFont(QFont(lato_family, 9, QFont.Weight.Bold))
        self.percent_label.setStyleSheet("color: #FFFFFF; background: transparent; border: none;")
        top_row.addWidget(self.percent_label)

        self.cancel_btn = QPushButton("Cancel")
        self.cancel_btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.cancel_btn.setFixedHeight(22)
        self.cancel_btn.setFont(QFont(lato_family, 8, QFont.Weight.Bold))
        self.cancel_btn.setStyleSheet(
            """
            QPushButton {
                background-color: rgba(255, 255, 255, 0.12);
                color: #FFFFFF;
                border: none;
                border-radius: 6px;
                padding: 0 10px;
            }
            QPushButton:hover {
                background-color: rgba(239, 68, 68, 0.35);
                color: #FCA5A5;
            }
            QPushButton:pressed {
                background-color: rgba(239, 68, 68, 0.55);
            }
            """
        )
        self.cancel_btn.clicked.connect(self.cancel_clicked.emit)
        top_row.addWidget(self.cancel_btn)

        layout.addLayout(top_row)

        self.progress_bar = QProgressBar()
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        self.progress_bar.setTextVisible(False)
        self.progress_bar.setFixedHeight(4)
        self.progress_bar.setStyleSheet(
            """
            QProgressBar {
                background-color: rgba(255, 255, 255, 0.12);
                border-radius: 2px;
                border: none;
            }
            QProgressBar::chunk {
                background-color: rgba(255, 255, 255, 0.90);
                border-radius: 2px;
            }
            """
        )
        layout.addWidget(self.progress_bar)

    def set_step(self, step_num: int, title: str, detail: str) -> None:
        self.stage_title.setText(title)
        self.detail_label.setText(detail)

    def set_progress(self, percent: int, status_text: str) -> None:
        pct = max(0, min(100, percent))
        self.progress_bar.setValue(pct)
        self.percent_label.setText(f"{pct}%")
        self.detail_label.setText(status_text)
