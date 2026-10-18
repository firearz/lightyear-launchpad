//! Minecraft launch pipeline, classpath builder, argument composer, and process supervisor.
//! Supports all Minecraft eras (Alpha, Beta, 1.0–1.12.2 LaunchWrapper, 1.13–1.20, 1.21+, 26.x).

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use anyhow::{Context, Result};

use crate::account::LocalAccount;
use crate::installer::{current_os_name, InstalledVersion};
use crate::java::JavaRuntime;
use crate::storage::{get_assets_dir, get_instances_dir};

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub max_ram: String,
    pub min_ram: String,
    pub game_dir: Option<PathBuf>,
    pub custom_jvm_args: Vec<String>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            max_ram: "2G".to_string(),
            min_ram: "512M".to_string(),
            game_dir: None,
            custom_jvm_args: Vec::new(),
        }
    }
}

pub struct LaunchCommand {
    pub java_path: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub working_dir: PathBuf,
}

/// Builds the complete launch command with classpath and substituted arguments for any Minecraft version.
pub fn build_launch_command(
    installed: &InstalledVersion,
    account: &LocalAccount,
    java: &JavaRuntime,
    config: &LaunchConfig,
) -> Result<LaunchCommand> {
    let version_id = &installed.version_id;
    let game_dir = config
        .game_dir
        .clone()
        .unwrap_or_else(|| get_instances_dir().join(version_id));
    fs::create_dir_all(&game_dir)?;

    let assets_dir = get_assets_dir();
    let natives_dir = &installed.natives_dir;

    // 1. Classpath (Windows: separated by ';', Unix: separated by ':')
    let cp_sep = if cfg!(windows) { ";" } else { ":" };
    let mut cp_entries = Vec::new();
    for jar in &installed.classpath_jars {
        if jar.exists() {
            cp_entries.push(jar.to_string_lossy().to_string());
        }
    }
    cp_entries.push(installed.client_jar_path.to_string_lossy().to_string());
    let full_classpath = cp_entries.join(cp_sep);

    // 2. Variable replacement map covering every Minecraft era (2010 Alpha -> 2026+)
    let asset_index_id = installed
        .package
        .asset_index
        .as_ref()
        .map(|a| a.id.as_str())
        .or(installed.package.assets.as_deref())
        .unwrap_or("legacy");

    let game_assets_dir = installed
        .virtual_assets_dir
        .clone()
        .unwrap_or_else(|| assets_dir.join("virtual").join(asset_index_id));

    let version_type = installed
        .package
        .version_type
        .clone()
        .unwrap_or_else(|| "release".to_string());

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("${auth_player_name}", account.username.clone());
    vars.insert("${version_name}", version_id.clone());
    vars.insert("${game_directory}", game_dir.to_string_lossy().to_string());
    vars.insert("${assets_root}", assets_dir.to_string_lossy().to_string());
    vars.insert("${game_assets}", game_assets_dir.to_string_lossy().to_string());
    vars.insert("${assets_index_name}", asset_index_id.to_string());
    vars.insert("${auth_uuid}", account.uuid_dashed());
    vars.insert("${auth_access_token}", account.access_token.clone());
    vars.insert(
        "${auth_session}",
        format!("token:{}:{}", account.access_token, account.uuid_undashed()),
    );
    vars.insert("${clientid}", "0".to_string());
    vars.insert("${auth_xuid}", "0".to_string());
    vars.insert("${user_type}", account.user_type.clone());
    vars.insert("${version_type}", version_type);
    vars.insert("${user_properties}", "{}".to_string());
    vars.insert("${natives_directory}", natives_dir.to_string_lossy().to_string());
    vars.insert("${launcher_name}", "LightYieldLauncher".to_string());
    vars.insert("${launcher_version}", "0.1.0".to_string());
    vars.insert("${classpath}", full_classpath.clone());

    // 3. JVM Arguments
    let mut jvm_args = Vec::new();
    if cfg!(target_os = "macos") {
        jvm_args.push("-XstartOnFirstThread".to_string());
    }
    if java.major_version >= 21 {
        jvm_args.push("--enable-native-access=ALL-UNNAMED".to_string());
    }

    jvm_args.push(format!("-Djava.library.path={}", natives_dir.display()));
    jvm_args.push(format!("-Djna.tmpdir={}", natives_dir.display()));
    jvm_args.push(format!("-Dorg.lwjgl.librarypath={}", natives_dir.display()));
    jvm_args.push(format!("-Dminecraft.applet.TargetDirectory={}", game_dir.display()));
    jvm_args.push(format!("-Xms{}", config.min_ram));
    jvm_args.push(format!("-Xmx{}", config.max_ram));

    // Modern JVM arguments from version JSON
    if let Some(arguments) = &installed.package.arguments {
        for arg in &arguments.jvm {
            parse_argument_value(arg, &vars, &mut jvm_args);
        }
    }

    // Custom JVM args
    for custom in &config.custom_jvm_args {
        jvm_args.push(custom.clone());
    }

    // Set classpath if not already passed via arguments.jvm
    if !jvm_args.iter().any(|a| a == "-cp" || a == "-classpath") {
        jvm_args.push("-cp".to_string());
        jvm_args.push(full_classpath.clone());
    }

    // Protect against Windows 32KB CreateProcess command-line limit on Java 9+ by writing @argfile if needed
    if java.major_version >= 9 && full_classpath.len() > 6000 {
        if let Some(cp_pos) = jvm_args.iter().position(|a| a == "-cp" || a == "-classpath") {
            if cp_pos + 1 < jvm_args.len() {
                let argfile_path = game_dir.join("lightyield_cp.args");
                let escaped_cp = jvm_args[cp_pos + 1].replace('\\', "\\\\").replace('"', "\\\"");
                let argfile_content = format!("-cp\n\"{}\"\n", escaped_cp);
                if fs::write(&argfile_path, argfile_content).is_ok() {
                    jvm_args.remove(cp_pos + 1);
                    jvm_args[cp_pos] = format!("@{}", argfile_path.display());
                }
            }
        }
    }

    // 4. Game Arguments
    let mut game_args = Vec::new();
    if let Some(arguments) = &installed.package.arguments {
        for arg in &arguments.game {
            parse_argument_value(arg, &vars, &mut game_args);
        }
    } else if let Some(legacy_args) = &installed.package.minecraft_arguments {
        for token in legacy_args.split_whitespace() {
            let replaced = substitute_variables(token, &vars);
            game_args.push(replaced);
        }
    }

    Ok(LaunchCommand {
        java_path: java.path.clone(),
        jvm_args,
        main_class: installed.package.main_class.clone(),
        game_args,
        working_dir: game_dir,
    })
}

/// Spawns the Minecraft JVM process and supervises execution in real time.
pub fn launch_and_supervise(cmd: &LaunchCommand) -> Result<()> {
    println!("\n============================================================");
    println!("  Spawning Minecraft: Java Edition");
    println!("  Java Runtime: {}", cmd.java_path.display());
    println!("  Main Class:   {}", cmd.main_class);
    println!("  Game Root:    {}", cmd.working_dir.display());
    println!("============================================================\n");

    let mut process_cmd = Command::new(&cmd.java_path);
    process_cmd.current_dir(&cmd.working_dir);
    process_cmd.args(&cmd.jvm_args);
    process_cmd.arg(&cmd.main_class);
    process_cmd.args(&cmd.game_args);

    process_cmd.stdout(Stdio::piped());
    process_cmd.stderr(Stdio::piped());

    let mut child = process_cmd
        .spawn()
        .with_context(|| format!("Failed to spawn Java process at {}", cmd.java_path.display()))?;

    let pid = child.id();
    println!("[INFO] Minecraft process started successfully (PID: {pid})!\n");

    let stdout = child.stdout.take();
    let t_out = std::thread::spawn(move || {
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                println!("[MC/stdout] {line}");
            }
        }
    });

    let stderr = child.stderr.take();
    let t_err = std::thread::spawn(move || {
        let mut err_lines = Vec::new();
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[MC/stderr] {line}");
                err_lines.push(line);
                if err_lines.len() > 30 {
                    err_lines.remove(0);
                }
            }
        }
        err_lines
    });

    let status = child.wait()?;
    let _ = t_out.join();
    let err_lines = t_err.join().unwrap_or_default();

    println!("\n[INFO] Minecraft process exited with: {status}");
    if !status.success() {
        eprintln!("\n============================================================");
        eprintln!("  [CRASH DETECTED] Process exited with error code!");
        eprintln!("============================================================");
        if !err_lines.is_empty() {
            eprintln!("Recent error output:");
            for l in &err_lines {
                eprintln!("  > {l}");
            }
        }
        let logs_file = cmd.working_dir.join("logs").join("latest.log");
        if logs_file.exists() {
            eprintln!("\nCheck log file for full trace: {}", logs_file.display());
        }
    }

    Ok(())
}

fn parse_argument_value(arg: &serde_json::Value, vars: &HashMap<&str, String>, out: &mut Vec<String>) {
    match arg {
        serde_json::Value::String(s) => {
            out.push(substitute_variables(s, vars));
        }
        serde_json::Value::Object(obj) => {
            // Skip feature-gated demo/quickPlay arguments unless explicitly configured
            if obj.get("rules").is_some() {
                if let Some(rules_val) = obj.get("rules").and_then(|r| r.as_array()) {
                    let current_os = current_os_name();
                    let mut allowed = false;
                    for r in rules_val {
                        if r.get("features").is_some() {
                            return;
                        }
                        let action = r.get("action").and_then(|a| a.as_str()).unwrap_or("allow");
                        let os_match = match r.get("os") {
                            Some(os_obj) => {
                                let name_match = os_obj
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .map(|n| n == current_os)
                                    .unwrap_or(true);
                                let arch_match = os_obj
                                    .get("arch")
                                    .and_then(|a| a.as_str())
                                    .map(|a| {
                                        if cfg!(target_arch = "x86_64") {
                                            a != "x86"
                                        } else {
                                            true
                                        }
                                    })
                                    .unwrap_or(true);
                                name_match && arch_match
                            }
                            None => true,
                        };
                        if os_match {
                            allowed = action == "allow";
                        }
                    }
                    if !allowed {
                        return;
                    }
                }
            }

            if let Some(val) = obj.get("value") {
                match val {
                    serde_json::Value::String(s) => out.push(substitute_variables(s, vars)),
                    serde_json::Value::Array(arr) => {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                out.push(substitute_variables(s, vars));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn substitute_variables(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, val) in vars {
        if result.contains(key) {
            result = result.replace(key, val);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_variables() {
        let mut vars = HashMap::new();
        vars.insert("${auth_player_name}", "Steve".into());
        vars.insert("${version_name}", "1.21.4".into());

        let res = substitute_variables("--username ${auth_player_name} --version ${version_name}", &vars);
        assert_eq!(res, "--username Steve --version 1.21.4");
    }
}
