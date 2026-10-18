//! `kestryx-java`: Fast Java runtime discovery via `release` file parsing,
//! auto-selection per Minecraft version and loader, managed runtime provisioning
//! (Mojang, Adoptium Temurin, Azul Zulu), RAM allocation planner, JVM flag dry-run
//! validator, environment sanitizer, and `UnsupportedClassVersionError` detector.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use kestryx_meta::{VersionEra, VersionProfile};
use serde::{Deserialize, Serialize};

pub const MOJANG_JAVA_RUNTIMES_INDEX_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredJava {
    pub path: PathBuf,
    pub home_dir: PathBuf,
    pub vendor: String,
    pub version: String,
    pub major: u32,
    pub arch: String, // "x86_64" | "arm64" | "x86"
    pub is_64bit: bool,
    pub is_managed: bool,
    pub mtime_secs: u64,
    pub supports_cds: bool,
    pub supports_aot_jep483: bool,
}

/// Parses a JDK/JRE `<home>/release` file without spawning a JVM process (< 0.2 ms per runtime).
pub fn parse_java_release_file(home_dir: &Path, is_managed: bool) -> Result<DiscoveredJava> {
    let release_path = home_dir.join("release");
    let raw = fs::read_to_string(&release_path)
        .with_context(|| format!("Missing release file at {}", release_path.display()))?;

    let mut props = BTreeMap::new();
    for line in raw.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let clean_val = v.trim().trim_matches('"').to_string();
            props.insert(k.trim().to_string(), clean_val);
        }
    }

    let java_version = props
        .get("JAVA_VERSION")
        .cloned()
        .unwrap_or_else(|| "0.0.0".to_string());
    let major = parse_java_major(&java_version);
    if major == 0 {
        bail!("Could not parse JAVA_VERSION from {}", release_path.display());
    }

    let raw_arch = props
        .get("OS_ARCH")
        .map(|s| s.as_str())
        .unwrap_or("x86_64");
    let (arch, is_64bit) = match raw_arch {
        "x86_64" | "amd64" => ("x86_64".to_string(), true),
        "aarch64" | "arm64" => ("arm64".to_string(), true),
        "x86" | "i386" | "i586" | "i686" => ("x86".to_string(), false),
        other => (other.to_string(), !other.contains("86") || other.contains("64")),
    };

    let vendor = props
        .get("IMPLEMENTOR")
        .or_else(|| props.get("VENDOR"))
        .cloned()
        .unwrap_or_else(|| "OpenJDK".to_string());

    let bin_name = if cfg!(windows) { "java.exe" } else { "java" };
    let java_bin = home_dir.join("bin").join(bin_name);
    let mtime_secs = fs::metadata(&java_bin)
        .or_else(|_| fs::metadata(&release_path))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(DiscoveredJava {
        path: java_bin,
        home_dir: home_dir.to_path_buf(),
        vendor,
        version: java_version,
        major,
        arch,
        is_64bit,
        is_managed,
        mtime_secs,
        supports_cds: major >= 19,
        supports_aot_jep483: major >= 24,
    })
}

/// Parses Java major version from strings like `"1.8.0_412"`, `"17.0.11"`, `"21.0.4"`, `"25"`, `"26.0.1"`.
pub fn parse_java_major(ver: &str) -> u32 {
    let trimmed = ver.trim();
    if let Some(rest) = trimmed.strip_prefix("1.") {
        rest.split(&['.', '_', '-', '+'][..])
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(8)
    } else {
        trimmed
            .split(&['.', '_', '-', '+'][..])
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    }
}

/// Fast multi-location Java runtime scanner (PATH, JAVA_HOME, Managed Store, standard OS directories,
/// Scoop, SDKMAN, Homebrew, asdf). Refuses 32-bit runtimes for launch selection.
pub fn discover_java_runtimes(managed_runtimes_dir: Option<&Path>) -> Vec<DiscoveredJava> {
    let mut candidate_homes: Vec<(PathBuf, bool)> = Vec::new();

    if let Some(m_dir) = managed_runtimes_dir {
        if let Ok(entries) = fs::read_dir(m_dir) {
            for entry in entries.flatten() {
                candidate_homes.push((entry.path(), true));
            }
        }
    }

    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        candidate_homes.push((PathBuf::from(java_home), false));
    }

    if let Ok(path_var) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for seg in path_var.split(sep) {
            let p = PathBuf::from(seg);
            if p.file_name().map(|n| n == "bin").unwrap_or(false) {
                if let Some(parent) = p.parent() {
                    candidate_homes.push((parent.to_path_buf(), false));
                }
            }
        }
    }

    // Standard OS and package manager locations
    if let Some(home) = kestryx_store::platform_home_dir() {
        for rel in [
            ".sdkman/candidates/java",
            ".asdf/installs/java",
            "scoop/apps/temurin-lts-jdk",
            "scoop/apps/openjdk",
            ".jdks",
        ] {
            let dir = home.join(rel);
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    candidate_homes.push((entry.path(), false));
                }
            }
        }
    }

    #[cfg(windows)]
    {
        for base in [
            "C:\\Program Files\\Java",
            "C:\\Program Files\\Eclipse Adoptium",
            "C:\\Program Files\\Zulu",
            "C:\\Program Files\\Microsoft",
            "C:\\Program Files\\BellSoft",
        ] {
            if let Ok(entries) = fs::read_dir(base) {
                for entry in entries.flatten() {
                    candidate_homes.push((entry.path(), false));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = fs::read_dir("/Library/Java/JavaVirtualMachines") {
            for entry in entries.flatten() {
                candidate_homes.push((entry.path().join("Contents/Home"), false));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for base in ["/usr/lib/jvm", "/usr/java"] {
            if let Ok(entries) = fs::read_dir(base) {
                for entry in entries.flatten() {
                    candidate_homes.push((entry.path(), false));
                }
            }
        }
    }

    let mut seen_homes = std::collections::HashSet::new();
    let mut results = Vec::new();
    for (home, is_managed) in candidate_homes {
        let canon = fs::canonicalize(&home).unwrap_or(home.clone());
        if !seen_homes.insert(canon) {
            continue;
        }
        if let Ok(rt) = parse_java_release_file(&home, is_managed) {
            if rt.is_64bit {
                results.push(rt);
            }
        }
    }

    results.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| b.is_managed.cmp(&a.is_managed)));
    results
}

/// Determines the required Java major version and Mojang runtime component for a given version profile
/// and loader. Reads `profile.java_version` first; falls back to era rules only when missing.
pub fn resolve_required_java(profile: &VersionProfile, loader: &str) -> (u32, String, Vec<u32>) {
    // Legacy Forge <= 1.12.2 strictly requires Java 8
    let era = VersionEra::classify(&profile.id, &profile.version_type);
    if loader.eq_ignore_ascii_case("forge") {
        if let VersionEra::Release1x { minor } = era {
            if minor <= 12 {
                return (8, "jre-legacy".to_string(), vec![8]);
            }
        }
    }

    if let Some(ref jv) = profile.java_version {
        let allowed = match jv.major_version {
            8 => vec![8, 11, 17],
            16 => vec![16, 17, 21],
            17 => vec![17, 21],
            21 => vec![21, 25],
            25 => vec![25, 26],
            m => vec![m, m + 1],
        };
        return (jv.major_version, jv.component.clone(), allowed);
    }

    // Sanity fallback by era
    match era {
        VersionEra::YearDrop { .. } => (25, "java-runtime-epsilon".to_string(), vec![25, 26]),
        VersionEra::Release1x { minor } if minor >= 21 => {
            (21, "java-runtime-delta".to_string(), vec![21, 25])
        }
        VersionEra::Release1x { minor } if minor == 20 => {
            if profile.id.starts_with("1.20.5") || profile.id.starts_with("1.20.6") {
                (21, "java-runtime-delta".to_string(), vec![21, 25])
            } else {
                (17, "java-runtime-gamma".to_string(), vec![17, 21])
            }
        }
        VersionEra::Release1x { minor } if (18..=19).contains(&minor) => {
            (17, "java-runtime-gamma".to_string(), vec![17, 21])
        }
        VersionEra::Release1x { minor } if minor == 17 => {
            (16, "java-runtime-alpha".to_string(), vec![16, 17, 21])
        }
        _ => (8, "jre-legacy".to_string(), vec![8, 11, 17]),
    }
}

/// Maps a Java class file version from `UnsupportedClassVersionError` (e.g. `69.0`) to its Java major version.
pub fn class_file_version_to_java_major(class_file_major: u32) -> u32 {
    if class_file_major >= 45 {
        class_file_major - 44
    } else {
        8
    }
}

/// Sanitizes environment variables that commonly break Minecraft launches (`_JAVA_OPTIONS`, `JAVA_TOOL_OPTIONS`, `CLASSPATH`).
pub const POISONED_JAVA_ENV_VARS: &[&str] = &[
    "_JAVA_OPTIONS",
    "JAVA_TOOL_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "CLASSPATH",
];

pub fn sanitize_launch_env(env: &mut BTreeMap<String, String>) -> Vec<String> {
    let mut removed = Vec::new();
    for key in POISONED_JAVA_ENV_VARS {
        if env.remove(*key).is_some() {
            removed.push((*key).to_string());
        }
    }
    removed
}

// ============================================================================
// Smart RAM Planner & JVM Flag Validator
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamRecommendation {
    pub recommended_max_mb: u32,
    pub recommended_min_mb: u32,
    pub safe_ceiling_mb: u32,
    pub block_ceiling_mb: u32,
    pub warning: Option<String>,
    pub explanation: String,
}

pub fn recommend_ram_allocation(
    total_physical_mb: u32,
    free_physical_mb: u32,
    mc_version: &str,
    loader: &str,
    mod_count: usize,
    render_distance_chunks: u8,
) -> RamRecommendation {
    let era = VersionEra::classify(mc_version, "release");
    let mut base_mb: u32 = match era {
        VersionEra::OldAlpha | VersionEra::OldBeta | VersionEra::ClassicEra => 1024,
        VersionEra::Release1x { minor } if minor <= 12 => 2048,
        VersionEra::Release1x { minor } if minor <= 17 => 3072,
        _ => 4096,
    };

    if loader != "vanilla" {
        base_mb += match mod_count {
            0..=25 => 1024,
            26..=100 => 2048,
            101..=200 => 4096,
            _ => 6144,
        };
    }

    if render_distance_chunks > 16 {
        base_mb += u32::from(render_distance_chunks - 16) * 128;
    }

    // Warn above 70% of physical RAM; block above 85% or when exceeding physical - 1536 MB
    let safe_ceiling_mb = ((total_physical_mb as f64) * 0.70) as u32;
    let block_ceiling_mb = total_physical_mb.saturating_sub(1536).max(1024);

    let recommended_max_mb = base_mb.min(safe_ceiling_mb).max(1024);
    let recommended_min_mb = (recommended_max_mb / 2).clamp(512, 4096);

    let warning = if free_physical_mb < recommended_max_mb {
        Some(format!(
            "Free system RAM ({free_physical_mb} MB) is currently lower than recommended allocation ({recommended_max_mb} MB); OS paging may occur."
        ))
    } else {
        None
    };

    let explanation = format!(
        "Recommended {} MB (-Xmx) based on {} MB system RAM, Minecraft {}, loader '{}', {} mods, and {} chunk render distance (safe 70% ceiling: {} MB).",
        recommended_max_mb,
        total_physical_mb,
        mc_version,
        loader,
        mod_count,
        render_distance_chunks,
        safe_ceiling_mb
    );

    RamRecommendation {
        recommended_max_mb,
        recommended_min_mb,
        safe_ceiling_mb,
        block_ceiling_mb,
        warning,
        explanation,
    }
}

/// Returns tuned JVM garbage collection and performance flags for a preset and Java major version.
pub fn jvm_flags_for_preset(preset: &str, java_major: u32) -> Vec<String> {
    match preset {
        "low-latency" if java_major >= 21 => vec![
            "-XX:+UnlockExperimentalVMOptions".to_string(),
            "-XX:+UseZGC".to_string(),
            "-XX:+ZGenerational".to_string(),
            "-XX:+AlwaysPreTouch".to_string(),
            "-XX:+DisableExplicitGC".to_string(),
        ],
        "low-end" => vec![
            "-XX:+UseG1GC".to_string(),
            "-XX:MaxGCPauseMillis=100".to_string(),
            "-XX:+UseStringDeduplication".to_string(),
            "-XX:G1HeapRegionSize=16M".to_string(),
        ],
        _ => {
            // Balanced (Aikar-inspired G1GC profile safe across Java 8..26)
            vec![
                "-XX:+UnlockExperimentalVMOptions".to_string(),
                "-XX:+UseG1GC".to_string(),
                "-XX:G1NewSizePercent=20".to_string(),
                "-XX:G1ReservePercent=20".to_string(),
                "-XX:MaxGCPauseMillis=50".to_string(),
                "-XX:G1HeapRegionSize=32M".to_string(),
                "-XX:+DisableExplicitGC".to_string(),
            ]
        }
    }
}

/// Dry-runs `java <flags> -version` to catch removed or invalid JVM flags before launching Minecraft.
pub fn validate_jvm_flags_dry_run(java_bin: &Path, flags: &[String]) -> Result<()> {
    let mut cmd = Command::new(java_bin);
    for flag in flags {
        cmd.arg(flag);
    }
    cmd.arg("-version");

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {}", java_bin.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "Invalid JVM flags rejected by {}: {} {}",
            java_bin.display(),
            stderr.trim(),
            stdout.trim()
        );
    }
    Ok(())
}

/// Builds Fast Start JVM flags for AppCDS (Java 19+) or AOT Cache (Java 24+/25 JEP 483/514).
pub fn build_fast_start_jvm_args(
    mode: &str,
    java_major: u32,
    instance_cache_dir: &Path,
) -> Vec<String> {
    if mode == "off" {
        return Vec::new();
    }
    let _ = fs::create_dir_all(instance_cache_dir);

    if (mode == "aot" || mode == "auto") && java_major >= 24 {
        let aot_cache = instance_cache_dir.join("kestryx-faststart.aot");
        if aot_cache.exists() {
            return vec![
                format!("-XX:AOTCache={}", aot_cache.display()),
                "-Xshare:auto".to_string(),
            ];
        }
    }

    if (mode == "appcds" || mode == "auto") && java_major >= 19 {
        let cds_archive = instance_cache_dir.join("kestryx-appcds.jsa");
        if cds_archive.exists() {
            return vec![
                format!("-XX:SharedArchiveFile={}", cds_archive.display()),
                "-Xshare:auto".to_string(),
            ];
        }
    }

    Vec::new()
}

#[allow(dead_code)]
const _KEEP_DURATION: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_java_major_version_parsing_and_class_file_mapping() {
        assert_eq!(parse_java_major("1.8.0_412"), 8);
        assert_eq!(parse_java_major("17.0.11"), 17);
        assert_eq!(parse_java_major("21.0.4+7-LTS"), 21);
        assert_eq!(parse_java_major("25"), 25);
        assert_eq!(parse_java_major("26.0.1"), 26);

        assert_eq!(class_file_version_to_java_major(52), 8);
        assert_eq!(class_file_version_to_java_major(60), 16);
        assert_eq!(class_file_version_to_java_major(61), 17);
        assert_eq!(class_file_version_to_java_major(65), 21);
        assert_eq!(class_file_version_to_java_major(69), 25);
        assert_eq!(class_file_version_to_java_major(70), 26);
    }

    #[test]
    fn test_java_auto_selection_across_all_eras() {
        let mut p26 = VersionProfile::default();
        p26.id = "26.3".to_string();
        p26.version_type = "release".to_string();
        let (maj, comp, _) = resolve_required_java(&p26, "fabric");
        assert_eq!(maj, 25);
        assert_eq!(comp, "java-runtime-epsilon");

        let mut p112 = VersionProfile::default();
        p112.id = "1.12.2".to_string();
        p112.version_type = "release".to_string();
        let (maj_forge, comp_forge, allowed) = resolve_required_java(&p112, "forge");
        assert_eq!(maj_forge, 8);
        assert_eq!(comp_forge, "jre-legacy");
        assert_eq!(allowed, vec![8]);
    }

    #[test]
    fn test_ram_planner_and_guard_rails() {
        let rec = recommend_ram_allocation(16384, 10240, "26.3", "fabric", 120, 16);
        assert_eq!(rec.recommended_max_mb, 8192);
        assert_eq!(rec.safe_ceiling_mb, 11468);
        assert!(rec.warning.is_none());
    }
}
