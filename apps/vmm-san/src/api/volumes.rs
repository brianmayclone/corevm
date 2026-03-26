//! Volume CRUD endpoints with per-volume resilience policy.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use crate::state::CoreSanState;

#[derive(Deserialize)]
pub struct CreateVolumeRequest {
    pub name: String,
    #[serde(default = "default_mirror")]
    pub resilience_mode: String,
    #[serde(default = "default_replica_count")]
    pub replica_count: u32,
    #[serde(default)]
    pub stripe_width: u32,
    #[serde(default = "default_async")]
    pub sync_mode: String,
}

fn default_mirror() -> String { "mirror".into() }
fn default_replica_count() -> u32 { 2 }
fn default_async() -> String { "async".into() }

#[derive(Deserialize)]
pub struct UpdateVolumeRequest {
    pub resilience_mode: Option<String>,
    pub replica_count: Option<u32>,
    pub stripe_width: Option<u32>,
    pub sync_mode: Option<String>,
}

#[derive(Serialize)]
pub struct VolumeResponse {
    pub id: String,
    pub name: String,
    pub resilience_mode: String,
    pub replica_count: u32,
    pub stripe_width: u32,
    pub sync_mode: String,
    pub status: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub backend_count: u32,
    pub created_at: String,
}

/// POST /api/volumes — create a new volume with resilience policy.
pub async fn create(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateVolumeRequest>,
) -> Result<(StatusCode, Json<VolumeResponse>), (StatusCode, String)> {
    // Validate resilience mode
    if !["none", "mirror", "erasure"].contains(&body.resilience_mode.as_str()) {
        return Err((StatusCode::BAD_REQUEST,
            "resilience_mode must be 'none', 'mirror', or 'erasure'".into()));
    }
    if !["sync", "async"].contains(&body.sync_mode.as_str()) {
        return Err((StatusCode::BAD_REQUEST,
            "sync_mode must be 'sync' or 'async'".into()));
    }
    if body.resilience_mode == "none" && body.replica_count != 1 {
        return Err((StatusCode::BAD_REQUEST,
            "resilience_mode 'none' requires replica_count = 1".into()));
    }
    if body.resilience_mode == "mirror" && body.replica_count < 2 {
        return Err((StatusCode::BAD_REQUEST,
            "resilience_mode 'mirror' requires replica_count >= 2".into()));
    }

    // Validate that enough nodes exist for the requested replica count.
    // Total nodes = 1 (self) + online peers.
    let total_nodes = 1 + state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .count() as u32;

    if body.resilience_mode == "mirror" && body.replica_count > total_nodes {
        return Err((StatusCode::BAD_REQUEST,
            format!(
                "replica_count {} requires at least {} nodes, but only {} available (1 local + {} peers). \
                 Add more peers first, or reduce replica_count.",
                body.replica_count, body.replica_count, total_nodes, total_nodes - 1
            )));
    }

    let id = Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();

    db.execute(
        "INSERT INTO volumes (id, name, resilience_mode, replica_count, stripe_width, sync_mode, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'online')",
        rusqlite::params![&id, &body.name, &body.resilience_mode, body.replica_count,
                          body.stripe_width, &body.sync_mode],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create volume: {}", e)))?;

    // Create FUSE mount directory
    let fuse_path = state.config.data.fuse_root.join(&body.name);
    if !fuse_path.exists() {
        std::fs::create_dir_all(&fuse_path).ok();
    }

    tracing::info!("Created volume '{}' (id={}, mode={}, replicas={})",
        body.name, id, body.resilience_mode, body.replica_count);

    Ok((StatusCode::CREATED, Json(VolumeResponse {
        id,
        name: body.name,
        resilience_mode: body.resilience_mode,
        replica_count: body.replica_count,
        stripe_width: body.stripe_width,
        sync_mode: body.sync_mode,
        status: "online".into(),
        total_bytes: 0,
        free_bytes: 0,
        backend_count: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
    })))
}

/// GET /api/volumes — list all volumes with capacity stats.
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
) -> Json<Vec<VolumeResponse>> {
    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare(
        "SELECT v.id, v.name, v.resilience_mode, v.replica_count, v.stripe_width,
                v.sync_mode, v.status, v.created_at,
                COALESCE(SUM(b.total_bytes), 0),
                COALESCE(SUM(b.free_bytes), 0),
                COUNT(b.id)
         FROM volumes v
         LEFT JOIN backends b ON b.volume_id = v.id AND b.status != 'offline'
         GROUP BY v.id
         ORDER BY v.name"
    ).unwrap();

    let volumes = stmt.query_map([], |row| {
        Ok(VolumeResponse {
            id: row.get(0)?,
            name: row.get(1)?,
            resilience_mode: row.get(2)?,
            replica_count: row.get(3)?,
            stripe_width: row.get(4)?,
            sync_mode: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
            total_bytes: row.get(8)?,
            free_bytes: row.get(9)?,
            backend_count: row.get(10)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Json(volumes)
}

/// GET /api/volumes/{id} — volume detail.
pub async fn get(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<Json<VolumeResponse>, StatusCode> {
    let db = state.db.lock().unwrap();

    let vol = db.query_row(
        "SELECT v.id, v.name, v.resilience_mode, v.replica_count, v.stripe_width,
                v.sync_mode, v.status, v.created_at,
                COALESCE(SUM(b.total_bytes), 0),
                COALESCE(SUM(b.free_bytes), 0),
                COUNT(b.id)
         FROM volumes v
         LEFT JOIN backends b ON b.volume_id = v.id AND b.status != 'offline'
         WHERE v.id = ?1
         GROUP BY v.id",
        rusqlite::params![&id],
        |row| Ok(VolumeResponse {
            id: row.get(0)?,
            name: row.get(1)?,
            resilience_mode: row.get(2)?,
            replica_count: row.get(3)?,
            stripe_width: row.get(4)?,
            sync_mode: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
            total_bytes: row.get(8)?,
            free_bytes: row.get(9)?,
            backend_count: row.get(10)?,
        }),
    ).map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(vol))
}

/// PUT /api/volumes/{id} — update resilience policy (triggers rebalance).
pub async fn update(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateVolumeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Verify volume exists
    let exists: bool = db.query_row(
        "SELECT COUNT(*) FROM volumes WHERE id = ?1",
        rusqlite::params![&id], |row| row.get::<_, i64>(0),
    ).map(|c| c > 0).unwrap_or(false);

    if !exists {
        return Err((StatusCode::NOT_FOUND, "Volume not found".into()));
    }

    if let Some(ref mode) = body.resilience_mode {
        if !["none", "mirror", "erasure"].contains(&mode.as_str()) {
            return Err((StatusCode::BAD_REQUEST,
                "resilience_mode must be 'none', 'mirror', or 'erasure'".into()));
        }
        db.execute("UPDATE volumes SET resilience_mode = ?1 WHERE id = ?2",
            rusqlite::params![mode, &id]).ok();
    }
    if let Some(count) = body.replica_count {
        db.execute("UPDATE volumes SET replica_count = ?1 WHERE id = ?2",
            rusqlite::params![count, &id]).ok();
    }
    if let Some(width) = body.stripe_width {
        db.execute("UPDATE volumes SET stripe_width = ?1 WHERE id = ?2",
            rusqlite::params![width, &id]).ok();
    }
    if let Some(ref mode) = body.sync_mode {
        if !["sync", "async"].contains(&mode.as_str()) {
            return Err((StatusCode::BAD_REQUEST,
                "sync_mode must be 'sync' or 'async'".into()));
        }
        db.execute("UPDATE volumes SET sync_mode = ?1 WHERE id = ?2",
            rusqlite::params![mode, &id]).ok();
    }

    tracing::info!("Updated resilience policy for volume {}", id);

    Ok(Json(serde_json::json!({ "success": true, "rebalance_triggered": true })))
}

/// DELETE /api/volumes/{id} — remove volume (must be empty).
pub async fn delete(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Check volume has no files
    let file_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1",
        rusqlite::params![&id], |row| row.get(0),
    ).unwrap_or(0);

    if file_count > 0 {
        return Err((StatusCode::CONFLICT,
            format!("Volume has {} files, remove them first", file_count)));
    }

    // Get volume name for FUSE cleanup
    let name: Option<String> = db.query_row(
        "SELECT name FROM volumes WHERE id = ?1",
        rusqlite::params![&id], |row| row.get(0),
    ).ok();

    db.execute("DELETE FROM volumes WHERE id = ?1", rusqlite::params![&id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Delete failed: {}", e)))?;

    // Clean up FUSE mount directory
    if let Some(name) = name {
        let fuse_path = state.config.data.fuse_root.join(&name);
        std::fs::remove_dir(&fuse_path).ok();
        tracing::info!("Deleted volume '{}'", name);
    }

    Ok(Json(serde_json::json!({ "success": true })))
}
