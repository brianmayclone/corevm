//! vmm-server: CoreVM web management server.
//!
//! Provides REST API for VM management + WebSocket for live console access.
//! Configured via TOML file (--config flag or /etc/vmm/vmm-server.toml).

mod config;
mod state;
mod db;
mod auth;
mod services;
mod api;
mod vm;
mod ws;
mod agent;
mod discovery;

use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use config::ServerConfig;
use state::AppState;

#[tokio::main]
async fn main() {
    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let force_reset = args.iter().any(|a| a == "--force-reset");
    let config_path = args.iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "/etc/vmm/vmm-server.toml".into());

    // Load config
    let mut cfg = ServerConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| {
            eprintln!("Config error: {}", e);
            std::process::exit(1);
        });

    // Init logging
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cfg.logging.level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    tracing::info!("vmm-server v{} ({}) built {}",
        env!("CARGO_PKG_VERSION"), env!("COREVM_GIT_SHA"), env!("COREVM_BUILD_TIMESTAMP"));

    // Auto-detect BIOS search paths if not configured
    tracing::info!("CWD: {:?}", std::env::current_dir());
    tracing::info!("EXE: {:?}", std::env::current_exe());
    tracing::info!("Config file: {}", config_path);
    if cfg.vms.bios_search_paths.is_empty() {
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();

        // Relative to executable (target/release/vmm-server → walk up to project root)
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(exe) = std::fs::canonicalize(&exe) {
                if let Some(exe_dir) = exe.parent() {
                    // target/release/assets/bios (deployed alongside binary)
                    candidates.push(exe_dir.join("assets/bios"));
                    // target/release/../../apps/vmm-server/assets/bios (dev layout)
                    if let Some(project_root) = exe_dir.parent().and_then(|p| p.parent()) {
                        candidates.push(project_root.join("apps/vmm-server/assets/bios"));
                    }
                }
            }
        }

        // Relative to CWD
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("assets/bios"));
            candidates.push(cwd.join("apps/vmm-server/assets/bios"));
            // If CWD is a subdirectory (e.g. tools/)
            candidates.push(cwd.join("../apps/vmm-server/assets/bios"));
        }

        // Relative to config file
        if let Some(cfg_dir) = std::path::Path::new(&config_path).parent() {
            candidates.push(cfg_dir.join("assets/bios"));
            candidates.push(cfg_dir.join("apps/vmm-server/assets/bios"));
        }

        for p in &candidates {
            let has_bios = p.join("bios.bin").exists();
            tracing::debug!("BIOS candidate: {} (exists={}, bios.bin={})", p.display(), p.exists(), has_bios);
            if has_bios || p.join("vgabios.bin").exists() {
                let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
                tracing::info!("Auto-detected BIOS path: {}", abs.display());
                cfg.vms.bios_search_paths.push(abs);
                break;
            }
        }
        // Also add standard system paths
        for p in &["/usr/share/seabios", "/usr/share/OVMF", "/usr/share/qemu"] {
            let path = std::path::PathBuf::from(p);
            if path.exists() { cfg.vms.bios_search_paths.push(path); }
        }
    }
    tracing::info!("BIOS search paths: {:?}", cfg.vms.bios_search_paths);

    // Ensure data directories exist
    let _ = std::fs::create_dir_all(&cfg.vms.config_dir);
    let _ = std::fs::create_dir_all(&cfg.storage.default_pool);
    let _ = std::fs::create_dir_all(&cfg.storage.iso_pool);

    // Init database — MUST be on a persistent filesystem (not /tmp!)
    let db_path = cfg.vms.config_dir.join("vmm.db");
    let db_existed = db_path.exists();
    if db_path.starts_with("/tmp") {
        tracing::warn!("Database path is in /tmp — data WILL be lost on reboot! Change vms.config_dir in vmm-server.toml");
    }
    tracing::info!("Database: {} ({})", db_path.display(),
        if db_existed { "existing" } else { "NEW — will be created" });
    let conn = rusqlite::Connection::open(&db_path)
        .unwrap_or_else(|e| {
            tracing::error!("Failed to open database at {}: {}", db_path.display(), e);
            std::process::exit(1);
        });
    // Enable WAL mode for better concurrent access and crash safety
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .unwrap_or_else(|e| tracing::warn!("Failed to set WAL mode: {}", e));
    db::init(&conn).unwrap_or_else(|e| {
        tracing::error!("Database init/migration error: {}", e);
        std::process::exit(1);
    });
    // Create initial backup
    let backup_dir = cfg.vms.config_dir.join("backups");
    let _ = std::fs::create_dir_all(&backup_dir);
    if db_existed {
        let backup_name = format!("vmm-startup-{}.db",
            chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let backup_path = backup_dir.join(&backup_name);
        match std::fs::copy(&db_path, &backup_path) {
            Ok(_) => tracing::info!("Database backup: {}", backup_path.display()),
            Err(e) => tracing::warn!("Failed to backup database: {}", e),
        }
        // Keep only last 10 backups
        if let Ok(entries) = std::fs::read_dir(&backup_dir) {
            let mut backups: Vec<_> = entries.filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("vmm-"))
                .collect();
            backups.sort_by_key(|e| e.file_name());
            if backups.len() > 10 {
                for old in &backups[..backups.len() - 10] {
                    let _ = std::fs::remove_file(old.path());
                }
            }
        }
    }

    let jwt_secret = cfg.auth.jwt_secret.clone();

    // Build app state
    let vms_map = dashmap::DashMap::new();

    // Load existing VMs from database into memory
    {
        use crate::services::vm::VmService;
        match VmService::list(&conn) {
            Ok(records) => {
                for r in records {
                    tracing::info!("Loaded VM '{}' ({})", r.name, r.id);
                    vms_map.insert(r.id.clone(), state::VmInstance {
                        id: r.id, config: r.config, state: state::VmState::Stopped,
                        vm_handle: None, control: None, framebuffer: None,
                        serial_tx: None, vm_thread: None, started_at: None,
                    });
                }
            }
            Err(e) => tracing::warn!("Failed to load VMs: {}", e),
        }
    }

    // Handle --force-reset: remove cluster registration and start in standalone mode
    let cluster_json_path = cfg.vms.config_dir.join("cluster.json");
    if force_reset {
        if cluster_json_path.exists() {
            match std::fs::remove_file(&cluster_json_path) {
                Ok(_) => tracing::warn!("--force-reset: removed cluster.json — node is now standalone"),
                Err(e) => {
                    tracing::error!("--force-reset: failed to remove cluster.json: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            tracing::info!("--force-reset: no cluster.json found — already standalone");
        }
    }

    // Load managed-mode config (if this node was previously registered with a cluster)
    let managed_config = {
        if cluster_json_path.exists() {
            match std::fs::read_to_string(&cluster_json_path) {
                Ok(content) => {
                    match serde_json::from_str::<vmm_core::cluster::ManagedNodeConfig>(&content) {
                        Ok(config) if config.managed => {
                            tracing::info!("Managed mode: registered with cluster {}", config.cluster_url);
                            Some(config)
                        }
                        _ => None,
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        }
    };

    let state = Arc::new(AppState {
        vms: vms_map,
        db: Mutex::new(conn),
        jwt_secret,
        config: cfg,
        started_at: std::time::Instant::now(),
        managed_config: Mutex::new(managed_config),
    });

    // Build router — managed-mode guard blocks /api/* when managed by a cluster
    // API access guard controls CLI/API access (can be disabled via config)
    let api_router = api::router()
        .layer(axum::middleware::from_fn_with_state(state.clone(), api::guard::managed_mode_guard))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth::api_access::api_access_guard))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    // Serve the embedded UI if a `ui/` directory exists next to the binary.
    let app = if let Some(ui_dir) = find_ui_dir() {
        tracing::info!("Serving UI from {}", ui_dir.display());
        let index = ui_dir.join("index.html");
        let serve_dir = ServeDir::new(&ui_dir).not_found_service(ServeFile::new(&index));
        api_router.fallback_service(serve_dir)
    } else {
        tracing::info!("No UI directory found — API-only mode (use Vite dev server for UI)");
        api_router
    };

    // Start server (with optional TLS)
    let bind = format!("{}:{}", state.config.server.bind, state.config.server.port);
    let use_tls = state.config.server.tls_cert.is_some() && state.config.server.tls_key.is_some();

    if use_tls {
        let cert_path = state.config.server.tls_cert.as_ref().unwrap();
        let key_path = state.config.server.tls_key.as_ref().unwrap();
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .unwrap_or_else(|e| {
                eprintln!("TLS config error: {} (cert={}, key={})", e, cert_path, key_path);
                std::process::exit(1);
            });
        tracing::info!("Listening on https://{} (TLS enabled)", bind);
        let addr: std::net::SocketAddr = bind.parse().unwrap_or_else(|e| {
            eprintln!("Invalid bind address {}: {}", bind, e);
            std::process::exit(1);
        });

    // Start UDP discovery beacon
    discovery::spawn(Arc::clone(&state));

        // Periodic database backup task (every 30 minutes)
        spawn_backup_task(db_path.clone(), backup_dir.clone());

        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await
            .unwrap_or_else(|e| {
                eprintln!("Server error: {}", e);
                std::process::exit(1);
            });
    } else {
        let listener = tokio::net::TcpListener::bind(&bind).await
            .unwrap_or_else(|e| {
                eprintln!("Failed to bind {}: {}", bind, e);
                std::process::exit(1);
            });
        tracing::info!("Listening on http://{}", bind);

        // Periodic database backup task (every 30 minutes)
        spawn_backup_task(db_path.clone(), backup_dir.clone());

        // Graceful shutdown on Ctrl+C
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .unwrap_or_else(|e| {
                eprintln!("Server error: {}", e);
                std::process::exit(1);
            });
    }

    tracing::info!("Server shut down");
}

/// Look for a `ui/` directory containing `index.html` next to the binary.
fn find_ui_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(exe).ok()?;
    let exe_dir = exe.parent()?;
    let ui_dir = exe_dir.join("ui");
    if ui_dir.join("index.html").exists() {
        Some(ui_dir)
    } else {
        None
    }
}

/// Spawn periodic database backup task (every 30 minutes).
fn spawn_backup_task(db_path: std::path::PathBuf, backup_dir: std::path::PathBuf) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30 * 60));
        interval.tick().await; // skip first immediate tick
        loop {
            interval.tick().await;
            let backup_name = format!("vmm-auto-{}.db",
                chrono::Local::now().format("%Y%m%d-%H%M%S"));
            let backup_path = backup_dir.join(&backup_name);
            match std::fs::copy(&db_path, &backup_path) {
                Ok(_) => tracing::info!("Auto-backup: {}", backup_path.display()),
                Err(e) => tracing::warn!("Auto-backup failed: {}", e),
            }
            // Keep only last 10 auto-backups
            if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                let mut backups: Vec<_> = entries.filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().starts_with("vmm-auto-"))
                    .collect();
                backups.sort_by_key(|e| e.file_name());
                if backups.len() > 10 {
                    for old in &backups[..backups.len() - 10] {
                        let _ = std::fs::remove_file(old.path());
                    }
                }
            }
        }
    });
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("Shutdown signal received");
}
