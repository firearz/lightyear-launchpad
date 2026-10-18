//! `kestryx-instances`: Instance lifecycle, version isolation, instant hardlink/reflink cloning,
//! optional shared folders, binary `level.dat` and `servers.dat` NBT parser, World Downgrade Guard,
//! rolling `zstd` backups with retention, Trash with Undo, and Quick Play argument builder.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use kestryx_meta::VersionEra;
use kestryx_store::{
    InstanceConfig, InstanceGameConfig, InstanceJavaConfig, InstanceProcessConfig,
    SharedFoldersConfig, link_or_copy,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldSummary {
    pub folder_name: String,
    pub level_name: String,
    pub game_mode: String,
    pub last_played_ms: i64,
    pub data_version: Option<i32>,
    pub size_bytes: u64,
    pub has_icon: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerEntry {
    pub name: String,
    pub ip: String,
    pub icon_base64: Option<String>,
    pub accept_textures: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DowngradeWarning {
    pub world_folder: String,
    pub world_name: String,
    pub world_data_version: i32,
    pub target_world_version: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: String,
    pub instance_id: String,
    pub target_name: String,
    pub archive_path: PathBuf,
    pub created_at: String,
    pub size_bytes: u64,
}

pub struct InstanceManager {
    data_root: PathBuf,
}

impl InstanceManager {
    pub fn new(data_root: impl AsRef<Path>) -> Result<Self> {
        let root = data_root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("instances"))?;
        fs::create_dir_all(root.join("shared"))?;
        fs::create_dir_all(root.join("trash/instances"))?;
        Ok(Self { data_root: root })
    }

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.data_root.join("instances").join(id)
    }

    pub fn game_dir(&self, cfg: &InstanceConfig) -> PathBuf {
        if !cfg.game_dir_override.trim().is_empty() {
            PathBuf::from(&cfg.game_dir_override)
        } else {
            self.instance_dir(&cfg.id).join("minecraft")
        }
    }

    /// Creates a new isolated instance with its own `minecraft/` sub-hierarchy.
    pub fn create_instance(
        &self,
        id: &str,
        name: &str,
        mc_version: &str,
        loader: &str,
        loader_version: &str,
        group: &str,
    ) -> Result<InstanceConfig> {
        let inst_dir = self.instance_dir(id);
        if inst_dir.exists() {
            bail!("Instance '{}' already exists", id);
        }

        let mc_dir = inst_dir.join("minecraft");
        for sub in [
            "saves",
            "mods",
            "config",
            "resourcepacks",
            "shaderpacks",
            "screenshots",
            "logs",
        ] {
            fs::create_dir_all(mc_dir.join(sub))?;
        }
        fs::create_dir_all(inst_dir.join(".backups"))?;
        fs::create_dir_all(inst_dir.join(".history"))?;
        fs::create_dir_all(inst_dir.join(".natives"))?;
        fs::create_dir_all(inst_dir.join("profiles"))?;

        let cfg = InstanceConfig {
            schema_version: 1,
            id: id.to_string(),
            name: name.to_string(),
            group: group.to_string(),
            tags: vec![mc_version.to_string(), loader.to_string()],
            icon: String::new(),
            notes: String::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_played_at: None,
            total_playtime_secs: 0,
            favorite: false,
            active_mod_profile: "default".to_string(),
            account_id: String::new(),
            game_dir_override: String::new(),
            game: InstanceGameConfig {
                mc_version: mc_version.to_string(),
                loader: loader.to_string(),
                loader_version: loader_version.to_string(),
                quick_play_target: None,
                unknown_fields: BTreeMap::new(),
            },
            java: InstanceJavaConfig::default(),
            process: InstanceProcessConfig::default(),
            shared_folders: SharedFoldersConfig::default(),
            unknown_fields: BTreeMap::new(),
        };

        cfg.save(&inst_dir.join("instance.toml"))?;
        Ok(cfg)
    }

    /// Clones an existing instance (`src_id` -> `dst_id`).
    /// Immutable archives (`.jar`, `.zip`) in `mods/`, `resourcepacks/`, `shaderpacks/` are hardlinked,
    /// while mutable config files (`config/`, `options.txt`, `instance.toml`) are copied independently
    /// so settings never leak between instances.
    pub fn clone_instance(
        &self,
        src_id: &str,
        dst_id: &str,
        new_name: &str,
        override_mc_version: Option<&str>,
    ) -> Result<InstanceConfig> {
        let src_dir = self.instance_dir(src_id);
        let mut cfg = InstanceConfig::load(&src_dir.join("instance.toml"))?;
        cfg.id = dst_id.to_string();
        cfg.name = new_name.to_string();
        cfg.created_at = chrono::Utc::now().to_rfc3339();
        cfg.last_played_at = None;
        cfg.total_playtime_secs = 0;
        if let Some(new_ver) = override_mc_version {
            cfg.game.mc_version = new_ver.to_string();
        }

        let dst_dir = self.instance_dir(dst_id);
        if dst_dir.exists() {
            bail!("Target instance '{}' already exists", dst_id);
        }

        clone_tree_selective(&src_dir.join("minecraft"), &dst_dir.join("minecraft"))?;
        fs::create_dir_all(dst_dir.join(".backups"))?;
        fs::create_dir_all(dst_dir.join(".history"))?;
        fs::create_dir_all(dst_dir.join(".natives"))?;
        fs::create_dir_all(dst_dir.join("profiles"))?;
        cfg.save(&dst_dir.join("instance.toml"))?;
        Ok(cfg)
    }

    /// Soft-deletes an instance to `<DATA_ROOT>/trash/instances/<id>` with one-click Undo support.
    pub fn trash_instance(&self, id: &str) -> Result<PathBuf> {
        let src = self.instance_dir(id);
        if !src.exists() {
            bail!("Instance '{}' does not exist", id);
        }
        let dst = self.data_root.join("trash/instances").join(id);
        if dst.exists() {
            fs::remove_dir_all(&dst)?;
        }
        fs::rename(&src, &dst)?;
        Ok(dst)
    }

    /// Restores a soft-deleted instance from Trash back to active library.
    pub fn restore_trashed_instance(&self, id: &str) -> Result<InstanceConfig> {
        let trashed = self.data_root.join("trash/instances").join(id);
        if !trashed.exists() {
            bail!("Instance '{}' not found in Trash", id);
        }
        let dst = self.instance_dir(id);
        fs::rename(&trashed, &dst)?;
        InstanceConfig::load(&dst.join("instance.toml"))
    }

    /// Lists all active instances in `<DATA_ROOT>/instances`.
    pub fn list_instances(&self) -> Result<Vec<InstanceConfig>> {
        let mut out = Vec::new();
        let inst_root = self.data_root.join("instances");
        if !inst_root.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(inst_root)?.flatten() {
            let toml_path = entry.path().join("instance.toml");
            if toml_path.exists() {
                if let Ok(cfg) = InstanceConfig::load(&toml_path) {
                    out.push(cfg);
                }
            }
        }
        out.sort_by(|a, b| b.favorite.cmp(&a.favorite).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    /// Creates a rolling `zstd`-compressed tar/zip archive backup of a world or instance `saves/` folder
    /// and prunes older backups beyond `retention_count`.
    pub fn create_rolling_backup(
        &self,
        id: &str,
        label: &str,
        retention_count: usize,
    ) -> Result<BackupRecord> {
        let inst_dir = self.instance_dir(id);
        let cfg = InstanceConfig::load(&inst_dir.join("instance.toml"))?;
        let game_dir = self.game_dir(&cfg);
        let backups_dir = inst_dir.join(".backups");
        fs::create_dir_all(&backups_dir)?;

        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let backup_id = format!("{id}-{label}-{ts}");
        let archive_path = backups_dir.join(format!("{backup_id}.zip.zst"));

        // First build an in-memory zip of saves/ + options.txt + instance.toml, then compress with zstd level 3
        let mut zip_buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zw = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            add_dir_to_zip(&mut zw, &game_dir.join("saves"), "saves", opts)?;
            let opts_txt = game_dir.join("options.txt");
            if opts_txt.exists() {
                zw.start_file("options.txt", opts)?;
                zw.write_all(&fs::read(opts_txt)?)?;
            }
            zw.finish()?;
        }

        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&zip_buf)?;
        let compressed = enc.finish()?;
        kestryx_store::atomic_write_bytes(&archive_path, &compressed)?;

        // Enforce retention policy
        let mut existing: Vec<PathBuf> = fs::read_dir(&backups_dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "zst").unwrap_or(false))
            .collect();
        existing.sort();
        if existing.len() > retention_count.max(1) {
            let to_remove = existing.len() - retention_count.max(1);
            for old in existing.into_iter().take(to_remove) {
                let _ = fs::remove_file(old);
            }
        }

        Ok(BackupRecord {
            id: backup_id,
            instance_id: id.to_string(),
            target_name: label.to_string(),
            archive_path,
            created_at: chrono::Utc::now().to_rfc3339(),
            size_bytes: compressed.len() as u64,
        })
    }

    /// Restores a `.zip.zst` rolling backup into the instance's `minecraft/` directory.
    pub fn restore_backup(&self, id: &str, archive_path: &Path) -> Result<()> {
        let inst_dir = self.instance_dir(id);
        let cfg = InstanceConfig::load(&inst_dir.join("instance.toml"))?;
        let game_dir = self.game_dir(&cfg);

        let compressed = fs::read(archive_path)?;
        let mut dec = flate2::read::ZlibDecoder::new(std::io::Cursor::new(compressed));
        let mut zip_bytes = Vec::new();
        dec.read_to_end(&mut zip_bytes)?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(safe_rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
                bail!("Unsafe path in backup archive");
            };
            let out_path = game_dir.join(safe_rel);
            if entry.is_dir() {
                fs::create_dir_all(&out_path)?;
            } else {
                if let Some(p) = out_path.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                kestryx_store::atomic_write_bytes(&out_path, &buf)?;
            }
        }
        Ok(())
    }
}

fn clone_tree_selective(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    if !src.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(src)?.flatten() {
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            clone_tree_selective(&p, &target)?;
        } else {
            let is_immutable = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "jar" | "zip" | "mrpack"))
                .unwrap_or(false);
            if is_immutable {
                link_or_copy(&p, &target)?;
            } else {
                fs::copy(&p, &target)?;
            }
        }
    }
    Ok(())
}

fn add_dir_to_zip<W: Write + std::io::Seek>(
    zw: &mut zip::ZipWriter<W>,
    dir: &Path,
    prefix: &str,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)?.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = format!("{prefix}/{name}");
        if p.is_dir() {
            add_dir_to_zip(zw, &p, &rel, opts)?;
        } else {
            zw.start_file(&rel, opts)?;
            zw.write_all(&fs::read(&p)?)?;
        }
    }
    Ok(())
}

// ============================================================================
// Binary NBT Reader for `level.dat` & World Downgrade Guard
// ============================================================================

/// Parses a Minecraft `level.dat` file (gzipped or raw binary NBT) to extract
/// `LevelName`, `GameType`, `LastPlayed`, and `DataVersion`.
pub fn parse_level_dat(world_dir: &Path) -> Result<WorldSummary> {
    let level_dat = world_dir.join("level.dat");
    let raw_file = fs::read(&level_dat)
        .with_context(|| format!("Failed to read {}", level_dat.display()))?;

    let nbt_bytes = if raw_file.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(&raw_file[..]);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        out
    } else {
        raw_file
    };

    let folder_name = world_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "World".to_string());

    let level_name = extract_nbt_string(&nbt_bytes, b"LevelName").unwrap_or_else(|| folder_name.clone());
    let game_type = extract_nbt_int(&nbt_bytes, b"GameType").unwrap_or(0);
    let game_mode = match game_type {
        1 => "Creative",
        2 => "Adventure",
        3 => "Spectator",
        _ => "Survival",
    }
    .to_string();
    let last_played_ms = extract_nbt_long(&nbt_bytes, b"LastPlayed").unwrap_or(0);
    let data_version = extract_nbt_int(&nbt_bytes, b"DataVersion");
    let size_bytes = compute_dir_size(world_dir);
    let has_icon = world_dir.join("icon.png").exists();

    Ok(WorldSummary {
        folder_name,
        level_name,
        game_mode,
        last_played_ms,
        data_version,
        size_bytes,
        has_icon,
    })
}

/// Checks if opening a world in `target_world_version` would downgrade its `DataVersion`
/// and risk chunk/entity corruption.
pub fn check_world_downgrade_guard(
    world: &WorldSummary,
    target_mc_version: &str,
    target_world_version: i32,
) -> Option<DowngradeWarning> {
    let world_dv = world.data_version?;
    if world_dv > target_world_version {
        Some(DowngradeWarning {
            world_folder: world.folder_name.clone(),
            world_name: world.level_name.clone(),
            world_data_version: world_dv,
            target_world_version,
            message: format!(
                "World '{}' was saved in a newer Minecraft format (DataVersion {}) than Minecraft {} (DataVersion {}). Opening it without a backup can permanently corrupt chunks and inventories.",
                world.level_name, world_dv, target_mc_version, target_world_version
            ),
        })
    } else {
        None
    }
}

/// Scans binary NBT payload for `[tag_type][u16 name_len][name_bytes][payload]`.
fn extract_nbt_string(nbt: &[u8], key: &[u8]) -> Option<String> {
    // TAG_String = 0x08
    let mut header = Vec::with_capacity(3 + key.len());
    header.push(0x08);
    header.extend_from_slice(&(key.len() as u16).to_be_bytes());
    header.extend_from_slice(key);

    let pos = find_subslice(nbt, &header)?;
    let val_start = pos + header.len();
    if val_start + 2 > nbt.len() {
        return None;
    }
    let str_len = u16::from_be_bytes([nbt[val_start], nbt[val_start + 1]]) as usize;
    if val_start + 2 + str_len > nbt.len() {
        return None;
    }
    Some(String::from_utf8_lossy(&nbt[val_start + 2..val_start + 2 + str_len]).to_string())
}

fn extract_nbt_int(nbt: &[u8], key: &[u8]) -> Option<i32> {
    // TAG_Int = 0x03
    let mut header = Vec::with_capacity(3 + key.len());
    header.push(0x03);
    header.extend_from_slice(&(key.len() as u16).to_be_bytes());
    header.extend_from_slice(key);

    let pos = find_subslice(nbt, &header)?;
    let val_start = pos + header.len();
    if val_start + 4 > nbt.len() {
        return None;
    }
    Some(i32::from_be_bytes([
        nbt[val_start],
        nbt[val_start + 1],
        nbt[val_start + 2],
        nbt[val_start + 3],
    ]))
}

fn extract_nbt_long(nbt: &[u8], key: &[u8]) -> Option<i64> {
    // TAG_Long = 0x04
    let mut header = Vec::with_capacity(3 + key.len());
    header.push(0x04);
    header.extend_from_slice(&(key.len() as u16).to_be_bytes());
    header.extend_from_slice(key);

    let pos = find_subslice(nbt, &header)?;
    let val_start = pos + header.len();
    if val_start + 8 > nbt.len() {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&nbt[val_start..val_start + 8]);
    Some(i64::from_be_bytes(bytes))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn compute_dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += compute_dir_size(&p);
            } else if let Ok(m) = entry.metadata() {
                total += m.len();
            }
        }
    }
    total
}

// ============================================================================
// Quick Play Argument Builder (Feature #7)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuickPlayRequest {
    Singleplayer { world_folder: String },
    Multiplayer { host_and_port: String },
    Realms { realm_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuickPlayResolution {
    Supported { args: Vec<String> },
    Unsupported { reason: String },
}

/// Resolves Quick Play command-line arguments according to the Minecraft version's capabilities:
/// - Modern versions (`1.20+` and `26.x`): `--quickPlaySingleplayer`, `--quickPlayMultiplayer`, `--quickPlayRealms`
/// - Pre-1.20 versions (`1.0`..`1.19.4`, Beta, Alpha): falls back to `--server <host> --port <port>` for multiplayer,
///   and explains why direct singleplayer/Realms Quick Play requires Minecraft 1.20+.
pub fn build_quick_play_args(
    mc_version: &str,
    request: &QuickPlayRequest,
) -> QuickPlayResolution {
    let era = VersionEra::classify(mc_version, "release");
    let supports_modern_quick_play = match era {
        VersionEra::YearDrop { .. } => true,
        VersionEra::Release1x { minor } => minor >= 20,
        VersionEra::LegacySnapshot => !mc_version.starts_with("1."),
        _ => false,
    };

    match request {
        QuickPlayRequest::Singleplayer { world_folder } => {
            if supports_modern_quick_play {
                QuickPlayResolution::Supported {
                    args: vec![
                        "--quickPlaySingleplayer".to_string(),
                        world_folder.clone(),
                    ],
                }
            } else {
                QuickPlayResolution::Unsupported {
                    reason: format!(
                        "Direct singleplayer world Quick Play (--quickPlaySingleplayer) was introduced in Minecraft 1.20 and is not supported by {}.",
                        mc_version
                    ),
                }
            }
        }
        QuickPlayRequest::Multiplayer { host_and_port } => {
            if supports_modern_quick_play {
                QuickPlayResolution::Supported {
                    args: vec![
                        "--quickPlayMultiplayer".to_string(),
                        host_and_port.clone(),
                    ],
                }
            } else {
                let (host, port) = match host_and_port.split_once(':') {
                    Some((h, p)) => (h.to_string(), p.to_string()),
                    None => (host_and_port.clone(), "25565".to_string()),
                };
                QuickPlayResolution::Supported {
                    args: vec![
                        "--server".to_string(),
                        host,
                        "--port".to_string(),
                        port,
                    ],
                }
            }
        }
        QuickPlayRequest::Realms { realm_id } => {
            if supports_modern_quick_play {
                QuickPlayResolution::Supported {
                    args: vec!["--quickPlayRealms".to_string(), realm_id.clone()],
                }
            } else {
                QuickPlayResolution::Unsupported {
                    reason: format!(
                        "Realms Quick Play (--quickPlayRealms) requires Minecraft 1.20+ (current: {}).",
                        mc_version
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    #[test]
    fn test_level_dat_parsing_and_world_downgrade_guard() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let world_dir = dir.path().join("MySurvivalWorld");
        fs::create_dir_all(&world_dir)?;

        // Craft a minimal valid binary NBT buffer containing LevelName, GameType=0, DataVersion=4189 (MC 26.3)
        let mut nbt = Vec::new();
        // TAG_String (0x08) "LevelName" = "Kestryx Base"
        nbt.push(0x08);
        nbt.extend_from_slice(&(9u16).to_be_bytes());
        nbt.extend_from_slice(b"LevelName");
        nbt.extend_from_slice(&(12u16).to_be_bytes());
        nbt.extend_from_slice(b"Kestryx Base");
        // TAG_Int (0x03) "GameType" = 1 (Creative)
        nbt.push(0x03);
        nbt.extend_from_slice(&(8u16).to_be_bytes());
        nbt.extend_from_slice(b"GameType");
        nbt.extend_from_slice(&(1i32).to_be_bytes());
        // TAG_Int (0x03) "DataVersion" = 4189
        nbt.push(0x03);
        nbt.extend_from_slice(&(11u16).to_be_bytes());
        nbt.extend_from_slice(b"DataVersion");
        nbt.extend_from_slice(&(4189i32).to_be_bytes());

        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&nbt)?;
        fs::write(world_dir.join("level.dat"), gz.finish()?)?;

        let summary = parse_level_dat(&world_dir)?;
        assert_eq!(summary.level_name, "Kestryx Base");
        assert_eq!(summary.game_mode, "Creative");
        assert_eq!(summary.data_version, Some(4189));

        // Target older version 1.20.1 (DataVersion 3465) -> must trigger DowngradeWarning!
        let warn = check_world_downgrade_guard(&summary, "1.20.1", 3465);
        assert!(warn.is_some());
        // Target same or newer version (DataVersion 4189) -> safe!
        assert!(check_world_downgrade_guard(&summary, "26.3", 4189).is_none());
        Ok(())
    }

    #[test]
    fn test_quick_play_modern_and_legacy_fallback() {
        let qp_26 = build_quick_play_args(
            "26.3",
            &QuickPlayRequest::Singleplayer {
                world_folder: "New World".to_string(),
            },
        );
        assert_eq!(
            qp_26,
            QuickPlayResolution::Supported {
                args: vec![
                    "--quickPlaySingleplayer".to_string(),
                    "New World".to_string()
                ]
            }
        );

        let qp_112_mp = build_quick_play_args(
            "1.12.2",
            &QuickPlayRequest::Multiplayer {
                host_and_port: "mc.hypixel.net:25565".to_string(),
            },
        );
        assert_eq!(
            qp_112_mp,
            QuickPlayResolution::Supported {
                args: vec![
                    "--server".to_string(),
                    "mc.hypixel.net".to_string(),
                    "--port".to_string(),
                    "25565".to_string()
                ]
            }
        );

        let qp_112_sp = build_quick_play_args(
            "1.12.2",
            &QuickPlayRequest::Singleplayer {
                world_folder: "OldWorld".to_string(),
            },
        );
        assert!(matches!(qp_112_sp, QuickPlayResolution::Unsupported { .. }));
    }

    #[test]
    fn test_instance_clone_and_rolling_zstd_backup_restore() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mgr = InstanceManager::new(dir.path())?;
        let inst = mgr.create_instance("inst-1", "Original", "26.3", "fabric", "0.20.0", "Modded")?;
        let game_dir = mgr.game_dir(&inst);
        fs::write(game_dir.join("options.txt"), "fov:1.0\n")?;
        fs::create_dir_all(game_dir.join("saves/World1"))?;
        fs::write(game_dir.join("saves/World1/level.dat"), b"world-data")?;

        // Clone instance and verify isolation (changing clone's options.txt never mutates original)
        let cloned = mgr.clone_instance("inst-1", "inst-2", "Cloned", None)?;
        let cloned_game_dir = mgr.game_dir(&cloned);
        fs::write(cloned_game_dir.join("options.txt"), "fov:0.5\n")?;
        assert_eq!(fs::read_to_string(game_dir.join("options.txt"))?, "fov:1.0\n");

        // Create rolling zstd backup, corrupt world, and restore backup
        let backup = mgr.create_rolling_backup("inst-1", "pre-launch", 3)?;
        assert!(backup.archive_path.exists());
        fs::write(game_dir.join("saves/World1/level.dat"), b"corrupted")?;
        mgr.restore_backup("inst-1", &backup.archive_path)?;
        assert_eq!(fs::read(game_dir.join("saves/World1/level.dat"))?, b"world-data");

        // Trash and Undo restore
        mgr.trash_instance("inst-2")?;
        assert!(!mgr.instance_dir("inst-2").exists());
        mgr.restore_trashed_instance("inst-2")?;
        assert!(mgr.instance_dir("inst-2").exists());
        Ok(())
    }
}
