//! Peer management endpoints — join, leave, list, heartbeat.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::{CoreSanState, PeerConnection, PeerStatus};

#[derive(Deserialize)]
pub struct JoinRequest {
    pub address: String,
    pub node_id: String,
    pub hostname: String,
    #[serde(default = "default_peer_port")]
    pub peer_port: u16,
    #[serde(default)]
    pub secret: String,
}

fn default_peer_port() -> u16 { 7444 }

#[derive(Serialize)]
pub struct PeerResponse {
    pub node_id: String,
    pub address: String,
    pub peer_port: u16,
    pub hostname: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
}

#[derive(Deserialize)]
pub struct HeartbeatRequest {
    pub node_id: String,
    pub hostname: String,
    pub uptime_secs: u64,
    /// The sender's reachable address — used to fix stale peer addresses.
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Serialize)]
pub struct HeartbeatResponse {
    pub node_id: String,
    pub hostname: String,
    pub accepted: bool,
}

/// POST /api/peers/join — register a new peer.
pub async fn join(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<JoinRequest>,
) -> Result<(StatusCode, Json<PeerResponse>), (StatusCode, String)> {
    // Validate secret if configured
    if !state.config.peer.secret.is_empty() && body.secret != state.config.peer.secret {
        return Err((StatusCode::UNAUTHORIZED, "Invalid peer secret".into()));
    }

    // Don't add ourselves
    if body.node_id == state.node_id {
        return Err((StatusCode::BAD_REQUEST, "Cannot add self as peer".into()));
    }

    let db = state.db.lock().unwrap();
    let now = chrono::Utc::now().to_rfc3339();

    db.execute(
        "INSERT OR REPLACE INTO peers (node_id, address, peer_port, hostname, status, last_heartbeat, joined_at)
         VALUES (?1, ?2, ?3, ?4, 'online', ?5, ?5)",
        rusqlite::params![&body.node_id, &body.address, body.peer_port, &body.hostname, &now],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    // Update in-memory state
    state.peers.insert(body.node_id.clone(), PeerConnection {
        node_id: body.node_id.clone(),
        address: body.address.clone(),
        peer_port: body.peer_port,
        hostname: body.hostname.clone(),
        status: PeerStatus::Online,
        missed_heartbeats: 0,
    });

    tracing::info!("Peer joined: {} ({}) at {}", body.hostname, body.node_id, body.address);

    // Sync all existing volumes to the new peer
    let volumes: Vec<serde_json::Value> = {
        let mut stmt = db.prepare(
            "SELECT id, name, ftt, chunk_size_bytes, local_raid FROM volumes WHERE status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "ftt": row.get::<_, u32>(2)?,
                "chunk_size_bytes": row.get::<_, u64>(3)?,
                "local_raid": row.get::<_, String>(4)?,
            }))
        }).unwrap().filter_map(|r| r.ok()).collect()
    };

    if !volumes.is_empty() {
        let peer_addr = body.address.clone();
        let secret = state.config.peer.secret.clone();
        let vol_count = volumes.len();
        tokio::spawn(async move {
            let client = crate::peer::client::PeerClient::new(&secret);
            for vol in &volumes {
                client.sync_volume(&peer_addr, vol).await.ok();
            }
            tracing::info!("Synced {} volumes to new peer {}", vol_count, peer_addr);
        });
    }

    Ok((StatusCode::CREATED, Json(PeerResponse {
        node_id: body.node_id,
        address: body.address,
        peer_port: body.peer_port,
        hostname: body.hostname,
        status: "online".into(),
        last_heartbeat: Some(now),
    })))
}

/// GET /api/peers — list all peers.
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
) -> Json<Vec<PeerResponse>> {
    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare(
        "SELECT node_id, address, peer_port, hostname, status, last_heartbeat
         FROM peers ORDER BY hostname"
    ).unwrap();

    let peers = stmt.query_map([], |row| {
        Ok(PeerResponse {
            node_id: row.get(0)?,
            address: row.get(1)?,
            peer_port: row.get(2)?,
            hostname: row.get(3)?,
            status: row.get(4)?,
            last_heartbeat: row.get(5)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Json(peers)
}

/// DELETE /api/peers/{node_id} — remove a peer.
pub async fn remove(
    State(state): State<Arc<CoreSanState>>,
    Path(node_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    db.execute("DELETE FROM peers WHERE node_id = ?1", rusqlite::params![&node_id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    state.peers.remove(&node_id);

    tracing::info!("Peer removed: {}", node_id);

    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/peers/heartbeat — peer heartbeat (called by peer monitor).
pub async fn heartbeat(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<HeartbeatRequest>,
) -> Json<HeartbeatResponse> {
    // Update last heartbeat time and fix stale address if provided
    if let Some(mut peer) = state.peers.get_mut(&body.node_id) {
        if let Some(ref addr) = body.address {
            if !addr.is_empty() && peer.address != *addr {
                tracing::info!("Peer {} address updated: {} -> {}", body.node_id, peer.address, addr);
                peer.address = addr.clone();
            }
        }
        peer.status = PeerStatus::Online;
        peer.missed_heartbeats = 0;
    }

    let db = state.db.lock().unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    if let Some(ref addr) = body.address {
        if !addr.is_empty() {
            db.execute(
                "UPDATE peers SET status = 'online', last_heartbeat = ?1, address = ?3 WHERE node_id = ?2",
                rusqlite::params![&now, &body.node_id, addr],
            ).ok();
        } else {
            db.execute(
                "UPDATE peers SET status = 'online', last_heartbeat = ?1 WHERE node_id = ?2",
                rusqlite::params![&now, &body.node_id],
            ).ok();
        }
    } else {
        db.execute(
            "UPDATE peers SET status = 'online', last_heartbeat = ?1 WHERE node_id = ?2",
            rusqlite::params![&now, &body.node_id],
        ).ok();
    }

    Json(HeartbeatResponse {
        node_id: state.node_id.clone(),
        hostname: state.hostname.clone(),
        accepted: true,
    })
}
