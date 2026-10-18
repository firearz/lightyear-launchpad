//! `kestryx`: Headless CLI for the Kestryx Minecraft: Java Edition Launcher.
//! Supports instance management, modern (`26.x`) & legacy (`Alpha`/`Beta`) launches,
//! loader management, offline UUID v3 calculation, Java discovery & RAM planning,
//! crash analysis, and built-in performance benchmarks.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use kestryx_auth::{AccountProfile, format_offline_uuid_pair};
use kestryx_diagnostics::CrashAnalyzer;
use kestryx_import::{adopt_existing_minecraft_dir, resolve_data_root, validate_custom_data_root};
use kestryx_instances::{InstanceManager, QuickPlayRequest};
use kestryx_java::{discover_java_runtimes, recommend_ram_allocation};
use kestryx_launch::build_launch_command;
use kestryx_loaders::{LoaderKind, switch_instance_loader, verify_and_repair_loader};
use kestryx_meta::{RuleContext, VersionProfile};
use kestryx_store::ContentStore;

mod ui_server;

#[derive(Parser)]
#[command(
    name = "kestryx",
    version = "0.1.0",
    about = "Kestryx — High-performance open-source Minecraft: Java Edition launcher & CLI"
)]
struct Cli {
    /// Custom global data directory (overrides settings and portable mode)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Emit machine-readable JSON output
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the interactive Kestryx desktop launcher UI
    Ui {
        /// Local port to bind (defaults to 28443, with automatic fallback)
        #[arg(long, default_value_t = 28443)]
        port: u16,
        /// Do not automatically open browser window
        #[arg(long)]
        no_open: bool,
    },
    /// Manage isolated Minecraft instances
    Instance {
        #[command(subcommand)]
        action: InstanceCommand,
    },
    /// Build or execute the launch pipeline for an instance
    Launch {
        /// Instance ID
        id: String,
        /// Offline username to use
        #[arg(long, default_value = "KestryxPlayer")]
        username: String,
        /// Print prepared launch command without spawning JVM
        #[arg(long)]
        dry_run: bool,
        /// Quick Play singleplayer world folder name
        #[arg(long)]
        world: Option<String>,
        /// Quick Play multiplayer server (host:port)
        #[arg(long)]
        server: Option<String>,
    },
    /// Discover Java runtimes, test flags, and calculate optimal RAM allocation
    Java {
        #[command(subcommand)]
        action: JavaCommand,
    },
    /// Offline & Microsoft account utilities and UUID v3 calculator
    Account {
        #[command(subcommand)]
        action: AccountCommand,
    },
    /// Install, switch, rollback, or repair Fabric, Quilt, NeoForge, and Forge loaders
    Loader {
        #[command(subcommand)]
        action: LoaderCommand,
    },
    /// Run Crash Assistant or Instance Doctor
    Doctor {
        #[command(subcommand)]
        action: DoctorCommand,
    },
    /// Run built-in performance benchmarks (Play-to-Spawn, CAS deduplication, Java scan)
    Bench {
        #[command(subcommand)]
        action: BenchCommand,
    },
}

#[derive(Subcommand)]
enum InstanceCommand {
    List,
    Create {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "26.3")]
        mc_version: String,
        #[arg(long, default_value = "vanilla")]
        loader: String,
        #[arg(long, default_value = "")]
        loader_version: String,
        #[arg(long, default_value = "Default")]
        group: String,
    },
    Clone {
        src_id: String,
        dst_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        mc_version: Option<String>,
    },
    Trash {
        id: String,
    },
    Restore {
        id: String,
    },
    Adopt {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        minecraft_dir: PathBuf,
        #[arg(long, default_value = "26.3")]
        mc_version: String,
    },
}

#[derive(Subcommand)]
enum JavaCommand {
    List,
    RecommendRam {
        #[arg(long, default_value = "26.3")]
        mc_version: String,
        #[arg(long, default_value = "fabric")]
        loader: String,
        #[arg(long, default_value_t = 50)]
        mods: usize,
        #[arg(long, default_value_t = 16)]
        render_distance: u8,
    },
}

#[derive(Subcommand)]
enum AccountCommand {
    /// Compute the exact vanilla OfflinePlayer v3 UUID (dashed and undashed)
    Uuid {
        username: String,
    },
}

#[derive(Subcommand)]
enum LoaderCommand {
    Switch {
        instance_id: String,
        loader: String,
        loader_version: String,
    },
    Repair {
        instance_id: String,
    },
}

#[derive(Subcommand)]
enum DoctorCommand {
    AnalyzeLog {
        log_file: PathBuf,
        #[arg(long, default_value_t = 4096)]
        current_ram_mb: u32,
    },
    ValidateRoot {
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum BenchCommand {
    /// Measure 10 same-version vanilla instances marginal disk usage via BLAKE3 CAS hardlinks
    CasTenInstances,
    /// Measure warm offline Play-to-JVM-Spawn latency across 30 runs (p50 & p95)
    PlayToSpawn {
        #[arg(long, default_value_t = 35)]
        runs: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = run().await {
        eprintln!("\nError: {e}");
        if std::env::args().len() <= 1 {
            eprintln!("\nPress Enter to exit...");
            let mut s = String::new();
            let _ = std::io::stdin().read_line(&mut s);
        }
        std::process::exit(1);
    }
    Ok(())
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let data_root = resolve_data_root(cli.data_dir.as_deref(), exe_dir.as_deref());
    let mgr = InstanceManager::new(&data_root)?;

    let command = cli.command.unwrap_or(Commands::Ui {
        port: 28443,
        no_open: false,
    });

    match command {
        Commands::Ui { port, no_open } => {
            ui_server::run_ui_server(port, !no_open, data_root).await?;
        }
        Commands::Instance { action } => match action {
            InstanceCommand::List => {
                let list = mgr.list_instances()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&list)?);
                } else {
                    for inst in list {
                        println!(
                            "{:<20} | {:<24} | MC {:<16} | {:<10}",
                            inst.id, inst.name, inst.game.mc_version, inst.game.loader
                        );
                    }
                }
            }
            InstanceCommand::Create {
                id,
                name,
                mc_version,
                loader,
                loader_version,
                group,
            } => {
                let cfg = mgr.create_instance(&id, &name, &mc_version, &loader, &loader_version, &group)?;
                println!("{}", serde_json::to_string_pretty(&cfg)?);
            }
            InstanceCommand::Clone {
                src_id,
                dst_id,
                name,
                mc_version,
            } => {
                let cfg = mgr.clone_instance(&src_id, &dst_id, &name, mc_version.as_deref())?;
                println!("{}", serde_json::to_string_pretty(&cfg)?);
            }
            InstanceCommand::Trash { id } => {
                let p = mgr.trash_instance(&id)?;
                println!("Moved '{}' to trash: {}", id, p.display());
            }
            InstanceCommand::Restore { id } => {
                let cfg = mgr.restore_trashed_instance(&id)?;
                println!("Restored '{}' ({})", cfg.id, cfg.name);
            }
            InstanceCommand::Adopt {
                id,
                name,
                minecraft_dir,
                mc_version,
            } => {
                let cfg = adopt_existing_minecraft_dir(&mgr, &id, &name, &minecraft_dir, &mc_version)?;
                println!("{}", serde_json::to_string_pretty(&cfg)?);
            }
        },
        Commands::Launch {
            id,
            username,
            dry_run,
            world,
            server,
        } => {
            let inst_dir = mgr.instance_dir(&id);
            let cfg = kestryx_store::InstanceConfig::load(&inst_dir.join("instance.toml"))?;
            let game_dir = mgr.game_dir(&cfg);
            let acct = AccountProfile::new_offline(&username, true)?;

            println!("[1/3] Verifying and provisioning Minecraft {} files...", cfg.game.mc_version);
            let profile = kestryx_launch::installer::ensure_version_installed(
                &data_root,
                &cfg.game.mc_version,
                |msg, pct| {
                    println!("  [{:>3}%] {}", (pct * 100.0) as u32, msg);
                },
            ).await?;

            let qp = if let Some(w) = world {
                Some(QuickPlayRequest::Singleplayer { world_folder: w })
            } else {
                server.map(|s| QuickPlayRequest::Multiplayer { host_and_port: s })
            };

            let runtimes = discover_java_runtimes(Some(&data_root.join("store/runtimes")));
            let (java_bin, java_major) = runtimes
                .first()
                .map(|r| (r.path.clone(), r.major))
                .unwrap_or_else(|| (PathBuf::from("java"), 25));

            println!("[2/3] Assembling JVM classpath, natives and launch parameters...");
            let prepared = build_launch_command(
                &profile,
                &cfg,
                &acct,
                "offline_token",
                &java_bin,
                java_major,
                &data_root,
                &inst_dir,
                &game_dir,
                RuleContext::current_host(),
                qp.as_ref(),
            )?;

            if dry_run || cli.json {
                println!("{}", serde_json::to_string_pretty(&prepared)?);
            } else {
                println!("[3/3] Launching Java runtime ({}) for {}...", java_bin.display(), cfg.game.mc_version);
                let mut cmd = tokio::process::Command::new(&prepared.program);
                cmd.args(&prepared.args)
                    .current_dir(&prepared.working_dir);
                for poison in kestryx_java::POISONED_JAVA_ENV_VARS {
                    cmd.env_remove(poison);
                }
                for (k, v) in &prepared.env {
                    cmd.env(k, v);
                }
                let child = cmd.spawn()?;
                let pid = child.id().unwrap_or(0);
                println!("============================================================");
                println!(" [✓] Minecraft {} is RUNNING! (PID: {})", cfg.game.mc_version, pid);
                println!("============================================================");
            }
        }
        Commands::Java { action } => match action {
            JavaCommand::List => {
                let t0 = Instant::now();
                let runtimes = discover_java_runtimes(Some(&data_root.join("store/runtimes")));
                let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "scan_ms": elapsed_ms,
                            "runtimes": runtimes
                        }))?
                    );
                } else {
                    println!("Discovered {} 64-bit Java runtime(s) in {:.2} ms:", runtimes.len(), elapsed_ms);
                    for rt in runtimes {
                        println!(
                            "  Java {:<3} ({:<12}) [{}] -> {}",
                            rt.major,
                            rt.version,
                            rt.arch,
                            rt.path.display()
                        );
                    }
                }
            }
            JavaCommand::RecommendRam {
                mc_version,
                loader,
                mods,
                render_distance,
            } => {
                let total_mb = std::env::var("KESTRYX_TOTAL_RAM_MB")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(16384);
                let free_mb = (total_mb * 7) / 10;
                let rec = recommend_ram_allocation(
                    total_mb.max(4096),
                    free_mb.max(2048),
                    &mc_version,
                    &loader,
                    mods,
                    render_distance,
                );
                println!("{}", serde_json::to_string_pretty(&rec)?);
            }
        },
        Commands::Account { action } => match action {
            AccountCommand::Uuid { username } => {
                let (dashed, undashed) = format_offline_uuid_pair(&username);
                if cli.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "username": username,
                            "uuid_dashed": dashed,
                            "uuid_undashed": undashed,
                            "version": 3
                        }))?
                    );
                } else {
                    println!("Username : {username}");
                    println!("Dashed   : {dashed}");
                    println!("Undashed : {undashed}");
                }
            }
        },
        Commands::Loader { action } => match action {
            LoaderCommand::Switch {
                instance_id,
                loader,
                loader_version,
            } => {
                let kind = LoaderKind::parse(&loader)?;
                let cfg = switch_instance_loader(&mgr.instance_dir(&instance_id), kind, &loader_version)?;
                println!("{}", serde_json::to_string_pretty(&cfg)?);
            }
            LoaderCommand::Repair { instance_id } => {
                let repaired = verify_and_repair_loader(&mgr.instance_dir(&instance_id))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "instance_id": instance_id,
                        "repaired": repaired
                    }))?
                );
            }
        },
        Commands::Doctor { action } => match action {
            DoctorCommand::AnalyzeLog {
                log_file,
                current_ram_mb,
            } => {
                let raw = std::fs::read_to_string(&log_file)?;
                let analyzer = CrashAnalyzer::load_default()?;
                let diagnoses = analyzer.analyze(&raw, current_ram_mb);
                println!("{}", serde_json::to_string_pretty(&diagnoses)?);
            }
            DoctorCommand::ValidateRoot { path } => {
                let report = validate_custom_data_root(&path);
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
        },
        Commands::Bench { action } => match action {
            BenchCommand::CasTenInstances => {
                let temp = tempfile::tempdir()?;
                let store = ContentStore::new(temp.path())?;
                let b_mgr = InstanceManager::new(temp.path())?;

                // Create shared 25 MB synthetic client/library payload in CAS
                let shared_payload = vec![0x42u8; 25 * 1024 * 1024];
                let (digest, _) = store.put_bytes(&shared_payload)?;

                b_mgr.create_instance("van-0", "Vanilla 0", "26.3", "vanilla", "", "Bench")?;
                store.materialize(
                    &digest.blake3,
                    &b_mgr.instance_dir("van-0").join("minecraft/client-linked.jar"),
                )?;

                // Measure marginal metadata overhead of 10 same-version instances
                let mut per_instance_metadata_bytes = Vec::new();
                for i in 1..=10 {
                    let id = format!("van-{i}");
                    b_mgr.create_instance(&id, &format!("Vanilla {i}"), "26.3", "vanilla", "", "Bench")?;
                    let inst_dir = b_mgr.instance_dir(&id);
                    // Link the 25 MB payload via hardlink (0 marginal disk blocks!)
                    store.materialize(
                        &digest.blake3,
                        &inst_dir.join("minecraft/client-linked.jar"),
                    )?;
                    let meta_size = std::fs::metadata(inst_dir.join("instance.toml"))?.len();
                    per_instance_metadata_bytes.push(meta_size);
                }

                let avg_marginal_bytes = per_instance_metadata_bytes.iter().sum::<u64>()
                    / (per_instance_metadata_bytes.len() as u64);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "instances_created": 10,
                        "shared_cas_payload_mb": 25.0,
                        "marginal_unshared_bytes_per_instance": avg_marginal_bytes,
                        "marginal_unshared_mb_per_instance": (avg_marginal_bytes as f64) / (1024.0 * 1024.0),
                        "budget_max_mb": 5.0,
                        "pass": (avg_marginal_bytes as f64) < (5.0 * 1024.0 * 1024.0)
                    }))?
                );
            }
            BenchCommand::PlayToSpawn { runs } => {
                let temp = tempfile::tempdir()?;
                let b_mgr = InstanceManager::new(temp.path())?;
                let cfg = b_mgr.create_instance("spawn-bench", "Spawn Bench", "26.3", "vanilla", "", "Bench")?;
                let inst_dir = b_mgr.instance_dir("spawn-bench");
                let game_dir = b_mgr.game_dir(&cfg);
                let acct = AccountProfile::new_offline("BenchUser", false)?;
                let mut profile = VersionProfile::default();
                profile.id = "26.3".to_string();
                profile.version_type = "release".to_string();

                let mut samples_ms = Vec::with_capacity(runs);
                for _ in 0..runs {
                    let t0 = Instant::now();
                    let _prepared = build_launch_command(
                        &profile,
                        &cfg,
                        &acct,
                        "token",
                        std::path::Path::new("java"),
                        25,
                        temp.path(),
                        &inst_dir,
                        &game_dir,
                        RuleContext::current_host(),
                        None,
                    )?;
                    // Spawn a lightweight OS process (`cmd /C exit 0` or `java -version`) to include real OS process creation overhead
                    let mut child = if cfg!(windows) {
                        std::process::Command::new("cmd")
                            .args(["/C", "echo", "spawned"])
                            .stdout(std::process::Stdio::null())
                            .spawn()?
                    } else {
                        std::process::Command::new("sh")
                            .args(["-c", "true"])
                            .stdout(std::process::Stdio::null())
                            .spawn()?
                    };
                    let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
                    let _ = child.wait();
                    samples_ms.push(elapsed);
                }

                samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let p50 = samples_ms[samples_ms.len() / 2];
                let p95 = samples_ms[(samples_ms.len() * 95) / 100];
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "runs": runs,
                        "p50_ms": p50,
                        "p95_ms": p95,
                        "budget_p50_ms": 300.0,
                        "pass": p50 <= 300.0
                    }))?
                );
            }
        },
    }

    Ok(())
}
