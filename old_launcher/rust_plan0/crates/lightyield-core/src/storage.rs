use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherSettings {
    pub username: String,
    pub selected_version: String,
    pub version_filter: String,
    pub max_ram: String,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            username: "Player".to_string(),
            selected_version: "1.21.4".to_string(),
            version_filter: "release".to_string(),
            max_ram: "2G".to_string(),
        }
    }
}

impl LauncherSettings {
    pub fn load() -> Self {
        let path = get_settings_file();
        if path.exists() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(parsed) = serde_json::from_str::<LauncherSettings>(&raw) {
                    return parsed;
                }
            }
        }
        let default = Self::default();
        let _ = default.save();
        default
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = get_settings_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        fs::write(path, json)
    }
}

/// Returns the primary Light Yield Launcher data directory.
/// On Windows, this resolves to `%APPDATA%\LightYieldLauncher`.
/// On macOS/Linux, resolves to `~/.lightyield`.
pub fn get_launcher_dir() -> PathBuf {
    if let Some(mut appdata) = dirs::data_dir() {
        appdata.push("LightYieldLauncher");
        appdata
    } else if let Some(mut home) = dirs::home_dir() {
        home.push(".lightyield");
        home
    } else {
        PathBuf::from(".lightyield")
    }
}

pub fn get_settings_file() -> PathBuf {
    get_launcher_dir().join("settings.json")
}

pub fn get_versions_cache_file() -> PathBuf {
    get_launcher_dir().join("versions.txt")
}

pub fn get_versions_dir() -> PathBuf {
    get_launcher_dir().join("versions")
}

pub fn get_libraries_dir() -> PathBuf {
    get_launcher_dir().join("libraries")
}

pub fn get_assets_dir() -> PathBuf {
    get_launcher_dir().join("assets")
}

pub fn get_instances_dir() -> PathBuf {
    get_launcher_dir().join("instances")
}

pub fn get_runtimes_dir() -> PathBuf {
    get_launcher_dir().join("runtimes")
}

/// Returns the official `.minecraft` directory on the user's PC if it exists,
/// allowing us to reuse already-downloaded assets/libraries for instant installation.
pub fn get_system_minecraft_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::data_dir().map(|d| d.join(".minecraft")).filter(|p| p.exists())
    } else if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|h| h.join("Library/Application Support/minecraft"))
            .filter(|p| p.exists())
    } else {
        dirs::home_dir().map(|h| h.join(".minecraft")).filter(|p| p.exists())
    }
}

pub fn ensure_directories() -> std::io::Result<()> {
    fs::create_dir_all(get_launcher_dir())?;
    fs::create_dir_all(get_versions_dir())?;
    fs::create_dir_all(get_libraries_dir())?;
    fs::create_dir_all(get_assets_dir())?;
    fs::create_dir_all(get_instances_dir())?;
    fs::create_dir_all(get_runtimes_dir())?;
    Ok(())
}
