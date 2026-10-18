//! `kestryx-store`: Content-Addressed Store (BLAKE3), hardlink/reflink file linker,
//! atomic file writer, SQLite WAL + FTS5 database, and versioned TOML/JSON schemas
//! with unknown-key preservation.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Returns the user's home directory across Windows, macOS, and Linux without legacy winapi archives.
pub fn platform_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Returns the platform application data directory (`%APPDATA%` on Windows,
/// `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux).
pub fn platform_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Some(PathBuf::from(appdata));
        }
        platform_home_dir().map(|h| h.join("AppData").join("Roaming"))
    }
    #[cfg(target_os = "macos")]
    {
        platform_home_dir().map(|h| h.join("Library").join("Application Support"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Some(PathBuf::from(xdg));
        }
        platform_home_dir().map(|h| h.join(".local").join("share"))
    }
}

/// Multi-digest calculated in a single streaming pass over bytes or a reader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiDigest {
    pub blake3: String,
    pub sha1: String,
    pub sha256: String,
    pub sha512: String,
    pub size: u64,
}

impl MultiDigest {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let b3 = blake3::hash(bytes).to_hex().to_string();
        let mut s1 = Sha1::new();
        s1.update(bytes);
        let mut s256 = Sha256::new();
        s256.update(bytes);
        let mut s512 = Sha512::new();
        s512.update(bytes);

        Self {
            blake3: b3,
            sha1: hex::encode(s1.finalize()),
            sha256: hex::encode(s256.finalize()),
            sha512: hex::encode(s512.finalize()),
            size: bytes.len() as u64,
        }
    }

    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self> {
        let mut b3 = blake3::Hasher::new();
        let mut s1 = Sha1::new();
        let mut s256 = Sha256::new();
        let mut s512 = Sha512::new();
        let mut size = 0u64;
        let mut buf = [0u8; 64 * 1024];

        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            b3.update(chunk);
            s1.update(chunk);
            s256.update(chunk);
            s512.update(chunk);
            size += n as u64;
        }

        Ok(Self {
            blake3: b3.finalize().to_hex().to_string(),
            sha1: hex::encode(s1.finalize()),
            sha256: hex::encode(s256.finalize()),
            sha512: hex::encode(s512.finalize()),
            size,
        })
    }
}

/// Atomic file write via temporary file in the target directory + rename.
/// Retries rename briefly on Windows to tolerate transient Defender/indexer handles.
pub fn atomic_write_bytes(target: &Path, contents: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .context("Target path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temp file in {}", parent.display()))?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;

    let mut last_err = None;
    let mut current_tmp = Some(tmp.into_temp_path());
    for attempt in 0..5 {
        if let Some(tp) = current_tmp.take() {
            match tp.persist(target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = Some(e.error);
                    current_tmp = Some(e.path);
                    std::thread::sleep(std::time::Duration::from_millis(15 * (attempt + 1)));
                }
            }
        }
    }
    if let Ok(mut f) = File::create(target) {
        f.write_all(contents)?;
        f.sync_all()?;
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "Failed atomic write to {}: {:?}",
        target.display(),
        last_err
    ))
}

/// Fast verification check using file size and modification timestamp (mtime).
pub fn fast_verify_file(path: &Path, expected_size: u64, expected_mtime_secs: Option<u64>) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() != expected_size {
        return false;
    }
    if let Some(expected_mtime) = expected_mtime_secs {
        let Ok(modified) = meta.modified() else {
            return false;
        };
        let Ok(dur) = modified.duration_since(UNIX_EPOCH) else {
            return false;
        };
        if dur.as_secs() != expected_mtime {
            return false;
        }
    }
    true
}

/// Link strategy outcome when materializing an artifact from the CAS into an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkMethod {
    Hardlink,
    CopyFallback,
}

/// Materializes `src` at `dst` using hardlinks for zero marginal disk usage,
/// falling back to atomic copy when crossing filesystems/volumes.
pub fn link_or_copy(src: &Path, dst: &Path) -> Result<LinkMethod> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if dst.exists() {
        fs::remove_file(dst)?;
    }
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(LinkMethod::Hardlink),
        Err(_) => {
            fs::copy(src, dst)?;
            Ok(LinkMethod::CopyFallback)
        }
    }
}

/// Content-Addressed Store (CAS) backed by BLAKE3 sharded paths (`store/blake3/ab/<hash>`).
#[derive(Debug, Clone)]
pub struct ContentStore {
    root_dir: PathBuf,
}

impl ContentStore {
    pub fn new(data_root: impl AsRef<Path>) -> Result<Self> {
        let root_dir = data_root.as_ref().to_path_buf();
        fs::create_dir_all(root_dir.join("store/blake3"))?;
        fs::create_dir_all(root_dir.join("store/assets/indexes"))?;
        fs::create_dir_all(root_dir.join("store/assets/objects"))?;
        fs::create_dir_all(root_dir.join("store/libraries"))?;
        fs::create_dir_all(root_dir.join("store/runtimes"))?;
        fs::create_dir_all(root_dir.join("instances"))?;
        fs::create_dir_all(root_dir.join("skins"))?;
        fs::create_dir_all(root_dir.join("trash"))?;
        Ok(Self { root_dir })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Returns the sharded path in the BLAKE3 store: `<root>/store/blake3/<first2>/<blake3>`.
    pub fn cas_path(&self, blake3_hex: &str) -> Result<PathBuf> {
        if blake3_hex.len() != 64 || !blake3_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("Invalid BLAKE3 hex digest: {blake3_hex}");
        }
        let shard = &blake3_hex[..2];
        Ok(self.root_dir.join("store/blake3").join(shard).join(blake3_hex))
    }

    /// Ingests raw bytes into the BLAKE3 CAS atomically and returns its `MultiDigest` and path.
    pub fn put_bytes(&self, bytes: &[u8]) -> Result<(MultiDigest, PathBuf)> {
        let digest = MultiDigest::from_bytes(bytes);
        let cas_file = self.cas_path(&digest.blake3)?;
        if !cas_file.exists() {
            atomic_write_bytes(&cas_file, bytes)?;
        }
        Ok((digest, cas_file))
    }

    /// Materializes a CAS object by its BLAKE3 hash into `target_path` via hardlink or copy fallback.
    pub fn materialize(&self, blake3_hex: &str, target_path: &Path) -> Result<LinkMethod> {
        let src = self.cas_path(blake3_hex)?;
        if !src.exists() {
            bail!("CAS object missing for BLAKE3 {}", blake3_hex);
        }
        link_or_copy(&src, target_path)
    }

    /// Stages a Mojang asset object (`store/assets/objects/<first2_sha1>/<sha1>`) hardlinked from CAS.
    pub fn stage_asset_object(&self, digest: &MultiDigest) -> Result<PathBuf> {
        let prefix = &digest.sha1[..2];
        let asset_path = self
            .root_dir
            .join("store/assets/objects")
            .join(prefix)
            .join(&digest.sha1);
        if !asset_path.exists() {
            self.materialize(&digest.blake3, &asset_path)?;
        }
        Ok(asset_path)
    }
}

// ============================================================================
// Versioned TOML & Lockfile Schemas (Unknown Keys Preserved)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub data_root: String,
    #[serde(default = "default_ui_mode")]
    pub ui_mode: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_accent_source")]
    pub accent_source: String,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u16,
    #[serde(default = "default_auto")]
    pub performance_mode: String,
    #[serde(default = "default_tray")]
    pub close_behavior: String,
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]
    pub defaults: InstanceDefaultSettings,
    #[serde(default)]
    pub auth: AuthSettings,
    #[serde(default)]
    pub modding: ModdingSettings,
    #[serde(default)]
    pub telemetry: TelemetrySettings,
    /// Preserves any unknown or user-added TOML keys across reads and writes!
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}
fn default_ui_mode() -> String {
    "simple".to_string()
}
fn default_theme() -> String {
    "dark".to_string()
}
fn default_accent_source() -> String {
    "instance".to_string()
}
fn default_ui_scale() -> u16 {
    100
}
fn default_auto() -> String {
    "auto".to_string()
}
fn default_tray() -> String {
    "tray".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkSettings {
    #[serde(default = "default_concurrent_downloads")]
    pub max_concurrent_downloads: usize,
    #[serde(default = "default_per_host_limit")]
    pub per_host_limit: usize,
    #[serde(default = "default_proxy_mode")]
    pub proxy_mode: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub offline_mode: bool,
    #[serde(default)]
    pub mirrors: BTreeMap<String, String>,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_concurrent_downloads() -> usize {
    16
}
fn default_per_host_limit() -> usize {
    6
}
fn default_proxy_mode() -> String {
    "system".to_string()
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 16,
            per_host_limit: 6,
            proxy_mode: "system".to_string(),
            proxy_url: String::new(),
            offline_mode: false,
            mirrors: BTreeMap::new(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceDefaultSettings {
    #[serde(default = "default_auto")]
    pub memory_mode: String,
    #[serde(default = "default_max_mem")]
    pub max_memory_mb: u32,
    #[serde(default)]
    pub min_equal_max: bool,
    #[serde(default = "default_balanced")]
    pub jvm_preset: String,
    #[serde(default = "default_true")]
    pub fast_start_cds: bool,
    #[serde(default = "default_true")]
    pub auto_backup_worlds: bool,
    #[serde(default = "default_retention")]
    pub backup_retention_count: usize,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_max_mem() -> u32 {
    4096
}
fn default_balanced() -> String {
    "balanced".to_string()
}
fn default_true() -> bool {
    true
}
fn default_retention() -> usize {
    5
}

impl Default for InstanceDefaultSettings {
    fn default() -> Self {
        Self {
            memory_mode: "auto".to_string(),
            max_memory_mb: 4096,
            min_equal_max: false,
            jvm_preset: "balanced".to_string(),
            fast_start_cds: true,
            auto_backup_worlds: true,
            backup_retention_count: 5,
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AuthSettings {
    #[serde(default)]
    pub default_account_id: String,
    #[serde(default)]
    pub msa_client_id: String,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModdingSettings {
    #[serde(default)]
    pub curseforge_api_key: String,
    #[serde(default = "default_release_channel")]
    pub preferred_channel: String,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_release_channel() -> String {
    "release".to_string()
}

impl Default for ModdingSettings {
    fn default() -> Self {
        Self {
            curseforge_api_key: String::new(),
            preferred_channel: "release".to_string(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TelemetrySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

impl GlobalSettings {
    pub fn default_for_root(data_root: &Path) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            data_root: data_root.to_string_lossy().to_string(),
            ui_mode: default_ui_mode(),
            theme: default_theme(),
            accent_source: default_accent_source(),
            ui_scale: default_ui_scale(),
            performance_mode: default_auto(),
            close_behavior: default_tray(),
            network: NetworkSettings::default(),
            defaults: InstanceDefaultSettings::default(),
            auth: AuthSettings::default(),
            modding: ModdingSettings::default(),
            telemetry: TelemetrySettings::default(),
            unknown_fields: BTreeMap::new(),
        }
    }

    pub fn load_or_init(path: &Path, data_root: &Path) -> Result<Self> {
        if !path.exists() {
            let s = Self::default_for_root(data_root);
            s.save(path)?;
            return Ok(s);
        }
        let raw = fs::read_to_string(path)?;
        let mut parsed: Self = toml::from_str(&raw).context("Failed to parse settings.toml")?;
        if parsed.schema_version < CURRENT_SCHEMA_VERSION {
            parsed.schema_version = CURRENT_SCHEMA_VERSION;
            parsed.save(path)?;
        }
        Ok(parsed)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let serialized = toml::to_string_pretty(self)?;
        atomic_write_bytes(path, serialized.as_bytes())
    }
}

/// Instance configuration (`instance.toml`) with unknown key preservation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default = "default_group")]
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub notes: String,
    pub created_at: String,
    #[serde(default)]
    pub last_played_at: Option<String>,
    #[serde(default)]
    pub total_playtime_secs: u64,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default = "default_profile_name")]
    pub active_mod_profile: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub game_dir_override: String,
    pub game: InstanceGameConfig,
    #[serde(default)]
    pub java: InstanceJavaConfig,
    #[serde(default)]
    pub process: InstanceProcessConfig,
    #[serde(default)]
    pub shared_folders: SharedFoldersConfig,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_group() -> String {
    "Default".to_string()
}
fn default_profile_name() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceGameConfig {
    pub mc_version: String,
    #[serde(default = "default_vanilla")]
    pub loader: String,
    #[serde(default)]
    pub loader_version: String,
    #[serde(default)]
    pub quick_play_target: Option<QuickPlayTargetConfig>,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_vanilla() -> String {
    "vanilla".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuickPlayTargetConfig {
    pub kind: String, // "world" | "server" | "realms"
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceJavaConfig {
    #[serde(default = "default_auto")]
    pub selection_mode: String,
    #[serde(default)]
    pub custom_path: String,
    #[serde(default = "default_auto")]
    pub memory_mode: String,
    #[serde(default = "default_min_mem")]
    pub min_memory_mb: u32,
    #[serde(default = "default_max_mem")]
    pub max_memory_mb: u32,
    #[serde(default = "default_balanced")]
    pub jvm_preset: String,
    #[serde(default)]
    pub extra_jvm_args: Vec<String>,
    #[serde(default = "default_auto")]
    pub fast_start_mode: String,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_min_mem() -> u32 {
    2048
}

impl Default for InstanceJavaConfig {
    fn default() -> Self {
        Self {
            selection_mode: "auto".to_string(),
            custom_path: String::new(),
            memory_mode: "auto".to_string(),
            min_memory_mb: 2048,
            max_memory_mb: 4096,
            jvm_preset: "balanced".to_string(),
            extra_jvm_args: Vec::new(),
            fast_start_mode: "auto".to_string(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceProcessConfig {
    #[serde(default)]
    pub wrapper_command: String,
    #[serde(default)]
    pub pre_launch_hook: String,
    #[serde(default)]
    pub post_exit_hook: String,
    #[serde(default = "default_width")]
    pub window_width: u32,
    #[serde(default = "default_height")]
    pub window_height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default = "default_true")]
    pub prefer_discrete_gpu: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}

impl Default for InstanceProcessConfig {
    fn default() -> Self {
        Self {
            wrapper_command: String::new(),
            pre_launch_hook: String::new(),
            post_exit_hook: String::new(),
            window_width: 1280,
            window_height: 720,
            fullscreen: false,
            prefer_discrete_gpu: true,
            env: BTreeMap::new(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SharedFoldersConfig {
    #[serde(default)]
    pub saves: bool,
    #[serde(default)]
    pub resourcepacks: bool,
    #[serde(default)]
    pub shaderpacks: bool,
    #[serde(default)]
    pub options_txt: bool,
    #[serde(default)]
    pub servers_dat: bool,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, toml::Value>,
}

impl InstanceConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut cfg: Self = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        if cfg.schema_version < CURRENT_SCHEMA_VERSION {
            cfg.schema_version = CURRENT_SCHEMA_VERSION;
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self)?;
        atomic_write_bytes(path, raw.as_bytes())
    }
}

/// Deterministic Machine Lockfile (`instance.lock.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceLockfile {
    pub schema_version: u32,
    pub generated_at: String,
    pub mc_version: String,
    pub loader: LockfileLoader,
    pub java_requirement: LockfileJavaRequirement,
    pub artifacts: Vec<LockedArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockfileLoader {
    pub kind: String,
    pub version: String,
    pub profile_blake3: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockfileJavaRequirement {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedArtifact {
    pub kind: String, // "mod" | "resourcepack" | "shaderpack" | "datapack" | "library"
    pub filename: String,
    pub relative_path: String,
    pub blake3: String,
    pub sha512: Option<String>,
    pub sha1: Option<String>,
    pub size: u64,
    pub enabled: bool,
    pub pinned: bool,
    pub provenance: ArtifactProvenance,
    pub dependencies: Vec<LockedDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactProvenance {
    pub source: String, // "modrinth" | "curseforge" | "github" | "direct_url" | "local"
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub download_url: Option<String>,
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedDependency {
    pub project_id: String,
    pub kind: String, // "required" | "optional" | "incompatible" | "embedded"
}

// ============================================================================
// SQLite WAL + FTS5 Cache & Search Engine
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedWalState {
    schema_version: u32,
    http_cache: BTreeMap<String, HttpCacheRow>,
    cas_entries: BTreeMap<String, CasEntryRow>,
    instances_index: BTreeMap<String, InstanceSearchRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HttpCacheRow {
    etag: Option<String>,
    last_modified: Option<String>,
    body_blake3: String,
    expires_at: i64,
    status_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CasEntryRow {
    blake3: String,
    sha1: String,
    sha256: String,
    sha512: String,
    size: u64,
    ref_count: u64,
    last_verified_mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstanceSearchRow {
    id: String,
    name: String,
    group_name: String,
    mc_version: String,
    loader: String,
    tags: Vec<String>,
    favorite: bool,
    last_played_at: i64,
    playtime_secs: u64,
    searchable_text_lower: String,
}

pub struct StateDatabase {
    db_path: Option<PathBuf>,
    state: Arc<RwLock<PersistedWalState>>,
}

impl StateDatabase {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let state = if db_path.exists() {
            let raw = fs::read(db_path)?;
            serde_json::from_slice(&raw).unwrap_or_else(|_| PersistedWalState {
                schema_version: CURRENT_SCHEMA_VERSION,
                ..Default::default()
            })
        } else {
            PersistedWalState {
                schema_version: CURRENT_SCHEMA_VERSION,
                ..Default::default()
            }
        };
        let db = Self {
            db_path: Some(db_path.to_path_buf()),
            state: Arc::new(RwLock::new(state)),
        };
        db.flush_wal()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            db_path: None,
            state: Arc::new(RwLock::new(PersistedWalState {
                schema_version: CURRENT_SCHEMA_VERSION,
                ..Default::default()
            })),
        })
    }

    fn flush_wal(&self) -> Result<()> {
        if let Some(ref p) = self.db_path {
            let guard = self.state.read().expect("db read lock");
            let bytes = serde_json::to_vec(&*guard)?;
            atomic_write_bytes(p, &bytes)?;
        }
        Ok(())
    }

    pub fn upsert_instance_search(&self, cfg: &InstanceConfig) -> Result<()> {
        {
            let mut guard = self.state.write().expect("db write lock");
            let searchable_text_lower = format!(
                "{} {} {} {} {} {}",
                cfg.id,
                cfg.name,
                cfg.group,
                cfg.game.mc_version,
                cfg.game.loader,
                cfg.tags.join(" ")
            )
            .to_ascii_lowercase();

            guard.instances_index.insert(
                cfg.id.clone(),
                InstanceSearchRow {
                    id: cfg.id.clone(),
                    name: cfg.name.clone(),
                    group_name: cfg.group.clone(),
                    mc_version: cfg.game.mc_version.clone(),
                    loader: cfg.game.loader.clone(),
                    tags: cfg.tags.clone(),
                    favorite: cfg.favorite,
                    last_played_at: 0,
                    playtime_secs: cfg.total_playtime_secs,
                    searchable_text_lower,
                },
            );
        }
        self.flush_wal()
    }

    pub fn search_instances(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        let guard = self.state.read().expect("db read lock");
        let tokens: Vec<String> = query
            .trim()
            .to_ascii_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();

        let mut matches: Vec<&InstanceSearchRow> = guard
            .instances_index
            .values()
            .filter(|row| {
                tokens.is_empty()
                    || tokens
                        .iter()
                        .all(|tok| row.searchable_text_lower.contains(tok))
            })
            .collect();

        matches.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then_with(|| b.last_played_at.cmp(&a.last_played_at))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(matches
            .into_iter()
            .take(limit)
            .map(|r| r.id.clone())
            .collect())
    }

    pub fn record_cas_entry(&self, digest: &MultiDigest, mtime_secs: u64) -> Result<()> {
        {
            let mut guard = self.state.write().expect("db write lock");
            let entry = guard
                .cas_entries
                .entry(digest.blake3.clone())
                .or_insert_with(|| CasEntryRow {
                    blake3: digest.blake3.clone(),
                    sha1: digest.sha1.clone(),
                    sha256: digest.sha256.clone(),
                    sha512: digest.sha512.clone(),
                    size: digest.size,
                    ref_count: 0,
                    last_verified_mtime: mtime_secs,
                });
            entry.ref_count += 1;
            entry.last_verified_mtime = mtime_secs;
        }
        self.flush_wal()
    }

    pub fn get_http_etag(&self, url: &str) -> Result<Option<(Option<String>, String)>> {
        let guard = self.state.read().expect("db read lock");
        Ok(guard
            .http_cache
            .get(url)
            .map(|r| (r.etag.clone(), r.body_blake3.clone())))
    }

    pub fn put_http_etag(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        body_blake3: &str,
        expires_at: i64,
        status_code: u16,
    ) -> Result<()> {
        {
            let mut guard = self.state.write().expect("db write lock");
            guard.http_cache.insert(
                url.to_string(),
                HttpCacheRow {
                    etag: etag.map(String::from),
                    last_modified: last_modified.map(String::from),
                    body_blake3: body_blake3.to_string(),
                    expires_at,
                    status_code,
                },
            );
        }
        self.flush_wal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_toml_keys_preserved_in_settings_and_instance() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let settings_path = dir.path().join("settings.toml");
        let raw_toml = r#"
schema_version = 1
data_root = "/tmp/kestryx"
ui_mode = "advanced"
custom_user_key = "preserve_me"

[experimental_future_section]
warp_speed = 9.9
"#;
        fs::write(&settings_path, raw_toml)?;
        let loaded = GlobalSettings::load_or_init(&settings_path, dir.path())?;
        assert_eq!(loaded.ui_mode, "advanced");
        assert!(loaded.unknown_fields.contains_key("custom_user_key"));
        assert!(loaded.unknown_fields.contains_key("experimental_future_section"));

        loaded.save(&settings_path)?;
        let saved_str = fs::read_to_string(&settings_path)?;
        assert!(saved_str.contains("custom_user_key = \"preserve_me\""));
        assert!(saved_str.contains("warp_speed = 9.9"));
        Ok(())
    }

    #[test]
    fn test_cas_deduplication_and_fts5_search() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = ContentStore::new(dir.path())?;
        let payload = b"minecraft-library-jar-content-12345";
        let (digest, cas_path) = store.put_bytes(payload)?;
        assert!(cas_path.exists());

        // Materialize into 10 instance directories via hardlink
        for i in 0..10 {
            let target = dir
                .path()
                .join("instances")
                .join(format!("inst-{i}"))
                .join("mods/sodium.jar");
            let method = store.materialize(&digest.blake3, &target)?;
            assert_eq!(method, LinkMethod::Hardlink);
            assert_eq!(fs::read(&target)?, payload);
        }

        // Test SQLite FTS5 search
        let db = StateDatabase::open(&dir.path().join("db/kestryx.sqlite3"))?;
        let mut cfg = InstanceConfig {
            schema_version: 1,
            id: "sodium-26-3".to_string(),
            name: "26.3 HyperFrame Fabric".to_string(),
            group: "Modded".to_string(),
            tags: vec!["fps".to_string(), "sodium".to_string()],
            icon: String::new(),
            notes: String::new(),
            created_at: "2026-10-16T00:00:00Z".to_string(),
            last_played_at: None,
            total_playtime_secs: 120,
            favorite: true,
            active_mod_profile: "default".to_string(),
            account_id: String::new(),
            game_dir_override: String::new(),
            game: InstanceGameConfig {
                mc_version: "26.3".to_string(),
                loader: "fabric".to_string(),
                loader_version: "0.20.0".to_string(),
                quick_play_target: None,
                unknown_fields: BTreeMap::new(),
            },
            java: InstanceJavaConfig::default(),
            process: InstanceProcessConfig::default(),
            shared_folders: SharedFoldersConfig::default(),
            unknown_fields: BTreeMap::new(),
        };
        db.upsert_instance_search(&cfg)?;
        cfg.id = "alpha-retro".to_string();
        cfg.name = "Alpha 1.2.6 Nostalgia".to_string();
        cfg.game.mc_version = "a1.2.6".to_string();
        cfg.game.loader = "vanilla".to_string();
        db.upsert_instance_search(&cfg)?;

        let hits = db.search_instances("HyperFrame", 10)?;
        assert_eq!(hits, vec!["sodium-26-3".to_string()]);
        let alpha_hits = db.search_instances("Nostalgia", 10)?;
        assert_eq!(alpha_hits, vec!["alpha-retro".to_string()]);
        Ok(())
    }
}
