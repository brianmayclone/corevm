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

use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use config::ServerConfig;
use state::AppState;

#[tokio::main]
async fn main() {
    // Parse CLI args
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
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

    tracing::info!("vmm-server v{} starting", env!("CARGO_PKG_VERSION"));

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

    // Init database
    let db_path = cfg.vms.config_dir.join("vmm.db");
    let conn = rusqlite::Connection::open(&db_path)
        .unwrap_or_else(|e| {
            eprintln!("Database error: {}", e);
            std::process::exit(1);
        });
    db::init(&conn).unwrap_or_else(|e| {
        eprintln!("Database init error: {}", e);
        std::process::exit(1);
    });

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
                        serial_tx: None, vm_thread: None,
                    });
                }
            }
            Err(e) => tracing::warn!("Failed to load VMs: {}", e),
        }
    }

    let state = Arc::new(AppState {
        vms: vms_map,
        db: Mutex::new(conn),
        jwt_secret,
        config: cfg,
    });

    // Build router
    let app = api::router()
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    // Start server
    let bind = format!("{}:{}", state.config.server.bind, state.config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind).await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind {}: {}", bind, e);
            std::process::exit(1);
        });
    tracing::info!("Listening on http://{}", bind);

    // Graceful shutdown on Ctrl+C
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {}", e);
            std::process::exit(1);
        });

    tracing::info!("Server shut down");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("Shutdown signal received");
}
