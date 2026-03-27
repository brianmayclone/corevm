//! vmm-cluster — CoreVM cluster orchestration server.
//!
//! Central authority managing multiple vmm-server nodes, analogous to VMware vCenter.
//! The cluster owns all state (VMs, storage, users, permissions) and pushes
//! commands to nodes via the Agent API.

mod config;
mod state;
mod db;
mod auth;
mod api;
mod services;
mod engine;
mod node_client;
mod san_client;
mod ws;

use std::sync::{Arc, Mutex};
use config::ClusterConfig;
use state::{ClusterState, NodeConnection, NodeStatus};
use dashmap::DashMap;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    // ── Parse CLI args ──────────────────────────────────────────────
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "/etc/vmm/vmm-cluster.toml".to_string());

    // ── Load configuration ──────────────────────────────────────────
    let config = ClusterConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| {
            eprintln!("Config error: {}", e);
            std::process::exit(1);
        });

    // ── Initialize logging ──────────────────────────────────────────
    {
        use tracing_subscriber::prelude::*;
        use tracing_subscriber::fmt;
        use tracing_subscriber::EnvFilter;

        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

        if let Some(ref log_path) = config.logging.log_file {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let file = std::fs::OpenOptions::new()
                .create(true).append(true)
                .open(log_path)
                .unwrap_or_else(|e| {
                    eprintln!("Cannot open log file {}: {}", log_path.display(), e);
                    std::process::exit(1);
                });
            let file_layer = fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file));
            let stdout_layer = fmt::layer();
            tracing_subscriber::registry()
                .with(env_filter)
                .with(file_layer)
                .with(stdout_layer)
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .init();
        }
    }

    tracing::info!("vmm-cluster v{} ({}) built {}",
        env!("CARGO_PKG_VERSION"), env!("COREVM_GIT_SHA"), env!("COREVM_BUILD_TIMESTAMP"));

    // ── Create data directory ───────────────────────────────────────
    let data_dir = &config.data.data_dir;
    if !data_dir.exists() {
        std::fs::create_dir_all(data_dir).unwrap_or_else(|e| {
            tracing::warn!("Cannot create data directory {}: {}", data_dir.display(), e);
        });
    }

    // ── Initialize database ─────────────────────────────────────────
    let db_path = data_dir.join("vmm-cluster.db");
    tracing::info!("Database: {}", db_path.display());

    let conn = rusqlite::Connection::open(&db_path).unwrap_or_else(|e| {
        eprintln!("Cannot open database: {}", e);
        std::process::exit(1);
    });
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .expect("Failed to set PRAGMA");

    db::init(&conn).unwrap_or_else(|e| {
        eprintln!("Database init failed: {}", e);
        std::process::exit(1);
    });

    // ── Load existing hosts into memory ─────────────────────────────
    let nodes: DashMap<String, NodeConnection> = DashMap::new();
    {
        let hosts = services::host::HostService::list(&conn).unwrap_or_default();
        for host in hosts {
            let agent_token = services::host::HostService::get_agent_token(&conn, &host.id)
                .unwrap_or_default();
            nodes.insert(host.id.clone(), NodeConnection {
                node_id: host.id,
                hostname: host.hostname,
                address: host.address,
                agent_token,
                status: NodeStatus::Connecting,
                missed_heartbeats: 0,
            });
        }
        tracing::info!("Loaded {} registered hosts", nodes.len());
    }

    // ── Build application state ─────────────────────────────────────
    let jwt_secret = config.auth.jwt_secret.clone();
    let bind = config.server.bind.clone();
    let port = config.server.port;

    let discovery_store = std::sync::Arc::new(engine::discovery::DiscoveryStore::new());

    let state = Arc::new(ClusterState {
        nodes,
        db: Mutex::new(conn),
        jwt_secret,
        config,
        started_at: std::time::Instant::now(),
        discovery: discovery_store.clone(),
        san_health: std::sync::RwLock::new(serde_json::json!({"hosts": []})),
    });

    // ── Start background engines ────────────────────────────────────
    engine::discovery::spawn(discovery_store);

    engine::heartbeat::spawn(Arc::clone(&state));
    tracing::info!("Heartbeat monitor started (10s interval)");

    engine::drs::spawn(Arc::clone(&state));
    tracing::info!("DRS engine started (5m interval)");

    engine::notifier::spawn(Arc::clone(&state));
    tracing::info!("Notification worker started");

    engine::san_health::spawn(Arc::clone(&state));
    tracing::info!("SAN health monitor started (30s interval)");

    // ── Build router ────────────────────────────────────────────────
    let api_router = api::router()
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Serve the embedded UI if a `ui/` directory exists next to the binary.
    // This enables single-binary deployment in production — in development,
    // Vite serves the UI separately and this directory won't exist.
    let app = if let Some(ui_dir) = find_ui_dir() {
        tracing::info!("Serving UI from {}", ui_dir.display());
        let index = ui_dir.join("index.html");
        let serve_dir = ServeDir::new(&ui_dir).not_found_service(ServeFile::new(&index));
        api_router.fallback_service(serve_dir)
    } else {
        tracing::info!("No UI directory found — API-only mode (use Vite dev server for UI)");
        api_router
    };

    // ── Start server ────────────────────────────────────────────────
    let addr = format!("{}:{}", bind, port);
    tracing::info!("Listening on {}", addr);

    let listener = TcpListener::bind(&addr).await.unwrap_or_else(|e| {
        eprintln!("Cannot bind to {}: {}", addr, e);
        std::process::exit(1);
    });

    // Graceful shutdown on Ctrl+C
    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutting down...");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {}", e);
            std::process::exit(1);
        });
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
