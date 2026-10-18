//! Version manifest fetcher, cache manager, and filter engine.
//! Manages online retrieval from Mojang and local caching to `%APPDATA%\LightYieldLauncher\versions.txt`.

use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionFilter {
    Release,
    Snapshot,
    Beta,
    Alpha,
    All,
}

impl VersionFilter {
    pub fn matches(&self, entry: &VersionEntry) -> bool {
        match self {
            VersionFilter::Release => entry.version_type == "release",
            VersionFilter::Snapshot => entry.version_type == "snapshot",
            VersionFilter::Beta => entry.version_type == "old_beta",
            VersionFilter::Alpha => entry.version_type == "old_alpha",
            VersionFilter::All => true,
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().trim() {
            "release" | "releases" => VersionFilter::Release,
            "snapshot" | "snapshots" => VersionFilter::Snapshot,
            "beta" | "old_beta" => VersionFilter::Beta,
            "alpha" | "old_alpha" => VersionFilter::Alpha,
            _ => VersionFilter::All,
        }
    }
}

/// Fetches the live manifest from Mojang if online and updates `versions.txt`.
/// If offline, transparently falls back to reading `versions.txt`.
pub async fn fetch_or_load_manifest(cache_file: &Path) -> Result<VersionManifest> {
    // Attempt online fetch first
    match fetch_online_manifest().await {
        Ok(manifest) => {
            // Write formatted text cache to disk
            if let Err(e) = save_manifest_to_txt(&manifest, cache_file) {
                eprintln!("[WARN] Failed to write versions cache to {}: {e}", cache_file.display());
            }
            Ok(manifest)
        }
        Err(net_err) => {
            eprintln!("[INFO] Network unavailable ({net_err}). Attempting to load offline cache...");
            if cache_file.exists() {
                load_manifest_from_txt(cache_file)
                    .with_context(|| format!("Failed to read offline version cache from {}", cache_file.display()))
            } else {
                Err(anyhow::anyhow!(
                    "No internet connection and no offline version cache found at {}. Please connect to the internet once.",
                    cache_file.display()
                ))
            }
        }
    }
}

/// Directly fetches the official manifest from Mojang via curl.
pub async fn fetch_online_manifest() -> Result<VersionManifest> {
    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
    let output = tokio::process::Command::new(curl_bin)
        .args([
            "-sS",
            "-L",
            "--retry",
            "2",
            "-A",
            "LightYieldLauncher/0.1.0",
            MANIFEST_URL,
        ])
        .output()
        .await
        .with_context(|| "Failed to execute curl.exe to fetch version manifest")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("curl failed to fetch version manifest: {err_msg}");
    }

    let manifest: VersionManifest = serde_json::from_slice(&output.stdout)
        .with_context(|| "Failed to deserialize Mojang version manifest")?;
    Ok(manifest)
}

/// Saves the version directory in clean human-readable and machine-parseable text format.
pub fn save_manifest_to_txt(manifest: &VersionManifest, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut content = String::with_capacity(64 * 1024);
    content.push_str("# Light Yield Launcher - Version Cache\n");
    content.push_str(&format!("# Latest Release: {}\n", manifest.latest.release));
    content.push_str(&format!("# Latest Snapshot: {}\n", manifest.latest.snapshot));
    content.push_str("# Columns: ID | TYPE | RELEASE_TIME | URL\n");
    content.push_str("# ------------------------------------------------------------\n");

    for v in &manifest.versions {
        content.push_str(&format!("{} | {} | {} | {}\n", v.id, v.version_type, v.release_time, v.url));
    }

    fs::write(path, content)?;
    Ok(())
}

/// Reads and parses the cached `versions.txt` file into a `VersionManifest`.
pub fn load_manifest_from_txt(path: &Path) -> Result<VersionManifest> {
    let raw = fs::read_to_string(path)?;
    let mut versions = Vec::new();
    let mut latest_release = "26.3".to_string();
    let mut latest_snapshot = "26.4-snapshot-4".to_string();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("# Latest Release:") {
            latest_release = line.trim_start_matches("# Latest Release:").trim().to_string();
            continue;
        }
        if line.starts_with("# Latest Snapshot:") {
            latest_snapshot = line.trim_start_matches("# Latest Snapshot:").trim().to_string();
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if parts.len() >= 4 {
            versions.push(VersionEntry {
                id: parts[0].to_string(),
                version_type: parts[1].to_string(),
                release_time: parts[2].to_string(),
                time: parts[2].to_string(),
                url: parts[3].to_string(),
            });
        }
    }

    Ok(VersionManifest {
        latest: LatestVersions {
            release: latest_release,
            snapshot: latest_snapshot,
        },
        versions,
    })
}

impl VersionManifest {
    /// Returns filtered versions according to the provided filter.
    pub fn filter(&self, filter: VersionFilter) -> Vec<&VersionEntry> {
        self.versions.iter().filter(|v| filter.matches(v)).collect()
    }

    /// Finds a version entry by its identifier (e.g. "1.21.4").
    pub fn find(&self, id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_manifest_txt() {
        let temp_dir = std::env::temp_dir().join("lightyield_test_manifest");
        let cache_file = temp_dir.join("versions.txt");

        let sample = VersionManifest {
            latest: LatestVersions {
                release: "1.21.4".into(),
                snapshot: "1.21.5-pre1".into(),
            },
            versions: vec![
                VersionEntry {
                    id: "1.21.4".into(),
                    version_type: "release".into(),
                    url: "https://example.com/1.21.4.json".into(),
                    time: "2024-12-03".into(),
                    release_time: "2024-12-03".into(),
                },
                VersionEntry {
                    id: "b1.7.3".into(),
                    version_type: "old_beta".into(),
                    url: "https://example.com/b1.7.3.json".into(),
                    time: "2011-07-07".into(),
                    release_time: "2011-07-07".into(),
                },
            ],
        };

        save_manifest_to_txt(&sample, &cache_file).unwrap();
        assert!(cache_file.exists());

        let loaded = load_manifest_from_txt(&cache_file).unwrap();
        assert_eq!(loaded.latest.release, "1.21.4");
        assert_eq!(loaded.versions.len(), 2);
        assert_eq!(loaded.find("1.21.4").unwrap().version_type, "release");
        assert_eq!(loaded.filter(VersionFilter::Beta).len(), 1);

        let _ = fs::remove_dir_all(temp_dir);
    }
}
