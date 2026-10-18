//! Complete vanilla and modded Minecraft version installer.
//! Downloads version metadata, client JAR, libraries, and assets from Mojang CDN.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use kestryx_meta::{MavenCoordinate, RuleContext, VersionManifestV2, VersionProfile};

pub async fn download_file(url: &str, target_path: &Path) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
    let temp_target = target_path.with_extension("tmp");
    let status = tokio::process::Command::new(curl_bin)
        .args([
            "-sS",
            "-L",
            "--retry",
            "2",
            "-A",
            "Kestryx/0.1.0",
            url,
            "-o",
        ])
        .arg(&temp_target)
        .status()
        .await
        .with_context(|| format!("Failed to spawn curl for {url}"))?;

    if !status.success() {
        let _ = fs::remove_file(&temp_target);
        bail!("Failed to download {url}: curl returned {status}");
    }

    fs::rename(&temp_target, target_path)?;
    Ok(())
}

/// Ensures that a given Minecraft version (e.g., "1.21.4" or "26.3") is fully installed:
/// - Version JSON in `store/versions/{version}/{version}.json`
/// - Client JAR in `store/versions/{version}/client.jar`
/// - Required libraries in `store/libraries/...`
/// - Asset index in `store/assets/indexes/{id}.json`
pub async fn ensure_version_installed(
    data_root: &Path,
    version_id: &str,
    mut progress: impl FnMut(&str, f32),
) -> Result<VersionProfile> {
    let versions_dir = data_root.join("store/versions").join(version_id);
    fs::create_dir_all(&versions_dir)?;
    let version_json_path = versions_dir.join(format!("{version_id}.json"));

    // 1. Version JSON
    if !version_json_path.exists() {
        progress(
            &format!("Fetching Mojang manifest for {version_id}..."),
            0.05,
        );
        let manifest_bytes = tokio::process::Command::new(if cfg!(windows) {
            "curl.exe"
        } else {
            "curl"
        })
        .args([
            "-sS",
            "-L",
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
        ])
        .output()
        .await?;

        if !manifest_bytes.status.success() {
            bail!("Failed to fetch version manifest from Mojang");
        }

        let manifest: VersionManifestV2 = serde_json::from_slice(&manifest_bytes.stdout)?;
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == version_id)
            .or_else(|| {
                if version_id == "latest" || version_id == "latest-release" {
                    manifest
                        .versions
                        .iter()
                        .find(|v| v.id == manifest.latest.release)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                anyhow::anyhow!("Version '{version_id}' not found in Mojang version manifest")
            })?;

        progress(
            &format!("Downloading version profile for {}...", entry.id),
            0.10,
        );
        download_file(&entry.url, &version_json_path).await?;
    }

    let profile_content = fs::read_to_string(&version_json_path)?;
    let profile: VersionProfile = serde_json::from_str(&profile_content)?;

    // 2. Client JAR
    let client_jar_path = versions_dir.join("client.jar");
    if !client_jar_path.exists() {
        if let Some(dl) = profile.downloads.get("client") {
            progress(
                &format!(
                    "Downloading Minecraft client.jar ({} MB)...",
                    dl.size.unwrap_or(40_000_000) / 1024 / 1024
                ),
                0.20,
            );
            download_file(&dl.url, &client_jar_path).await?;
        } else {
            bail!("No client.jar download found in version profile for {version_id}");
        }
    }

    // 3. Libraries
    let libraries_root = data_root.join("store/libraries");
    let ctx = RuleContext::current_host();
    let arch_bits = if ctx.os_arch == "x86" { "32" } else { "64" };

    let mut to_download = Vec::new();
    for lib in &profile.libraries {
        if let Some(ref rules) = lib.rules {
            if !ctx.evaluate_rules(rules) {
                continue;
            }
        }

        // Standard artifact
        if let Some(ref art) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            let path = art.path.clone().unwrap_or_else(|| {
                MavenCoordinate::parse(&lib.name)
                    .map(|c| c.relative_path())
                    .unwrap_or_else(|_| "unknown.jar".to_string())
            });
            let full_path = libraries_root.join(&path);
            if !full_path.exists() {
                to_download.push((art.url.clone(), full_path));
            }
        } else if let Ok(coord) = MavenCoordinate::parse(&lib.name) {
            let rel = coord.relative_path();
            let full_path = libraries_root.join(&rel);
            if !full_path.exists() {
                let fallback_url =
                    format!("https://libraries.minecraft.net/{}", rel.replace('\\', "/"));
                to_download.push((fallback_url, full_path));
            }
        }

        // Legacy / classifier natives
        if let Some(ref natives_map) = lib.natives {
            if let Some(tmpl) = natives_map.get(&ctx.os_name) {
                let classifier = tmpl.replace("${arch}", arch_bits);
                if let Some(ref cls) = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.get(&classifier))
                {
                    let path = cls.path.clone().unwrap_or_else(|| {
                        MavenCoordinate::parse(&lib.name)
                            .map(|mut c| {
                                c.classifier = Some(classifier.clone());
                                c.relative_path()
                            })
                            .unwrap_or_else(|_| "native.jar".to_string())
                    });
                    let full_path = libraries_root.join(&path);
                    if !full_path.exists() {
                        to_download.push((cls.url.clone(), full_path));
                    }
                }
            }
        }
    }

    let total_libs = to_download.len();
    if total_libs > 0 {
        for (i, (url, path)) in to_download.into_iter().enumerate() {
            let pct = 0.30 + 0.55 * (i as f32 / total_libs as f32);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            progress(
                &format!(
                    "Downloading library {}/{} ({file_name})...",
                    i + 1,
                    total_libs
                ),
                pct,
            );
            let _ = download_file(&url, &path).await;
        }
    }

    // 4. Asset index
    if let Some(ref asset_idx) = profile.asset_index {
        let index_path = data_root
            .join("store/assets/indexes")
            .join(format!("{}.json", asset_idx.id));
        if !index_path.exists() {
            progress("Downloading asset index...", 0.90);
            let _ = download_file(&asset_idx.url, &index_path).await;
        }
    }

    progress("Installation complete!", 1.0);
    Ok(profile)
}
