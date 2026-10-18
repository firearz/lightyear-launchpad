//! Light Yield Launcher — Interactive Console & CLI Engine
//! Works seamlessly both when double-clicked in Windows Explorer (interactive mode)
//! and when invoked with command-line arguments.

use std::io::{self, Write};
use std::path::PathBuf;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lightyield_core::{
    account::LocalAccount,
    build_launch_command,
    discover_all_java_runtimes,
    ensure_compatible_java_runtime,
    fetch_or_load_manifest,
    get_launcher_dir,
    get_required_java_version,
    get_versions_cache_file,
    get_versions_dir,
    install_version,
    launch_and_supervise,
    storage::ensure_directories,
    LaunchConfig,
    LauncherSettings,
    VersionFilter,
    VersionManifest,
};

#[derive(Parser)]
#[command(
    name = "lightyield",
    version = "0.1.0",
    about = "Light Yield Launcher — High-performance Minecraft: Java Edition Core Engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List available Minecraft versions (fetches online on startup and updates cache)
    Versions {
        /// Filter by version type: release, snapshot, beta, alpha, or all
        #[arg(short, long, default_value = "release")]
        r#type: String,
        /// Limit the number of versions displayed
        #[arg(short, long, default_value_t = 30)]
        limit: usize,
    },

    /// Download and install a specific Minecraft version (client jar, libraries, native DLLs, assets)
    Install {
        /// Minecraft version ID (e.g. 1.21.4, 1.12.2, b1.7.3)
        version: String,
        /// Force offline mode (do not query remote network)
        #[arg(long)]
        offline: bool,
    },

    /// Launch Minecraft with local account authentication, auto-Java provisioning, and supervision
    Run {
        /// Minecraft version ID (e.g. 1.21.4, 1.12.2, 26.3, b1.7.3)
        version: String,
        /// Local player username
        #[arg(short, long, default_value = "LightYieldPlayer")]
        username: String,
        /// Maximum RAM allocation (e.g. 2G, 4G, 8G)
        #[arg(long, default_value = "2G")]
        ram: String,
        /// Custom Java executable path (overrides auto-detection)
        #[arg(long)]
        java: Option<PathBuf>,
        /// Custom game directory for instance isolation
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Force offline mode
        #[arg(long)]
        offline: bool,
    },

    /// Compute the Java-exact RFC 4122 v3 UUID for a local offline account
    Account {
        /// Username
        username: String,
    },
}

#[tokio::main]
async fn main() {
    let is_interactive = std::env::args().len() <= 1;

    if let Err(err) = run_app().await {
        eprintln!("\n============================================================");
        eprintln!("  [ERROR] {err:#}");
        eprintln!("============================================================");
        if is_interactive {
            pause_console("\nPress Enter to close...");
        } else {
            std::process::exit(1);
        }
    }
}

async fn run_app() -> Result<()> {
    ensure_directories()?;
    let cli = Cli::parse();

    match cli.command {
        None => run_interactive_console().await,

        Some(Commands::Versions { r#type, limit }) => {
            let cache_file = get_versions_cache_file();
            println!("[INFO] Fetching/loading version manifest...");
            let manifest = fetch_or_load_manifest(&cache_file).await?;
            println!("[INFO] Cache saved to: {}", cache_file.display());
            println!("       Latest Release:  {}", manifest.latest.release);
            println!("       Latest Snapshot: {}", manifest.latest.snapshot);

            let filter = VersionFilter::from_str_loose(&r#type);
            let filtered = manifest.filter(filter);
            println!("\nFound {} versions matching filter '{:?}':", filtered.len(), filter);
            println!("{:<18} | {:<12} | {:<20}", "ID", "TYPE", "RELEASE DATE");
            println!("{:-<18}-+-{:-<12}-+-{:-<20}", "", "", "");

            for v in filtered.into_iter().take(limit) {
                let date = v.release_time.split('T').next().unwrap_or(&v.release_time);
                println!("{:<18} | {:<12} | {:<20}", v.id, v.version_type, date);
            }
            Ok(())
        }

        Some(Commands::Install { version, offline }) => {
            let cache_file = get_versions_cache_file();
            let manifest = fetch_or_load_manifest(&cache_file).await?;
            let entry = manifest
                .find(&version)
                .with_context(|| format!("Version '{version}' not found in manifest."))?;

            let installed = install_version(&version, &entry.url, offline).await?;
            let req_major = installed.package.java_version.as_ref().map(|j| j.major_version);
            let req_comp = installed.package.java_version.as_ref().map(|j| j.component.as_str());
            let java_rt = ensure_compatible_java_runtime(None, &version, req_major, req_comp, offline).await?;

            println!(
                "\n[SUCCESS] Version '{version}' and Java {} ({}) are installed and ready!",
                java_rt.major_version,
                java_rt.path.display()
            );
            Ok(())
        }

        Some(Commands::Run {
            version,
            username,
            ram,
            java,
            game_dir,
            offline,
        }) => {
            let cache_file = get_versions_cache_file();
            let manifest = fetch_or_load_manifest(&cache_file).await?;
            execute_install_and_launch(
                &manifest,
                &version,
                &username,
                &ram,
                java.as_deref(),
                game_dir,
                offline,
            )
            .await
        }

        Some(Commands::Account { username }) => {
            let account = LocalAccount::new(&username);
            println!("Username:         {}", account.username);
            println!("Dashed UUID (v3): {}", account.uuid_dashed());
            println!("Undashed UUID:    {}", account.uuid_undashed());
            Ok(())
        }
    }
}

async fn execute_install_and_launch(
    manifest: &VersionManifest,
    version: &str,
    username: &str,
    ram: &str,
    java_override: Option<&std::path::Path>,
    game_dir: Option<PathBuf>,
    offline: bool,
) -> Result<()> {
    let entry = manifest
        .find(version)
        .with_context(|| format!("Version '{version}' not found in manifest."))?;

    // 1. Ensure version metadata, client.jar, libraries, natives, and 100% of assets are installed
    let installed = install_version(version, &entry.url, offline).await?;

    // 2. Ensure a compatible Java runtime is present (auto-downloads official Mojang JRE if missing)
    let req_major = installed.package.java_version.as_ref().map(|j| j.major_version);
    let req_comp = installed.package.java_version.as_ref().map(|j| j.component.as_str());
    let java_runtime = ensure_compatible_java_runtime(
        java_override,
        version,
        req_major,
        req_comp,
        offline,
    )
    .await?;
    println!(
        "[INFO] Using Java Runtime: {} (Java {} / {})",
        java_runtime.path.display(),
        java_runtime.major_version,
        java_runtime.version_string
    );

    // 3. Create local offline account
    let account = LocalAccount::new(username);
    println!(
        "[INFO] Local Account: {} (UUID v3: {})",
        account.username,
        account.uuid_dashed()
    );

    // 4. Build launch command
    let config = LaunchConfig {
        max_ram: ram.to_string(),
        min_ram: "512M".to_string(),
        game_dir,
        custom_jvm_args: Vec::new(),
    };
    let launch_cmd = build_launch_command(&installed, &account, &java_runtime, &config)?;

    // 5. Spawn and supervise
    launch_and_supervise(&launch_cmd)?;
    Ok(())
}

async fn run_interactive_console() -> Result<()> {
    println!("============================================================");
    println!("          LIGHT YIELD LAUNCHER — CORE ENGINE v0.1.0         ");
    println!("  Universal Minecraft: Java Edition Launcher (Plan 0 Core)  ");
    println!("============================================================");
    println!("Data Directory: {}", get_launcher_dir().display());

    let cache_file = get_versions_cache_file();
    print!("Syncing Mojang version manifest... ");
    let _ = io::stdout().flush();
    let mut manifest = fetch_or_load_manifest(&cache_file).await?;
    println!(
        "OK ({} versions cached in {})",
        manifest.versions.len(),
        cache_file.display()
    );

    let mut settings = LauncherSettings::load();
    if manifest.find(&settings.selected_version).is_none() && !manifest.latest.release.is_empty() {
        settings.selected_version = manifest.latest.release.clone();
        let _ = settings.save();
    }

    loop {
        let account = LocalAccount::new(&settings.username);
        let filter = VersionFilter::from_str_loose(&settings.version_filter);
        let filtered_count = manifest.filter(filter).len();
        let is_installed = get_versions_dir()
            .join(&settings.selected_version)
            .join(format!("{}.jar", settings.selected_version))
            .exists();
        let req_java = get_required_java_version(&settings.selected_version);
        let local_javas = discover_all_java_runtimes();
        let java_status = if req_java == 8 {
            if let Some(j8) = local_javas.iter().find(|r| r.major_version == 8) {
                format!("Ready (Java 8 — {})", j8.version_string)
            } else {
                "Auto-Download Mojang Official Java 8 (jre-legacy) on Launch".to_string()
            }
        } else if let Some(j) = local_javas.iter().find(|r| r.major_version >= req_java) {
            format!("Ready (Java {} — {})", j.major_version, j.version_string)
        } else {
            format!("Auto-Download Mojang Official Java {req_java} on Launch")
        };

        println!("\n------------------------------------------------------------");
        println!(
            "  Player Username : {}  (Offline UUID: {})",
            account.username,
            account.uuid_dashed()
        );
        println!(
            "  Selected Version: {}  [{}]  (Requires Java {})",
            settings.selected_version,
            if is_installed { "Installed" } else { "Will Download on Play" },
            req_java
        );
        println!(
            "  Version Filter  : {}  ({} versions available)",
            settings.version_filter.to_uppercase(),
            filtered_count
        );
        println!("  Max RAM         : {}", settings.max_ram);
        println!("  Java Runtime    : {java_status}");
        println!("------------------------------------------------------------");
        println!("  [1] PLAY / Launch Minecraft ({})", settings.selected_version);
        println!("  [2] Change Local Username (Current: {})", account.username);
        println!("  [3] Select Minecraft Version (Browse / Search Dropdown)");
        println!("  [4] Change Version Filter (Release / Snapshot / Beta / Alpha / All)");
        println!("  [5] Install / Verify Selected Version Files Only");
        println!("  [6] Change Max RAM Allocation (Current: {})", settings.max_ram);
        println!("  [7] Refresh Manifest & Inspect Detected PC Java Runtimes");
        println!("  [Q] Quit Launcher");
        println!("------------------------------------------------------------");

        let choice = prompt_line("Select an option [1-7, Q] (or press Enter to Play): ")?;
        let trimmed = choice.trim();

        match trimmed.to_ascii_lowercase().as_str() {
            "" | "1" | "play" | "run" | "launch" => {
                println!(
                    "\n>>> Preparing to launch Minecraft {} as '{}'...",
                    settings.selected_version, account.username
                );
                if let Err(e) = execute_install_and_launch(
                    &manifest,
                    &settings.selected_version,
                    &account.username,
                    &settings.max_ram,
                    None,
                    None,
                    false,
                )
                .await
                {
                    eprintln!("\n[LAUNCH ERROR] {e:#}");
                    pause_console("\nPress Enter to return to the launcher menu...");
                }
            }

            "2" | "user" | "username" => {
                let input = prompt_line("Enter new local username (3-16 chars, A-Z, 0-9, _): ")?;
                if !input.trim().is_empty() {
                    let acc = LocalAccount::new(input.trim());
                    settings.username = acc.username.clone();
                    let _ = settings.save();
                    println!(
                        "  [OK] Username saved as '{}' (UUID v3: {})",
                        acc.username,
                        acc.uuid_dashed()
                    );
                }
            }

            "3" | "version" | "versions" => {
                select_version_interactive(&manifest, &mut settings)?;
            }

            "4" | "filter" => {
                println!("\nAvailable Version Filters:");
                println!("  [1] release   — Official full releases (e.g. {}, 1.21.4, 1.12.2, 1.8.9)", manifest.latest.release);
                println!("  [2] snapshot  — Experimental snapshots (e.g. {})", manifest.latest.snapshot);
                println!("  [3] beta      — Classic Beta versions (b1.0 – b1.8.1)");
                println!("  [4] alpha     — Original Alpha / Infdev / Classic versions (a1.2.6, c0.30, rd-132211)");
                println!("  [5] all       — Every version in Minecraft history ({} total)", manifest.versions.len());
                let f_in = prompt_line("Choose filter [1-5 or name]: ")?;
                let new_filter = match f_in.trim().to_ascii_lowercase().as_str() {
                    "1" | "release" | "releases" => "release",
                    "2" | "snapshot" | "snapshots" => "snapshot",
                    "3" | "beta" | "old_beta" => "beta",
                    "4" | "alpha" | "old_alpha" => "alpha",
                    "5" | "all" => "all",
                    _ => settings.version_filter.as_str(),
                };
                settings.version_filter = new_filter.to_string();
                let _ = settings.save();
                println!("  [OK] Active filter set to '{}'.", settings.version_filter);
            }

            "5" | "install" => {
                if let Some(entry) = manifest.find(&settings.selected_version) {
                    println!("\n>>> Installing and verifying '{}'...", settings.selected_version);
                    match install_version(&settings.selected_version, &entry.url, false).await {
                        Ok(installed) => {
                            let req_major = installed.package.java_version.as_ref().map(|j| j.major_version);
                            let req_comp = installed.package.java_version.as_ref().map(|j| j.component.as_str());
                            match ensure_compatible_java_runtime(
                                None,
                                &settings.selected_version,
                                req_major,
                                req_comp,
                                false,
                            )
                            .await
                            {
                                Ok(rt) => println!(
                                    "\n[SUCCESS] '{}' and Java {} ({}) are 100% verified and ready!",
                                    settings.selected_version,
                                    rt.major_version,
                                    rt.path.display()
                                ),
                                Err(e) => eprintln!("\n[JAVA ERROR] {e:#}"),
                            }
                        }
                        Err(e) => eprintln!("\n[INSTALL ERROR] {e:#}"),
                    }
                }
            }

            "6" | "ram" => {
                let ram_in = prompt_line("Enter max RAM allocation (e.g. 2G, 4G, 6G, 8G): ")?;
                let r = ram_in.trim().to_ascii_uppercase();
                if !r.is_empty() && (r.ends_with('G') || r.ends_with('M')) {
                    settings.max_ram = r;
                    let _ = settings.save();
                    println!("  [OK] Max RAM saved as {}", settings.max_ram);
                } else {
                    println!("  [WARN] Please specify RAM ending with 'G' or 'M' (e.g., 4G or 2048M).");
                }
            }

            "7" | "refresh" | "java" => {
                println!("\nRefreshing version manifest from Mojang...");
                manifest = fetch_or_load_manifest(&cache_file).await?;
                println!(
                    "  [OK] Cached {} versions to {}",
                    manifest.versions.len(),
                    cache_file.display()
                );
                let runtimes = discover_all_java_runtimes();
                println!("\nDetected Java Runtimes on this PC ({} found):", runtimes.len());
                for (i, rt) in runtimes.iter().enumerate() {
                    println!(
                        "  {}. Java {:<2} ({:<10}) -> {}",
                        i + 1,
                        rt.major_version,
                        rt.version_string,
                        rt.path.display()
                    );
                }
                if runtimes.is_empty() {
                    println!("  (None pre-installed — Light Yield will automatically download Mojang's official portable JRE on launch!)");
                }
            }

            "q" | "quit" | "exit" => {
                println!("Goodbye!");
                break;
            }

            other => {
                // Allow typing a version ID directly at the main prompt
                if manifest.find(other).is_some() {
                    settings.selected_version = other.to_string();
                    let _ = settings.save();
                    println!("  [OK] Selected version '{}'.", settings.selected_version);
                } else {
                    println!("  [WARN] Unrecognized option '{trimmed}'. Choose 1-7 or Q.");
                }
            }
        }
    }

    Ok(())
}

fn select_version_interactive(
    manifest: &VersionManifest,
    settings: &mut LauncherSettings,
) -> Result<()> {
    let filter = VersionFilter::from_str_loose(&settings.version_filter);
    let filtered = manifest.filter(filter);

    println!(
        "\n=== Version Dropdown [Filter: {} — {} versions] ===",
        settings.version_filter.to_uppercase(),
        filtered.len()
    );
    println!("Showing latest 35 matching versions:");
    println!("{:<5} {:<18} | {:<11} | {:<12}", "#", "VERSION ID", "TYPE", "RELEASE DATE");
    println!("{:-<5}-{:-<18}-+-{:-<11}-+-{:-<12}", "", "", "", "");

    for (idx, v) in filtered.iter().take(35).enumerate() {
        let date = v.release_time.split('T').next().unwrap_or(&v.release_time);
        let marker = if v.id == settings.selected_version { "*" } else { " " };
        println!(
            "{}{:<4} {:<18} | {:<11} | {:<12}",
            marker,
            idx + 1,
            v.id,
            v.version_type,
            date
        );
    }
    if filtered.len() > 35 {
        println!("  ... and {} older versions (type any version ID or search query below)", filtered.len() - 35);
    }

    let input = prompt_line(
        "\nEnter list number (#), exact Version ID (e.g. 1.21.4, 1.8.9, b1.7.3), or search term: ",
    )?;
    let query = input.trim();
    if query.is_empty() {
        return Ok(());
    }

    // 1. Exact version ID match in full manifest
    if let Some(exact) = manifest.find(query) {
        settings.selected_version = exact.id.clone();
        let _ = settings.save();
        println!("  [OK] Selected version: {}", settings.selected_version);
        return Ok(());
    }

    // 2. Number index from the displayed list (1..=35)
    if let Ok(num) = query.parse::<usize>() {
        if num >= 1 && num <= filtered.len().min(35) {
            settings.selected_version = filtered[num - 1].id.clone();
            let _ = settings.save();
            println!("  [OK] Selected version: {}", settings.selected_version);
            return Ok(());
        }
    }

    // 3. Substring search across all versions
    let matches: Vec<_> = manifest
        .versions
        .iter()
        .filter(|v| v.id.to_ascii_lowercase().contains(&query.to_ascii_lowercase()))
        .take(20)
        .collect();

    if matches.is_empty() {
        println!("  [WARN] No Minecraft version found matching '{query}'.");
    } else if matches.len() == 1 {
        settings.selected_version = matches[0].id.clone();
        let _ = settings.save();
        println!("  [OK] Selected version: {}", settings.selected_version);
    } else {
        println!("\nMatching versions for '{query}':");
        for (i, m) in matches.iter().enumerate() {
            let date = m.release_time.split('T').next().unwrap_or(&m.release_time);
            println!("  [{}] {:<18} ({}, {})", i + 1, m.id, m.version_type, date);
        }
        let pick = prompt_line("Pick a match number (or Enter to cancel): ")?;
        if let Ok(idx) = pick.trim().parse::<usize>() {
            if idx >= 1 && idx <= matches.len() {
                settings.selected_version = matches[idx - 1].id.clone();
                let _ = settings.save();
                println!("  [OK] Selected version: {}", settings.selected_version);
            }
        }
    }

    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut buf = String::new();
    let bytes = io::stdin().read_line(&mut buf)?;
    if bytes == 0 {
        return Ok("q".to_string());
    }
    Ok(buf)
}

fn pause_console(msg: &str) {
    println!("{msg}");
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
}
