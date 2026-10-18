"""
Main Window for the Lightyear Minecraft Launcher.
Minimalist Flat Frosted Glass Edition (Strict Windowed Mode, Auto-Hide on Game Launch).
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

from PySide6.QtCore import QEvent, QThread, QTimer, Qt, Signal
from PySide6.QtGui import (
    QColor,
    QIcon,
    QLinearGradient,
    QPainter,
)
from PySide6.QtWidgets import (
    QFileDialog,
    QMainWindow,
    QMessageBox,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from .core import (
    GameLaunchWorker,
    InstallationPipelineWorker,
    InstanceConfig,
    LauncherStore,
    VersionInfo,
    check_java_compatibility,
    fetch_or_load_versions,
    get_background_path,
    load_cached_versions_fast,
)
from .ui_components import (
    BackdropManager,
    DeleteConfirmationDialog,
    HelpPage,
    HeroSection,
    InstallationsSection,
    InstalledVersionsPage,
    InstallProgressDock,
    InstanceSettingsDialog,
    JavaMissingOrIncompatibleDialog,
    NewInstallationDialog,
    RenameInstanceDialog,
    TopNavBar,
    ensure_circular_logo_ico,
    make_circular_logo_icon,
    make_circular_logo_pixmap,
)


class ManifestSyncWorker(QThread):
    """Fetches and caches the Mojang version manifest in the background on startup."""

    loaded = Signal(list)

    def __init__(self, root: Path, parent=None) -> None:
        super().__init__(parent)
        self.root = root

    def run(self) -> None:
        versions = fetch_or_load_versions(self.root)
        self.loaded.emit(versions)


class LightyearMainWindow(QMainWindow):
    """
    Strictly windowed-mode minimalist launcher:
    1. Top Navigation Bar (flat frosted glass: 'lightyear' + Home/Installed/Help pills on left, 'hello! username' on right)
    2. Home Page: Hero Rectangle + Compact Instance Grid
    3. Installed Page: Manage & completely uninstall installed Minecraft game versions
    4. Help Page: Comprehensive FAQs & live search bar
    5. Auto-hides launcher window while Minecraft is running.
    """

    def __init__(self, playfair_family: str, lato_family: str) -> None:
        super().__init__()
        self.playfair_family = playfair_family
        self.lato_family = lato_family

        self.store = LauncherStore()
        self.backdrop = BackdropManager(get_background_path())
        self.versions: list[VersionInfo] = load_cached_versions_fast(self.store.root)
        self._install_worker: InstallationPipelineWorker | None = None
        self._launch_worker: GameLaunchWorker | None = None

        # Strict Windowed Mode Only (No maximize, no fullscreen, fixed dimensions)
        self.setWindowTitle("lightyear")
        self.setWindowIcon(make_circular_logo_icon())
        self.setWindowFlags(
            Qt.WindowType.Window
            | Qt.WindowType.WindowTitleHint
            | Qt.WindowType.WindowSystemMenuHint
            | Qt.WindowType.WindowMinimizeButtonHint
            | Qt.WindowType.WindowCloseButtonHint
        )
        self.setFixedSize(960, 560)
        self._apply_windows_taskbar_icon()

        # Central container
        self.central = QWidget()
        self.setCentralWidget(self.central)

        self.main_layout = QVBoxLayout(self.central)
        self.main_layout.setContentsMargins(22, 18, 22, 18)
        self.main_layout.setSpacing(12)

        # 1. Top Navigation Bar
        self.nav_bar = TopNavBar(
            backdrop=self.backdrop,
            initial_username=self.store.username,
            playfair_family=self.playfair_family,
            lato_family=self.lato_family,
            parent=self.central,
        )
        self.nav_bar.username_changed.connect(self._on_username_changed)
        self.nav_bar.page_changed.connect(self._on_page_changed)
        self.main_layout.addWidget(self.nav_bar)

        # Page Stack (Page 0: Home, Page 1: Installed Game Versions, Page 2: Help & FAQs)
        self.page_stack = QStackedWidget(self.central)

        self.home_page = QWidget(self.page_stack)
        home_layout = QVBoxLayout(self.home_page)
        home_layout.setContentsMargins(0, 0, 0, 0)
        home_layout.setSpacing(12)

        # 2. Hero Rectangle
        self.hero_section = HeroSection(
            backdrop=self.backdrop,
            store=self.store,
            lato_family=self.lato_family,
            parent=self.home_page,
        )
        self.hero_section.setFixedHeight(180)
        self.hero_section.play_requested.connect(self.launch_instance)
        self.hero_section.install_requested.connect(self.open_new_installation_popup)
        home_layout.addWidget(self.hero_section)

        # 3. Compact Instance Blocks (No Scrollbars, No Selection Feature)
        self.installations_section = InstallationsSection(
            backdrop=self.backdrop,
            store=self.store,
            lato_family=self.lato_family,
            parent=self.home_page,
        )
        self.installations_section.instance_play.connect(self.launch_instance)
        self.installations_section.instance_open_folder.connect(self._on_open_instance_folder)
        self.installations_section.instance_rename.connect(self._on_rename_instance)
        self.installations_section.instance_duplicate.connect(self._on_duplicate_instance)
        self.installations_section.instance_settings.connect(self._on_edit_instance_settings)
        self.installations_section.instance_repair.connect(self._on_repair_instance)
        self.installations_section.instance_add_resource_packs.connect(
            lambda iid: self._on_add_content(iid, "resourcepacks", "Resource Packs", "Resource Packs (*.zip)")
        )
        self.installations_section.instance_add_mods.connect(
            lambda iid: self._on_add_content(iid, "mods", "Mods", "Minecraft Mods (*.jar)")
        )
        self.installations_section.instance_add_shaderpacks.connect(
            lambda iid: self._on_add_content(iid, "shaderpacks", "Shaderpacks", "Shaderpacks (*.zip)")
        )
        self.installations_section.instance_delete.connect(self._on_delete_instance)
        home_layout.addWidget(self.installations_section, 1)

        # Installed Game Versions Page
        self.installed_page = InstalledVersionsPage(
            backdrop=self.backdrop,
            store=self.store,
            lato_family=self.lato_family,
            parent=self.page_stack,
        )
        self.installed_page.version_open_folder.connect(self._on_open_version_folder)
        self.installed_page.version_uninstall.connect(self._on_uninstall_game_version)

        # Help & FAQs Page
        self.help_page = HelpPage(
            backdrop=self.backdrop,
            lato_family=self.lato_family,
            parent=self.page_stack,
        )

        self.page_stack.addWidget(self.home_page)
        self.page_stack.addWidget(self.installed_page)
        self.page_stack.addWidget(self.help_page)
        self.main_layout.addWidget(self.page_stack, 1)

        # 4. Minimal Progress Bar Dock (hidden when idle)
        self.progress_dock = InstallProgressDock(
            backdrop=self.backdrop,
            lato_family=self.lato_family,
            parent=self.central,
        )
        self.progress_dock.cancel_clicked.connect(self._on_cancel_worker)
        self.progress_dock.hide()
        self.main_layout.addWidget(self.progress_dock)

        self.backdrop.ensure_scaled(self.size())
        self._sync_ui()
        self._start_manifest_sync()

    # -----------------------------------------------------------------------
    # Enforce Windowed Mode Only & Paint Backdrop
    # -----------------------------------------------------------------------

    def _apply_windows_taskbar_icon(self) -> None:
        if os.name != "nt":
            return
        try:
            import ctypes

            ico_path = ensure_circular_logo_ico()
            if not ico_path or not ico_path.exists():
                return
            hwnd = int(self.winId())
            user32 = ctypes.windll.user32
            IMAGE_ICON = 1
            LR_LOADFROMFILE = 0x0010
            WM_SETICON = 0x0080
            ICON_SMALL = 0
            ICON_BIG = 1
            hicon_big = user32.LoadImageW(0, str(ico_path), IMAGE_ICON, 256, 256, LR_LOADFROMFILE)
            hicon_small = user32.LoadImageW(0, str(ico_path), IMAGE_ICON, 32, 32, LR_LOADFROMFILE)
            if hicon_big:
                user32.SendMessageW(hwnd, WM_SETICON, ICON_BIG, hicon_big)
            if hicon_small:
                user32.SendMessageW(hwnd, WM_SETICON, ICON_SMALL, hicon_small)
        except Exception:
            pass

    def showEvent(self, event) -> None:
        super().showEvent(event)
        self._apply_windows_taskbar_icon()

    def changeEvent(self, event: QEvent) -> None:
        if event.type() == QEvent.Type.WindowStateChange:
            if self.windowState() & (Qt.WindowState.WindowMaximized | Qt.WindowState.WindowFullScreen):
                self.setWindowState(Qt.WindowState.WindowNoState)
                self.showNormal()
        super().changeEvent(event)

    def resizeEvent(self, event) -> None:
        super().resizeEvent(event)
        self.backdrop.ensure_scaled(self.size())

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, True)

        sharp_pm = self.backdrop.get_sharp_pixmap(self.size())
        if sharp_pm is not None:
            painter.drawPixmap(self.rect(), sharp_pm)

        painter.fillRect(self.rect(), QColor(8, 18, 36, 95))

    # -----------------------------------------------------------------------
    # State & Manifest Sync
    # -----------------------------------------------------------------------

    def _start_manifest_sync(self) -> None:
        self._manifest_worker = ManifestSyncWorker(self.store.root, self)
        self._manifest_worker.loaded.connect(self._on_versions_loaded)
        self._manifest_worker.start()

    def _on_versions_loaded(self, versions: list[VersionInfo]) -> None:
        self.versions = versions

    def closeEvent(self, event) -> None:
        if hasattr(self, "_manifest_worker") and self._manifest_worker.isRunning():
            self._manifest_worker.wait(1500)
        super().closeEvent(event)

    def _sync_ui(self, select_id: str | None = None) -> None:
        self.hero_section.refresh_from_store(select_id=select_id)
        self.installations_section.refresh_cards()
        self.installed_page.refresh_versions()
        self.update()

    def _on_page_changed(self, page_key: str) -> None:
        if page_key == "installed":
            self.installed_page.refresh_versions()
            self.page_stack.setCurrentWidget(self.installed_page)
        elif page_key == "help":
            self.help_page.refresh_faqs()
            self.page_stack.setCurrentWidget(self.help_page)
        else:
            self.page_stack.setCurrentWidget(self.home_page)
        self.update()

    # -----------------------------------------------------------------------
    # User Actions & Right-Click Context Handlers
    # -----------------------------------------------------------------------

    def _on_username_changed(self, new_username: str) -> None:
        self.store.username = new_username
        self.store.save()

    def _open_directory(self, path: Path) -> None:
        path.mkdir(parents=True, exist_ok=True)
        if os.name == "nt":
            os.startfile(str(path))
        else:
            subprocess.Popen(["xdg-open", str(path)])

    def _on_open_instance_folder(self, instance_id: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        self._open_directory(inst.resolved_game_dir())

    def _on_open_version_folder(self, mc_version: str) -> None:
        v_dir = self.store.root / "versions" / mc_version
        if not v_dir.exists():
            v_dir = self.store.root / "versions"
        self._open_directory(v_dir)

    def _on_add_content(self, instance_id: str, subfolder: str, label: str, file_filter: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        target_dir = inst.resolved_game_dir() / subfolder
        target_dir.mkdir(parents=True, exist_ok=True)
        files, _ = QFileDialog.getOpenFileNames(
            self,
            f"Add {label} — {inst.mc_version} {inst.loader}",
            "",
            file_filter,
        )
        if files:
            for f in files:
                src = Path(f)
                if src.is_file():
                    shutil.copy2(src, target_dir / src.name)
        else:
            self._open_directory(target_dir)

    def _on_cancel_worker(self) -> None:
        if self._install_worker and self._install_worker.isRunning():
            self._install_worker.cancel()
            self.progress_dock.set_progress(self.progress_dock.progress_bar.value(), "Cancelling installation...")
        elif self._launch_worker and self._launch_worker.isRunning():
            self._launch_worker.cancel()
            self.progress_dock.set_progress(self.progress_dock.progress_bar.value(), "Cancelling launch...")

    def _on_rename_instance(self, instance_id: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        dlg = RenameInstanceDialog(current_name=inst.name, lato_family=self.lato_family, parent=self)
        if dlg.exec() == RenameInstanceDialog.DialogCode.Accepted:
            new_name = dlg.get_new_name()
            if new_name and new_name != inst.name:
                self.store.rename_instance(instance_id, new_name)
                self._sync_ui(select_id=instance_id)

    def _on_duplicate_instance(self, instance_id: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        new_inst = self.store.duplicate_instance(instance_id)
        if new_inst:
            self._sync_ui(select_id=new_inst.id)

    def _on_edit_instance_settings(self, instance_id: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        dlg = InstanceSettingsDialog(store=self.store, instance=inst, lato_family=self.lato_family, parent=self)
        if dlg.exec() == InstanceSettingsDialog.DialogCode.Accepted:
            self._sync_ui(select_id=instance_id)

    def _on_repair_instance(self, instance_id: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        # Re-run installation pipeline which verifies hashes and repairs missing or damaged JARs and libraries
        self.start_5_step_installation(inst.name, inst.mc_version, inst.loader, inst.icon_key)

    def _on_delete_instance(self, instance_id: str) -> None:
        inst = self.store.get_instance(instance_id)
        if not inst:
            return
        label = f"{inst.mc_version} {inst.loader}"
        confirm_1 = DeleteConfirmationDialog(
            title_text="Delete Installation?",
            message_text=f"Are you sure you want to delete instance '{label}'? (The game version itself will stay installed.)",
            confirm_text="Continue",
            lato_family=self.lato_family,
            parent=self,
            step_badge="STEP 1 OF 2",
        )
        if confirm_1.exec() != DeleteConfirmationDialog.DialogCode.Accepted:
            return

        confirm_2 = DeleteConfirmationDialog(
            title_text="Final Confirmation",
            message_text=f"Please confirm a second time to permanently delete instance '{label}' and its folder.",
            confirm_text="Confirm Delete",
            lato_family=self.lato_family,
            parent=self,
            step_badge="STEP 2 OF 2 · FINAL",
        )
        if confirm_2.exec() != DeleteConfirmationDialog.DialogCode.Accepted:
            return

        self.store.delete_instance(instance_id)
        self._sync_ui()

    def _on_uninstall_game_version(self, mc_version: str) -> None:
        confirm_1 = DeleteConfirmationDialog(
            title_text="Uninstall Game Version?",
            message_text=f"Are you sure you want to uninstall Minecraft {mc_version} from disk?",
            confirm_text="Continue",
            lato_family=self.lato_family,
            parent=self,
            step_badge="STEP 1 OF 2",
        )
        if confirm_1.exec() != DeleteConfirmationDialog.DialogCode.Accepted:
            return

        confirm_2 = DeleteConfirmationDialog(
            title_text="Final Confirmation",
            message_text=f"Please confirm a second time to completely uninstall Minecraft {mc_version} and remove its game files.",
            confirm_text="Confirm Uninstall",
            lato_family=self.lato_family,
            parent=self,
            step_badge="STEP 2 OF 2 · FINAL",
        )
        if confirm_2.exec() != DeleteConfirmationDialog.DialogCode.Accepted:
            return

        self.store.uninstall_game_version(mc_version)
        self._sync_ui()

    # -----------------------------------------------------------------------
    # New Installation Popup & Background Pipeline
    # -----------------------------------------------------------------------

    def open_new_installation_popup(self) -> None:
        if not self.versions:
            self.versions = fetch_or_load_versions(self.store.root)

        dlg = NewInstallationDialog(
            store=self.store,
            versions=self.versions,
            playfair_family=self.playfair_family,
            lato_family=self.lato_family,
            parent=self,
        )
        if dlg.exec() == NewInstallationDialog.DialogCode.Accepted:
            name, mc_ver, loader, icon_key = dlg.get_result_data()
            self.start_5_step_installation(name, mc_ver, loader, icon_key)

    def start_5_step_installation(self, name: str, mc_version: str, loader: str, icon_key: str) -> None:
        if self._install_worker and self._install_worker.isRunning():
            return

        self.hero_section.set_busy(True, "Installing...")
        self.progress_dock.show()
        self.progress_dock.set_step(1, "Installing", f"{mc_version} {loader}")
        self.progress_dock.set_progress(4, "Validating setup...")

        self._install_worker = InstallationPipelineWorker(
            store=self.store,
            instance_name=name,
            mc_version=mc_version,
            loader=loader,
            icon_key=icon_key,
            parent=self,
        )
        self._install_worker.step_changed.connect(self.progress_dock.set_step)
        self._install_worker.progress_changed.connect(self.progress_dock.set_progress)
        self._install_worker.completed.connect(self._on_installation_completed)
        self._install_worker.start()

    def _on_installation_completed(self, success: bool, inst_obj: object, message: str) -> None:
        self.hero_section.set_busy(False)
        self._sync_ui()
        if success and isinstance(inst_obj, InstanceConfig):
            self._sync_ui(select_id=inst_obj.id)
            self.progress_dock.show()
            self.progress_dock.set_step(5, "Ready", message)
            self.progress_dock.set_progress(100, f"{inst_obj.name} ready")
            QTimer.singleShot(2000, self.progress_dock.hide)
        elif "cancelled" in message.lower():
            self.progress_dock.hide()
        else:
            self.progress_dock.hide()
            QMessageBox.warning(self, "Installation Error", f"Could not complete installation:\n{message}")

    # -----------------------------------------------------------------------
    # Launching Minecraft (Hides App Window While Game Runs)
    # -----------------------------------------------------------------------

    def launch_instance(self, instance_id: str) -> None:
        if self._launch_worker and self._launch_worker.isRunning():
            return

        inst = self.store.get_instance(instance_id)
        if not inst:
            self.open_new_installation_popup()
            return

        # Pre-launch Java Compatibility Verification & Guidance
        compat, req_major, det_major, auto_path = check_java_compatibility(
            inst.mc_version,
            self.store.root,
            custom_path=inst.java_path,
        )
        if not compat:
            dlg = JavaMissingOrIncompatibleDialog(
                mc_version=inst.mc_version,
                req_major=req_major,
                det_major=det_major,
                current_path=auto_path,
                lato_family=self.lato_family,
                parent=self,
            )
            if dlg.exec() == JavaMissingOrIncompatibleDialog.DialogCode.Accepted and dlg.selected_java_path:
                self.store.update_instance_settings(instance_id=inst.id, java_path=dlg.selected_java_path)
                inst = self.store.get_instance(instance_id)
                if not inst:
                    return
            else:
                return

        self.hero_section.select_instance(inst.id)
        self.hero_section.set_busy(True, "Launching...")
        self.progress_dock.show()
        self.progress_dock.set_step(4, "Launching", f"{inst.mc_version} {inst.loader}")
        self.progress_dock.set_progress(25, "Preparing runtime...")

        self._launch_worker = GameLaunchWorker(self.store, inst, self)
        self._launch_worker.status_updated.connect(self.progress_dock.set_progress)
        self._launch_worker.game_started.connect(self._on_game_started)
        self._launch_worker.game_finished.connect(self._on_game_finished)
        self._launch_worker.launch_failed.connect(self._on_launch_failed)
        self._launch_worker.start()

    def _on_game_started(self, pid: int, instance_name: str) -> None:
        self.progress_dock.set_progress(100, "Running")
        # Hide the launcher window while Minecraft is running
        QTimer.singleShot(350, self.hide)

    def _restore_window(self) -> None:
        self.showNormal()
        self.show()
        self.raise_()
        self.activateWindow()

    def _on_game_finished(self, exit_code: int, message: str) -> None:
        self.hero_section.set_busy(False)
        self.progress_dock.hide()
        self._sync_ui()
        self._restore_window()
        if exit_code != 0:
            QMessageBox.warning(self, "Minecraft Error", message)

    def _on_launch_failed(self, err_msg: str) -> None:
        self.hero_section.set_busy(False)
        self.progress_dock.hide()
        self._sync_ui()
        self._restore_window()
        if "cancelled" in err_msg.lower():
            return
        QMessageBox.critical(self, "Launch Failed", f"Failed to launch Minecraft:\n{err_msg}")
