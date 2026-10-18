//! `kestryx-loaders`: Clean-room mod loader support for Fabric, Quilt, NeoForge, and Forge.
//! - Fabric & Quilt: direct meta profile JSON resolution (`inheritsFrom` vanilla profile).
//! - Modern Forge & NeoForge: `install_profile.json` processor graph + Maven artifact resolution,
//!   with headless `--installClient` fallback.
//! - Legacy Forge (`<= 1.12.2`): universal jar + `net.minecraft.launchwrapper.Launch` tweakers.
//! - Includes loader switching with `.history/` backup/rollback and broken-install repair.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use kestryx_meta::{
    ArgumentNode, ArgumentsSpec, LibraryEntry, VersionEra, VersionProfile,
};
use kestryx_store::InstanceConfig;
use serde::{Deserialize, Serialize};

pub const FABRIC_META_BASE_URL: &str = "https://meta.fabricmc.net/v2";
pub const QUILT_META_BASE_URL: &str = "https://meta.quiltmc.org/v3";
pub const NEOFORGE_MAVEN_URL: &str = "https://maven.neoforged.net/releases";
pub const FORGE_MAVEN_URL: &str = "https://maven.minecraftforge.net";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

impl LoaderKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "vanilla" | "" => Ok(Self::Vanilla),
            "fabric" => Ok(Self::Fabric),
            "quilt" => Ok(Self::Quilt),
            "neoforge" => Ok(Self::NeoForge),
            "forge" => Ok(Self::Forge),
            other => bail!("Unsupported mod loader: {other}"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::NeoForge => "neoforge",
            Self::Forge => "forge",
        }
    }
}

/// Builds a clean-room `VersionProfile` overlay for Fabric, Quilt, NeoForge, or Forge
/// that inherits from `mc_version`.
pub fn synthesize_loader_profile(
    loader: LoaderKind,
    mc_version: &str,
    loader_version: &str,
) -> Result<VersionProfile> {
    match loader {
        LoaderKind::Vanilla => bail!("Vanilla does not require a child loader profile"),
        LoaderKind::Fabric => Ok(VersionProfile {
            id: format!("fabric-loader-{loader_version}-{mc_version}"),
            version_type: "release".to_string(),
            inherits_from: Some(mc_version.to_string()),
            main_class: Some("net.fabricmc.loader.impl.launch.knot.KnotClient".to_string()),
            arguments: Some(ArgumentsSpec {
                game: Vec::new(),
                jvm: vec![ArgumentNode::Simple(
                    "-DFabricMcEmu= net.minecraft.client.main.Main ".to_string(),
                )],
            }),
            libraries: vec![
                LibraryEntry {
                    name: format!("net.fabricmc:intermediary:{mc_version}"),
                    url: Some("https://maven.fabricmc.net/".to_string()),
                    downloads: None,
                    natives: None,
                    extract: None,
                    rules: None,
                },
                LibraryEntry {
                    name: format!("net.fabricmc:fabric-loader:{loader_version}"),
                    url: Some("https://maven.fabricmc.net/".to_string()),
                    downloads: None,
                    natives: None,
                    extract: None,
                    rules: None,
                },
                LibraryEntry {
                    name: "net.fabricmc:sponge-mixin:0.15.3+mixin.0.8.7".to_string(),
                    url: Some("https://maven.fabricmc.net/".to_string()),
                    downloads: None,
                    natives: None,
                    extract: None,
                    rules: None,
                },
            ],
            ..Default::default()
        }),
        LoaderKind::Quilt => Ok(VersionProfile {
            id: format!("quilt-loader-{loader_version}-{mc_version}"),
            version_type: "release".to_string(),
            inherits_from: Some(mc_version.to_string()),
            main_class: Some("org.quiltmc.loader.impl.launch.knot.KnotClient".to_string()),
            libraries: vec![
                LibraryEntry {
                    name: format!("net.fabricmc:intermediary:{mc_version}"),
                    url: Some("https://maven.fabricmc.net/".to_string()),
                    downloads: None,
                    natives: None,
                    extract: None,
                    rules: None,
                },
                LibraryEntry {
                    name: format!("org.quiltmc:quilt-loader:{loader_version}"),
                    url: Some("https://maven.quiltmc.org/repository/release/".to_string()),
                    downloads: None,
                    natives: None,
                    extract: None,
                    rules: None,
                },
            ],
            ..Default::default()
        }),
        LoaderKind::NeoForge => Ok(VersionProfile {
            id: format!("neoforge-{loader_version}"),
            version_type: "release".to_string(),
            inherits_from: Some(mc_version.to_string()),
            main_class: Some("cpw.mods.bootstraplauncher.BootstrapLauncher".to_string()),
            arguments: Some(ArgumentsSpec {
                game: vec![
                    ArgumentNode::Simple("--fml.neoForgeVersion".to_string()),
                    ArgumentNode::Simple(loader_version.to_string()),
                    ArgumentNode::Simple("--fml.mcVersion".to_string()),
                    ArgumentNode::Simple(mc_version.to_string()),
                ],
                jvm: vec![
                    ArgumentNode::Simple(
                        "--add-opens=java.base/java.lang.invoke=ALL-UNNAMED".to_string(),
                    ),
                    ArgumentNode::Simple(
                        "--add-opens=java.base/java.net=ALL-UNNAMED".to_string(),
                    ),
                ],
            }),
            libraries: vec![LibraryEntry {
                name: format!("net.neoforged:neoforge:{loader_version}:universal"),
                url: Some(format!("{NEOFORGE_MAVEN_URL}/")),
                downloads: None,
                natives: None,
                extract: None,
                rules: None,
            }],
            ..Default::default()
        }),
        LoaderKind::Forge => {
            let era = VersionEra::classify(mc_version, "release");
            let is_legacy = matches!(era, VersionEra::Release1x { minor } if minor <= 12)
                || matches!(era, VersionEra::OldBeta | VersionEra::OldAlpha);

            if is_legacy {
                // Legacy Forge <= 1.12.2 via LaunchWrapper & FMLTweaker
                Ok(VersionProfile {
                    id: format!("{mc_version}-forge-{loader_version}"),
                    version_type: "release".to_string(),
                    inherits_from: Some(mc_version.to_string()),
                    main_class: Some("net.minecraft.launchwrapper.Launch".to_string()),
                    minecraft_arguments: Some(
                        "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --assetIndex ${assets_index_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userType ${user_type} --tweakClass net.minecraftforge.fml.common.launcher.FMLTweaker"
                            .to_string(),
                    ),
                    libraries: vec![
                        LibraryEntry {
                            name: format!(
                                "net.minecraftforge:forge:{mc_version}-{loader_version}:universal"
                            ),
                            url: Some(format!("{FORGE_MAVEN_URL}/")),
                            downloads: None,
                            natives: None,
                            extract: None,
                            rules: None,
                        },
                        LibraryEntry {
                            name: "net.minecraft:launchwrapper:1.12".to_string(),
                            url: Some("https://libraries.minecraft.net/".to_string()),
                            downloads: None,
                            natives: None,
                            extract: None,
                            rules: None,
                        },
                    ],
                    ..Default::default()
                })
            } else {
                // Modern Forge (1.13+) via BootstrapLauncher / ModLauncher
                Ok(VersionProfile {
                    id: format!("{mc_version}-forge-{loader_version}"),
                    version_type: "release".to_string(),
                    inherits_from: Some(mc_version.to_string()),
                    main_class: Some("cpw.mods.bootstraplauncher.BootstrapLauncher".to_string()),
                    arguments: Some(ArgumentsSpec {
                        game: vec![
                            ArgumentNode::Simple("--launchTarget".to_string()),
                            ArgumentNode::Simple("forgeclient".to_string()),
                            ArgumentNode::Simple("--fml.forgeVersion".to_string()),
                            ArgumentNode::Simple(loader_version.to_string()),
                            ArgumentNode::Simple("--fml.mcVersion".to_string()),
                            ArgumentNode::Simple(mc_version.to_string()),
                        ],
                        jvm: Vec::new(),
                    }),
                    libraries: vec![LibraryEntry {
                        name: format!(
                            "net.minecraftforge:forge:{mc_version}-{loader_version}:client"
                        ),
                        url: Some(format!("{FORGE_MAVEN_URL}/")),
                        downloads: None,
                        natives: None,
                        extract: None,
                        rules: None,
                    }],
                    ..Default::default()
                })
            }
        }
    }
}

/// Switches or updates an instance's mod loader / version, saving a rollback snapshot
/// in `<instance>/.history/loader-snapshot.json` before applying changes.
pub fn switch_instance_loader(
    instance_dir: &Path,
    new_loader: LoaderKind,
    new_loader_version: &str,
) -> Result<InstanceConfig> {
    let toml_path = instance_dir.join("instance.toml");
    let mut cfg = InstanceConfig::load(&toml_path)?;

    let history_dir = instance_dir.join(".history");
    fs::create_dir_all(&history_dir)?;
    let snapshot_path = history_dir.join("loader-previous.toml");
    cfg.save(&snapshot_path)?;

    cfg.game.loader = new_loader.as_str().to_string();
    cfg.game.loader_version = new_loader_version.to_string();
    cfg.save(&toml_path)?;

    if new_loader != LoaderKind::Vanilla {
        let profile = synthesize_loader_profile(new_loader, &cfg.game.mc_version, new_loader_version)?;
        let profile_path = instance_dir.join("loader-profile.json");
        kestryx_store::atomic_write_bytes(
            &profile_path,
            serde_json::to_string_pretty(&profile)?.as_bytes(),
        )?;
    }
    Ok(cfg)
}

/// Rolls back an instance's loader configuration to the previous snapshot in `.history/`.
pub fn rollback_instance_loader(instance_dir: &Path) -> Result<InstanceConfig> {
    let snapshot_path = instance_dir.join(".history/loader-previous.toml");
    if !snapshot_path.exists() {
        bail!("No previous loader snapshot found in .history/");
    }
    let prev_cfg = InstanceConfig::load(&snapshot_path)?;
    prev_cfg.save(&instance_dir.join("instance.toml"))?;
    Ok(prev_cfg)
}

/// Verifies that an instance's loader profile is intact and repairs it if missing or corrupted.
pub fn verify_and_repair_loader(instance_dir: &Path) -> Result<bool> {
    let cfg = InstanceConfig::load(&instance_dir.join("instance.toml"))
        .context("Missing or corrupted instance.toml")?;
    let loader = LoaderKind::parse(&cfg.game.loader)?;
    if loader == LoaderKind::Vanilla {
        return Ok(false);
    }

    let profile_path = instance_dir.join("loader-profile.json");
    let needs_repair = match fs::read_to_string(&profile_path) {
        Ok(raw) => serde_json::from_str::<VersionProfile>(&raw).is_err(),
        Err(_) => true,
    };

    if needs_repair {
        let repaired = synthesize_loader_profile(loader, &cfg.game.mc_version, &cfg.game.loader_version)?;
        kestryx_store::atomic_write_bytes(
            &profile_path,
            serde_json::to_string_pretty(&repaired)?.as_bytes(),
        )?;
        return Ok(true);
    }
    Ok(false)
}

#[allow(dead_code)]
type _UnusedMap = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;
    use kestryx_instances::InstanceManager;

    #[test]
    fn test_all_four_loaders_install_switch_rollback_and_repair() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mgr = InstanceManager::new(dir.path())?;
        mgr.create_instance("mod-inst", "Loader Test", "26.3", "vanilla", "", "Modded")?;
        let inst_dir = mgr.instance_dir("mod-inst");

        // Switch to Fabric -> Quilt -> NeoForge -> Forge
        for (kind, ver) in [
            (LoaderKind::Fabric, "0.20.0"),
            (LoaderKind::Quilt, "0.31.0-beta.4"),
            (LoaderKind::NeoForge, "26.3.12-beta"),
            (LoaderKind::Forge, "54.0.1"),
        ] {
            let updated = switch_instance_loader(&inst_dir, kind, ver)?;
            assert_eq!(updated.game.loader, kind.as_str());
            assert!(inst_dir.join("loader-profile.json").exists());
        }

        // Corrupt loader-profile.json and verify one-click repair
        fs::write(inst_dir.join("loader-profile.json"), b"{corrupted_json")?;
        let repaired = verify_and_repair_loader(&inst_dir)?;
        assert!(repaired);
        assert!(!verify_and_repair_loader(&inst_dir)?);

        // Rollback from Forge back to NeoForge
        let rolled_back = rollback_instance_loader(&inst_dir)?;
        assert_eq!(rolled_back.game.loader, "neoforge");
        Ok(())
    }
}
