//! Embedded HTTP server and desktop launcher for Kestryx.
//! Serves the Svelte 5 Single Page Application directly from the compiled binary,
//! provides live REST endpoints (`/api/launch`, `/api/launch-status`),
//! and launches Chrome in dedicated desktop app mode (`--app=http://...`) or the default browser.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use kestryx_auth::AccountProfile;
use kestryx_instances::InstanceManager;
use kestryx_java::discover_java_runtimes;
use kestryx_launch::build_launch_command;
use kestryx_meta::RuleContext;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchStatus {
    pub status: String, // "idle" | "downloading" | "launching" | "running" | "error"
    pub message: String,
    pub pct: f32,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LaunchRequest {
    #[serde(rename = "instanceId")]
    pub instance_id: Option<String>,
    #[serde(rename = "mcVersion")]
    pub mc_version: Option<String>,
    pub username: Option<String>,
}

struct EmbeddedAsset {
    path: &'static str,
    bytes: &'static [u8],
    content_type: &'static str,
}

static ASSETS: &[EmbeddedAsset] = &[
    EmbeddedAsset {
        path: "/index.html",
        bytes: include_bytes!("../../../apps/desktop/dist/index.html"),
        content_type: "text/html; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/AccountsSkinsView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/AccountsSkinsView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/ConsoleCrashView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/ConsoleCrashView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/CreateInstanceModal.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/CreateInstanceModal.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/DiscoverView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/DiscoverView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/GalleryView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/GalleryView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/index.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/index.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/index.css",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/index.css"),
        content_type: "text/css; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/InstanceDashboardView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/InstanceDashboardView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/JavaPerformanceView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/JavaPerformanceView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/SettingsView.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/SettingsView.js"),
        content_type: "application/javascript; charset=utf-8",
    },
    EmbeddedAsset {
        path: "/assets/vendor.js",
        bytes: include_bytes!("../../../apps/desktop/dist/assets/vendor.js"),
        content_type: "application/javascript; charset=utf-8",
    },
];

fn guess_content_type(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") || path.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    }
}

fn lookup_asset(req_path: &str) -> (std::borrow::Cow<'static, [u8]>, &'static str) {
    let clean = req_path.split('?').next().unwrap_or(req_path).trim_start_matches('/');
    let target = if clean.is_empty() { "index.html" } else { clean };

    // Try reading live from disk first if development/dist directory is present
    let disk_candidates = [
        PathBuf::from("apps/desktop/dist").join(target),
        PathBuf::from("../apps/desktop/dist").join(target),
    ];
    for p in &disk_candidates {
        if p.exists() && p.is_file() {
            if let Ok(bytes) = std::fs::read(p) {
                return (std::borrow::Cow::Owned(bytes), guess_content_type(target));
            }
        }
    }

    // Match against embedded fallback table
    let check_path = if req_path.starts_with('/') { req_path.split('?').next().unwrap() } else { "/" };
    for asset in ASSETS {
        if asset.path == check_path {
            return (std::borrow::Cow::Borrowed(asset.bytes), asset.content_type);
        }
    }

    // Single Page Application fallback for client routes
    (std::borrow::Cow::Borrowed(ASSETS[0].bytes), ASSETS[0].content_type)
}

/// Spawns the desktop browser in application window mode or default system browser.
pub fn launch_browser_window(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let chrome_candidates = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        ];

        let mut opened_app_window = false;
        for candidate in chrome_candidates {
            if std::path::Path::new(candidate).exists() {
                let app_arg = format!("--app={}", url);
                if std::process::Command::new(candidate)
                    .arg(app_arg)
                    .spawn()
                    .is_ok()
                {
                    opened_app_window = true;
                    break;
                }
            }
        }

        if !opened_app_window {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .spawn();
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

async fn execute_launch_pipeline(
    data_root: PathBuf,
    req: LaunchRequest,
    status_state: Arc<RwLock<LaunchStatus>>,
) {
    let version = req.mc_version.unwrap_or_else(|| "1.21.4".to_string());
    let username = req.username.unwrap_or_else(|| "KestryxPlayer".to_string());
    let inst_id = req.instance_id.unwrap_or_else(|| "default-instance".to_string());

    let state_clone = status_state.clone();
    let result: Result<u32> = async {
        {
            let mut s = state_clone.write().await;
            s.status = "downloading".to_string();
            s.message = format!("Provisioning Minecraft {version} files...");
            s.pct = 0.05;
        }

        let mgr = InstanceManager::new(&data_root)?;
        let inst_dir = mgr.instance_dir(&inst_id);
        let cfg_path = inst_dir.join("instance.toml");
        let cfg = if cfg_path.exists() {
            kestryx_store::InstanceConfig::load(&cfg_path)?
        } else if inst_dir.exists() {
            let _ = std::fs::remove_dir_all(&inst_dir);
            mgr.create_instance(&inst_id, &format!("Minecraft {version}"), &version, "vanilla", "", "General")?
        } else {
            mgr.create_instance(&inst_id, &format!("Minecraft {version}"), &version, "vanilla", "", "General")?
        };
        let game_dir = mgr.game_dir(&cfg);
        std::fs::create_dir_all(&game_dir)?;

        let progress_state = state_clone.clone();
        let profile = kestryx_launch::installer::ensure_version_installed(
            &data_root,
            &version,
            move |msg, pct| {
                if let Ok(mut s) = progress_state.try_write() {
                    s.status = "downloading".to_string();
                    s.message = msg.to_string();
                    s.pct = pct;
                }
            },
        ).await?;

        {
            let mut s = state_clone.write().await;
            s.status = "launching".to_string();
            s.message = "Assembling classpath, natives and launch parameters...".to_string();
            s.pct = 0.95;
        }

        let acct = AccountProfile::new_offline(&username, true)?;
        let runtimes = discover_java_runtimes(Some(&data_root.join("store/runtimes")));
        let (java_bin, java_major) = runtimes
            .first()
            .map(|r| (r.path.clone(), r.major))
            .unwrap_or_else(|| (PathBuf::from("java"), 25));

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
            None,
        )?;

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
        Ok(pid)
    }.await;

    match result {
        Ok(pid) => {
            let mut s = status_state.write().await;
            s.status = "running".to_string();
            s.message = format!("Minecraft {version} is running! (PID: {pid})");
            s.pct = 1.0;
            s.pid = Some(pid);
            println!(" [✓] Minecraft {} spawned with PID {}", version, pid);
        }
        Err(e) => {
            let mut s = status_state.write().await;
            s.status = "error".to_string();
            s.message = format!("Launch failed: {e}");
            s.pct = 0.0;
            eprintln!(" [!] Launch error: {e}");
        }
    }
}

/// Starts the embedded launcher server and waits until interrupted.
pub async fn run_ui_server(preferred_port: u16, open_browser: bool, data_root: PathBuf) -> Result<()> {
    let primary_addr = format!("127.0.0.1:{}", preferred_port);
    let listener = match TcpListener::bind(&primary_addr).await {
        Ok(l) => l,
        Err(_) => TcpListener::bind("127.0.0.1:0").await?,
    };

    let actual_addr: SocketAddr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}", actual_addr.port());

    let status_state = Arc::new(RwLock::new(LaunchStatus {
        status: "idle".to_string(),
        message: "Ready to launch".to_string(),
        pct: 0.0,
        pid: None,
    }));

    println!("========================================================================");
    println!("   Kestryx — High-Performance Minecraft: Java Edition Launcher");
    println!("========================================================================");
    println!(" [✓] Launcher UI active: {}", url);
    println!(" [✓] Mode: Standalone Desktop Web Server with Live Launch Engine");
    println!();
    println!(" Keep this console window open while using Kestryx.");
    println!(" Press Ctrl+C at any time to quit.");
    println!("========================================================================");

    if open_browser {
        launch_browser_window(&url);
    }

    let server_data_root = data_root.clone();
    let server_task = async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };

            let data_root = server_data_root.clone();
            let status_state = status_state.clone();

            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let n = match socket.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };

                let req_text = String::from_utf8_lossy(&buf[..n]);
                let mut req_path = "/";
                let mut req_method = "GET";

                if let Some(first_line) = req_text.lines().next() {
                    let mut parts = first_line.split_whitespace();
                    if let Some(m) = parts.next() {
                        req_method = m;
                    }
                    if let Some(path) = parts.next() {
                        req_path = path;
                    }
                }

                // API Endpoint: /api/launch-status
                if req_path.starts_with("/api/launch-status") {
                    let status = status_state.read().await;
                    let json = serde_json::to_string(&*status).unwrap_or_default();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
                        json.len(),
                        json
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.flush().await;
                    return;
                }

                // API Endpoint: /api/launch
                if req_path.starts_with("/api/launch") && req_method == "POST" {
                    let body = req_text.split("\r\n\r\n").nth(1).unwrap_or("");
                    let req: LaunchRequest = serde_json::from_str(body).unwrap_or(LaunchRequest {
                        instance_id: None,
                        mc_version: None,
                        username: None,
                    });

                    tokio::spawn(execute_launch_pipeline(data_root, req, status_state));

                    let resp_body = "{\"success\":true,\"message\":\"Launch sequence started\"}";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
                        resp_body.len(),
                        resp_body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.flush().await;
                    return;
                }

                // Static SPA file serving
                let (body, content_type) = lookup_asset(req_path);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: public, max-age=3600\r\n\r\n",
                    content_type,
                    body.len()
                );

                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            });
        }
    };

    tokio::select! {
        _ = server_task => {},
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down Kestryx launcher...");
        }
    }

    Ok(())
}
