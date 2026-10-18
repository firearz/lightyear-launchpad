//! `kestryx-launch`: Complete Minecraft launch pipeline:
//! 1. Resolve version JSON (`inheritsFrom` merge with loader profile)
//! 2. Evaluate rules (`os.name`, `os.version`, `os.arch`, `features`)
//! 3. Resolve libraries & native classifiers (`${arch}`, `extract.exclude`, ARM64 LWJGL substitution)
//! 4. Deduplicate classpath by `group:artifact:classifier` (loader wins)
//! 5. Stage modern and legacy (`virtual`, `map_to_resources`) assets
//! 6. Extract per-launch natives (skipped when library-set BLAKE3 hash is unchanged)
//! 7. Substitute JVM & game arguments (both `arguments` and legacy `minecraftArguments`)
//! 8. Apply Log4j mitigations, macOS `-XstartOnFirstThread`, Fast Start (AppCDS/AOT), and `@argfiles`
//! 9. Spawn & supervise with pre/post hooks, Startup Profiler (time to title screen), and Crash Analyzer.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use kestryx_auth::{AccountProfile, SecretRedactor};
use kestryx_instances::{QuickPlayRequest, QuickPlayResolution, build_quick_play_args};
use kestryx_java::{build_fast_start_jvm_args, jvm_flags_for_preset, sanitize_launch_env};
use kestryx_meta::{
    ArgumentNode, AssetIndexFile, MavenCoordinate, RuleContext, VersionEra, VersionProfile,
};
use kestryx_store::{InstanceConfig, link_or_copy};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub const ARGFILE_THRESHOLD_CHARS: usize = 7000;

pub mod installer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedLaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub redacted_args_display: String,
    pub working_dir: PathBuf,
    pub env: BTreeMap<String, String>,
    pub classpath_entries: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub used_argfile: Option<PathBuf>,
    pub library_set_blake3: String,
    pub natives_extraction_skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSupervisionResult {
    pub exit_code: i32,
    pub time_to_spawn_ms: u64,
    pub time_to_title_screen_ms: Option<u64>,
    pub playtime_secs: u64,
    pub log_tail: Vec<String>,
}

/// Resolves and deduplicates the library classpath (`group:artifact:classifier`, first/loader wins)
/// and collects any native jars needing extraction for the target `RuleContext`.
pub fn resolve_libraries_and_natives(
    profile: &VersionProfile,
    ctx: &RuleContext,
    libraries_root: &Path,
    client_jar_path: &Path,
) -> Result<(Vec<PathBuf>, Vec<(PathBuf, Vec<String>)>, String)> {
    let mut seen_coords: HashSet<(String, String, Option<String>)> = HashSet::new();
    let mut classpath = Vec::new();
    let mut native_jars: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut hash_input = blake3::Hasher::new();

    let arch_bits = if ctx.os_arch == "x86" { "32" } else { "64" };

    for lib in &profile.libraries {
        if let Some(ref rules) = lib.rules {
            if !ctx.evaluate_rules(rules) {
                continue;
            }
        }

        let mut coord = MavenCoordinate::parse(&lib.name)?;

        // Apple Silicon / ARM64 LWJGL 3 native substitution for versions lacking arm64 classifiers
        if ctx.os_arch == "arm64" && coord.group == "org.lwjgl" {
            if let Some(ref mut c) = coord.classifier {
                if c == "natives-macos" {
                    *c = "natives-macos-arm64".to_string();
                } else if c == "natives-windows" {
                    *c = "natives-windows-arm64".to_string();
                } else if c == "natives-linux" {
                    *c = "natives-linux-arm64".to_string();
                }
            }
        }

        // Check if this entry has a legacy `natives` map (e.g. `natives: { "windows": "natives-windows-${arch}" }`)
        if let Some(ref natives_map) = lib.natives {
            if let Some(classifier_tmpl) = natives_map.get(&ctx.os_name) {
                let resolved_classifier = classifier_tmpl.replace("${arch}", arch_bits);
                let mut native_coord = coord.clone();
                native_coord.classifier = Some(resolved_classifier.clone());

                let rel_path = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.get(&resolved_classifier))
                    .and_then(|e| e.path.clone())
                    .unwrap_or_else(|| native_coord.relative_path());

                let full_native_path = libraries_root.join(&rel_path);
                let excludes = lib
                    .extract
                    .as_ref()
                    .map(|ex| ex.exclude.clone())
                    .unwrap_or_else(|| vec!["META-INF/".to_string()]);
                hash_input.update(rel_path.as_bytes());
                native_jars.push((full_native_path, excludes));
            }
        }

        // Deduplicate by `(group, artifact, classifier)` — loader entries appear first and win!
        let dedup_key = coord.dedup_key();
        if !seen_coords.insert(dedup_key) {
            continue;
        }

        let rel_path = lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref())
            .and_then(|a| a.path.clone())
            .unwrap_or_else(|| coord.relative_path());

        hash_input.update(rel_path.as_bytes());
        let jar_path = libraries_root.join(rel_path);

        // Modern LWJGL 3.3+ includes `:natives-*` as regular library artifacts with extract rules or JNI auto-loading
        if coord
            .classifier
            .as_deref()
            .map(|c| c.starts_with("natives-"))
            .unwrap_or(false)
        {
            let excludes = lib
                .extract
                .as_ref()
                .map(|ex| ex.exclude.clone())
                .unwrap_or_else(|| vec!["META-INF/".to_string()]);
            native_jars.push((jar_path.clone(), excludes));
        }

        classpath.push(jar_path);
    }

    classpath.push(client_jar_path.to_path_buf());
    let lib_set_hash = hash_input.finalize().to_hex().to_string();
    Ok((classpath, native_jars, lib_set_hash))
}

/// Extracts native shared libraries (`.dll`, `.dylib`, `.so`, `.jnilib`) into `<instance>/.natives/`,
/// skipping extraction altogether when `.natives/.hash` matches `library_set_blake3`.
pub fn ensure_natives_extracted(
    natives_dir: &Path,
    native_jars: &[(PathBuf, Vec<String>)],
    library_set_blake3: &str,
) -> Result<bool> {
    fs::create_dir_all(natives_dir)?;
    let hash_marker = natives_dir.join(".hash");
    if let Ok(existing_hash) = fs::read_to_string(&hash_marker) {
        if existing_hash.trim() == library_set_blake3 {
            return Ok(true); // Skipped extraction (fast path!)
        }
    }

    for (jar_path, excludes) in native_jars {
        if !jar_path.exists() {
            continue;
        }
        let f = fs::File::open(jar_path)?;
        let Ok(mut zip) = zip::ZipArchive::new(f) else {
            continue;
        };
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name()?.to_string();
            if entry.is_dir() || excludes.iter().any(|ex| name.starts_with(ex)) {
                continue;
            }
            let Some(safe_path) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
                continue;
            };
            let out_path = natives_dir.join(safe_path);
            if let Some(p) = out_path.parent() {
                fs::create_dir_all(p)?;
            }
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            kestryx_store::atomic_write_bytes(&out_path, &buf)?;
            if let Some(file_name) = out_path.file_name() {
                let flat_path = natives_dir.join(file_name);
                if !flat_path.exists() {
                    let _ = kestryx_store::atomic_write_bytes(&flat_path, &buf);
                }
            }
        }
    }

    kestryx_store::atomic_write_bytes(&hash_marker, library_set_blake3.as_bytes())?;
    Ok(false)
}

/// Stages legacy `virtual` (`<assets>/virtual/legacy/<virtual_path>`) or `map_to_resources`
/// (`<game_dir>/resources/<virtual_path>`) assets for pre-1.7.3 / Alpha / Beta versions.
pub fn stage_legacy_virtual_assets(
    asset_index_id: &str,
    index_file: &AssetIndexFile,
    assets_root: &Path,
    game_dir: &Path,
) -> Result<PathBuf> {
    if !index_file.is_virtual && !index_file.map_to_resources {
        return Ok(assets_root.to_path_buf());
    }

    let target_base = if index_file.map_to_resources {
        game_dir.join("resources")
    } else {
        assets_root.join("virtual").join(asset_index_id)
    };
    fs::create_dir_all(&target_base)?;

    for (rel_path, obj) in &index_file.objects {
        if obj.hash.len() < 2 {
            continue;
        }
        let prefix = &obj.hash[..2];
        let src = assets_root.join("objects").join(prefix).join(&obj.hash);
        let dst = target_base.join(rel_path);
        if src.exists() && !dst.exists() {
            let _ = link_or_copy(&src, &dst);
        }
    }

    Ok(target_base)
}

/// Builds the complete, deterministic launch command for any Minecraft version (Alpha through `26.x`)
/// across any OS and CPU architecture.
pub fn build_launch_command(
    profile: &VersionProfile,
    instance: &InstanceConfig,
    account: &AccountProfile,
    access_token: &str,
    java_bin: &Path,
    java_major: u32,
    data_root: &Path,
    instance_dir: &Path,
    game_dir: &Path,
    mut rule_ctx: RuleContext,
    quick_play: Option<&QuickPlayRequest>,
) -> Result<PreparedLaunchCommand> {
    let libraries_root = data_root.join("store/libraries");
    let assets_root = data_root.join("store/assets");
    let natives_dir = instance_dir.join(".natives");
    let client_jar_candidates = [
        data_root.join("store/versions").join(&profile.id).join("client.jar"),
        data_root.join("store/versions").join(&profile.id).join(format!("{}.jar", profile.id)),
        libraries_root.join("net/minecraft/client").join(&profile.id).join(format!("{}.jar", profile.id)),
    ];
    let client_jar = client_jar_candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| client_jar_candidates[0].clone());

    if instance.process.window_width > 0 && instance.process.window_height > 0 {
        rule_ctx
            .features
            .insert("has_custom_resolution".to_string(), true);
    }

    let (classpath_entries, native_jars, lib_set_blake3) =
        resolve_libraries_and_natives(profile, &rule_ctx, &libraries_root, &client_jar)?;
    let natives_extraction_skipped =
        ensure_natives_extracted(&natives_dir, &native_jars, &lib_set_blake3)?;

    let cp_sep = if rule_ctx.os_name == "windows" {
        ";"
    } else {
        ":"
    };
    let classpath_str = classpath_entries
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(cp_sep);

    let asset_index_name = profile
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| profile.assets.clone())
        .unwrap_or_else(|| "legacy".to_string());

    let game_assets_dir = assets_root.join("virtual").join(&asset_index_name);

    let mut placeholders = BTreeMap::new();
    placeholders.insert("auth_player_name", account.username.clone());
    placeholders.insert("auth_uuid", account.uuid_undashed.clone());
    placeholders.insert("auth_access_token", access_token.to_string());
    placeholders.insert("auth_session", access_token.to_string());
    placeholders.insert(
        "user_type",
        match account.kind {
            kestryx_auth::AccountKind::Microsoft { .. } => "msa".to_string(),
            kestryx_auth::AccountKind::Offline => "legacy".to_string(),
        },
    );
    placeholders.insert("user_properties", "{}".to_string());
    placeholders.insert("clientid", "kestryx".to_string());
    placeholders.insert(
        "auth_xuid",
        match &account.kind {
            kestryx_auth::AccountKind::Microsoft { xuid, .. } => xuid.clone(),
            kestryx_auth::AccountKind::Offline => "0".to_string(),
        },
    );
    placeholders.insert("version_name", profile.id.clone());
    placeholders.insert(
        "version_type",
        if profile.version_type.is_empty() {
            "release".to_string()
        } else {
            profile.version_type.clone()
        },
    );
    placeholders.insert("game_directory", game_dir.to_string_lossy().to_string());
    placeholders.insert("assets_root", assets_root.to_string_lossy().to_string());
    placeholders.insert("assets_index_name", asset_index_name);
    placeholders.insert("game_assets", game_assets_dir.to_string_lossy().to_string());
    placeholders.insert("natives_directory", natives_dir.to_string_lossy().to_string());
    placeholders.insert("classpath", classpath_str.clone());
    placeholders.insert("classpath_separator", cp_sep.to_string());
    placeholders.insert("library_directory", libraries_root.to_string_lossy().to_string());
    placeholders.insert("launcher_name", "Kestryx".to_string());
    placeholders.insert("launcher_version", "0.1.0".to_string());
    placeholders.insert(
        "resolution_width",
        instance.process.window_width.to_string(),
    );
    placeholders.insert(
        "resolution_height",
        instance.process.window_height.to_string(),
    );

    let mut jvm_args: Vec<String> = Vec::new();

    // 1. macOS requires `-XstartOnFirstThread`
    if rule_ctx.os_name == "osx" {
        jvm_args.push("-XstartOnFirstThread".to_string());
    }

    // 2. Memory allocation (`-Xms` and `-Xmx`)
    let min_mb = instance.java.min_memory_mb.max(512);
    let max_mb = instance.java.max_memory_mb.max(min_mb);
    jvm_args.push(format!("-Xms{min_mb}M"));
    jvm_args.push(format!("-Xmx{max_mb}M"));

    // 3. Preset GC flags + Fast Start (AppCDS / Java 25 AOT Cache)
    jvm_args.extend(jvm_flags_for_preset(&instance.java.jvm_preset, java_major));
    jvm_args.extend(build_fast_start_jvm_args(
        &instance.java.fast_start_mode,
        java_major,
        &instance_dir.join(".cache"),
    ));
    jvm_args.extend(instance.java.extra_jvm_args.clone());

    // 4. Mojang Log4j CVE-2021-44228 mitigation
    jvm_args.push("-Dlog4j2.formatMsgNoLookups=true".to_string());
    if let Some(ref logging) = profile.logging {
        if let Some(ref client_log) = logging.client {
            let log_xml_path = assets_root
                .join("log_configs")
                .join(client_log.file.path.as_deref().unwrap_or("client-1.12.xml"));
            jvm_args.push(
                client_log
                    .argument
                    .replace("${path}", &log_xml_path.to_string_lossy()),
            );
        }
    }

    // 5. Evaluate profile JVM arguments (or legacy default JVM arguments)
    if let Some(ref args_spec) = profile.arguments {
        for node in &args_spec.jvm {
            match node {
                ArgumentNode::Simple(s) => {
                    jvm_args.push(substitute_placeholders(s, &placeholders));
                }
                ArgumentNode::Conditional { rules, value } => {
                    if rule_ctx.evaluate_rules(rules) {
                        for v in value.as_slice() {
                            jvm_args.push(substitute_placeholders(v, &placeholders));
                        }
                    }
                }
            }
        }
    } else {
        // Legacy pre-1.13 JVM arguments
        jvm_args.push(format!(
            "-Djava.library.path={}",
            natives_dir.to_string_lossy()
        ));
        jvm_args.push("-cp".to_string());
        jvm_args.push(classpath_str);
    }

    // Ensure `-Djava.library.path` and `-cp` are present even if a minimal profile omitted them
    if !jvm_args.iter().any(|a| a.starts_with("-Djava.library.path=")) {
        jvm_args.push(format!(
            "-Djava.library.path={}",
            natives_dir.to_string_lossy()
        ));
    }
    if !jvm_args.iter().any(|a| a == "-cp" || a == "-classpath") {
        jvm_args.push("-cp".to_string());
        jvm_args.push(placeholders["classpath"].clone());
    }

    // Deduplicate `-XstartOnFirstThread` if both OS check and rule added it
    let mut deduped_jvm = Vec::with_capacity(jvm_args.len());
    let mut seen_start_first = false;
    for arg in jvm_args {
        if arg == "-XstartOnFirstThread" {
            if seen_start_first {
                continue;
            }
            seen_start_first = true;
        }
        deduped_jvm.push(arg);
    }

    // 6. Main Class
    let main_class = profile
        .main_class
        .clone()
        .unwrap_or_else(|| {
            let era = VersionEra::classify(&profile.id, &profile.version_type);
            if matches!(era, VersionEra::OldAlpha | VersionEra::OldBeta | VersionEra::ClassicEra) {
                "net.minecraft.launchwrapper.Launch".to_string()
            } else {
                "net.minecraft.client.main.Main".to_string()
            }
        });

    // 7. Game arguments (`arguments.game` or legacy `minecraftArguments`)
    let mut game_args: Vec<String> = Vec::new();
    if let Some(ref legacy_str) = profile.minecraft_arguments {
        for token in legacy_str.split_whitespace() {
            game_args.push(substitute_placeholders(token, &placeholders));
        }
        if instance.process.window_width > 0 && instance.process.window_height > 0 {
            game_args.push("--width".to_string());
            game_args.push(instance.process.window_width.to_string());
            game_args.push("--height".to_string());
            game_args.push(instance.process.window_height.to_string());
        }
    } else if let Some(ref args_spec) = profile.arguments {
        for node in &args_spec.game {
            match node {
                ArgumentNode::Simple(s) => {
                    game_args.push(substitute_placeholders(s, &placeholders));
                }
                ArgumentNode::Conditional { rules, value } => {
                    if rule_ctx.evaluate_rules(rules) {
                        for v in value.as_slice() {
                            game_args.push(substitute_placeholders(v, &placeholders));
                        }
                    }
                }
            }
        }
    }

    if instance.process.fullscreen && !game_args.contains(&"--fullscreen".to_string()) {
        game_args.push("--fullscreen".to_string());
    }

    // 8. Quick Play arguments
    if let Some(qp_req) = quick_play {
        if let QuickPlayResolution::Supported { args } = build_quick_play_args(&profile.id, qp_req) {
            game_args.extend(args);
        }
    }

    // 9. Combine & check if `@argfile` is needed to avoid OS command-line length limits
    let mut final_args = Vec::new();
    let total_chars: usize = deduped_jvm.iter().map(|s| s.len() + 1).sum::<usize>()
        + main_class.len()
        + game_args.iter().map(|s| s.len() + 1).sum::<usize>();

    let mut used_argfile = None;
    if total_chars >= ARGFILE_THRESHOLD_CHARS && java_major >= 9 {
        let argfile_path = instance_dir.join(".natives/jvm_args.txt");
        let escaped_lines = deduped_jvm
            .iter()
            .map(|a| format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join("\n");
        kestryx_store::atomic_write_bytes(&argfile_path, escaped_lines.as_bytes())?;
        final_args.push(format!("@{}", argfile_path.to_string_lossy()));
        used_argfile = Some(argfile_path);
    } else {
        final_args.extend(deduped_jvm);
    }

    final_args.push(main_class);
    final_args.extend(game_args);

    // 10. Sanitize environment & apply hybrid-GPU variables
    let mut env = instance.process.env.clone();
    sanitize_launch_env(&mut env);
    if instance.process.prefer_discrete_gpu {
        env.insert("__NV_PRIME_RENDER_OFFLOAD".to_string(), "1".to_string());
        env.insert(
            "__GLX_VENDOR_LIBRARY_NAME".to_string(),
            "nvidia".to_string(),
        );
        env.insert("DRI_PRIME".to_string(), "1".to_string());
    }

    // Handle optional wrapper command (e.g., `gamemoderun`)
    let (program, full_args) = if !instance.process.wrapper_command.trim().is_empty() {
        let mut parts = instance
            .process
            .wrapper_command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        let wrapper_prog = parts.remove(0);
        parts.push(java_bin.to_string_lossy().to_string());
        parts.extend(final_args);
        (wrapper_prog, parts)
    } else {
        (java_bin.to_string_lossy().to_string(), final_args)
    };

    let redactor = SecretRedactor::new(vec![access_token.to_string()]);
    let raw_cmd_str = format!("{} {}", program, full_args.join(" "));
    let redacted_args_display = redactor.redact(&raw_cmd_str);

    Ok(PreparedLaunchCommand {
        program,
        args: full_args,
        redacted_args_display,
        working_dir: game_dir.to_path_buf(),
        env,
        classpath_entries,
        natives_dir,
        used_argfile,
        library_set_blake3: lib_set_blake3,
        natives_extraction_skipped,
    })
}

fn substitute_placeholders(template: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        let token = format!("${{{k}}}");
        if out.contains(&token) {
            out = out.replace(&token, v);
        }
    }
    out
}

/// Spawns and supervises the game process, measuring:
/// 1. `time_to_spawn_ms` (must be <= 300ms p50 on warm cache)
/// 2. `time_to_title_screen_ms` (Startup Profiler detecting `Sound engine started` or `Created: ...-atlas`)
/// 3. Capturing a redacted stdout/stderr ring buffer for the live console and Crash Assistant.
pub async fn spawn_and_supervise(
    prepared: &PreparedLaunchCommand,
    pre_hook: Option<&str>,
    post_hook: Option<&str>,
) -> Result<LaunchSupervisionResult> {
    let start_instant = Instant::now();

    if let Some(hook) = pre_hook {
        if !hook.trim().is_empty() {
            run_hook_command(hook, &prepared.working_dir).await?;
        }
    }

    fs::create_dir_all(&prepared.working_dir)?;
    let mut cmd = Command::new(&prepared.program);
    cmd.args(&prepared.args)
        .current_dir(&prepared.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for poison in kestryx_java::POISONED_JAVA_ENV_VARS {
        cmd.env_remove(poison);
    }
    for (k, v) in &prepared.env {
        cmd.env(k, v);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn {}", prepared.program))?;
    let time_to_spawn_ms = start_instant.elapsed().as_millis() as u64;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let mut out_reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();

    let mut log_tail: Vec<String> = Vec::with_capacity(256);
    let mut time_to_title_screen_ms = None;

    loop {
        tokio::select! {
            line_res = out_reader.next_line() => {
                match line_res {
                    Ok(Some(line)) => {
                        if time_to_title_screen_ms.is_none()
                            && (line.contains("Sound engine started")
                                || line.contains("OpenAL initialized")
                                || line.contains("Created: 1024x512x4")
                                || line.contains("KestryxTitleReady"))
                        {
                            time_to_title_screen_ms = Some(start_instant.elapsed().as_millis() as u64);
                        }
                        if log_tail.len() >= 500 {
                            log_tail.remove(0);
                        }
                        log_tail.push(line);
                    }
                    _ => break,
                }
            }
            err_res = err_reader.next_line() => {
                if let Ok(Some(line)) = err_res {
                    if log_tail.len() >= 500 {
                        log_tail.remove(0);
                    }
                    log_tail.push(line);
                }
            }
        }
    }

    let status = child.wait().await?;
    let playtime_secs = start_instant.elapsed().as_secs();

    if let Some(hook) = post_hook {
        if !hook.trim().is_empty() {
            let _ = run_hook_command(hook, &prepared.working_dir).await;
        }
    }

    Ok(LaunchSupervisionResult {
        exit_code: status.code().unwrap_or(-1),
        time_to_spawn_ms,
        time_to_title_screen_ms,
        playtime_secs,
        log_tail,
    })
}

async fn run_hook_command(hook: &str, working_dir: &Path) -> Result<()> {
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let status = Command::new(shell)
        .arg(flag)
        .arg(hook)
        .current_dir(working_dir)
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("Hook command '{}' exited with {}", hook, status);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestryx_auth::AccountProfile;
    use kestryx_instances::InstanceManager;
    use kestryx_loaders::{LoaderKind, synthesize_loader_profile};
    use kestryx_meta::{
        ArgumentsSpec, LibraryEntry, OsRule, Rule, StringOrVec, merge_inherited_profile,
    };

    fn fixture_profile_for_era(id: &str, ver_type: &str, is_legacy_args: bool) -> VersionProfile {
        let mut libs = vec![LibraryEntry {
            name: "org.lwjgl:lwjgl:3.3.3".to_string(),
            downloads: None,
            url: None,
            natives: None,
            extract: None,
            rules: None,
        }];
        if is_legacy_args {
            VersionProfile {
                id: id.to_string(),
                version_type: ver_type.to_string(),
                main_class: Some("net.minecraft.launchwrapper.Launch".to_string()),
                minecraft_arguments: Some(
                    "--username ${auth_player_name} --session ${auth_session} --gameDir ${game_directory} --assetsDir ${game_assets}"
                        .to_string(),
                ),
                assets: Some("legacy".to_string()),
                libraries: libs,
                ..Default::default()
            }
        } else {
            libs.push(LibraryEntry {
                name: "org.lwjgl:lwjgl:3.3.3:natives-macos".to_string(),
                downloads: None,
                url: None,
                natives: None,
                extract: None,
                rules: Some(vec![Rule {
                    action: "allow".to_string(),
                    os: Some(OsRule {
                        name: Some("osx".to_string()),
                        version: None,
                        arch: None,
                    }),
                    features: None,
                }]),
            });
            VersionProfile {
                id: id.to_string(),
                version_type: ver_type.to_string(),
                main_class: Some("net.minecraft.client.main.Main".to_string()),
                arguments: Some(ArgumentsSpec {
                    game: vec![
                        ArgumentNode::Simple("--username".to_string()),
                        ArgumentNode::Simple("${auth_player_name}".to_string()),
                        ArgumentNode::Simple("--uuid".to_string()),
                        ArgumentNode::Simple("${auth_uuid}".to_string()),
                        ArgumentNode::Simple("--accessToken".to_string()),
                        ArgumentNode::Simple("${auth_access_token}".to_string()),
                        ArgumentNode::Simple("--version".to_string()),
                        ArgumentNode::Simple("${version_name}".to_string()),
                        ArgumentNode::Simple("--gameDir".to_string()),
                        ArgumentNode::Simple("${game_directory}".to_string()),
                    ],
                    jvm: vec![
                        ArgumentNode::Conditional {
                            rules: vec![Rule {
                                action: "allow".to_string(),
                                os: Some(OsRule {
                                    name: Some("osx".to_string()),
                                    version: None,
                                    arch: None,
                                }),
                                features: None,
                            }],
                            value: StringOrVec::Single("-XstartOnFirstThread".to_string()),
                        },
                        ArgumentNode::Simple("-Djava.library.path=${natives_directory}".to_string()),
                        ArgumentNode::Simple("-cp".to_string()),
                        ArgumentNode::Simple("${classpath}".to_string()),
                    ],
                }),
                assets: Some("34".to_string()),
                libraries: libs,
                ..Default::default()
            }
        }
    }

    #[test]
    fn test_golden_launch_command_matrix_all_eras_os_and_arch() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mgr = InstanceManager::new(dir.path())?;
        let acct = AccountProfile::new_offline("KestryxPilot", false)?;

        let eras = [
            ("a1.2.6", "old_alpha", true, 8),
            ("b1.7.3", "old_beta", true, 8),
            ("1.7.10", "release", true, 8),
            ("1.12.2", "release", true, 8),
            ("1.16.5", "release", false, 8),
            ("1.21.4", "release", false, 21),
            ("26.3", "release", false, 25),
            ("26.4-snapshot-4", "snapshot", false, 25),
        ];

        let targets = [
            ("windows", "x86_64"),
            ("windows", "arm64"),
            ("osx", "x86_64"),
            ("osx", "arm64"),
            ("linux", "x86_64"),
            ("linux", "arm64"),
        ];

        for (ver_id, ver_type, is_legacy, java_major) in eras {
            let slug = format!("inst-{}", ver_id.replace('.', "-"));
            let inst = mgr.create_instance(&slug, ver_id, ver_id, "vanilla", "", "Golden")?;
            let inst_dir = mgr.instance_dir(&slug);
            let game_dir = mgr.game_dir(&inst);
            let profile = fixture_profile_for_era(ver_id, ver_type, is_legacy);

            for (os_name, os_arch) in targets {
                let ctx = RuleContext {
                    os_name: os_name.to_string(),
                    os_version: "14.0".to_string(),
                    os_arch: os_arch.to_string(),
                    features: std::collections::HashMap::new(),
                };

                let cmd = build_launch_command(
                    &profile,
                    &inst,
                    &acct,
                    "secret-session-token-xyz",
                    Path::new("/usr/bin/java"),
                    java_major,
                    dir.path(),
                    &inst_dir,
                    &game_dir,
                    ctx,
                    None,
                )?;

                // 1. Secret token must be redacted in display string
                assert!(!cmd.redacted_args_display.contains("secret-session-token-xyz"));
                // 2. Log4j mitigation must be present
                assert!(cmd.args.contains(&"-Dlog4j2.formatMsgNoLookups=true".to_string()));
                // 3. macOS must include `-XstartOnFirstThread` exactly once
                let start_first_count = cmd
                    .args
                    .iter()
                    .filter(|a| *a == "-XstartOnFirstThread")
                    .count();
                if os_name == "osx" {
                    assert_eq!(start_first_count, 1);
                } else {
                    assert_eq!(start_first_count, 0);
                }
                // 4. Apple Silicon (osx + arm64) must substitute `natives-macos-arm64`
                if os_name == "osx" && os_arch == "arm64" && !is_legacy {
                    assert!(
                        cmd.classpath_entries
                            .iter()
                            .any(|p| p.to_string_lossy().contains("natives-macos-arm64"))
                    );
                }
            }

            // Second launch must skip native extraction via `.natives/.hash` cache!
            let repeat_cmd = build_launch_command(
                &profile,
                &inst,
                &acct,
                "secret-session-token-xyz",
                Path::new("/usr/bin/java"),
                java_major,
                dir.path(),
                &inst_dir,
                &game_dir,
                RuleContext::current_host(),
                None,
            )?;
            assert!(repeat_cmd.natives_extraction_skipped);
        }
        Ok(())
    }

    #[test]
    fn test_classpath_deduplication_loader_wins_over_vanilla() -> Result<()> {
        let mut parent = fixture_profile_for_era("26.3", "release", false);
        parent.libraries.push(LibraryEntry {
            name: "org.ow2.asm:asm:9.3".to_string(),
            downloads: None,
            url: None,
            natives: None,
            extract: None,
            rules: None,
        });

        let mut child = synthesize_loader_profile(LoaderKind::Fabric, "26.3", "0.20.0")?;
        child.libraries.push(LibraryEntry {
            name: "org.ow2.asm:asm:9.7.1".to_string(),
            downloads: None,
            url: None,
            natives: None,
            extract: None,
            rules: None,
        });

        let merged = merge_inherited_profile(&parent, &child);
        let (cp, _, _) = resolve_libraries_and_natives(
            &merged,
            &RuleContext::current_host(),
            Path::new("/libs"),
            Path::new("/client.jar"),
        )?;

        let asm_entries: Vec<_> = cp
            .iter()
            .filter(|p| p.to_string_lossy().contains("org/ow2/asm/asm/") || p.to_string_lossy().contains("org\\ow2\\asm\\asm\\"))
            .collect();
        assert_eq!(asm_entries.len(), 1);
        assert!(asm_entries[0].to_string_lossy().contains("9.7.1"));
        Ok(())
    }
}
