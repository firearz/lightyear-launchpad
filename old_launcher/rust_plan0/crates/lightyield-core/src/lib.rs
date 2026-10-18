//! Light Yield Launcher Core Library
//! High-performance Minecraft Java Edition engine: local auth, manifest caching,
//! installation, universal Java detection/auto-provisioning, and process supervision.

pub mod account;
pub mod manifest;
pub mod installer;
pub mod java;
pub mod launcher;
pub mod storage;

pub use account::{LocalAccount, offline_uuid_v3, sanitize_username};
pub use manifest::{VersionManifest, VersionEntry, VersionFilter, fetch_or_load_manifest};
pub use installer::{
    install_version, download_batch_parallel, download_file,
    InstalledVersion, VersionPackage, BatchDownloadItem,
};
pub use java::{
    ensure_compatible_java_runtime, resolve_java_runtime,
    discover_all_java_runtimes, get_required_java_version, JavaRuntime,
};
pub use launcher::{build_launch_command, launch_and_supervise, LaunchConfig, LaunchCommand};
pub use storage::{
    get_launcher_dir, get_versions_cache_file, get_versions_dir,
    LauncherSettings,
};
