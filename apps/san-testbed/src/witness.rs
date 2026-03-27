//! Mock witness HTTP server for testbed.
//!
//! Modes:
//! - AllowAll: always returns {"allowed": true}
//! - DenyAll: always returns {"allowed": false}
//! - Smart: grants quorum to the lowest node_id only (tie-breaking)
//! - Off: drops connections (simulates unreachable witness)

use axum::{extract::{Path, State}, http::StatusCode, Json, Router, routing::get, response::IntoResponse};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq)]
pub enum WitnessMode {
    AllowAll,
    DenyAll,
    Smart,
    Off,
}

pub struct WitnessState {
    pub mode: RwLock<WitnessMode>,
    /// In Smart mode, track which node_ids have asked for witness.
    /// The lowest node_id gets allowed, others denied.
    pub requesting_nodes: RwLock<HashSet<String>>,
    /// Send to this channel to shut down the server.
    shutdown_tx: RwLock<Option<oneshot::Sender<()>>>,
}

pub type WitnessHandle = Arc<WitnessState>;

async fn witness_handler(
    State(state): State<WitnessHandle>,
    Path(node_id): Path<String>,
) -> impl IntoResponse {
    let mode = state.mode.read().unwrap().clone();
    match mode {
        WitnessMode::AllowAll => {
            Json(serde_json::json!({"allowed": true})).into_response()
        }
        WitnessMode::DenyAll => {
            Json(serde_json::json!({"allowed": false, "reason": "mock deny-all"})).into_response()
        }
        WitnessMode::Smart => {
            let mut nodes = state.requesting_nodes.write().unwrap();
            nodes.insert(node_id.clone());
            let lowest = nodes.iter().min().cloned().unwrap_or_default();
            let allowed = node_id == lowest;
            Json(serde_json::json!({"allowed": allowed})).into_response()
        }
        WitnessMode::Off => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Start the witness mock server. Returns the handle for mode control.
pub async fn spawn(port: u16) -> WitnessHandle {
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let handle = Arc::new(WitnessState {
        mode: RwLock::new(WitnessMode::AllowAll),
        requesting_nodes: RwLock::new(HashSet::new()),
        shutdown_tx: RwLock::new(Some(shutdown_tx)),
    });

    let state = handle.clone();

    let app = Router::new()
        .route("/api/san/witness/{node_id}", get(witness_handler))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await
        .unwrap_or_else(|e| panic!("Cannot bind witness to {}: {}", addr, e));

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    // Give server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    handle
}

/// Shut down the witness server so the port is released.
pub fn shutdown(handle: &WitnessHandle) {
    if let Some(tx) = handle.shutdown_tx.write().unwrap().take() {
        tx.send(()).ok();
    }
}

pub fn set_mode(handle: &WitnessHandle, mode: WitnessMode) {
    if mode == WitnessMode::Smart {
        handle.requesting_nodes.write().unwrap().clear();
    }
    *handle.mode.write().unwrap() = mode;
}
