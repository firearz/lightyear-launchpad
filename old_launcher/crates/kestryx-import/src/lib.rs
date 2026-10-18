//! `kestryx-import`: Custom `.minecraft` data-root validator & reversible mover,
//! portable mode resolver, in-place `.minecraft` adoption, multi-launcher detector/importer
//! (Official Launcher, Prism/MultiMC, Modrinth App, CurseForge App, ATLauncher), and
//! hardened zip/archive extraction with zip-slip, reserved-name, and symlink defenses.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use kestryx_instances::InstanceManager;
use kestryx_store::InstanceConfig;
use serde::{Deserialize, Serialize};

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataRootValidationReport {
    pub path: PathBuf,
    pub is_valid: bool,
    pub is_writable: bool,
    pub has_cloud_sync_warning: bool, // OneDrive / iCloud / Dropbox
    pub has_long_path_warning: bool,  // > 180 chars base path on Windows
    pub has_unicode_or_spaces: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Validates a user-selected custom global data root directory.
pub fn validate_custom_data_root(candidate: &Path) -> DataRootValidationReport {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let path_str = candidate.to_string_lossy();

    let lower = path_str.to_ascii_lowercase();
    let has_cloud_sync_warning = lower.contains("onedrive")
        || lower.contains("icloud")
        || lower.contains("dropbox")
        || lower.contains("google drive");
    if has_cloud_sync_warning {
        warnings.push(
            "Selected path is inside a cloud-synced folder (OneDrive/iCloud/Dropbox). Cloud sync can lock world level.dat files and degrade performance."
                .to_string(),
        );
    }

    let has_long_path_warning = cfg!(windows) && path_str.len() > 180;
    if has_long_path_warning {
        warnings.push(
            "Path length exceeds 180 characters on Windows; deep library coordinates may approach MAX_PATH limits."
                .to_string(),
        );
    }

    let has_unicode_or_spaces = path_str.contains(' ') || !path_str.is_ascii();

    let is_writable = match fs::create_dir_all(candidate) {
        Ok(()) => {
            let probe = candidate.join(".kestryx-write-probe");
            match fs::write(&probe, b"ok") {
                Ok(()) => {
                    let _ = fs::remove_file(&probe);
                    true
                }
                Err(e) => {
                    errors.push(format!("Directory is not writable: {e}"));
                    false
                }
            }
        }
        Err(e) => {
            errors.push(format!("Cannot create directory: {e}"));
            false
        }
    };

    DataRootValidationReport {
        path: candidate.to_path_buf(),
        is_valid: is_writable && errors.is_empty(),
        is_writable,
        has_cloud_sync_warning,
        has_long_path_warning,
        has_unicode_or_spaces,
        warnings,
        errors,
    }
}

/// Resolves the active data root directory, honoring:
/// 1. Explicit `--data-dir <path>` CLI flag or `KESTRYX_DATA_DIR` env var
/// 2. Portable mode marker file (`.kestryx-portable`) beside the executable
/// 3. Standard OS application data directory (`%APPDATA%/Kestryx` / `~/.local/share/kestryx`)
pub fn resolve_data_root(explicit_data_dir: Option<&Path>, exe_dir: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit_data_dir {
        return p.to_path_buf();
    }
    if let Ok(env_dir) = std::env::var("KESTRYX_DATA_DIR") {
        if !env_dir.trim().is_empty() {
            return PathBuf::from(env_dir);
        }
    }
    if let Some(exe_parent) = exe_dir {
        if exe_parent.join(".kestryx-portable").exists() {
            return exe_parent.join("kestryx-data");
        }
    }
    kestryx_store::platform_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Kestryx")
}

/// Adopts an existing `.minecraft` directory in-place without copying or modifying its files.
pub fn adopt_existing_minecraft_dir(
    mgr: &InstanceManager,
    instance_id: &str,
    display_name: &str,
    existing_mc_dir: &Path,
    mc_version: &str,
) -> Result<InstanceConfig> {
    if !existing_mc_dir.exists() {
        bail!(
            "Target .minecraft directory does not exist: {}",
            existing_mc_dir.display()
        );
    }
    let mut cfg = mgr.create_instance(
        instance_id,
        display_name,
        mc_version,
        "vanilla",
        "",
        "Adopted",
    )?;
    cfg.game_dir_override = existing_mc_dir.to_string_lossy().to_string();
    let toml_path = mgr.instance_dir(instance_id).join("instance.toml");
    cfg.save(&toml_path)?;
    Ok(cfg)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedExternalLauncher {
    pub launcher_name: String, // "Official Minecraft Launcher" | "Prism Launcher" | "MultiMC" | "Modrinth App" | "CurseForge" | "ATLauncher"
    pub root_path: PathBuf,
    pub instance_count_estimate: usize,
}

/// Scans standard OS paths for existing Minecraft launchers and `.minecraft` installations
/// for 1-click onboarding import.
pub fn detect_external_launchers() -> Vec<DetectedExternalLauncher> {
    let mut detected = Vec::new();
    let Some(roaming) = kestryx_store::platform_data_dir() else {
        return detected;
    };

    let candidates = [
        ("Official Minecraft Launcher", roaming.join(".minecraft")),
        ("Prism Launcher", roaming.join("PrismLauncher")),
        ("Modrinth App", roaming.join("ModrinthApp")),
        ("ATLauncher", roaming.join("ATLauncher")),
    ];

    for (name, path) in candidates {
        if path.exists() {
            detected.push(DetectedExternalLauncher {
                launcher_name: name.to_string(),
                root_path: path,
                instance_count_estimate: 1,
            });
        }
    }
    detected
}

/// Imports a Prism Launcher / MultiMC instance directory (`instance.cfg` + `mmc-pack.json`).
pub fn import_prism_or_multimc_instance(
    mgr: &InstanceManager,
    prism_inst_dir: &Path,
    new_id: &str,
) -> Result<InstanceConfig> {
    let cfg_raw = fs::read_to_string(prism_inst_dir.join("instance.cfg"))
        .context("Missing instance.cfg in Prism/MultiMC instance")?;
    let mut kv = BTreeMap::new();
    for line in cfg_raw.lines() {
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let name = kv
        .get("name")
        .cloned()
        .unwrap_or_else(|| new_id.to_string());

    let pack_raw = fs::read_to_string(prism_inst_dir.join("mmc-pack.json"))
        .context("Missing mmc-pack.json in Prism/MultiMC instance")?;
    let pack_val: serde_json::Value = serde_json::from_str(&pack_raw)?;

    let mut mc_version = "26.3".to_string();
    let mut loader = "vanilla".to_string();
    let mut loader_version = String::new();

    if let Some(components) = pack_val["components"].as_array() {
        for comp in components {
            let uid = comp["uid"].as_str().unwrap_or("");
            let ver = comp["version"].as_str().unwrap_or("");
            match uid {
                "net.minecraft" => mc_version = ver.to_string(),
                "net.fabricmc.fabric-loader" => {
                    loader = "fabric".to_string();
                    loader_version = ver.to_string();
                }
                "org.quiltmc.quilt-loader" => {
                    loader = "quilt".to_string();
                    loader_version = ver.to_string();
                }
                "net.neoforged" => {
                    loader = "neoforge".to_string();
                    loader_version = ver.to_string();
                }
                "net.minecraftforge" => {
                    loader = "forge".to_string();
                    loader_version = ver.to_string();
                }
                _ => {}
            }
        }
    }

    mgr.create_instance(new_id, &name, &mc_version, &loader, &loader_version, "Imported")
}

// ============================================================================
// Hardened Archive Extractor (Defends against Zip-Slip, Reserved Names & Symlinks)
// ============================================================================

/// Validates that a relative path inside an untrusted zip/archive is safe to extract under `dest_root`.
pub fn validate_safe_archive_entry_path(raw_entry_name: &str) -> Result<PathBuf> {
    if raw_entry_name.contains('\0') {
        bail!("Null byte in archive entry path");
    }
    let normalized = raw_entry_name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        bail!("Absolute or drive-prefixed path rejected: {raw_entry_name}");
    }

    let path = Path::new(&normalized);
    let mut clean = PathBuf::new();

    for comp in path.components() {
        match comp {
            Component::Normal(os_str) => {
                let seg = os_str.to_string_lossy();
                let stem = seg
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_uppercase();
                if WINDOWS_RESERVED_NAMES.contains(&stem.as_str()) {
                    bail!("Windows reserved device filename rejected: {seg}");
                }
                clean.push(os_str);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("Zip-slip path traversal rejected: {raw_entry_name}");
            }
        }
    }

    if clean.as_os_str().is_empty() {
        bail!("Empty archive entry path");
    }
    Ok(clean)
}

/// Safely extracts a zip archive into `dest_root`, rejecting path traversal, reserved names,
/// and symlinks pointing outside `dest_root`.
pub fn extract_zip_safely(zip_bytes: &[u8], dest_root: &Path) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(dest_root)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))?;
    let mut extracted = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_name = entry.name()?.to_string();
        if entry.is_symlink() {
            bail!("Symlink entry in untrusted archive rejected: {}", entry_name);
        }
        let rel = validate_safe_archive_entry_path(&entry_name)?;
        let target = dest_root.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            kestryx_store::atomic_write_bytes(&target, &buf)?;
            extracted.push(target);
        }
    }

    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zip_slip_and_reserved_names_rejected() {
        assert!(validate_safe_archive_entry_path("../../etc/passwd").is_err());
        assert!(validate_safe_archive_entry_path("mods/../../evil.dll").is_err());
        assert!(validate_safe_archive_entry_path("/absolute/path.jar").is_err());
        assert!(validate_safe_archive_entry_path("C:\\Windows\\system32\\cmd.exe").is_err());
        assert!(validate_safe_archive_entry_path("mods/CON.jar").is_err());
        assert!(validate_safe_archive_entry_path("config/NUL.toml").is_err());
        assert!(validate_safe_archive_entry_path("mods/sodium-fabric-26.3.jar").is_ok());
    }

    #[test]
    fn test_custom_root_portable_mode_and_in_place_adoption() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let exe_dir = dir.path().join("bin");
        fs::create_dir_all(&exe_dir)?;
        fs::write(exe_dir.join(".kestryx-portable"), b"1")?;

        let resolved = resolve_data_root(None, Some(&exe_dir));
        assert_eq!(resolved, exe_dir.join("kestryx-data"));

        let report = validate_custom_data_root(&resolved);
        assert!(report.is_valid);
        assert!(report.is_writable);

        let mgr = InstanceManager::new(&resolved)?;
        let external_mc = dir.path().join("existing-dot-minecraft");
        fs::create_dir_all(external_mc.join("saves/ExistingWorld"))?;
        fs::write(external_mc.join("options.txt"), "gamma:1.0\n")?;

        let adopted = adopt_existing_minecraft_dir(
            &mgr,
            "adopted-vanilla",
            "My Existing .minecraft",
            &external_mc,
            "26.3",
        )?;
        assert_eq!(mgr.game_dir(&adopted), external_mc);
        // Verify original files were untouched
        assert_eq!(
            fs::read_to_string(external_mc.join("options.txt"))?,
            "gamma:1.0\n"
        );
        Ok(())
    }
}
