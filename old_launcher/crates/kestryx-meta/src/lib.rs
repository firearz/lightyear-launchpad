//! `kestryx-meta`: Clean-room Mojang `version_manifest_v2.json` and version profile parser,
//! opaque `releaseTime` version ordering, era classifier (supporting `26.x` year.drop.hotfix,
//! `1.x`, `YYwWWx` snapshots, Beta, Alpha, Infdev/Indev/Classic, and April Fools),
//! `inheritsFrom` profile merger, and rule evaluation engine.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub const MOJANG_VERSION_MANIFEST_V2_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifestV2 {
    pub latest: LatestVersions,
    pub versions: Vec<VersionManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    pub release_time: String,
    pub sha1: Option<String>,
    #[serde(default)]
    pub compliance_level: u8,
}

/// Era classification for UI grouping and Java fallback selection.
/// Never assumes `1.x` numbering—first-class support for `26.1`, `26.3`, `26.4-snapshot-4`!
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionEra {
    /// Year.Drop.Hotfix scheme introduced in 2026 (e.g., `26.1`, `26.3`, `26.4-snapshot-4`, `26.3-rc-1`)
    YearDrop { year: u16, drop: u16, is_pre_or_snapshot: bool },
    /// Standard 1.x releases (`1.0` through `1.21.x`)
    Release1x { minor: u16 },
    /// Weekly snapshots (`24w14a`, `1.21.4-pre1`, `1.20-rc1`)
    LegacySnapshot,
    /// Beta (`b1.0` .. `b1.8.1`)
    OldBeta,
    /// Alpha (`a1.0.1_01` .. `a1.2.6`)
    OldAlpha,
    /// Classic, Indev, Infdev, Pre-Classic (`c0.30_01c`, `in-20100223`, `inf-20100618`, `rd-132211`)
    ClassicEra,
    /// Experimental / April Fools (`24w14potato`, `23w13a_or_b`, `3D Shareware v1.34`)
    ExperimentalOrJoke,
}

impl VersionEra {
    pub fn classify(id: &str, version_type: &str) -> Self {
        let lower = id.to_ascii_lowercase();
        if lower.contains("potato")
            || lower.contains("_or_")
            || lower.contains("shareware")
            || lower.contains("infinite")
            || lower.contains("craftmine")
            || lower.contains("rv-pre1")
        {
            return VersionEra::ExperimentalOrJoke;
        }
        if lower.starts_with("a1.") || version_type == "old_alpha" && lower.starts_with('a') {
            return VersionEra::OldAlpha;
        }
        if lower.starts_with("b1.") || version_type == "old_beta" {
            return VersionEra::OldBeta;
        }
        if lower.starts_with("rd-")
            || lower.starts_with("c0.")
            || lower.starts_with("in-")
            || lower.starts_with("inf-")
            || version_type == "old_alpha"
        {
            return VersionEra::ClassicEra;
        }

        // Check Year.Drop.Hotfix (`26.1`, `26.3`, `26.4-snapshot-4`, `26.3-rc-1`, `26.2-pre-1`)
        let parts: Vec<&str> = id.split(&['.', '-'][..]).collect();
        if parts.len() >= 2 {
            if let (Ok(first), Ok(second)) = (parts[0].parse::<u16>(), parts[1].parse::<u16>()) {
                if first >= 26 {
                    let is_pre = id.contains("snapshot")
                        || id.contains("-pre")
                        || id.contains("-rc")
                        || version_type != "release";
                    return VersionEra::YearDrop {
                        year: first,
                        drop: second,
                        is_pre_or_snapshot: is_pre,
                    };
                }
                if first == 1 {
                    if id.contains("-pre") || id.contains("-rc") || version_type == "snapshot" {
                        return VersionEra::LegacySnapshot;
                    }
                    return VersionEra::Release1x { minor: second };
                }
            }
        }

        VersionEra::LegacySnapshot
    }
}

/// Orders version manifest entries strictly by `releaseTime` descending (newest first),
/// breaking ties deterministically by opaque ID.
pub fn sort_versions_by_release_time(entries: &mut [VersionManifestEntry]) {
    entries.sort_by(|a, b| {
        let ta = DateTime::parse_from_rfc3339(&a.release_time)
            .map(|dt| dt.with_timezone(&Utc).timestamp())
            .unwrap_or(0);
        let tb = DateTime::parse_from_rfc3339(&b.release_time)
            .map(|dt| dt.with_timezone(&Utc).timestamp())
            .unwrap_or(0);
        tb.cmp(&ta).then_with(|| b.id.cmp(&a.id))
    });
}

// ============================================================================
// Version Profile Schema & `inheritsFrom` Merger
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VersionProfile {
    pub id: String,
    #[serde(rename = "type", default)]
    pub version_type: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub release_time: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub arguments: Option<ArgumentsSpec>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub downloads: BTreeMap<String, DownloadEntry>,
    #[serde(default)]
    pub java_version: Option<JavaVersionSpec>,
    #[serde(default)]
    pub libraries: Vec<LibraryEntry>,
    #[serde(default)]
    pub logging: Option<LoggingSpec>,
    #[serde(default)]
    pub world_version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArgumentsSpec {
    #[serde(default)]
    pub game: Vec<ArgumentNode>,
    #[serde(default)]
    pub jvm: Vec<ArgumentNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentNode {
    Simple(String),
    Conditional {
        rules: Vec<Rule>,
        value: StringOrVec,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrVec {
    pub fn as_slice(&self) -> Vec<&str> {
        match self {
            StringOrVec::Single(s) => vec![s.as_str()],
            StringOrVec::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: String, // "allow" | "disallow"
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub version: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(default)]
    pub total_size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadEntry {
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionSpec {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub natives: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub extract: Option<ExtractRule>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadEntry>,
    #[serde(default)]
    pub classifiers: BTreeMap<String, DownloadEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRule {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSpec {
    pub client: Option<LoggingClientSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingClientSpec {
    pub argument: String,
    pub file: DownloadEntry,
    #[serde(rename = "type")]
    pub log_type: String,
}

/// Asset Index file (`<assets>/indexes/<id>.json`), including legacy `virtual` and `map_to_resources`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetIndexFile {
    #[serde(rename = "virtual", default)]
    pub is_virtual: bool,
    #[serde(default)]
    pub map_to_resources: bool,
    pub objects: BTreeMap<String, AssetObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

// ============================================================================
// Rule Evaluation Context
// ============================================================================

#[derive(Debug, Clone)]
pub struct RuleContext {
    /// Mojang OS name: `"windows"`, `"osx"`, or `"linux"`
    pub os_name: String,
    pub os_version: String,
    /// Mojang arch name: `"x86"`, `"x86_64"`, or `"arm64"`
    pub os_arch: String,
    pub features: HashMap<String, bool>,
}

impl RuleContext {
    pub fn current_host() -> Self {
        let os_name = match std::env::consts::OS {
            "windows" => "windows",
            "macos" => "osx",
            _ => "linux",
        }
        .to_string();

        let os_arch = match std::env::consts::ARCH {
            "x86" => "x86",
            "aarch64" => "arm64",
            _ => "x86_64",
        }
        .to_string();

        Self {
            os_name,
            os_version: "10.0".to_string(),
            os_arch,
            features: HashMap::new(),
        }
    }

    pub fn evaluate_rules(&self, rules: &[Rule]) -> bool {
        if rules.is_empty() {
            return true;
        }
        // Mojang rule semantics: default disallow when rules are present, evaluate sequentially
        let mut allowed = false;
        for rule in rules {
            if self.rule_matches(rule) {
                allowed = rule.action == "allow";
            }
        }
        allowed
    }

    fn rule_matches(&self, rule: &Rule) -> bool {
        if let Some(ref os) = rule.os {
            if let Some(ref req_name) = os.name {
                if req_name != &self.os_name {
                    return false;
                }
            }
            if let Some(ref req_arch) = os.arch {
                // Treat x86_64 and amd64 or x86 checks carefully per Mojang launcher
                let arch_matches = if req_arch == "x86" {
                    self.os_arch == "x86"
                } else if req_arch == "arm64" || req_arch == "aarch64" {
                    self.os_arch == "arm64"
                } else {
                    req_arch == &self.os_arch
                };
                if !arch_matches {
                    return false;
                }
            }
            if let Some(ref ver_regex) = os.version {
                if let Ok(re) = Regex::new(ver_regex) {
                    if !re.is_match(&self.os_version) {
                        return false;
                    }
                }
            }
        }

        if let Some(ref req_features) = rule.features {
            for (feat_key, expected_val) in req_features {
                let actual_val = self.features.get(feat_key).copied().unwrap_or(false);
                if actual_val != *expected_val {
                    return false;
                }
            }
        }

        true
    }
}

/// Merges a child loader profile (e.g., Fabric/Quilt/Forge/NeoForge with `inheritsFrom`)
/// onto its resolved parent profile (`parent`).
/// Child libraries are placed BEFORE parent libraries so loader overrides win in classpath deduplication.
pub fn merge_inherited_profile(parent: &VersionProfile, child: &VersionProfile) -> VersionProfile {
    let mut merged = parent.clone();
    merged.id = child.id.clone();
    if !child.version_type.is_empty() {
        merged.version_type = child.version_type.clone();
    }
    if child.release_time.is_some() {
        merged.release_time = child.release_time.clone();
    }
    if child.main_class.is_some() {
        merged.main_class = child.main_class.clone();
    }
    if child.minecraft_arguments.is_some() {
        merged.minecraft_arguments = child.minecraft_arguments.clone();
    }
    if child.java_version.is_some() {
        merged.java_version = child.java_version.clone();
    }
    if child.asset_index.is_some() {
        merged.asset_index = child.asset_index.clone();
    }
    if child.assets.is_some() {
        merged.assets = child.assets.clone();
    }
    if child.logging.is_some() {
        merged.logging = child.logging.clone();
    }
    for (k, v) in &child.downloads {
        merged.downloads.insert(k.clone(), v.clone());
    }

    // Prepend child libraries so loader artifacts take precedence over vanilla libraries!
    let mut combined_libs = child.libraries.clone();
    combined_libs.extend(parent.libraries.clone());
    merged.libraries = combined_libs;

    // Merge arguments (`jvm` and `game`)
    match (&parent.arguments, &child.arguments) {
        (Some(p_args), Some(c_args)) => {
            let mut game = p_args.game.clone();
            game.extend(c_args.game.clone());
            let mut jvm = p_args.jvm.clone();
            jvm.extend(c_args.jvm.clone());
            merged.arguments = Some(ArgumentsSpec { game, jvm });
        }
        (None, Some(c_args)) => {
            merged.arguments = Some(c_args.clone());
        }
        _ => {}
    }

    merged.inherits_from = None;
    merged
}

/// Parses a Maven coordinate (`group:artifact:version[:classifier][@ext]`) into its relative repository path
/// and deduplication key `(group, artifact, classifier)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MavenCoordinate {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl MavenCoordinate {
    pub fn parse(raw: &str) -> Result<Self> {
        let (coord_part, ext) = match raw.split_once('@') {
            Some((c, e)) => (c, e.to_string()),
            None => (raw, "jar".to_string()),
        };
        let parts: Vec<&str> = coord_part.split(':').collect();
        if parts.len() < 3 {
            anyhow::bail!("Invalid Maven coordinate: {raw}");
        }
        let group = parts[0].to_string();
        let artifact = parts[1].to_string();
        let version = parts[2].to_string();
        let classifier = if parts.len() >= 4 && !parts[3].is_empty() {
            Some(parts[3].to_string())
        } else {
            None
        };

        Ok(Self {
            group,
            artifact,
            version,
            classifier,
            extension: ext,
        })
    }

    /// Key used to deduplicate libraries on the classpath (`group:artifact[:classifier]`).
    pub fn dedup_key(&self) -> (String, String, Option<String>) {
        (
            self.group.clone(),
            self.artifact.clone(),
            self.classifier.clone(),
        )
    }

    /// Relative path inside `<libraries>/` or a Maven repository URL.
    pub fn relative_path(&self) -> String {
        let group_path = self.group.replace('.', "/");
        let file_name = match &self.classifier {
            Some(c) => format!("{}-{}-{}.{}", self.artifact, self.version, c, self.extension),
            None => format!("{}-{}.{}", self.artifact, self.version, self.extension),
        };
        format!("{}/{}/{}/{}", group_path, self.artifact, self.version, file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_era_classification_across_all_eras() {
        assert_eq!(
            VersionEra::classify("26.3", "release"),
            VersionEra::YearDrop {
                year: 26,
                drop: 3,
                is_pre_or_snapshot: false
            }
        );
        assert_eq!(
            VersionEra::classify("26.4-snapshot-4", "snapshot"),
            VersionEra::YearDrop {
                year: 26,
                drop: 4,
                is_pre_or_snapshot: true
            }
        );
        assert_eq!(
            VersionEra::classify("1.21.4", "release"),
            VersionEra::Release1x { minor: 21 }
        );
        assert_eq!(
            VersionEra::classify("1.7.10", "release"),
            VersionEra::Release1x { minor: 7 }
        );
        assert_eq!(
            VersionEra::classify("b1.7.3", "old_beta"),
            VersionEra::OldBeta
        );
        assert_eq!(
            VersionEra::classify("a1.2.6", "old_alpha"),
            VersionEra::OldAlpha
        );
        assert_eq!(
            VersionEra::classify("c0.30_01c", "old_alpha"),
            VersionEra::ClassicEra
        );
        assert_eq!(
            VersionEra::classify("rd-132211", "old_alpha"),
            VersionEra::ClassicEra
        );
        assert_eq!(
            VersionEra::classify("24w14potato", "snapshot"),
            VersionEra::ExperimentalOrJoke
        );
    }

    #[test]
    fn test_rule_evaluation_os_arch_and_features() {
        let mut ctx = RuleContext {
            os_name: "osx".to_string(),
            os_version: "14.5".to_string(),
            os_arch: "arm64".to_string(),
            features: HashMap::new(),
        };
        ctx.features.insert("is_quick_play_singleplayer".to_string(), true);

        let mac_rule = vec![Rule {
            action: "allow".to_string(),
            os: Some(OsRule {
                name: Some("osx".to_string()),
                version: None,
                arch: None,
            }),
            features: None,
        }];
        assert!(ctx.evaluate_rules(&mac_rule));

        let win_rule = vec![Rule {
            action: "allow".to_string(),
            os: Some(OsRule {
                name: Some("windows".to_string()),
                version: None,
                arch: None,
            }),
            features: None,
        }];
        assert!(!ctx.evaluate_rules(&win_rule));

        let qp_rule = vec![Rule {
            action: "allow".to_string(),
            os: None,
            features: Some(BTreeMap::from([(
                "is_quick_play_singleplayer".to_string(),
                true,
            )])),
        }];
        assert!(ctx.evaluate_rules(&qp_rule));
    }

    #[test]
    fn test_maven_coordinate_parsing_and_paths() -> Result<()> {
        let coord = MavenCoordinate::parse("org.lwjgl:lwjgl-glfw:3.3.3:natives-windows-arm64")?;
        assert_eq!(
            coord.relative_path(),
            "org/lwjgl/lwjgl-glfw/3.3.3/lwjgl-glfw-3.3.3-natives-windows-arm64.jar"
        );
        assert_eq!(
            coord.dedup_key(),
            (
                "org.lwjgl".to_string(),
                "lwjgl-glfw".to_string(),
                Some("natives-windows-arm64".to_string())
            )
        );
        Ok(())
    }
}
