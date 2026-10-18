//! Minecraft version installer.
//! Downloads version metadata, client.jar, platform libraries, extracts native binaries,
//! and fetches 100% of assets (including legacy virtual & pre-1.6 resource layouts)
//! using high-speed parallel HTTP/2 transfers.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::storage::{
    get_assets_dir, get_instances_dir, get_launcher_dir, get_libraries_dir,
    get_system_minecraft_dir, get_versions_dir,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionPackage {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    pub downloads: PackageDownloads,
    #[serde(default)]
    pub libraries: Vec<PackageLibrary>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<PackageAssetIndex>,
    pub assets: Option<String>,
    pub arguments: Option<PackageArguments>,
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<PackageJavaVersion>,
    #[serde(rename = "type")]
    pub version_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageDownloads {
    pub client: DownloadItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageAssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexFile {
    #[serde(default)]
    pub r#virtual: bool,
    #[serde(default)]
    pub map_to_resources: bool,
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageJavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageArguments {
    #[serde(default)]
    pub game: Vec<serde_json::Value>,
    #[serde(default)]
    pub jvm: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageLibrary {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<LibraryRule>,
    pub natives: Option<serde_json::Value>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<LibraryArtifact>,
    pub classifiers: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryArtifact {
    pub path: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryRule {
    pub action: String,
    pub os: Option<RuleOs>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleOs {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BatchDownloadItem {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
}

/// Result of an installation containing local file locations ready for launch.
pub struct InstalledVersion {
    pub version_id: String,
    pub version_json_path: PathBuf,
    pub client_jar_path: PathBuf,
    pub natives_dir: PathBuf,
    pub classpath_jars: Vec<PathBuf>,
    pub package: VersionPackage,
    pub virtual_assets_dir: Option<PathBuf>,
}

/// Installs the given version ID from Mojang:
/// 1. Fetches version JSON
/// 2. Downloads & verifies client.jar
/// 3. Resolves & batch-downloads matching platform libraries
/// 4. Extracts architecture-matched native libraries (.dll / .so / .dylib)
/// 5. Downloads asset index & 100% of game assets (including virtual/pre-1.6 reconstruction)
pub async fn install_version(
    version_id: &str,
    manifest_url: &str,
    offline_mode: bool,
) -> Result<InstalledVersion> {
    let v_dir = get_versions_dir().join(version_id);
    fs::create_dir_all(&v_dir)?;

    let v_json_path = v_dir.join(format!("{version_id}.json"));
    let v_jar_path = v_dir.join(format!("{version_id}.jar"));
    let natives_dir = v_dir.join("natives");
    fs::create_dir_all(&natives_dir)?;

    let sys_mc_dir = get_system_minecraft_dir();

    // 1. Version JSON
    println!("[1/5] Fetching version metadata for '{version_id}'...");
    let package: VersionPackage = if v_json_path.exists() {
        let content = fs::read_to_string(&v_json_path)?;
        serde_json::from_str(&content)?
    } else {
        if offline_mode {
            bail!("Version '{version_id}' is not installed locally and offline mode is active.");
        }
        download_file(manifest_url, &v_json_path, None).await?;
        let content = fs::read_to_string(&v_json_path)?;
        serde_json::from_str(&content)?
    };

    // 2. Client.jar
    println!("[2/5] Checking client.jar...");
    let client_download = &package.downloads.client;
    if !is_file_valid(&v_jar_path, client_download.sha1.as_deref()) {
        // Check if existing `.minecraft` has a verified copy first
        if let Some(sys_mc) = &sys_mc_dir {
            let sys_jar = sys_mc.join("versions").join(version_id).join(format!("{version_id}.jar"));
            if is_file_valid(&sys_jar, client_download.sha1.as_deref()) {
                let _ = fs::copy(&sys_jar, &v_jar_path);
            }
        }
    }

    if !is_file_valid(&v_jar_path, client_download.sha1.as_deref()) {
        if offline_mode {
            bail!("Client jar for '{version_id}' is missing in offline mode.");
        }
        let size_str = client_download
            .size
            .map(|s| format!(" ({:.1} MB)", s as f64 / 1_048_576.0))
            .unwrap_or_default();
        println!("      Downloading client.jar{size_str}...");
        download_file(&client_download.url, &v_jar_path, client_download.sha1.as_deref()).await?;
    } else {
        println!("      client.jar is verified on disk.");
    }

    // 3. Libraries & Natives
    println!("[3/5] Resolving and verifying required libraries...");
    let libs_dir = get_libraries_dir();
    fs::create_dir_all(&libs_dir)?;

    let mut classpath_jars = Vec::new();
    let mut native_jars = Vec::new();
    let mut missing_libs = Vec::new();

    let arch_bits = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
    let os_key = current_os_name();

    for lib in &package.libraries {
        if !is_library_allowed_for_current_os(&lib.rules) {
            continue;
        }

        // Strictly reject mismatched CPU architecture native classifiers on 64-bit x86_64
        if cfg!(target_arch = "x86_64")
            && (lib.name.contains("natives-windows-x86")
                || lib.name.contains("natives-windows-arm")
                || lib.name.contains("natives-macos-arm")
                || lib.name.contains("natives-linux-arm")
                || (lib.name.contains("x86") && lib.name.contains("natives")))
        {
            continue;
        }

        let native_classifier_key = lib
            .natives
            .as_ref()
            .and_then(|n| n.get(os_key))
            .and_then(|v| v.as_str())
            .map(|s| s.replace("${arch}", arch_bits));

        let is_modern_native = lib.name.contains(":natives-");

        if let Some(downloads) = &lib.downloads {
            // Standard library artifact (or modern native jar)
            if let Some(artifact) = &downloads.artifact {
                let dest = libs_dir.join(&artifact.path);
                if is_modern_native || native_classifier_key.is_some() {
                    if is_modern_native && !native_jars.contains(&dest) {
                        native_jars.push(dest.clone());
                    }
                    // Note: some legacy libraries have both a main jar AND a classifier native jar
                    if !is_modern_native && !classpath_jars.contains(&dest) {
                        classpath_jars.push(dest.clone());
                    }
                } else if !classpath_jars.contains(&dest) {
                    classpath_jars.push(dest.clone());
                }

                if !is_file_valid(&dest, artifact.sha1.as_deref()) {
                    if let Some(sys_mc) = &sys_mc_dir {
                        let sys_lib = sys_mc.join("libraries").join(&artifact.path);
                        if is_file_valid(&sys_lib, artifact.sha1.as_deref()) {
                            if let Some(p) = dest.parent() {
                                let _ = fs::create_dir_all(p);
                            }
                            let _ = fs::copy(&sys_lib, &dest);
                        }
                    }
                }

                if !is_file_valid(&dest, artifact.sha1.as_deref()) && !offline_mode {
                    missing_libs.push(BatchDownloadItem {
                        url: artifact.url.clone(),
                        dest,
                        sha1: artifact.sha1.clone(),
                    });
                }
            }

            // Classifier native jar (e.g. `natives-windows`, `natives-windows-64`, `natives-osx`, `natives-linux`)
            if let Some(classifiers) = &downloads.classifiers {
                let lookup_keys: Vec<String> = if let Some(ref k) = native_classifier_key {
                    vec![k.clone(), format!("natives-{os_key}")]
                } else {
                    vec![format!("natives-{os_key}")]
                };

                for key in lookup_keys {
                    if let Some(nat_val) = classifiers.get(&key) {
                        if let Ok(native_artifact) = serde_json::from_value::<LibraryArtifact>(nat_val.clone()) {
                            let dest = libs_dir.join(&native_artifact.path);
                            if !native_jars.contains(&dest) {
                                native_jars.push(dest.clone());
                            }
                            if !is_file_valid(&dest, native_artifact.sha1.as_deref()) {
                                if let Some(sys_mc) = &sys_mc_dir {
                                    let sys_lib = sys_mc.join("libraries").join(&native_artifact.path);
                                    if is_file_valid(&sys_lib, native_artifact.sha1.as_deref()) {
                                        if let Some(p) = dest.parent() {
                                            let _ = fs::create_dir_all(p);
                                        }
                                        let _ = fs::copy(&sys_lib, &dest);
                                    }
                                }
                            }
                            if !is_file_valid(&dest, native_artifact.sha1.as_deref()) && !offline_mode {
                                missing_libs.push(BatchDownloadItem {
                                    url: native_artifact.url.clone(),
                                    dest,
                                    sha1: native_artifact.sha1.clone(),
                                });
                            }
                            break;
                        }
                    }
                }
            }
        } else if let Some(path) = maven_coordinate_to_path(&lib.name) {
            let dest = libs_dir.join(&path);
            if !classpath_jars.contains(&dest) {
                classpath_jars.push(dest.clone());
            }
            if !dest.exists() && !offline_mode {
                let base_url = lib.url.as_deref().unwrap_or("https://libraries.minecraft.net/");
                let full_url = format!(
                    "{}{}",
                    base_url.trim_end_matches('/'),
                    if path.starts_with('/') { path.clone() } else { format!("/{path}") }
                );
                missing_libs.push(BatchDownloadItem {
                    url: full_url,
                    dest,
                    sha1: None,
                });
            }
        }
    }

    if !missing_libs.is_empty() {
        println!("      Downloading {} missing libraries in parallel...", missing_libs.len());
        download_batch_parallel(&missing_libs, "Libraries").await?;
    }
    println!("      {} classpath libraries verified.", classpath_jars.len());

    // 4. Extract Native Binaries
    println!("[4/5] Extracting native platform libraries...");
    let _ = fs::remove_dir_all(&natives_dir);
    fs::create_dir_all(&natives_dir)?;

    let mut extracted_count = 0;
    for nat_jar in &native_jars {
        if nat_jar.exists() {
            if let Ok(count) = extract_natives_from_jar(nat_jar, &natives_dir) {
                extracted_count += count;
            }
        }
    }
    println!("      {extracted_count} native binaries ready in natives/.");

    // 5. Asset Index & Complete Game Assets
    let mut virtual_assets_dir = None;
    if let Some(asset_index) = &package.asset_index {
        println!("[5/5] Verifying asset index '{}' and game resources...", asset_index.id);
        let index_path = get_assets_dir()
            .join("indexes")
            .join(format!("{}.json", asset_index.id));

        if !is_file_valid(&index_path, Some(&asset_index.sha1)) && !offline_mode {
            download_file(&asset_index.url, &index_path, Some(&asset_index.sha1)).await?;
        }

        if index_path.exists() {
            let content = fs::read_to_string(&index_path)?;
            let parsed: AssetIndexFile = serde_json::from_str(&content)?;
            let objects_dir = get_assets_dir().join("objects");
            fs::create_dir_all(&objects_dir)?;

            let mut missing_assets = Vec::new();
            let mut seen_hashes = std::collections::HashSet::new();

            for obj in parsed.objects.values() {
                if obj.hash.len() < 2 || !seen_hashes.insert(obj.hash.clone()) {
                    continue;
                }
                let sub = &obj.hash[..2];
                let target = objects_dir.join(sub).join(&obj.hash);

                if !target.exists() {
                    // Reuse from system `.minecraft/assets/objects` if available
                    if let Some(sys_mc) = &sys_mc_dir {
                        let sys_obj = sys_mc.join("assets").join("objects").join(sub).join(&obj.hash);
                        if sys_obj.exists() {
                            if let Some(p) = target.parent() {
                                let _ = fs::create_dir_all(p);
                            }
                            let _ = fs::copy(&sys_obj, &target);
                        }
                    }
                }

                if !target.exists() && !offline_mode {
                    let url = format!("https://resources.download.minecraft.net/{sub}/{}", obj.hash);
                    missing_assets.push(BatchDownloadItem {
                        url,
                        dest: target,
                        sha1: Some(obj.hash.clone()),
                    });
                }
            }

            if !missing_assets.is_empty() {
                println!(
                    "      Downloading {} missing game assets (sounds, textures, languages) in parallel...",
                    missing_assets.len()
                );
                download_batch_parallel(&missing_assets, "Assets").await?;
            } else {
                println!("      All {} unique asset objects verified on disk.", seen_hashes.len());
            }

            // Reconstruct legacy virtual / map_to_resources directories for <= 1.7.2, Beta, Alpha
            if parsed.r#virtual || parsed.map_to_resources || asset_index.id == "legacy" || asset_index.id == "pre-1.6" {
                let virt_root = get_assets_dir().join("virtual").join(&asset_index.id);
                let game_resources_root = get_instances_dir().join(version_id).join("resources");
                fs::create_dir_all(&virt_root)?;
                if parsed.map_to_resources {
                    fs::create_dir_all(&game_resources_root)?;
                }

                for (logical_path, obj) in &parsed.objects {
                    if obj.hash.len() < 2 {
                        continue;
                    }
                    let src = objects_dir.join(&obj.hash[..2]).join(&obj.hash);
                    if !src.exists() {
                        continue;
                    }

                    let virt_dest = virt_root.join(logical_path);
                    if !virt_dest.exists() {
                        if let Some(p) = virt_dest.parent() {
                            let _ = fs::create_dir_all(p);
                        }
                        let _ = fs::copy(&src, &virt_dest);
                    }

                    if parsed.map_to_resources {
                        let res_dest = game_resources_root.join(logical_path);
                        if !res_dest.exists() {
                            if let Some(p) = res_dest.parent() {
                                let _ = fs::create_dir_all(p);
                            }
                            let _ = fs::copy(&src, &res_dest);
                        }
                    }
                }
                virtual_assets_dir = Some(virt_root);
            }
        }
    }

    Ok(InstalledVersion {
        version_id: version_id.to_string(),
        version_json_path: v_json_path,
        client_jar_path: v_jar_path,
        natives_dir,
        classpath_jars,
        package,
        virtual_assets_dir,
    })
}

pub fn current_os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Evaluates whether a library is allowed on the current OS & CPU architecture.
pub fn is_library_allowed_for_current_os(rules: &[LibraryRule]) -> bool {
    if rules.is_empty() {
        return true;
    }

    let current_os = current_os_name();
    let mut allowed = false;

    for rule in rules {
        let os_match = match &rule.os {
            Some(os) => {
                let name_match = os.name.as_deref().map(|n| n == current_os).unwrap_or(true);
                let arch_match = os
                    .arch
                    .as_deref()
                    .map(|a| {
                        if cfg!(target_arch = "x86_64") {
                            a != "x86" && a != "arm64"
                        } else if cfg!(target_arch = "aarch64") {
                            a == "arm64" || a == "aarch64"
                        } else {
                            a == "x86"
                        }
                    })
                    .unwrap_or(true);
                name_match && arch_match
            }
            None => true,
        };

        if os_match {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

/// Backwards-compatible alias for unit tests.
pub fn is_library_allowed_on_windows(rules: &[LibraryRule]) -> bool {
    is_library_allowed_for_current_os(rules)
}

/// Extracts all native shared libraries (`.dll`, `.so`, `.dylib`, `.jnilib`) from a JAR zip archive.
pub fn extract_natives_from_jar(jar_path: &Path, dest_dir: &Path) -> Result<usize> {
    let file = File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut count = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        let is_native_ext = name.ends_with(".dll")
            || name.ends_with(".so")
            || name.ends_with(".dylib")
            || name.ends_with(".jnilib");

        if is_native_ext
            && !name.contains("META-INF")
            && !name.contains("/x86/")
            && !name.contains("\\x86\\")
            && !name.contains("/arm64/")
            && !name.contains("\\arm64\\")
        {
            let file_name = Path::new(&name)
                .file_name()
                .context("Invalid native file name")?;
            let dest_path = dest_dir.join(file_name);

            let mut out = File::create(&dest_path)?;
            io::copy(&mut file, &mut out)?;
            count += 1;
        }
    }

    Ok(count)
}

/// Converts a Maven coordinate like `org.lwjgl:lwjgl:3.3.3` into path `org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar`.
pub fn maven_coordinate_to_path(coord: &str) -> Option<String> {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = if parts.len() >= 4 {
        format!("-{}", parts[3])
    } else {
        String::new()
    };

    Some(format!(
        "{group}/{artifact}/{version}/{artifact}-{version}{classifier}.jar"
    ))
}

fn is_file_valid(path: &Path, expected_sha1: Option<&str>) -> bool {
    if !path.exists() {
        return false;
    }
    if let Some(expected) = expected_sha1 {
        verify_file_sha1(path, expected)
    } else {
        true
    }
}

pub fn verify_file_sha1(path: &Path, expected: &str) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 16384];

    while let Ok(n) = file.read(&mut buffer) {
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let actual = format!("{:x}", hasher.finalize());
    actual.eq_ignore_ascii_case(expected)
}

/// High-speed parallel batch downloader using `curl --parallel --parallel-max 16 -K <config>`.
/// Downloads in chunks of 150 items with live progress reporting and automatic individual retry fallback.
pub async fn download_batch_parallel(items: &[BatchDownloadItem], label: &str) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }

    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
    let total = items.len();
    let chunk_size = 150;
    let cfg_path = get_launcher_dir().join("curl_batch.cfg");

    for (chunk_idx, chunk) in items.chunks(chunk_size).enumerate() {
        let mut cfg_file = File::create(&cfg_path)?;
        for item in chunk {
            if let Some(parent) = item.dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let escaped_dest = item
                .dest
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            writeln!(cfg_file, "url = \"{}\"", item.url)?;
            writeln!(cfg_file, "output = \"{}\"", escaped_dest)?;
        }
        cfg_file.flush()?;
        drop(cfg_file);

        let _ = tokio::process::Command::new(curl_bin)
            .args([
                "--parallel",
                "--parallel-max",
                "16",
                "--retry",
                "3",
                "-sS",
                "-L",
                "-A",
                "LightYieldLauncher/0.1.0",
                "-K",
            ])
            .arg(&cfg_path)
            .status()
            .await;

        // Verify every file in the chunk and retry any corrupt/missing file individually
        for item in chunk {
            let valid = if let Some(ref sha1) = item.sha1 {
                verify_file_sha1(&item.dest, sha1)
            } else {
                item.dest.exists()
            };
            if !valid {
                download_file(&item.url, &item.dest, item.sha1.as_deref()).await?;
            }
        }

        let completed = ((chunk_idx + 1) * chunk_size).min(total);
        let pct = (completed * 100) / total;
        println!("      [{label}] Progress: {completed}/{total} ({pct}%)");
    }

    let _ = fs::remove_file(&cfg_path);
    Ok(())
}

pub async fn download_file(url: &str, dest: &Path, expected_sha1: Option<&str>) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
    let temp_dest = dest.with_extension("tmp");

    let status = tokio::process::Command::new(curl_bin)
        .args([
            "-sS",
            "-L",
            "--retry",
            "3",
            "-A",
            "LightYieldLauncher/0.1.0",
            url,
            "-o",
        ])
        .arg(&temp_dest)
        .status()
        .await
        .with_context(|| format!("Failed to spawn curl for {url}"))?;

    if !status.success() {
        let _ = fs::remove_file(&temp_dest);
        bail!("Failed to download {url}: curl exit code {status}");
    }

    if let Some(sha1) = expected_sha1 {
        if !verify_file_sha1(&temp_dest, sha1) {
            let _ = fs::remove_file(&temp_dest);
            bail!("SHA-1 checksum verification failed for {url}");
        }
    }

    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(&temp_dest, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_rule_windows() {
        let rules = vec![
            LibraryRule {
                action: "allow".into(),
                os: None,
            },
            LibraryRule {
                action: "disallow".into(),
                os: Some(RuleOs {
                    name: Some("osx".into()),
                    arch: None,
                }),
            },
        ];
        assert!(is_library_allowed_on_windows(&rules));

        let osx_only = vec![LibraryRule {
            action: "allow".into(),
            os: Some(RuleOs {
                name: Some("osx".into()),
                arch: None,
            }),
        }];
        assert!(!is_library_allowed_on_windows(&osx_only));
    }

    #[test]
    fn test_maven_coordinate_to_path() {
        let p = maven_coordinate_to_path("net.sf.jopt-simple:jopt-simple:5.0.4").unwrap();
        assert_eq!(p, "net/sf/jopt-simple/jopt-simple/5.0.4/jopt-simple-5.0.4.jar");
    }
}
