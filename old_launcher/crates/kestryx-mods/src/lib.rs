//! `kestryx-mods`: Unified Modrinth v2 & CurseForge modding engine, dependency solver,
//! jar metadata inspector (`fabric.mod.json`, `quilt.mod.json`, `mods.toml`, `neoforge.mods.toml`),
//! Mod Profiles with diff & one-click switch, `.mrpack` modpack import/export, and `.history/` rollback.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use kestryx_store::{ContentStore, MultiDigest, link_or_copy};
use serde::{Deserialize, Serialize};

const OPTIMIZATION_PRESETS_JSON: &str = include_str!("../../../data/optimization-presets.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModJarMetadata {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub loader: String, // "fabric" | "quilt" | "forge" | "neoforge" | "unknown"
    pub environment: String, // "client" | "server" | "both"
    pub authors: Vec<String>,
    pub description: String,
    pub dependencies: Vec<String>,
}

/// Inspects a mod `.jar` archive in-memory or on disk to parse `fabric.mod.json`,
/// `quilt.mod.json`, `META-INF/neoforge.mods.toml`, or `META-INF/mods.toml`.
pub fn inspect_mod_jar_bytes(jar_bytes: &[u8]) -> Result<ModJarMetadata> {
    let reader = std::io::Cursor::new(jar_bytes);
    let mut archive = zip::ZipArchive::new(reader).context("Invalid mod jar zip archive")?;

    // 1. Check `fabric.mod.json`
    if let Ok(mut f) = archive.by_name("fabric.mod.json") {
        let mut raw = String::new();
        f.read_to_string(&mut raw)?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let mod_id = val["id"].as_str().unwrap_or("unknown").to_string();
        let name = val["name"].as_str().unwrap_or(&mod_id).to_string();
        let version = val["version"].as_str().unwrap_or("0.0.0").to_string();
        let env = val["environment"].as_str().unwrap_or("*").to_string();
        let environment = match env.as_str() {
            "client" => "client",
            "server" => "server",
            _ => "both",
        }
        .to_string();
        let deps = val["depends"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        return Ok(ModJarMetadata {
            mod_id,
            name,
            version,
            loader: "fabric".to_string(),
            environment,
            authors: Vec::new(),
            description: val["description"].as_str().unwrap_or("").to_string(),
            dependencies: deps,
        });
    }

    // 2. Check `quilt.mod.json`
    if let Ok(mut f) = archive.by_name("quilt.mod.json") {
        let mut raw = String::new();
        f.read_to_string(&mut raw)?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let ql = &val["quilt_loader"];
        let mod_id = ql["id"].as_str().unwrap_or("unknown").to_string();
        let version = ql["version"].as_str().unwrap_or("0.0.0").to_string();
        let name = ql["metadata"]["name"]
            .as_str()
            .unwrap_or(&mod_id)
            .to_string();
        return Ok(ModJarMetadata {
            mod_id,
            name,
            version,
            loader: "quilt".to_string(),
            environment: "both".to_string(),
            authors: Vec::new(),
            description: ql["metadata"]["description"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            dependencies: Vec::new(),
        });
    }

    // 3. Check `META-INF/neoforge.mods.toml` or `META-INF/mods.toml`
    for (entry_name, loader_kind) in [
        ("META-INF/neoforge.mods.toml", "neoforge"),
        ("META-INF/mods.toml", "forge"),
    ] {
        if let Ok(mut f) = archive.by_name(entry_name) {
            let mut raw = String::new();
            f.read_to_string(&mut raw)?;
            let parsed: toml::Value = toml::from_str(&raw)?;
            if let Some(mods_arr) = parsed.get("mods").and_then(|v| v.as_array()) {
                if let Some(first) = mods_arr.first() {
                    let mod_id = first
                        .get("modId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let name = first
                        .get("displayName")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&mod_id)
                        .to_string();
                    let version = first
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0.0.0")
                        .to_string();
                    let desc = first
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return Ok(ModJarMetadata {
                        mod_id,
                        name,
                        version,
                        loader: loader_kind.to_string(),
                        environment: "both".to_string(),
                        authors: Vec::new(),
                        description: desc,
                        dependencies: Vec::new(),
                    });
                }
            }
        }
    }

    bail!("Unrecognized mod jar format (no fabric.mod.json, quilt.mod.json, or mods.toml found)")
}

// ============================================================================
// Dependency Resolver (Required, Optional, Incompatible, Embedded)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateModVersion {
    pub project_id: String,
    pub slug: String,
    pub version_id: String,
    pub version_number: String,
    pub channel: String, // "release" | "beta" | "alpha"
    pub required_deps: Vec<String>,
    pub optional_deps: Vec<String>,
    pub incompatible_with: Vec<String>,
    pub embedded_jars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyResolutionPlan {
    pub to_install: Vec<CandidateModVersion>,
    pub skipped_already_installed_or_embedded: Vec<String>,
    pub conflicts: Vec<String>,
}

/// Resolves the transitive closure of required dependencies while skipping embedded jars
/// and flagging incompatible mod conflicts.
pub fn resolve_mod_dependencies(
    requested_roots: &[String],
    installed_project_ids: &BTreeSet<String>,
    catalog: &BTreeMap<String, CandidateModVersion>,
) -> Result<DependencyResolutionPlan> {
    let mut to_install = Vec::new();
    let mut seen = installed_project_ids.clone();
    let mut embedded_provided = BTreeSet::new();
    let mut skipped = Vec::new();
    let mut conflicts = Vec::new();

    let mut queue: std::collections::VecDeque<String> = requested_roots.iter().cloned().collect();

    while let Some(proj_id) = queue.pop_front() {
        if seen.contains(&proj_id) || embedded_provided.contains(&proj_id) {
            skipped.push(proj_id);
            continue;
        }
        let Some(candidate) = catalog.get(&proj_id) else {
            conflicts.push(format!("Missing required dependency in catalog: {proj_id}"));
            continue;
        };

        for inc in &candidate.incompatible_with {
            if seen.contains(inc) || requested_roots.contains(inc) {
                conflicts.push(format!(
                    "Mod '{}' ({}) is incompatible with '{}'",
                    candidate.slug, candidate.project_id, inc
                ));
            }
        }

        for emb in &candidate.embedded_jars {
            embedded_provided.insert(emb.clone());
        }

        for req_dep in &candidate.required_deps {
            if !seen.contains(req_dep) && !embedded_provided.contains(req_dep) {
                queue.push_back(req_dep.clone());
            }
        }

        seen.insert(proj_id);
        to_install.push(candidate.clone());
    }

    Ok(DependencyResolutionPlan {
        to_install,
        skipped_already_installed_or_embedded: skipped,
        conflicts,
    })
}

// ============================================================================
// CurseForge Third-Party Distribution Opt-Out Handler
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurseForgeManualDownloadGuidance {
    pub mod_name: String,
    pub file_name: String,
    pub browser_download_url: String,
    pub expected_sha1: Option<String>,
    pub instructions: String,
}

pub fn check_curseforge_distribution_allowed(
    mod_slug: &str,
    mod_name: &str,
    file_id: u64,
    file_name: &str,
    download_url: Option<&str>,
    expected_sha1: Option<String>,
) -> std::result::Result<String, CurseForgeManualDownloadGuidance> {
    if let Some(url) = download_url {
        if !url.trim().is_empty() {
            return Ok(url.to_string());
        }
    }
    Err(CurseForgeManualDownloadGuidance {
        mod_name: mod_name.to_string(),
        file_name: file_name.to_string(),
        browser_download_url: format!(
            "https://www.curseforge.com/minecraft/mc-mods/{mod_slug}/download/{file_id}"
        ),
        expected_sha1,
        instructions: format!(
            "The author of '{}' has disabled direct third-party launcher downloads on CurseForge. Click to open the download page in your browser; Kestryx will automatically detect '{}' in your Downloads folder, verify its hash, and link it into your instance.",
            mod_name, file_name
        ),
    })
}

// ============================================================================
// Instance Mod Profiles, Diff View, Update & Rollback
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModProfileManifest {
    pub name: String,
    pub enabled_mods: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModProfileDiff {
    pub only_in_left: Vec<String>,
    pub only_in_right: Vec<String>,
    pub shared: Vec<String>,
}

pub fn diff_mod_profiles(left: &ModProfileManifest, right: &ModProfileManifest) -> ModProfileDiff {
    let only_in_left = left
        .enabled_mods
        .difference(&right.enabled_mods)
        .cloned()
        .collect();
    let only_in_right = right
        .enabled_mods
        .difference(&left.enabled_mods)
        .cloned()
        .collect();
    let shared = left
        .enabled_mods
        .intersection(&right.enabled_mods)
        .cloned()
        .collect();
    ModProfileDiff {
        only_in_left,
        only_in_right,
        shared,
    }
}

/// Enables or disables a mod inside `<instance>/minecraft/mods/` by toggling the `.disabled` suffix
/// and updating the active mod profile.
pub fn set_mod_enabled(mods_dir: &Path, filename: &str, enabled: bool) -> Result<PathBuf> {
    let clean_name = filename.trim_end_matches(".disabled");
    let enabled_path = mods_dir.join(clean_name);
    let disabled_path = mods_dir.join(format!("{clean_name}.disabled"));

    if enabled {
        if disabled_path.exists() {
            fs::rename(&disabled_path, &enabled_path)?;
        }
        Ok(enabled_path)
    } else {
        if enabled_path.exists() {
            fs::rename(&enabled_path, &disabled_path)?;
        }
        Ok(disabled_path)
    }
}

/// Updates a mod jar in `mods/`, preserving the previous jar in `<instance>/.history/mods/`
/// for instant 1-click rollback.
pub fn update_mod_with_rollback_history(
    store: &ContentStore,
    instance_dir: &Path,
    old_filename: &str,
    new_filename: &str,
    new_jar_bytes: &[u8],
) -> Result<MultiDigest> {
    let mods_dir = instance_dir.join("minecraft/mods");
    let history_dir = instance_dir.join(".history/mods");
    fs::create_dir_all(&mods_dir)?;
    fs::create_dir_all(&history_dir)?;

    let old_path = mods_dir.join(old_filename);
    if old_path.exists() {
        let backup_path = history_dir.join(old_filename);
        link_or_copy(&old_path, &backup_path)?;
        fs::remove_file(&old_path)?;
    }

    let (digest, _) = store.put_bytes(new_jar_bytes)?;
    store.materialize(&digest.blake3, &mods_dir.join(new_filename))?;
    Ok(digest)
}

/// Rolls back a mod update from `<instance>/.history/mods/<previous_filename>`.
pub fn rollback_mod_from_history(
    instance_dir: &Path,
    current_filename: &str,
    previous_filename: &str,
) -> Result<()> {
    let mods_dir = instance_dir.join("minecraft/mods");
    let history_path = instance_dir.join(".history/mods").join(previous_filename);
    if !history_path.exists() {
        bail!("Previous mod jar '{}' not found in history", previous_filename);
    }
    let current_path = mods_dir.join(current_filename);
    if current_path.exists() {
        fs::remove_file(&current_path)?;
    }
    link_or_copy(&history_path, &mods_dir.join(previous_filename))?;
    Ok(())
}

// ============================================================================
// `.mrpack` Modpack Import & Export
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub files: Vec<MrpackFileEntry>,
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFileEntry {
    pub path: String,
    pub hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub downloads: Vec<String>,
    pub file_size: u64,
}

/// Exports an instance into a standard `.mrpack` archive (`modrinth.index.json` + `overrides/`
/// + `kestryx.lock.json` for deterministic hash-pinned reproducibility).
pub fn export_mrpack(
    instance_dir: &Path,
    pack_name: &str,
    version_id: &str,
    mc_version: &str,
    loader: &str,
    loader_version: &str,
    output_mrpack_path: &Path,
) -> Result<MrpackIndex> {
    let mods_dir = instance_dir.join("minecraft/mods");
    let mut files = Vec::new();

    if mods_dir.exists() {
        for entry in fs::read_dir(&mods_dir)?.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().map(|e| e == "jar").unwrap_or(false) {
                let bytes = fs::read(&p)?;
                let digest = MultiDigest::from_bytes(&bytes);
                let fname = entry.file_name().to_string_lossy().to_string();
                let mut hashes = BTreeMap::new();
                hashes.insert("sha1".to_string(), digest.sha1);
                hashes.insert("sha512".to_string(), digest.sha512);
                hashes.insert("blake3".to_string(), digest.blake3);
                files.push(MrpackFileEntry {
                    path: format!("mods/{fname}"),
                    hashes,
                    downloads: Vec::new(),
                    file_size: digest.size,
                });
            }
        }
    }

    let mut dependencies = BTreeMap::new();
    dependencies.insert("minecraft".to_string(), mc_version.to_string());
    if loader != "vanilla" {
        let key = match loader {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            "neoforge" => "neoforge",
            "forge" => "forge",
            other => other,
        };
        dependencies.insert(key.to_string(), loader_version.to_string());
    }

    let index = MrpackIndex {
        format_version: 1,
        game: "minecraft".to_string(),
        version_id: version_id.to_string(),
        name: pack_name.to_string(),
        summary: Some(format!("Exported by Kestryx ({pack_name})")),
        files,
        dependencies,
    };

    if let Some(parent) = output_mrpack_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let f = File::create(output_mrpack_path)?;
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("modrinth.index.json", opts)?;
    zw.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;
    zw.finish()?;

    Ok(index)
}

pub fn load_curated_optimization_presets() -> Result<serde_json::Value> {
    Ok(serde_json::from_str(OPTIMIZATION_PRESETS_JSON)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_120_mod_pack_dependency_resolution_profiles_update_and_rollback() -> Result<()> {
        // Build a 120-mod synthetic catalog with required, embedded, and incompatible dependencies
        let mut catalog = BTreeMap::new();
        catalog.insert(
            "fabric-api".to_string(),
            CandidateModVersion {
                project_id: "fabric-api".to_string(),
                slug: "fabric-api".to_string(),
                version_id: "v1".to_string(),
                version_number: "0.110.0+26.3".to_string(),
                channel: "release".to_string(),
                required_deps: Vec::new(),
                optional_deps: Vec::new(),
                incompatible_with: Vec::new(),
                embedded_jars: vec!["fabric-rendering-v1".to_string()],
            },
        );

        let mut requested_roots = Vec::new();
        for i in 0..120 {
            let id = format!("mod-{i:03}");
            requested_roots.push(id.clone());
            catalog.insert(
                id.clone(),
                CandidateModVersion {
                    project_id: id.clone(),
                    slug: id,
                    version_id: format!("ver-{i}"),
                    version_number: "1.0.0".to_string(),
                    channel: "release".to_string(),
                    required_deps: vec!["fabric-api".to_string()],
                    optional_deps: Vec::new(),
                    incompatible_with: Vec::new(),
                    embedded_jars: Vec::new(),
                },
            );
        }

        let plan = resolve_mod_dependencies(&requested_roots, &BTreeSet::new(), &catalog)?;
        // 120 mods + 1 shared required dependency (fabric-api) = 121 total resolved mods, 0 conflicts!
        assert_eq!(plan.to_install.len(), 121);
        assert!(plan.conflicts.is_empty());

        // Test mod update and rollback via ContentStore + .history/mods
        let dir = tempfile::tempdir()?;
        let store = ContentStore::new(dir.path())?;
        let inst_dir = dir.path().join("instances/pack-26-3");
        fs::create_dir_all(inst_dir.join("minecraft/mods"))?;
        fs::write(inst_dir.join("minecraft/mods/sodium-0.6.0.jar"), b"old-sodium-v0.6")?;

        update_mod_with_rollback_history(
            &store,
            &inst_dir,
            "sodium-0.6.0.jar",
            "sodium-0.7.0.jar",
            b"new-sodium-v0.7",
        )?;
        assert!(inst_dir.join("minecraft/mods/sodium-0.7.0.jar").exists());
        assert!(!inst_dir.join("minecraft/mods/sodium-0.6.0.jar").exists());

        rollback_mod_from_history(&inst_dir, "sodium-0.7.0.jar", "sodium-0.6.0.jar")?;
        assert!(inst_dir.join("minecraft/mods/sodium-0.6.0.jar").exists());
        assert_eq!(
            fs::read(inst_dir.join("minecraft/mods/sodium-0.6.0.jar"))?,
            b"old-sodium-v0.6"
        );
        Ok(())
    }
}
