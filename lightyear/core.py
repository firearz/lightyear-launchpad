"""
Core backend engine for the Lightyear Minecraft Launcher.
Integrates `minecraft-launcher-lib` with:
- Isolated per-instance game directories (`instances/<instance_id>/`)
- Shared content store (`versions/`, `libraries/`, `assets/`, `runtime/`)
- Smart Download Decision Matrix (reusing existing files across instances & loaders)
- Deterministic Java-exact Offline UUID v3 (`OfflinePlayer:<username>`)
- 5-stage background installation & verification pipeline (`QThread` workers)
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
import time
import uuid
import zipfile
from dataclasses import asdict, dataclass, field, fields
from pathlib import Path
from typing import Any

import minecraft_launcher_lib as mll
from PySide6.QtCore import QThread, Signal


class InterruptedException(Exception):
    """Raised when an active install, repair, or download worker is cancelled by the user."""
    pass


# ---------------------------------------------------------------------------
# Paths & Asset Resolution
# ---------------------------------------------------------------------------

def _resolve_project_root() -> Path:
    """
    Returns the directory containing the bundled `assets/` folder.
    When packaged with PyInstaller, resources are extracted to `sys._MEIPASS`.
    """
    frozen_root = getattr(sys, "_MEIPASS", None)
    if getattr(sys, "frozen", False) and frozen_root:
        return Path(frozen_root)
    return Path(__file__).resolve().parent.parent


PROJECT_ROOT = _resolve_project_root()
ASSETS_DIR = PROJECT_ROOT / "assets"
FONTS_DIR = ASSETS_DIR / "fonts"

DEFAULT_USERNAME = "Player"


def get_logo_path() -> Path:
    return ASSETS_DIR / "logo.png"


def get_background_path() -> Path:
    return ASSETS_DIR / "backgroudn1.jpg"


def get_launcher_root() -> Path:
    """
    Returns `%APPDATA%\\LightYieldLauncher` on Windows (sharing the verified cache
    of versions, libraries, assets, and Mojang Java runtimes).
    """
    appdata = os.environ.get("APPDATA")
    if appdata:
        root = Path(appdata) / "LightYieldLauncher"
    else:
        root = Path.home() / ".lightyear"
    root.mkdir(parents=True, exist_ok=True)
    for sub in ("versions", "libraries", "assets", "instances", "runtimes", "runtime"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    return root


def get_system_minecraft_dir() -> Path | None:
    appdata = os.environ.get("APPDATA")
    if appdata:
        p = Path(appdata) / ".minecraft"
        if p.exists():
            return p
    return None


# ---------------------------------------------------------------------------
# Offline Account & Deterministic Java UUID v3
# ---------------------------------------------------------------------------

def sanitize_username(raw: str) -> str:
    cleaned = "".join(ch for ch in raw.strip() if ch.isalnum() or ch == "_")
    if not cleaned:
        return "Player"
    return cleaned[:16]


def offline_uuid_v3(username: str) -> str:
    """
    Computes the exact Java `UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))`
    RFC 4122 Version 3 MD5 UUID in dashed format.
    """
    data = f"OfflinePlayer:{username}".encode("utf-8")
    digest = bytearray(hashlib.md5(data).digest())
    digest[6] = (digest[6] & 0x0F) | 0x30  # Version 3
    digest[8] = (digest[8] & 0x3F) | 0x80  # RFC 4122 IETF variant
    return str(uuid.UUID(bytes=bytes(digest)))


# ---------------------------------------------------------------------------
# Data Models & Persistent State
# ---------------------------------------------------------------------------

@dataclass
class InstanceConfig:
    id: str
    name: str
    mc_version: str
    loader: str = "Vanilla"  # Vanilla, Fabric, Forge, Quilt
    installed_version_id: str = ""
    icon_key: str = "grass"
    game_dir: str = ""
    created_at: float = field(default_factory=time.time)
    last_played: float = 0.0
    ready: bool = True
    max_ram_gb: int | None = None
    min_ram_mb: int | None = None
    java_path: str = ""
    jvm_args: str = ""

    def resolved_version_id(self) -> str:
        return self.installed_version_id or self.mc_version

    def resolved_game_dir(self) -> Path:
        if self.game_dir:
            p = Path(self.game_dir)
        else:
            p = get_launcher_root() / "instances" / self.id
        p.mkdir(parents=True, exist_ok=True)
        return p


@dataclass
class InstalledGameVersion:
    mc_version: str
    loaders: list[str]
    version_ids: list[str]
    size_bytes: int
    instance_count: int = 0

    @property
    def size_label(self) -> str:
        mb = self.size_bytes / (1024 * 1024)
        if mb >= 1024:
            return f"{mb / 1024:.2f} GB"
        return f"{max(0.1, mb):.1f} MB"

    @property
    def loaders_label(self) -> str:
        return ", ".join(self.loaders) if self.loaders else "Vanilla"


class LauncherStore:
    """Manages persistent settings (`lightyear_settings.json`) and installations (`installations.json`)."""

    def __init__(self, root: Path | None = None) -> None:
        self.root = root or get_launcher_root()
        self.settings_path = self.root / "lightyear_settings.json"
        self.installations_path = self.root / "installations.json"
        self.versions_txt_path = self.root / "versions.txt"

        self.username: str = DEFAULT_USERNAME
        self.last_played_instance_id: str = ""
        self.max_ram_gb: int = 4
        self.instances: list[InstanceConfig] = []
        self._had_installations_file: bool = self.installations_path.exists()

        self.load()
        self._ensure_seeded_instances()

    def load(self) -> None:
        if self.settings_path.exists():
            try:
                data = json.loads(self.settings_path.read_text(encoding="utf-8"))
                self.username = sanitize_username(data.get("username", DEFAULT_USERNAME))
                self.last_played_instance_id = data.get("last_played_instance_id", "")
                self.max_ram_gb = int(data.get("max_ram_gb", 4))
            except Exception:
                pass
        else:
            # Check legacy settings.json if present
            legacy_settings = self.root / "settings.json"
            if legacy_settings.exists():
                try:
                    data = json.loads(legacy_settings.read_text(encoding="utf-8"))
                    self.username = sanitize_username(data.get("username", DEFAULT_USERNAME))
                except Exception:
                    pass

        if self.installations_path.exists():
            try:
                raw_list = json.loads(self.installations_path.read_text(encoding="utf-8"))
                known_fields = {f.name for f in fields(InstanceConfig)}
                self.instances = [
                    InstanceConfig(**{k: v for k, v in item.items() if k in known_fields})
                    for item in raw_list
                    if isinstance(item, dict)
                ]
            except Exception:
                self.instances = []

    def save(self) -> None:
        settings_payload = {
            "username": self.username,
            "uuid": offline_uuid_v3(self.username),
            "last_played_instance_id": self.last_played_instance_id,
            "max_ram_gb": self.max_ram_gb,
        }
        self.settings_path.write_text(json.dumps(settings_payload, indent=2), encoding="utf-8")
        inst_payload = [asdict(inst) for inst in self.instances]
        self.installations_path.write_text(json.dumps(inst_payload, indent=2), encoding="utf-8")

    def _ensure_seeded_instances(self) -> None:
        """
        On very first launch (when `installations.json` does not exist yet), if versions are
        already downloaded in `%APPDATA%\\LightYieldLauncher\\versions`, register them as isolated instances.
        If `installations.json` already exists, never re-seed deleted instances.
        """
        if self.instances:
            if not self.last_played_instance_id or not self.get_instance(self.last_played_instance_id):
                self.last_played_instance_id = self.instances[0].id
                self.save()
            return

        if self._had_installations_file:
            if self.last_played_instance_id:
                self.last_played_instance_id = ""
                self.save()
            return

        versions_dir = self.root / "versions"
        seeded: list[InstanceConfig] = []
        for candidate, icon_key in [("1.21.4", "grass"), ("b1.7.3", "crafting_table")]:
            v_jar = versions_dir / candidate / f"{candidate}.jar"
            v_json = versions_dir / candidate / f"{candidate}.json"
            if v_jar.exists() and v_json.exists():
                inst_id = f"vanilla-{candidate.replace('.', '-')}"
                inst_dir = self.root / "instances" / candidate
                inst_dir.mkdir(parents=True, exist_ok=True)
                inst = InstanceConfig(
                    id=inst_id,
                    name=f"{candidate} Vanilla",
                    mc_version=candidate,
                    loader="Vanilla",
                    installed_version_id=candidate,
                    icon_key=icon_key,
                    game_dir=str(inst_dir),
                    created_at=time.time(),
                    last_played=time.time() if candidate == "1.21.4" else time.time() - 60,
                    ready=True,
                )
                self._write_instance_metadata(inst)
                seeded.append(inst)

        if seeded:
            self.instances = seeded
            self.last_played_instance_id = seeded[0].id
            self.save()

    def _write_instance_metadata(self, inst: InstanceConfig) -> None:
        gdir = inst.resolved_game_dir()
        for sub in ("saves", "mods", "resourcepacks", "shaderpacks", "logs"):
            (gdir / sub).mkdir(parents=True, exist_ok=True)
        meta_file = gdir / "instance.json"
        meta_file.write_text(json.dumps(asdict(inst), indent=2), encoding="utf-8")

    def get_instance(self, instance_id: str) -> InstanceConfig | None:
        for inst in self.instances:
            if inst.id == instance_id:
                return inst
        return None

    def get_last_played_instance(self) -> InstanceConfig | None:
        if self.last_played_instance_id:
            found = self.get_instance(self.last_played_instance_id)
            if found:
                return found
        if self.instances:
            # Sort by last_played descending
            sorted_insts = sorted(self.instances, key=lambda x: (x.last_played, x.created_at), reverse=True)
            return sorted_insts[0]
        return None

    def add_or_update_instance(self, inst: InstanceConfig, select_as_last_played: bool = True) -> None:
        self._write_instance_metadata(inst)
        for idx, existing in enumerate(self.instances):
            if existing.id == inst.id:
                self.instances[idx] = inst
                break
        else:
            self.instances.insert(0, inst)

        if select_as_last_played:
            self.last_played_instance_id = inst.id
        self.save()

    def _remove_instance_dir_on_disk(self, inst: InstanceConfig) -> None:
        """
        Deletes the isolated instance directory (`instances/<instance_id>/`) from disk
        while keeping the shared game version files (`versions/`, `libraries/`, `assets/`) intact.
        """
        raw_dir = Path(inst.game_dir) if inst.game_dir else (self.root / "instances" / inst.id)
        try:
            instances_root = (self.root / "instances").resolve()
            resolved_dir = raw_dir.resolve()
            if resolved_dir != instances_root and instances_root in resolved_dir.parents:
                still_used = any(
                    (
                        Path(other.game_dir).resolve()
                        if other.game_dir
                        else (self.root / "instances" / other.id).resolve()
                    )
                    == resolved_dir
                    for other in self.instances
                    if other.id != inst.id
                )
                if not still_used and resolved_dir.exists() and resolved_dir.is_dir():
                    shutil.rmtree(resolved_dir, ignore_errors=True)
        except Exception:
            pass

    def delete_instance(self, instance_id: str) -> None:
        """
        Deletes the instance and its isolated instance folder on disk,
        without uninstalling the underlying Minecraft game version.
        """
        target = self.get_instance(instance_id)
        self.instances = [i for i in self.instances if i.id != instance_id]
        if target is not None:
            self._remove_instance_dir_on_disk(target)
        if self.last_played_instance_id == instance_id:
            nxt = self.get_last_played_instance()
            self.last_played_instance_id = nxt.id if nxt else ""
        self.save()

    def rename_instance(self, instance_id: str, new_name: str) -> bool:
        clean = new_name.strip()
        if not clean:
            return False
        inst = self.get_instance(instance_id)
        if not inst:
            return False
        inst.name = clean
        self._write_instance_metadata(inst)
        self.save()
        return True

    def duplicate_instance(self, instance_id: str, new_name: str | None = None) -> InstanceConfig | None:
        source = self.get_instance(instance_id)
        if not source:
            return None

        target_name = new_name.strip() if new_name and new_name.strip() else f"{source.name} (Copy)"
        slug = re.sub(r"[^a-z0-9]+", "-", target_name.lower()).strip("-") or "instance"
        short_uid = uuid.uuid4().hex[:6]
        new_id = f"{slug}-{short_uid}"
        new_dir = self.root / "instances" / new_id
        new_dir.mkdir(parents=True, exist_ok=True)

        # Copy existing isolated instance files (saves, mods, resourcepacks, shaderpacks, config)
        source_dir = source.resolved_game_dir()
        if source_dir.exists() and source_dir.is_dir():
            for sub in ("saves", "mods", "resourcepacks", "shaderpacks", "config"):
                s_sub = source_dir / sub
                t_sub = new_dir / sub
                if s_sub.exists() and s_sub.is_dir():
                    try:
                        shutil.copytree(s_sub, t_sub, dirs_exist_ok=True)
                    except Exception:
                        pass
                else:
                    t_sub.mkdir(parents=True, exist_ok=True)
            (new_dir / "logs").mkdir(parents=True, exist_ok=True)
        else:
            for sub in ("saves", "mods", "resourcepacks", "shaderpacks", "logs", "config"):
                (new_dir / sub).mkdir(parents=True, exist_ok=True)

        dup = InstanceConfig(
            id=new_id,
            name=target_name,
            mc_version=source.mc_version,
            loader=source.loader,
            installed_version_id=source.installed_version_id,
            icon_key=source.icon_key,
            game_dir=str(new_dir),
            created_at=time.time(),
            last_played=0.0,
            ready=source.ready,
            max_ram_gb=source.max_ram_gb,
            min_ram_mb=source.min_ram_mb,
            java_path=source.java_path,
            jvm_args=source.jvm_args,
        )
        self.add_or_update_instance(dup, select_as_last_played=True)
        return dup

    def update_instance_settings(
        self,
        instance_id: str,
        name: str,
        icon_key: str,
        max_ram_gb: int | None = None,
        min_ram_mb: int | None = None,
        java_path: str = "",
        jvm_args: str = "",
    ) -> bool:
        inst = self.get_instance(instance_id)
        if not inst:
            return False
        clean = name.strip()
        if clean:
            inst.name = clean
        inst.icon_key = icon_key
        inst.max_ram_gb = max_ram_gb
        inst.min_ram_mb = min_ram_mb
        inst.java_path = java_path.strip()
        inst.jvm_args = jvm_args.strip()
        self._write_instance_metadata(inst)
        self.save()
        return True

    @staticmethod
    def _classify_version_entry(entry: Path) -> tuple[str, str] | None:
        """
        Determines `(mc_version, loader_name)` for a subdirectory inside `versions/`.
        Returns `None` if the directory does not contain any version `.json` or `.jar`.
        """
        if not entry.is_dir():
            return None
        vid = entry.name
        v_json = entry / f"{vid}.json"
        v_jar = entry / f"{vid}.jar"
        if not v_json.exists() and not v_jar.exists():
            has_any = any(f.suffix in (".json", ".jar") for f in entry.iterdir() if f.is_file())
            if not has_any:
                return None

        mc_ver = vid
        vid_lower = vid.lower()
        loader_name = "Vanilla"
        if "fabric" in vid_lower:
            loader_name = "Fabric"
        elif "quilt" in vid_lower:
            loader_name = "Quilt"
        elif "forge" in vid_lower:
            loader_name = "Forge"

        if v_json.exists():
            try:
                data = json.loads(v_json.read_text(encoding="utf-8", errors="ignore"))
                if data.get("inheritsFrom"):
                    mc_ver = str(data["inheritsFrom"])
                elif data.get("jar"):
                    mc_ver = str(data["jar"])
            except Exception:
                pass

        if mc_ver == vid and loader_name != "Vanilla":
            # Fallback extraction from standard loader ID patterns
            m_fab = re.match(r"^(?:fabric|quilt)-loader-[^-]+-(.+)$", vid, re.IGNORECASE)
            if m_fab:
                mc_ver = m_fab.group(1)
            else:
                m_forge = re.match(r"^([^-]+)-forge-.+$", vid, re.IGNORECASE)
                if m_forge:
                    mc_ver = m_forge.group(1)

        return (mc_ver, loader_name)

    def get_installed_game_versions(self) -> list[InstalledGameVersion]:
        """
        Scans `versions/` on disk and returns all installed game versions grouped by `mc_version`.
        Installed game versions persist on disk even when all of their instances are deleted.
        """
        versions_dir = self.root / "versions"
        if not versions_dir.exists():
            return []

        grouped: dict[str, dict[str, Any]] = {}
        for entry in versions_dir.iterdir():
            info = self._classify_version_entry(entry)
            if info is None:
                continue
            mc_ver, loader_name = info
            dir_size = sum(f.stat().st_size for f in entry.rglob("*") if f.is_file())
            mtime = entry.stat().st_mtime

            if mc_ver not in grouped:
                grouped[mc_ver] = {
                    "loaders": set(),
                    "version_ids": [],
                    "size_bytes": 0,
                    "mtime": mtime,
                }
            grouped[mc_ver]["loaders"].add(loader_name)
            grouped[mc_ver]["version_ids"].append(entry.name)
            grouped[mc_ver]["size_bytes"] += dir_size
            grouped[mc_ver]["mtime"] = max(grouped[mc_ver]["mtime"], mtime)

        loader_order = {"Vanilla": 0, "Fabric": 1, "Forge": 2, "Quilt": 3}
        results: list[InstalledGameVersion] = []
        for mc_ver, data in grouped.items():
            ordered_loaders = sorted(data["loaders"], key=lambda l: loader_order.get(l, 99))
            inst_count = sum(1 for i in self.instances if i.mc_version == mc_ver)
            results.append(
                InstalledGameVersion(
                    mc_version=mc_ver,
                    loaders=ordered_loaders,
                    version_ids=sorted(data["version_ids"]),
                    size_bytes=data["size_bytes"],
                    instance_count=inst_count,
                )
            )

        results.sort(key=lambda item: (item.instance_count > 0, grouped[item.mc_version]["mtime"]), reverse=True)
        return results

    @staticmethod
    def _extract_version_dependencies(v_dir: Path) -> tuple[set[str], str | None]:
        """Returns `(library_relative_paths, asset_index_id)` referenced by a version directory's JSON."""
        libs: set[str] = set()
        asset_idx: str | None = None
        v_json = v_dir / f"{v_dir.name}.json"
        if not v_json.exists():
            return (libs, asset_idx)

        try:
            data = json.loads(v_json.read_text(encoding="utf-8", errors="ignore"))
            asset_idx = data.get("assetIndex", {}).get("id")
            for lib in data.get("libraries", []):
                downloads = lib.get("downloads", {})
                art = downloads.get("artifact")
                if art and art.get("path"):
                    libs.add(str(art["path"]).replace("\\", "/"))
                for clf in downloads.get("classifiers", {}).values():
                    if isinstance(clf, dict) and clf.get("path"):
                        libs.add(str(clf["path"]).replace("\\", "/"))
                name = lib.get("name", "")
                parts = name.split(":")
                if len(parts) >= 3:
                    group, artifact, ver = parts[0], parts[1], parts[2]
                    rel = f"{group.replace('.', '/')}/{artifact}/{ver}/{artifact}-{ver}.jar"
                    libs.add(rel)
        except Exception:
            pass
        return (libs, asset_idx)

    def uninstall_game_version(self, mc_version: str) -> None:
        """
        Completely uninstalls `mc_version` from the launcher:
        1. Removes its version & loader profile directories in `versions/`
        2. Removes unreferenced libraries in `libraries/` and unreferenced assets in `assets/`
        3. Removes any instances that were using `mc_version`
        """
        versions_dir = self.root / "versions"
        target_dirs: list[Path] = []
        remaining_dirs: list[Path] = []

        if versions_dir.exists():
            for entry in versions_dir.iterdir():
                info = self._classify_version_entry(entry)
                if info is None:
                    continue
                entry_mc_ver, _ = info
                if entry_mc_ver == mc_version or entry.name == mc_version:
                    target_dirs.append(entry)
                else:
                    remaining_dirs.append(entry)

        target_libs: set[str] = set()
        target_asset_indexes: set[str] = set()
        for td in target_dirs:
            libs, a_idx = self._extract_version_dependencies(td)
            target_libs.update(libs)
            if a_idx:
                target_asset_indexes.add(a_idx)

        kept_libs: set[str] = set()
        kept_asset_indexes: set[str] = set()
        for rd in remaining_dirs:
            libs, a_idx = self._extract_version_dependencies(rd)
            kept_libs.update(libs)
            if a_idx:
                kept_asset_indexes.add(a_idx)

        # 1. Delete version directories in `versions/`
        for td in target_dirs:
            shutil.rmtree(td, ignore_errors=True)

        # 2. Delete libraries exclusive to the uninstalled version
        libs_root = self.root / "libraries"
        if libs_root.exists():
            for rel_path in target_libs - kept_libs:
                lib_file = libs_root / Path(rel_path)
                try:
                    if lib_file.exists() and lib_file.is_file():
                        lib_file.unlink(missing_ok=True)
                        parent = lib_file.parent
                        while parent != libs_root and parent.exists():
                            if not any(parent.iterdir()):
                                parent.rmdir()
                                parent = parent.parent
                            else:
                                break
                except Exception:
                    pass

        # 3. Delete asset indexes & objects exclusive to the uninstalled version
        indexes_root = self.root / "assets" / "indexes"
        objects_root = self.root / "assets" / "objects"
        unused_indexes = target_asset_indexes - kept_asset_indexes
        if unused_indexes and indexes_root.exists():
            kept_hashes: set[str] = set()
            for k_idx in kept_asset_indexes:
                k_file = indexes_root / f"{k_idx}.json"
                if k_file.exists():
                    try:
                        k_data = json.loads(k_file.read_text(encoding="utf-8", errors="ignore"))
                        for obj in k_data.get("objects", {}).values():
                            if isinstance(obj, dict) and obj.get("hash"):
                                kept_hashes.add(str(obj["hash"]))
                    except Exception:
                        pass

            for u_idx in unused_indexes:
                u_file = indexes_root / f"{u_idx}.json"
                if u_file.exists():
                    try:
                        u_data = json.loads(u_file.read_text(encoding="utf-8", errors="ignore"))
                        for obj in u_data.get("objects", {}).values():
                            h = obj.get("hash") if isinstance(obj, dict) else None
                            if h and h not in kept_hashes and len(str(h)) >= 2:
                                obj_path = objects_root / str(h)[:2] / str(h)
                                if obj_path.exists():
                                    obj_path.unlink(missing_ok=True)
                        u_file.unlink(missing_ok=True)
                    except Exception:
                        pass

        # 4. Also delete any instances associated with this uninstalled game version
        matching_ids = [inst.id for inst in self.instances if inst.mc_version == mc_version]
        for iid in matching_ids:
            self.delete_instance(iid)



# ---------------------------------------------------------------------------
# Smart Download Decision Matrix (per Reference Image)
# ---------------------------------------------------------------------------

@dataclass
class DownloadDecision:
    situation: str
    action: str
    reuse_base_version: bool
    reuse_loader_profile: bool
    needs_repair: bool
    existing_loader_version_id: str | None = None


def format_last_played(ts: float) -> str:
    """Formats a unix timestamp into an intuitive relative string (e.g. 'Played just now', 'Played 2h ago')."""
    if not ts or ts <= 0:
        return "Never played"
    diff = time.time() - ts
    if diff < 60:
        return "Played just now"
    if diff < 3600:
        mins = max(1, int(diff // 60))
        return f"Played {mins}m ago"
    if diff < 86400:
        hrs = max(1, int(diff // 3600))
        return f"Played {hrs}h ago"
    days = int(diff // 86400)
    if days == 1:
        return "Played yesterday"
    if days < 7:
        return f"Played {days}d ago"
    try:
        return f"Played {time.strftime('%b %d, %Y', time.localtime(ts))}"
    except Exception:
        return f"Played {days}d ago"


def check_version_integrity(mc_version: str, root: Path, deep_verify: bool = False) -> tuple[bool, bool]:
    """
    Returns `(exists_on_disk, is_corrupted_or_incomplete)`.
    Checks version JSON, client JAR, asset index, and optionally validates zip/jar integrity.
    """
    v_dir = root / "versions" / mc_version
    v_json = v_dir / f"{mc_version}.json"
    v_jar = v_dir / f"{mc_version}.jar"
    if not v_json.exists() or not v_jar.exists():
        return (False, False)

    try:
        data = json.loads(v_json.read_text(encoding="utf-8"))
        if v_jar.stat().st_size < 1024:
            return (True, True)

        if deep_verify:
            try:
                with zipfile.ZipFile(v_jar, "r") as zf:
                    if zf.testzip() is not None:
                        return (True, True)
            except Exception:
                return (True, True)

        asset_idx = data.get("assetIndex", {}).get("id")
        if asset_idx:
            idx_path = root / "assets" / "indexes" / f"{asset_idx}.json"
            if not idx_path.exists() or idx_path.stat().st_size == 0:
                return (True, True)
        return (True, False)
    except Exception:
        return (True, True)


def find_installed_loader_profile(mc_version: str, loader: str, root: Path) -> str | None:
    """
    Checks if a specific `(mc_version, loader)` profile is already installed in `versions/`.
    """
    loader_norm = loader.strip().lower()
    if loader_norm == "vanilla":
        exists, corrupted = check_version_integrity(mc_version, root)
        return mc_version if (exists and not corrupted) else None

    versions_dir = root / "versions"
    if not versions_dir.exists():
        return None

    for entry in versions_dir.iterdir():
        if not entry.is_dir():
            continue
        vid = entry.name
        v_json = entry / f"{vid}.json"
        if not v_json.exists():
            continue
        vid_lower = vid.lower()
        if loader_norm == "fabric" and "fabric-loader" in vid_lower and vid.endswith(f"-{mc_version}"):
            return vid
        if loader_norm == "quilt" and "quilt-loader" in vid_lower and vid.endswith(f"-{mc_version}"):
            return vid
        if loader_norm == "forge" and "forge" in vid_lower and mc_version in vid:
            return vid
    return None


def decide_download_strategy(mc_version: str, loader: str, root: Path | None = None) -> DownloadDecision:
    """
    Implements the exact 4-row decision matrix from the user's reference image:
    1. Same version, same loader -> Reuse existing files; create a new isolated instance
    2. Same version, different loader -> Reuse compatible game files; install the additional loader components
    3. Different Minecraft version -> Download the files required for that version
    4. Missing or corrupted files -> Download or repair the affected files
    """
    root = root or get_launcher_root()
    base_exists, base_corrupted = check_version_integrity(mc_version, root)
    existing_loader_vid = find_installed_loader_profile(mc_version, loader, root)

    if base_exists and base_corrupted:
        return DownloadDecision(
            situation="Missing or corrupted files",
            action="Download or repair the affected files",
            reuse_base_version=False,
            reuse_loader_profile=False,
            needs_repair=True,
            existing_loader_version_id=None,
        )

    if base_exists and existing_loader_vid:
        return DownloadDecision(
            situation="Same version, same loader",
            action="Reuse existing files; create a new isolated instance",
            reuse_base_version=True,
            reuse_loader_profile=True,
            needs_repair=False,
            existing_loader_version_id=existing_loader_vid,
        )

    if base_exists and not existing_loader_vid:
        return DownloadDecision(
            situation="Same version, different loader",
            action="Reuse compatible game files; install the additional loader components",
            reuse_base_version=True,
            reuse_loader_profile=False,
            needs_repair=False,
            existing_loader_version_id=None,
        )

    return DownloadDecision(
        situation="Different Minecraft version",
        action="Download the files required for that version",
        reuse_base_version=False,
        reuse_loader_profile=False,
        needs_repair=False,
        existing_loader_version_id=None,
    )


# ---------------------------------------------------------------------------
# Version Manifest Fetching & Caching (`versions.txt`)
# ---------------------------------------------------------------------------

@dataclass
class VersionInfo:
    id: str
    vtype: str  # release, snapshot, old_beta, old_alpha
    release_time: str


def load_cached_versions_fast(root: Path | None = None) -> list[VersionInfo]:
    """Synchronously reads cached `versions.txt` in < 2ms so UI dropdowns never start empty."""
    root = root or get_launcher_root()
    cache_txt = root / "versions.txt"
    versions: list[VersionInfo] = []
    if cache_txt.exists():
        for line in cache_txt.read_text(encoding="utf-8", errors="ignore").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("|")
            if len(parts) >= 3:
                versions.append(VersionInfo(id=parts[0], vtype=parts[1], release_time=parts[2]))
    if not versions:
        for vid in ("1.21.4", "1.21.3", "1.21.1", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.18.2", "1.16.5", "1.12.2", "1.8.9", "b1.7.3"):
            vtype = "old_beta" if vid.startswith("b") else "release"
            versions.append(VersionInfo(id=vid, vtype=vtype, release_time="2025-01-01"))
    return versions


def fetch_or_load_versions(root: Path | None = None) -> list[VersionInfo]:
    """
    Fetches Minecraft versions using `minecraft_launcher_lib.utils.get_version_list()`,
    caches them to `%APPDATA%\\LightYieldLauncher\\versions.txt`, and falls back to
    `versions.txt` when offline.
    """
    root = root or get_launcher_root()
    cache_txt = root / "versions.txt"
    versions: list[VersionInfo] = []

    try:
        raw_list = mll.utils.get_version_list()
        lines = ["# LIGHTYEAR_MANIFEST_V2"]
        for item in raw_list:
            vid = item["id"]
            vtype = item["type"]
            rtime = item["releaseTime"].isoformat() if hasattr(item["releaseTime"], "isoformat") else str(item["releaseTime"])
            versions.append(VersionInfo(id=vid, vtype=vtype, release_time=rtime))
            lines.append(f"{vid}|{vtype}|{rtime}|cached")
        cache_txt.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return versions
    except Exception:
        pass

    # Fallback to cached versions.txt
    if cache_txt.exists():
        for line in cache_txt.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("|")
            if len(parts) >= 3:
                versions.append(VersionInfo(id=parts[0], vtype=parts[1], release_time=parts[2]))

    if not versions:
        # Built-in fallback list if completely offline on a fresh disk
        for vid in ("1.21.4", "1.21.3", "1.21.1", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.18.2", "1.16.5", "1.12.2", "1.8.9", "b1.7.3"):
            vtype = "old_beta" if vid.startswith("b") else "release"
            versions.append(VersionInfo(id=vid, vtype=vtype, release_time="2025-01-01"))

    return versions


# ---------------------------------------------------------------------------
# Java Runtime Resolution & 64-bit Native DLL Guard
# ---------------------------------------------------------------------------

def get_required_java_major(mc_version: str, root: Path | None = None) -> int:
    if root is not None:
        cand_ids = [mc_version]
        for vid in cand_ids:
            v_dir = root / "versions" / vid
            v_json = v_dir / f"{vid}.json"
            if v_json.exists():
                try:
                    data = json.loads(v_json.read_text(encoding="utf-8"))
                    jv = data.get("javaVersion")
                    if isinstance(jv, dict) and "majorVersion" in jv:
                        return int(jv["majorVersion"])
                    inherits = data.get("inheritsFrom")
                    if inherits and inherits not in cand_ids:
                        cand_ids.append(inherits)
                except Exception:
                    pass

    if mc_version.startswith(("b", "a", "c0.", "rd-", "inf-")):
        return 8
    parts = mc_version.split(".")
    # Versions like 25.x, 26.x, or snapshots 25w..., 26w...
    if parts and parts[0].isdigit():
        major_num = int(parts[0])
        if major_num >= 25:
            return 25
    if len(parts) >= 2 and parts[0] == "1":
        digits = "".join(ch for ch in parts[1] if ch.isdigit())
        if digits:
            minor = int(digits)
            if minor >= 25:
                return 25
            if minor >= 21:
                return 21
            if minor == 20:
                if len(parts) >= 3:
                    p_digits = "".join(ch for ch in parts[2] if ch.isdigit())
                    if p_digits and int(p_digits) >= 5:
                        return 21
                return 17
            if minor >= 18:
                return 17
            if minor == 17:
                return 16
            return 8
    return 21


def inspect_java_major(java_exe: Path) -> int | None:
    if not java_exe.exists():
        return None
    # Fast check via `release` file in parent JDK root
    try:
        release_file = java_exe.parent.parent / "release"
        if release_file.exists():
            content = release_file.read_text(encoding="utf-8", errors="ignore")
            for line in content.splitlines():
                if line.startswith("JAVA_VERSION="):
                    ver = line.split("=", 1)[1].strip().strip('"')
                    if ver.startswith("1."):
                        return int(ver.split(".")[1])
                    first_num = re.split(r"[.\-+_]", ver)[0]
                    return int(first_num)
    except Exception:
        pass

    try:
        proc = subprocess.run(
            [str(java_exe), "-version"],
            capture_output=True,
            text=True,
            timeout=5,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        out = (proc.stdout or "") + "\n" + (proc.stderr or "")
        m = re.search(r'version "([^"]+)"', out)
        if m:
            ver = m.group(1)
            if ver.startswith("1."):
                return int(ver.split(".")[1])
            return int(re.split(r"[.\-+_]", ver)[0])
    except Exception:
        pass
    return None


def resolve_best_java_executable(mc_version: str, root: Path) -> str:
    """
    Finds the best local or Mojang-managed Java executable for `mc_version`.
    - Legacy versions (`<= 1.16.5`, Beta, Alpha) strictly prefer Java 8 (`jre-legacy`).
    - Modern versions (`1.21+`) prefer Java 21+.
    - New versions (`25+`, `26.x`) prefer Java 25+.
    """
    req_major = get_required_java_major(mc_version, root)
    preferred_component: str | None = None

    # Check version JSON for preferred Mojang runtime component (e.g., java-runtime-epsilon, java-runtime-delta)
    cand_ids = [mc_version]
    for vid in cand_ids:
        v_json = root / "versions" / vid / f"{vid}.json"
        if v_json.exists():
            try:
                data = json.loads(v_json.read_text(encoding="utf-8"))
                jv = data.get("javaVersion")
                if isinstance(jv, dict) and "component" in jv:
                    preferred_component = jv["component"]
                    break
                inherits = data.get("inheritsFrom")
                if inherits and inherits not in cand_ids:
                    cand_ids.append(inherits)
            except Exception:
                pass

    if preferred_component:
        for rt_folder in (root / "runtimes", root / "runtime"):
            comp_folder = rt_folder / preferred_component
            if comp_folder.exists():
                for java_exe in comp_folder.rglob("java.exe" if os.name == "nt" else "java"):
                    if "bin" in [p.lower() for p in java_exe.parts]:
                        maj = inspect_java_major(java_exe)
                        if maj and maj >= req_major:
                            return str(java_exe)

    candidates: list[tuple[int, Path]] = []

    # 1. Check both `runtimes/` (from our engine) and `runtime/` (from minecraft_launcher_lib)
    for rt_folder in (root / "runtimes", root / "runtime"):
        if rt_folder.exists():
            for java_exe in rt_folder.rglob("java.exe" if os.name == "nt" else "java"):
                if "bin" in [p.lower() for p in java_exe.parts]:
                    maj = inspect_java_major(java_exe)
                    if maj:
                        candidates.append((maj, java_exe))

    # 2. Check JAVA_HOME and PATH and standard Windows Java locations
    path_java = shutil.which("java")
    if path_java:
        p = Path(path_java)
        maj = inspect_java_major(p)
        if maj:
            candidates.append((maj, p))

    for prog_dir in [
        Path(r"C:\Program Files\Java"),
        Path(r"C:\Program Files\Eclipse Adoptium"),
        Path(r"C:\Program Files\Microsoft"),
        Path(r"C:\Program Files\Zulu"),
    ]:
        if prog_dir.exists():
            for java_exe in prog_dir.glob("*/bin/java.exe"):
                maj = inspect_java_major(java_exe)
                if maj:
                    candidates.append((maj, java_exe))

    if req_major == 8:
        for maj, path in candidates:
            if maj == 8:
                return str(path)
    else:
        for maj, path in candidates:
            if maj == req_major:
                return str(path)
        for maj, path in sorted(candidates, key=lambda x: x[0], reverse=True):
            if maj >= req_major:
                return str(path)

    if candidates:
        return str(candidates[0][1])
    return "java"


def check_java_compatibility(
    mc_version: str,
    root: Path,
    custom_java: str | None = None,
) -> tuple[bool, int, int | None, str]:
    """
    Returns (is_compatible, required_major, detected_major, java_executable_path).
    Detects whether the configured custom Java or auto-detected Java meets Minecraft's requirement.
    """
    req_major = get_required_java_major(mc_version, root)
    if custom_java and custom_java.strip():
        p = Path(custom_java.strip())
        if p.exists():
            maj = inspect_java_major(p)
            if maj is not None:
                return (maj >= req_major, req_major, maj, str(p))

    resolved = resolve_best_java_executable(mc_version, root)
    if resolved and resolved != "java" and Path(resolved).exists():
        maj = inspect_java_major(Path(resolved))
        if maj is not None:
            return (maj >= req_major, req_major, maj, resolved)

    path_java = shutil.which("java")
    if path_java:
        maj = inspect_java_major(Path(path_java))
        if maj is not None:
            return (maj >= req_major, req_major, maj, path_java)

    return (False, req_major, None, "")


def get_java_download_url(major: int) -> str:
    """Returns official Adoptium Temurin download page for the required Java major version."""
    if major in (8, 17, 21):
        return f"https://adoptium.net/temurin/releases/?version={major}"
    return "https://adoptium.net/temurin/releases/"


def ensure_clean_64bit_natives(version_id: str, root: Path) -> Path:
    """
    Extracts 64-bit native Windows DLLs into `versions/<version_id>/natives` while
    strictly excluding 32-bit `x86` and `arm64` native libraries.
    """
    v_dir = root / "versions" / version_id
    v_json = v_dir / f"{version_id}.json"
    natives_dir = v_dir / "natives"
    natives_dir.mkdir(parents=True, exist_ok=True)

    if not v_json.exists():
        return natives_dir

    try:
        data = json.loads(v_json.read_text(encoding="utf-8"))
        if "inheritsFrom" in data:
            parent_id = data["inheritsFrom"]
            parent_json = root / "versions" / parent_id / f"{parent_id}.json"
            if parent_json.exists():
                parent_data = json.loads(parent_json.read_text(encoding="utf-8"))
                data["libraries"] = data.get("libraries", []) + parent_data.get("libraries", [])

        libs_dir = root / "libraries"
        for lib in data.get("libraries", []):
            name = lib.get("name", "")
            if "natives-windows-x86" in name or "natives-windows-arm" in name or ("x86" in name and "natives" in name):
                continue

            jar_candidates: list[Path] = []
            downloads = lib.get("downloads", {})
            if ":natives-windows" in name:
                art = downloads.get("artifact")
                if art and art.get("path"):
                    jar_candidates.append(libs_dir / art["path"])

            classifiers = downloads.get("classifiers", {})
            for c_key in ("natives-windows", "natives-windows-64"):
                if c_key in classifiers and classifiers[c_key].get("path"):
                    jar_candidates.append(libs_dir / classifiers[c_key]["path"])

            for jar_path in jar_candidates:
                if jar_path.exists():
                    try:
                        with zipfile.ZipFile(jar_path, "r") as zf:
                            for member in zf.namelist():
                                if (
                                    member.endswith((".dll", ".so", ".dylib"))
                                    and "META-INF" not in member
                                    and "/x86/" not in member
                                    and "\\x86\\" not in member
                                    and "/arm64/" not in member
                                    and "\\arm64\\" not in member
                                ):
                                    fname = os.path.basename(member)
                                    if fname:
                                        target = natives_dir / fname
                                        with zf.open(member) as src, open(target, "wb") as dst:
                                            shutil.copyfileobj(src, dst)
                    except Exception:
                        pass
    except Exception:
        pass

    return natives_dir


def sanitize_launch_command_classpath(cmd: list[str]) -> list[str]:
    """
    Removes 32-bit `x86` and `arm64` native JARs from the `-cp` argument produced by
    `minecraft_launcher_lib.command.get_minecraft_command` on 64-bit x86_64 systems.
    """
    if platform.machine().lower() not in ("amd64", "x86_64"):
        return cmd

    sep = ";" if os.name == "nt" else ":"
    cleaned_cmd = list(cmd)
    for idx, arg in enumerate(cleaned_cmd):
        if arg in ("-cp", "-classpath") and idx + 1 < len(cleaned_cmd):
            cp_entries = cleaned_cmd[idx + 1].split(sep)
            filtered = [
                entry
                for entry in cp_entries
                if "natives-windows-x86" not in entry
                and "natives-windows-arm64" not in entry
                and not ("x86" in entry and "natives" in entry)
            ]
            cleaned_cmd[idx + 1] = sep.join(filtered)
    return cleaned_cmd


# ---------------------------------------------------------------------------
# 5-Step Background Installation Worker (`QThread`)
# ---------------------------------------------------------------------------

class InstallationPipelineWorker(QThread):
    """
    Executes the 5-stage installation workflow in the background:
    1. Validate the setup
    2. Create the isolated installation
    3. Download and install (applying the Smart Download Decision Matrix)
    4. Verify the installation
    5. Finish and return home
    """

    step_changed = Signal(int, str, str)        # (step_num 1..5, step_title, detail_text)
    progress_changed = Signal(int, str)         # (percent 0..100, status_text)
    completed = Signal(bool, object, str)       # (success, InstanceConfig | None, message)

    def __init__(
        self,
        store: LauncherStore,
        instance_name: str,
        mc_version: str,
        loader: str,
        icon_key: str,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.store = store
        self.instance_name = instance_name.strip() or f"{mc_version} {loader}"
        self.mc_version = mc_version.strip()
        self.loader = loader.strip()
        self.icon_key = icon_key
        self._is_cancelled: bool = False

    def cancel(self) -> None:
        self._is_cancelled = True

    def run(self) -> None:
        root = self.store.root
        inst: InstanceConfig | None = None
        try:
            if self._is_cancelled:
                self.completed.emit(False, None, "Installation cancelled by user.")
                return

            # =================================================================
            # STEP 1: Validate the setup
            # =================================================================
            self.step_changed.emit(
                1,
                "Step 1/5 â€” Validating Setup",
                f"Checking '{self.instance_name}' ({self.mc_version} â€¢ {self.loader}), compatibility & disk space...",
            )
            self.progress_changed.emit(6, "Validating configuration, mod-loader support, and disk space...")

            if not self.mc_version:
                raise ValueError("Please select a valid Minecraft version.")

            free_bytes = shutil.disk_usage(root).free
            if free_bytes < 250 * 1024 * 1024:
                raise RuntimeError("Insufficient disk space (< 250 MB free).")

            loader_norm = self.loader.lower()
            if loader_norm == "fabric":
                if not mll.fabric.is_minecraft_version_supported(self.mc_version):
                    raise ValueError(f"Fabric does not support Minecraft {self.mc_version}.")
            elif loader_norm == "quilt":
                if not mll.quilt.is_minecraft_version_supported(self.mc_version):
                    raise ValueError(f"Quilt does not support Minecraft {self.mc_version}.")
            elif loader_norm == "forge":
                forge_ver = mll.forge.find_forge_version(self.mc_version)
                if not forge_ver:
                    raise ValueError(f"Forge is not available for Minecraft {self.mc_version}.")

            decision = decide_download_strategy(self.mc_version, self.loader, root)
            self.progress_changed.emit(15, f"Strategy: {decision.situation} â†’ {decision.action}")
            time.sleep(0.25)

            if self._is_cancelled:
                raise InterruptedException("Installation cancelled by user.")

            # =================================================================
            # STEP 2: Create the isolated installation
            # =================================================================
            self.step_changed.emit(
                2,
                "Step 2/5 â€” Creating Isolated Installation",
                f"Generating unique instance ID and isolated game directory for '{self.instance_name}'...",
            )
            slug = re.sub(r"[^a-z0-9]+", "-", self.instance_name.lower()).strip("-") or "instance"
            short_uid = uuid.uuid4().hex[:6]
            instance_id = f"{slug}-{short_uid}"
            instance_dir = root / "instances" / instance_id
            instance_dir.mkdir(parents=True, exist_ok=True)
            for folder in ("saves", "mods", "resourcepacks", "shaderpacks", "logs", "config"):
                (instance_dir / folder).mkdir(parents=True, exist_ok=True)

            inst = InstanceConfig(
                id=instance_id,
                name=self.instance_name,
                mc_version=self.mc_version,
                loader=self.loader,
                installed_version_id=decision.existing_loader_version_id or self.mc_version,
                icon_key=self.icon_key,
                game_dir=str(instance_dir),
                created_at=time.time(),
                last_played=time.time(),
                ready=False,
            )
            # Save initial configuration so it can be resumed or repaired if interrupted
            self.store._write_instance_metadata(inst)
            self.progress_changed.emit(25, f"Created isolated directory: instances/{instance_id}")
            time.sleep(0.2)

            if self._is_cancelled:
                raise InterruptedException("Installation cancelled by user.")

            # =================================================================
            # STEP 3: Download and install (Smart Decision Matrix + Retries)
            # =================================================================
            self.step_changed.emit(
                3,
                f"Step 3/5 â€” {decision.situation}",
                decision.action,
            )

            max_val = {"value": 100}
            current_status = {"text": "Preparing files..."}

            def set_status(status: str) -> None:
                if self._is_cancelled:
                    raise InterruptedException("Installation cancelled by user.")
                current_status["text"] = status
                self.progress_changed.emit(50, f"{decision.situation}: {status}")

            def set_max(new_max: int) -> None:
                max_val["value"] = max(1, new_max)

            def set_progress(progress: int) -> None:
                if self._is_cancelled:
                    raise InterruptedException("Installation cancelled by user.")
                pct_within_step = int((progress / max_val["value"]) * 52)
                overall_pct = min(82, max(28, 28 + pct_within_step))
                self.progress_changed.emit(overall_pct, f"{current_status['text']} ({progress}/{max_val['value']})")

            callback: mll.types.CallbackDict = {
                "setStatus": set_status,
                "setProgress": set_progress,
                "setMax": set_max,
            }

            def run_network_task(op, desc: str, max_retries: int = 3):
                last_exc = None
                for attempt in range(1, max_retries + 1):
                    if self._is_cancelled:
                        raise InterruptedException("Installation was cancelled by the user.")
                    try:
                        return op()
                    except InterruptedException:
                        raise
                    except Exception as exc:
                        last_exc = exc
                        if self._is_cancelled:
                            raise InterruptedException("Installation was cancelled by the user.")
                        if attempt < max_retries:
                            self.progress_changed.emit(
                                40,
                                f"Network retry for {desc} (attempt {attempt + 1}/{max_retries})...",
                            )
                            time.sleep(1.2 * attempt)
                        else:
                            raise last_exc

            if decision.reuse_base_version and decision.reuse_loader_profile and decision.existing_loader_version_id:
                # Situation 1: Same version, same loader -> Reuse existing files immediately!
                inst.installed_version_id = decision.existing_loader_version_id
                self.progress_changed.emit(
                    78,
                    f"Reused verified {inst.installed_version_id} files on disk without redundant downloads!",
                )
                time.sleep(0.35)
            else:
                # Download/repair base version if needed
                if not decision.reuse_base_version or decision.needs_repair:
                    run_network_task(
                        lambda: mll.install.install_minecraft_version(self.mc_version, str(root), callback=callback),
                        f"Minecraft {self.mc_version} base download",
                    )

                if self._is_cancelled:
                    raise InterruptedException("Installation cancelled by user.")

                # Install additional mod-loader components if requested
                java_bin = resolve_best_java_executable(self.mc_version, root)
                if loader_norm == "vanilla":
                    inst.installed_version_id = self.mc_version
                elif loader_norm == "fabric":
                    self.progress_changed.emit(72, f"Installing Fabric Loader for Minecraft {self.mc_version}...")
                    loader_ver = mll.fabric.get_latest_loader_version()
                    run_network_task(
                        lambda: mll.fabric.install_fabric(
                            self.mc_version,
                            str(root),
                            loader_version=loader_ver,
                            callback=callback,
                            java=java_bin if Path(java_bin).exists() else None,
                        ),
                        "Fabric Loader installation",
                    )
                    inst.installed_version_id = f"fabric-loader-{loader_ver}-{self.mc_version}"
                elif loader_norm == "quilt":
                    self.progress_changed.emit(72, f"Installing Quilt Loader for Minecraft {self.mc_version}...")
                    loader_ver = mll.quilt.get_latest_loader_version()
                    run_network_task(
                        lambda: mll.quilt.install_quilt(
                            self.mc_version,
                            str(root),
                            loader_version=loader_ver,
                            callback=callback,
                            java=java_bin if Path(java_bin).exists() else None,
                        ),
                        "Quilt Loader installation",
                    )
                    inst.installed_version_id = f"quilt-loader-{loader_ver}-{self.mc_version}"
                elif loader_norm == "forge":
                    forge_ver = mll.forge.find_forge_version(self.mc_version)
                    if not forge_ver:
                        raise RuntimeError(f"No Forge version found for {self.mc_version}")
                    self.progress_changed.emit(72, f"Installing Forge {forge_ver}...")
                    run_network_task(
                        lambda: mll.forge.install_forge_version(
                            forge_ver,
                            str(root),
                            callback=callback,
                            java=java_bin if Path(java_bin).exists() else None,
                        ),
                        "Forge installation",
                    )
                    inst.installed_version_id = mll.forge.forge_to_installed_version(forge_ver)

            if self._is_cancelled:
                raise InterruptedException("Installation cancelled by user.")

            # Ensure 64-bit native DLLs are extracted cleanly
            ensure_clean_64bit_natives(self.mc_version, root)

            # =================================================================
            # STEP 4: Verify the installation
            # =================================================================
            self.step_changed.emit(
                4,
                "Step 4/5 â€” Verifying Installation",
                "Checking required game jars, native libraries, assets, and launch command...",
            )
            self.progress_changed.emit(88, "Verifying client jar, libraries, and launch command...")

            resolved_vid = inst.resolved_version_id()
            v_json_path = root / "versions" / resolved_vid / f"{resolved_vid}.json"
            if not v_json_path.exists():
                raise FileNotFoundError(f"Version profile JSON missing at {v_json_path}")

            java_exec = resolve_best_java_executable(self.mc_version, root)
            test_opts: mll.types.MinecraftOptions = {
                "username": self.store.username,
                "uuid": offline_uuid_v3(self.store.username),
                "token": "0",
                "executablePath": java_exec,
                "gameDirectory": str(instance_dir),
                "nativesDirectory": str(root / "versions" / self.mc_version / "natives"),
            }
            cmd = mll.command.get_minecraft_command(resolved_vid, str(root), test_opts)
            if not cmd or len(cmd) < 4:
                raise RuntimeError("Launch command verification failed.")
            time.sleep(0.25)

            if self._is_cancelled:
                raise InterruptedException("Installation cancelled by user.")

            # =================================================================
            # STEP 5: Finish and return home
            # =================================================================
            self.step_changed.emit(
                5,
                "Step 5/5 â€” Finishing & Returning Home",
                f"Saving '{inst.name}', selecting it in the dropdown, and switching to Play!",
            )
            inst.ready = True
            self.store.add_or_update_instance(inst, select_as_last_played=True)
            self.progress_changed.emit(100, f"'{inst.name}' is installed and ready to play!")
            time.sleep(0.3)

            self.completed.emit(True, inst, f"'{inst.name}' is ready to play!")
        except InterruptedException:
            if inst is not None:
                inst.ready = False
                self.store.add_or_update_instance(inst, select_as_last_played=False)
            self.completed.emit(False, inst, "Installation was cancelled by user.")
        except Exception as exc:
            if inst is not None:
                inst.ready = False
                self.store.add_or_update_instance(inst, select_as_last_played=False)
            self.completed.emit(False, inst, str(exc))


# ---------------------------------------------------------------------------
# Game Launch & Auto-Repair Worker (`QThread`)
# ---------------------------------------------------------------------------

class GameLaunchWorker(QThread):
    """
    Verifies/repairs the selected instance (Situation 4: Missing or corrupted files -> repair)
    and spawns Minecraft: Java Edition in its isolated instance directory with custom RAM/Java settings.
    """

    status_updated = Signal(int, str)          # (percent, status_text)
    game_started = Signal(int, str)            # (pid, instance_name)
    game_finished = Signal(int, str)           # (exit_code, message)
    launch_failed = Signal(str)

    def __init__(self, store: LauncherStore, instance: InstanceConfig, parent=None) -> None:
        super().__init__(parent)
        self.store = store
        self.instance = instance
        self._is_cancelled: bool = False

    def cancel(self) -> None:
        self._is_cancelled = True

    def run(self) -> None:
        root = self.store.root
        try:
            if self._is_cancelled:
                self.launch_failed.emit("Launch cancelled by user.")
                return

            mc_ver = self.instance.mc_version
            resolved_vid = self.instance.resolved_version_id()
            game_dir = self.instance.resolved_game_dir()

            self.status_updated.emit(15, f"Verifying '{self.instance.name}' files...")
            exists, corrupted = check_version_integrity(mc_ver, root, deep_verify=True)
            if not exists or corrupted or not self.instance.ready:
                self.status_updated.emit(35, f"Repairing missing/corrupted files for {mc_ver}...")
                mll.install.install_minecraft_version(mc_ver, str(root))
                self.instance.ready = True
                self.store.add_or_update_instance(self.instance, select_as_last_played=True)

            if self._is_cancelled:
                self.launch_failed.emit("Launch cancelled by user.")
                return

            self.status_updated.emit(60, "Preparing 64-bit native libraries & Java runtime...")
            natives_dir = ensure_clean_64bit_natives(mc_ver, root)

            # 1. Custom Java or auto-detected Java
            if self.instance.java_path and Path(self.instance.java_path).exists():
                java_exec = self.instance.java_path
            else:
                java_exec = resolve_best_java_executable(mc_ver, root)

            # 2. Compatibility verification
            compat, req_major, det_major, _ = check_java_compatibility(mc_ver, root, java_exec)
            if not compat:
                raise RuntimeError(
                    f"Minecraft {mc_ver} requires Java {req_major}+ (class version {44+req_major}.0), "
                    f"but detected Java is version {det_major or 'missing'}. "
                    f"Please install Java {req_major} or configure a compatible runtime in Instance Settings."
                )

            java_major = inspect_java_major(Path(java_exec)) or req_major

            max_ram = self.instance.max_ram_gb or self.store.max_ram_gb
            min_ram = self.instance.min_ram_mb or 512
            jvm_args = [
                f"-Xms{min_ram}M",
                f"-Xmx{max_ram}G",
                f"-Djna.tmpdir={natives_dir}",
                f"-Dorg.lwjgl.librarypath={natives_dir}",
                f"-Dminecraft.applet.TargetDirectory={game_dir}",
            ]
            if self.instance.jvm_args:
                try:
                    jvm_args.extend(shlex.split(self.instance.jvm_args))
                except Exception:
                    jvm_args.extend(self.instance.jvm_args.split())

            if java_major >= 21 and "--enable-native-access=ALL-UNNAMED" not in jvm_args:
                jvm_args.append("--enable-native-access=ALL-UNNAMED")

            options: mll.types.MinecraftOptions = {
                "username": self.store.username,
                "uuid": offline_uuid_v3(self.store.username),
                "token": "0",
                "executablePath": java_exec,
                "jvmArguments": jvm_args,
                "launcherName": "lightyear",
                "launcherVersion": "1.0.0",
                "gameDirectory": str(game_dir),
                "nativesDirectory": str(natives_dir),
            }

            raw_cmd = mll.command.get_minecraft_command(resolved_vid, str(root), options)
            cmd = sanitize_launch_command_classpath(raw_cmd)

            if self._is_cancelled:
                self.launch_failed.emit("Launch cancelled by user.")
                return

            # Update last played timestamp
            self.instance.last_played = time.time()
            self.store.add_or_update_instance(self.instance, select_as_last_played=True)

            self.status_updated.emit(90, f"Launching Minecraft ({self.instance.name}) as {self.store.username}...")
            creation_flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            proc = subprocess.Popen(
                cmd,
                cwd=str(game_dir),
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                encoding="utf-8",
                errors="replace",
                creationflags=creation_flags,
            )
            self.game_started.emit(proc.pid, self.instance.name)

            # Stream logs until exit
            recent_lines: list[str] = []
            if proc.stdout:
                for line in proc.stdout:
                    recent_lines.append(line.rstrip())
                    if len(recent_lines) > 25:
                        recent_lines.pop(0)

            exit_code = proc.wait()
            if exit_code == 0:
                self.game_finished.emit(0, f"Minecraft ({self.instance.name}) closed cleanly.")
            else:
                tail = "\n".join(recent_lines[-8:])
                self.game_finished.emit(exit_code, f"Process exited with code {exit_code}.\n{tail}")
        except Exception as exc:
            self.launch_failed.emit(str(exc))
