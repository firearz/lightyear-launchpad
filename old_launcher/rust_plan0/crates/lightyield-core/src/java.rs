//! Universal Java runtime detection, era compatibility guard, and automatic
//! Mojang official portable JRE provisioning for any PC.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::installer::{download_batch_parallel, download_file, verify_file_sha1, BatchDownloadItem};
use crate::storage::get_runtimes_dir;

pub const MOJANG_JAVA_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Clone)]
pub struct JavaRuntime {
    pub path: PathBuf,
    pub major_version: u32,
    pub version_string: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MojangRuntimeComponentEntry {
    manifest: MojangRuntimeManifestRef,
    version: MojangRuntimeVersionInfo,
}

#[derive(Debug, Clone, Deserialize)]
struct MojangRuntimeManifestRef {
    url: String,
    sha1: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MojangRuntimeVersionInfo {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct MojangRuntimeFilesManifest {
    files: HashMap<String, MojangRuntimeFileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct MojangRuntimeFileEntry {
    #[serde(rename = "type")]
    entry_type: String,
    downloads: Option<MojangRuntimeFileDownloads>,
    #[serde(default)]
    #[allow(dead_code)]
    executable: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct MojangRuntimeFileDownloads {
    raw: Option<MojangRuntimeRawDownload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MojangRuntimeRawDownload {
    url: String,
    sha1: String,
    size: u64,
}

/// Resolves or automatically provisions a compatible Java runtime for the target Minecraft version.
/// Guarantees that any PC capable of running Minecraft Java has the exact required Java version.
pub async fn ensure_compatible_java_runtime(
    override_path: Option<&Path>,
    target_mc_version: &str,
    required_major_hint: Option<u32>,
    required_component_hint: Option<&str>,
    offline_mode: bool,
) -> Result<JavaRuntime> {
    if let Some(p) = override_path {
        if !p.exists() {
            bail!("Custom Java path does not exist: {}", p.display());
        }
        return inspect_java_binary(p);
    }

    let required_major = required_major_hint.unwrap_or_else(|| get_required_java_version(target_mc_version));
    let component = required_component_hint
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_mojang_component_for_major(required_major));

    // 1. Scan all local Java installations on the PC (including our managed runtimes dir)
    let runtimes = discover_all_java_runtimes();

    // Strict matching rule:
    // - Legacy Minecraft requiring Java 8 (`required_major == 8`) MUST run on Java 8 (`major_version == 8`)
    //   because `launchwrapper` and old Forge/Vanilla cast AppClassLoader to URLClassLoader, which fails on Java 9+.
    // - Modern Minecraft (`required_major >= 16`) prefers an exact major match first, or any `major_version >= required_major`.
    if required_major == 8 {
        if let Some(java8) = runtimes.iter().find(|r| r.major_version == 8) {
            return Ok(java8.clone());
        }
    } else {
        if let Some(exact) = runtimes.iter().find(|r| r.major_version == required_major) {
            return Ok(exact.clone());
        }
        if let Some(compatible) = runtimes.iter().find(|r| r.major_version >= required_major) {
            return Ok(compatible.clone());
        }
    }

    // 2. No compatible Java found locally on this PC -> Auto-provision official Mojang portable JRE!
    if !offline_mode {
        println!(
            "[JAVA] No local Java {} runtime found for Minecraft '{}'. Auto-downloading official Mojang '{}' runtime...",
            required_major, target_mc_version, component
        );
        if let Ok(provisioned) = download_mojang_java_runtime(&component, required_major).await {
            return Ok(provisioned);
        }
    }

    // 3. Last resort fallback if offline and any Java exists
    if let Some(any) = runtimes.first() {
        eprintln!(
            "[WARN] Using fallback Java {} ({}) for Minecraft '{}' (recommended: Java {}).",
            any.major_version,
            any.path.display(),
            target_mc_version,
            required_major
        );
        return Ok(any.clone());
    }

    bail!(
        "No compatible Java {} runtime found on this PC and automatic download failed. Please connect to the internet or pass --java <path>.",
        required_major
    )
}

/// Synchronously resolves best currently-installed Java runtime.
pub fn resolve_java_runtime(override_path: Option<&Path>, target_mc_version: &str) -> Result<JavaRuntime> {
    if let Some(p) = override_path {
        if !p.exists() {
            bail!("Custom Java path does not exist: {}", p.display());
        }
        return inspect_java_binary(p);
    }

    let runtimes = discover_all_java_runtimes();
    if runtimes.is_empty() {
        bail!("No local Java runtime detected yet. Launch a version while online to auto-install Mojang Java.");
    }

    let required_major = get_required_java_version(target_mc_version);
    if required_major == 8 {
        if let Some(java8) = runtimes.iter().find(|r| r.major_version == 8) {
            return Ok(java8.clone());
        }
    }
    if let Some(exact) = runtimes.iter().find(|r| r.major_version == required_major) {
        return Ok(exact.clone());
    }
    if let Some(best) = runtimes.iter().find(|r| r.major_version >= required_major) {
        return Ok(best.clone());
    }

    Ok(runtimes[0].clone())
}

/// Automatically downloads and verifies the official Mojang portable JRE for the current OS & architecture.
pub async fn download_mojang_java_runtime(component: &str, expected_major: u32) -> Result<JavaRuntime> {
    let platform_key = get_mojang_platform_key();
    let runtime_root = get_runtimes_dir().join(component).join(platform_key);
    let java_bin_rel = if cfg!(windows) {
        "bin/java.exe"
    } else if cfg!(target_os = "macos") {
        "jre.bundle/Contents/Home/bin/java"
    } else {
        "bin/java"
    };
    let java_exe_path = runtime_root.join(java_bin_rel);

    if java_exe_path.exists() {
        if let Ok(rt) = inspect_java_binary(&java_exe_path) {
            return Ok(rt);
        }
    }

    fs::create_dir_all(&runtime_root)?;
    let all_json_path = get_runtimes_dir().join("mojang_java_all.json");
    download_file(MOJANG_JAVA_MANIFEST_URL, &all_json_path, None).await?;

    let raw_all = fs::read_to_string(&all_json_path)?;
    let parsed_all: serde_json::Value = serde_json::from_str(&raw_all)?;

    let comp_arr = parsed_all
        .get(platform_key)
        .and_then(|plat| plat.get(component))
        .and_then(|c| c.as_array())
        .with_context(|| format!("Mojang runtime '{component}' not available for platform '{platform_key}'"))?;

    let first_entry: MojangRuntimeComponentEntry = serde_json::from_value(
        comp_arr
            .first()
            .cloned()
            .with_context(|| format!("Empty runtime array for '{component}' on '{platform_key}'"))?,
    )?;

    println!(
        "       Resolved official Mojang JRE: {} ({})",
        component, first_entry.version.name
    );

    let manifest_path = runtime_root.join("manifest.json");
    download_file(
        &first_entry.manifest.url,
        &manifest_path,
        first_entry.manifest.sha1.as_deref(),
    )
    .await?;

    let raw_manifest = fs::read_to_string(&manifest_path)?;
    let files_manifest: MojangRuntimeFilesManifest = serde_json::from_str(&raw_manifest)?;

    let mut batch_items = Vec::new();
    for (rel_path, entry) in &files_manifest.files {
        let dest = runtime_root.join(rel_path);
        if entry.entry_type == "directory" {
            let _ = fs::create_dir_all(&dest);
            continue;
        }
        if let Some(downloads) = &entry.downloads {
            if let Some(raw) = &downloads.raw {
                let needs_dl = !dest.exists() || !verify_file_sha1(&dest, &raw.sha1);
                if needs_dl {
                    batch_items.push(BatchDownloadItem {
                        url: raw.url.clone(),
                        dest,
                        sha1: Some(raw.sha1.clone()),
                    });
                }
            }
        }
    }

    if !batch_items.is_empty() {
        println!(
            "       Downloading {} JRE files in parallel...",
            batch_items.len()
        );
        download_batch_parallel(&batch_items, "Java Runtime").await?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for (rel_path, entry) in &files_manifest.files {
            if entry.executable {
                let dest = runtime_root.join(rel_path);
                if let Ok(meta) = fs::metadata(&dest) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&dest, perms);
                }
            }
        }
    }

    if !java_exe_path.exists() {
        bail!(
            "Downloaded Mojang runtime '{}', but java binary was not found at {}",
            component,
            java_exe_path.display()
        );
    }

    inspect_java_binary(&java_exe_path).or_else(|_| {
        Ok(JavaRuntime {
            path: java_exe_path,
            major_version: expected_major,
            version_string: first_entry.version.name,
        })
    })
}

pub fn default_mojang_component_for_major(major: u32) -> String {
    match major {
        0..=8 => "jre-legacy".to_string(),
        9..=16 => "java-runtime-alpha".to_string(),
        17..=20 => "java-runtime-gamma".to_string(),
        21..=24 => "java-runtime-delta".to_string(),
        _ => "java-runtime-epsilon".to_string(),
    }
}

pub fn get_mojang_platform_key() -> &'static str {
    if cfg!(windows) {
        if cfg!(target_arch = "aarch64") {
            "windows-arm64"
        } else if cfg!(target_arch = "x86") {
            "windows-x86"
        } else {
            "windows-x64"
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "mac-os-arm64"
        } else {
            "mac-os"
        }
    } else if cfg!(target_arch = "x86") {
        "linux-i386"
    } else {
        "linux"
    }
}

/// Discovers all available Java installations across the PC.
pub fn discover_all_java_runtimes() -> Vec<JavaRuntime> {
    let mut candidates = Vec::new();
    let java_bin_name = if cfg!(windows) { "java.exe" } else { "java" };

    // 1. Light Yield managed runtimes directory (`%APPDATA%\LightYieldLauncher\runtimes`)
    let managed_root = get_runtimes_dir();
    scan_runtime_tree(&managed_root, java_bin_name, &mut candidates, 4);

    // 2. Official Minecraft Launcher runtimes on Windows
    if cfg!(windows) {
        if let Some(local_app_data) = dirs::data_local_dir() {
            let store_runtime = local_app_data
                .join(r"Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime");
            scan_runtime_tree(&store_runtime, java_bin_name, &mut candidates, 5);
        }
        scan_runtime_tree(
            Path::new(r"C:\Program Files (x86)\Minecraft Launcher\runtime"),
            java_bin_name,
            &mut candidates,
            5,
        );
    }

    // 3. JAVA_HOME environment variable
    if let Ok(jh) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(jh).join("bin").join(java_bin_name);
        if p.exists() && !candidates.contains(&p) {
            candidates.push(p);
        }
    }

    // 4. PATH resolution
    if let Ok(path_var) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path_var) {
            let p = entry.join(java_bin_name);
            if p.exists() && !candidates.contains(&p) {
                candidates.push(p);
            }
        }
    }

    // 5. Common installation directories on Windows / macOS / Linux
    let mut search_roots = vec![
        PathBuf::from(r"C:\Program Files\Java"),
        PathBuf::from(r"C:\Program Files\Eclipse Adoptium"),
        PathBuf::from(r"C:\Program Files\Zulu"),
        PathBuf::from(r"C:\Program Files\BellSoft"),
        PathBuf::from(r"C:\Program Files\Microsoft"),
        PathBuf::from(r"C:\Program Files\Amazon Corretto"),
        PathBuf::from(r"C:\Program Files (x86)\Java"),
        PathBuf::from("/usr/lib/jvm"),
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
    ];
    if let Some(home) = dirs::home_dir() {
        search_roots.push(home.join(".jdks"));
        search_roots.push(home.join(".sdkman/candidates/java"));
    }

    for root in &search_roots {
        scan_runtime_tree(root, java_bin_name, &mut candidates, 3);
    }

    let mut runtimes = Vec::new();
    for p in candidates {
        if let Ok(rt) = inspect_java_binary(&p) {
            if !runtimes.iter().any(|existing: &JavaRuntime| existing.path == rt.path) {
                runtimes.push(rt);
            }
        }
    }

    // Sort descending by major version
    runtimes.sort_by(|a, b| b.major_version.cmp(&a.major_version));
    runtimes
}

fn scan_runtime_tree(dir: &Path, java_bin_name: &str, out: &mut Vec<PathBuf>, max_depth: u8) {
    if max_depth == 0 || !dir.exists() {
        return;
    }
    let direct_bin = dir.join("bin").join(java_bin_name);
    if direct_bin.exists() {
        if !out.contains(&direct_bin) {
            out.push(direct_bin);
        }
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_runtime_tree(&path, java_bin_name, out, max_depth - 1);
            }
        }
    }
}

/// Inspects a `java` binary to extract its major version.
pub fn inspect_java_binary(java_exe: &Path) -> Result<JavaRuntime> {
    // 1. Try reading the fast `release` file first (zero-spawn)
    if let Some(bin_dir) = java_exe.parent() {
        if let Some(jdk_root) = bin_dir.parent() {
            let release_file = jdk_root.join("release");
            if release_file.exists() {
                if let Ok(content) = fs::read_to_string(&release_file) {
                    for line in content.lines() {
                        if line.starts_with("JAVA_VERSION=") {
                            let ver_str = line
                                .trim_start_matches("JAVA_VERSION=")
                                .trim_matches('"')
                                .trim();
                            let major = parse_major_version(ver_str);
                            return Ok(JavaRuntime {
                                path: java_exe.to_path_buf(),
                                major_version: major,
                                version_string: ver_str.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Fall back to running `java -version`
    let output = Command::new(java_exe)
        .arg("-version")
        .output()
        .with_context(|| format!("Failed to execute Java binary at {}", java_exe.display()))?;

    let full_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let ver_str = extract_version_from_output(&full_out).unwrap_or_else(|| "17.0.0".to_string());
    let major = parse_major_version(&ver_str);

    Ok(JavaRuntime {
        path: java_exe.to_path_buf(),
        major_version: major,
        version_string: ver_str,
    })
}

/// Determines the required minimum Java major version for a given Minecraft version string.
pub fn get_required_java_version(mc_version: &str) -> u32 {
    if mc_version.starts_with('b')
        || mc_version.starts_with('a')
        || mc_version.starts_with("c0.")
        || mc_version.starts_with("rd-")
        || mc_version.starts_with("inf-")
    {
        return 8;
    }

    let parts: Vec<&str> = mc_version.split('.').collect();
    if parts.len() >= 2 && parts[0] == "1" {
        let minor_clean: String = parts[1].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(minor) = minor_clean.parse::<u32>() {
            if minor >= 21 {
                return 21;
            } else if minor == 20 {
                if parts.len() >= 3 {
                    let patch_clean: String = parts[2].chars().take_while(|c| c.is_ascii_digit()).collect();
                    if patch_clean.parse::<u32>().unwrap_or(0) >= 5 {
                        return 21;
                    }
                }
                return 17;
            } else if minor >= 18 {
                return 17;
            } else if minor == 17 {
                return 16;
            } else {
                return 8;
            }
        }
    } else if let Ok(year) = parts[0].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>() {
        // Year-based versions (e.g. 26.3, 25w14a, 24w14a, 13w36a)
        if mc_version.contains('w') {
            if year >= 26 {
                return 25;
            } else if year >= 24 {
                return 21;
            } else if year >= 21 {
                return 17;
            } else {
                return 8;
            }
        } else if year >= 26 {
            return 25;
        } else if year >= 20 {
            return 21;
        }
    }

    21
}

pub fn parse_major_version(ver: &str) -> u32 {
    if ver.starts_with("1.") {
        ver.split('.').nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(8)
    } else {
        let first_seg = ver.split(&['.', '-', '+', '_'][..]).next().unwrap_or("17");
        first_seg.parse::<u32>().unwrap_or(17)
    }
}

fn extract_version_from_output(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("version \"") {
            let parts: Vec<&str> = line.split("version \"").collect();
            if parts.len() >= 2 {
                let ver = parts[1].split('"').next()?;
                return Some(ver.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_major_version() {
        assert_eq!(parse_major_version("1.8.0_381"), 8);
        assert_eq!(parse_major_version("1.8.0_51"), 8);
        assert_eq!(parse_major_version("17.0.8"), 17);
        assert_eq!(parse_major_version("21.0.2"), 21);
        assert_eq!(parse_major_version("26.0.1"), 26);
        assert_eq!(parse_major_version("26-ea"), 26);
    }

    #[test]
    fn test_required_java_version() {
        assert_eq!(get_required_java_version("1.8.9"), 8);
        assert_eq!(get_required_java_version("1.12.2"), 8);
        assert_eq!(get_required_java_version("1.16.5"), 8);
        assert_eq!(get_required_java_version("1.17.1"), 16);
        assert_eq!(get_required_java_version("1.18.2"), 17);
        assert_eq!(get_required_java_version("1.20.1"), 17);
        assert_eq!(get_required_java_version("1.20.6"), 21);
        assert_eq!(get_required_java_version("1.21.4"), 21);
        assert_eq!(get_required_java_version("26.3"), 25);
        assert_eq!(get_required_java_version("b1.7.3"), 8);
        assert_eq!(get_required_java_version("a1.2.6"), 8);
    }
}
