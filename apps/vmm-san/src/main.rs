//! vmm-san — CoreSAN Software-Defined Storage daemon.
//!
//! Dedicated storage service that runs independently on every node.
//! CoreSAN peers communicate directly with each other — no dependency
//! on vmm-cluster. The cluster can discover and manage CoreSAN through
//! vmm-server, but CoreSAN operates autonomously.

mod config;
mod state;
mod db;
mod auth;
mod api;
mod engine;
mod storage;
mod peer;

use std::sync::{Arc, Mutex};
use config::CoreSanConfig;
use state::{CoreSanState, PeerConnection, PeerStatus};
use dashmap::DashMap;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    // ── Parse CLI args ──────────────────────────────────────────────
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "/etc/vmm/vmm-san.toml".to_string());

    // ── Load configuration ──────────────────────────────────────────
    let config = CoreSanConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| {
            eprintln!("Config error: {}", e);
            std::process::exit(1);
        });

    // ── Initialize logging ──────────────────────────────────────────
    let log_filter = config.logging.level.clone();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&log_filter))
        )
        .init();

    tracing::info!("CoreSAN v{} ({}) built {}",
        env!("CARGO_PKG_VERSION"), env!("COREVM_GIT_SHA"), env!("COREVM_BUILD_TIMESTAMP"));

    // ── Create data directory ───────────────────────────────────────
    let data_dir = &config.data.data_dir;
    if !data_dir.exists() {
        std::fs::create_dir_all(data_dir).unwrap_or_else(|e| {
            tracing::warn!("Cannot create data directory {}: {}", data_dir.display(), e);
        });
    }

    // ── Create FUSE root directory ──────────────────────────────────
    let fuse_root = &config.data.fuse_root;
    if !fuse_root.exists() {
        std::fs::create_dir_all(fuse_root).unwrap_or_else(|e| {
            tracing::warn!("Cannot create FUSE root {}: {}", fuse_root.display(), e);
        });
    }

    // ── Initialize database ─────────────────────────────────────────
    let db_path = data_dir.join("vmm-san.db");
    tracing::info!("Database: {}", db_path.display());

    let conn = rusqlite::Connection::open(&db_path).unwrap_or_else(|e| {
        eprintln!("Cannot open database: {}", e);
        std::process::exit(1);
    });

    db::init(&conn).unwrap_or_else(|e| {
        eprintln!("Database init failed: {}", e);
        std::process::exit(1);
    });

    // ── Generate or load node ID ────────────────────────────────────
    let node_id = load_or_create_node_id(&conn);
    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    tracing::info!("Node ID: {}", node_id);
    tracing::info!("Hostname: {}", hostname);

    // ── Auto-generate peer secret if empty ──────────────────────────
    let _peer_secret = if config.peer.secret.is_empty() {
        let s = uuid::Uuid::new_v4().to_string();
        tracing::info!("Generated peer secret (single-node mode, no auth required)");
        s
    } else {
        config.peer.secret.clone()
    };

    // ── Load existing peers into memory ─────────────────────────────
    let peers: DashMap<String, PeerConnection> = DashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT node_id, address, peer_port, hostname, status FROM peers"
        ).unwrap();
        let peer_list: Vec<_> = stmt.query_map([], |row| {
            Ok(PeerConnection {
                node_id: row.get(0)?,
                address: row.get(1)?,
                peer_port: row.get(2)?,
                hostname: row.get(3)?,
                status: match row.get::<_, String>(4)?.as_str() {
                    "online" => PeerStatus::Online,
                    "offline" => PeerStatus::Offline,
                    _ => PeerStatus::Connecting,
                },
                missed_heartbeats: 0,
            })
        }).unwrap().filter_map(|r| r.ok()).collect();

        for p in peer_list {
            tracing::info!("Loaded peer: {} ({})", p.hostname, p.node_id);
            peers.insert(p.node_id.clone(), p);
        }
    }

    // ── Build application state ─────────────────────────────────────
    let bind = config.server.bind.clone();
    let port = config.server.port;

    // Create push-replication channel before building state.
    // The receiver will be consumed by the push replicator task.
    let (write_tx, write_rx) = tokio::sync::mpsc::unbounded_channel();

    let state = Arc::new(CoreSanState {
        peers,
        db: Mutex::new(conn),
        config,
        node_id,
        hostname,
        started_at: std::time::Instant::now(),
        write_tx,
    });

    // ── Start background engines ────────────────────────────────────
    // All engines operate autonomously — no vmm-cluster dependency.

    // Push replicator — immediate write distribution to peers
    engine::push_replicator::spawn_with_rx(Arc::clone(&state), write_rx);
    tracing::info!("Push replicator started (immediate write distribution)");

    engine::push_replicator::spawn_log_cleanup(Arc::clone(&state));

    engine::peer_monitor::spawn(Arc::clone(&state));
    tracing::info!("Peer monitor started (5s heartbeat interval)");

    engine::replication::spawn(Arc::clone(&state));
    tracing::info!("Replication engine started");

    engine::repair::spawn(Arc::clone(&state));
    tracing::info!("Repair engine started ({}s interval)", state.config.integrity.repair_interval_secs);

    engine::integrity::spawn(Arc::clone(&state));
    tracing::info!("Integrity engine started ({}s interval)", state.config.integrity.interval_secs);

    engine::benchmark::spawn(Arc::clone(&state));
    tracing::info!("Benchmark engine started ({}s interval)", state.config.benchmark.interval_secs);

    engine::backend_refresh::spawn(Arc::clone(&state));
    tracing::info!("Backend refresh engine started (30s interval)");

    engine::fuse_mount::spawn_all(Arc::clone(&state));
    tracing::info!("FUSE mounts started");

    engine::rebalancer::spawn(Arc::clone(&state));
    tracing::info!("Rebalancer started (30s interval)");

    engine::discovery::spawn(Arc::clone(&state));
    tracing::info!("Discovery beacon started");

    // ── Build router ────────────────────────────────────────────────
    let app = api::router()
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    // ── Start server ────────────────────────────────────────────────
    let addr = format!("{}:{}", bind, port);
    tracing::info!("Listening on {}", addr);

    let listener = TcpListener::bind(&addr).await.unwrap_or_else(|e| {
        eprintln!("Cannot bind to {}: {}", addr, e);
        std::process::exit(1);
    });

    // Graceful shutdown on Ctrl+C
    let shutdown_state = state.clone();
    let shutdown = async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutting down CoreSAN...");
        engine::fuse_mount::unmount_all(&shutdown_state);
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {}", e);
            std::process::exit(1);
        });
}

/// Load node ID from the database, or generate a new one.
fn load_or_create_node_id(conn: &rusqlite::Connection) -> String {
    // Use a simple key-value table for node settings
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS node_settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )"
    ).ok();

    match conn.query_row(
        "SELECT value FROM node_settings WHERE key = 'node_id'",
        [], |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO node_settings (key, value) VALUES ('node_id', ?1)",
                rusqlite::params![&id],
            ).ok();
            id
        }
    }
}
