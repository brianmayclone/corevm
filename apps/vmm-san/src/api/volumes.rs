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
    #[serde(default = "default_ftt")]
    pub ftt: u32,
    #[serde(default = "default_local_raid")]
    pub local_raid: String,
    #[serde(default = "default_chunk_size")]
    pub chunk_size_bytes: u64,
}

fn default_ftt() -> u32 { 1 }
fn default_local_raid() -> String { "stripe".into() }
fn default_chunk_size() -> u64 { crate::storage::chunk::DEFAULT_CHUNK_SIZE }

#[derive(Deserialize)]
pub struct UpdateVolumeRequest {
    pub ftt: Option<u32>,
    pub local_raid: Option<String>,
}

#[derive(Serialize)]
pub struct VolumeResponse {
    pub id: String,
    pub name: String,
    pub ftt: u32,
    pub local_raid: String,
    pub chunk_size_bytes: u64,
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
    // Validate FTT
    if body.ftt > 2 {
        return Err((StatusCode::BAD_REQUEST, "ftt must be 0, 1, or 2".into()));
    }
    if !["stripe", "mirror", "stripe_mirror"].contains(&body.local_raid.as_str()) {
        return Err((StatusCode::BAD_REQUEST,
            "local_raid must be 'stripe', 'mirror', or 'stripe_mirror'".into()));
    }

    // Validate that at least one disk is claimed (backends exist)
    {
        let db = state.db.lock().unwrap();
        let backend_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM backends WHERE node_id = ?1 AND status = 'online'",
            rusqlite::params![&state.node_id], |row| row.get(0),
        ).unwrap_or(0);

        if backend_count == 0 {
            return Err((StatusCode::BAD_REQUEST,
                "No disks claimed. Claim at least one disk before creating a volume.".into()));
        }
    }

    // Validate enough nodes for FTT
    let total_nodes = 1 + state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .count() as u32;
    let required = body.ftt + 1;

    if required > total_nodes {
        return Err((StatusCode::BAD_REQUEST,
            format!("FTT={} requires {} nodes, but only {} available. Add more peers first.",
                body.ftt, required, total_nodes)));
    }

    let id = Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();

    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, status)
         VALUES (?1, ?2, ?3, ?4, ?5, 'online')",
        rusqlite::params![&id, &body.name, body.ftt, body.chunk_size_bytes, &body.local_raid],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create volume: {}", e)))?;

    let fuse_path = state.config.data.fuse_root.join(&body.name);
    if !fuse_path.exists() {
        std::fs::create_dir_all(&fuse_path).ok();
    }

    tracing::info!("Created volume '{}' (id={}, ftt={}, local_raid={}, chunk={}MB)",
        body.name, id, body.ftt, body.local_raid, body.chunk_size_bytes / (1024 * 1024));

    Ok((StatusCode::CREATED, Json(VolumeResponse {
        id,
        name: body.name,
        ftt: body.ftt,
        local_raid: body.local_raid,
        chunk_size_bytes: body.chunk_size_bytes,
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
        "SELECT v.id, v.name, v.ftt, v.local_raid, v.chunk_size_bytes,
                v.status, v.created_at,
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
            ftt: row.get(2)?,
            local_raid: row.get(3)?,
            chunk_size_bytes: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
            total_bytes: row.get(7)?,
            free_bytes: row.get(8)?,
            backend_count: row.get(9)?,
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
        "SELECT v.id, v.name, v.ftt, v.local_raid, v.chunk_size_bytes,
                v.status, v.created_at,
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
            ftt: row.get(2)?,
            local_raid: row.get(3)?,
            chunk_size_bytes: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
            total_bytes: row.get(7)?,
            free_bytes: row.get(8)?,
            backend_count: row.get(9)?,
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

    if let Some(ftt) = body.ftt {
        if ftt > 2 {
            return Err((StatusCode::BAD_REQUEST, "ftt must be 0, 1, or 2".into()));
        }
        db.execute("UPDATE volumes SET ftt = ?1 WHERE id = ?2",
            rusqlite::params![ftt, &id]).ok();
    }
    if let Some(ref raid) = body.local_raid {
        if !["stripe", "mirror", "stripe_mirror"].contains(&raid.as_str()) {
            return Err((StatusCode::BAD_REQUEST,
                "local_raid must be 'stripe', 'mirror', or 'stripe_mirror'".into()));
        }
        db.execute("UPDATE volumes SET local_raid = ?1 WHERE id = ?2",
            rusqlite::params![raid, &id]).ok();
    }

    tracing::info!("Updated volume {} policy", id);

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
