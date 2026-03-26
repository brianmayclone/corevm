//! Backend (mountpoint) management endpoints.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use crate::state::CoreSanState;

#[derive(Deserialize)]
pub struct AddBackendRequest {
    pub path: String,
}

#[derive(Serialize)]
pub struct BackendResponse {
    pub id: String,
    pub volume_id: String,
    pub node_id: String,
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub status: String,
    pub last_check: Option<String>,
}

/// POST /api/volumes/{id}/backends — add a local mountpoint as backend.
pub async fn add(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
    Json(body): Json<AddBackendRequest>,
) -> Result<(StatusCode, Json<BackendResponse>), (StatusCode, String)> {
    let path = std::path::Path::new(&body.path);

    // Create the directory if it doesn't exist
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|e| (StatusCode::BAD_REQUEST,
                format!("Cannot create directory {}: {}", body.path, e)))?;
        tracing::info!("Created backend directory: {}", body.path);
    }
    if !path.is_dir() {
        return Err((StatusCode::BAD_REQUEST,
            format!("Path is not a directory: {}", body.path)));
    }

    // Get filesystem stats
    let (total_bytes, free_bytes) = get_fs_stats(&body.path);

    let id = Uuid::new_v4().to_string();
    let db = state.db.lock().unwrap();

    // Verify volume exists
    let exists: bool = db.query_row(
        "SELECT COUNT(*) FROM volumes WHERE id = ?1",
        rusqlite::params![&volume_id], |row| row.get::<_, i64>(0),
    ).map(|c| c > 0).unwrap_or(false);

    if !exists {
        return Err((StatusCode::NOT_FOUND, "Volume not found".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO backends (id, volume_id, node_id, path, total_bytes, free_bytes, status, last_check)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'online', ?7)",
        rusqlite::params![&id, &volume_id, &state.node_id, &body.path,
                          total_bytes, free_bytes, &now],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to add backend: {}", e)))?;

    tracing::info!("Added backend '{}' to volume {} ({}B total, {}B free)",
        body.path, volume_id, total_bytes, free_bytes);

    Ok((StatusCode::CREATED, Json(BackendResponse {
        id,
        volume_id,
        node_id: state.node_id.clone(),
        path: body.path,
        total_bytes,
        free_bytes,
        status: "online".into(),
        last_check: Some(now),
    })))
}

/// GET /api/volumes/{id}/backends — list all backends for a volume.
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
) -> Json<Vec<BackendResponse>> {
    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare(
        "SELECT id, volume_id, node_id, path, total_bytes, free_bytes, status, last_check
         FROM backends WHERE volume_id = ?1 ORDER BY path"
    ).unwrap();

    let backends = stmt.query_map(rusqlite::params![&volume_id], |row| {
        Ok(BackendResponse {
            id: row.get(0)?,
            volume_id: row.get(1)?,
            node_id: row.get(2)?,
            path: row.get(3)?,
            total_bytes: row.get(4)?,
            free_bytes: row.get(5)?,
            status: row.get(6)?,
            last_check: row.get(7)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Json(backends)
}

/// DELETE /api/volumes/{volume_id}/backends/{backend_id} — remove backend (drains first).
pub async fn remove(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, backend_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Check if backend has files that need to be replicated elsewhere
    let replica_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM file_replicas WHERE backend_id = ?1 AND state = 'synced'",
        rusqlite::params![&backend_id], |row| row.get(0),
    ).unwrap_or(0);

    if replica_count > 0 {
        // Mark as draining — the repair engine will move data to other backends
        db.execute(
            "UPDATE backends SET status = 'draining' WHERE id = ?1 AND volume_id = ?2",
            rusqlite::params![&backend_id, &volume_id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

        tracing::info!("Backend {} marked as draining ({} replicas to relocate)",
            backend_id, replica_count);

        return Ok(Json(serde_json::json!({
            "success": true,
            "draining": true,
            "replicas_to_relocate": replica_count
        })));
    }

    db.execute(
        "DELETE FROM backends WHERE id = ?1 AND volume_id = ?2",
        rusqlite::params![&backend_id, &volume_id],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    tracing::info!("Removed backend {} from volume {}", backend_id, volume_id);

    Ok(Json(serde_json::json!({ "success": true, "draining": false })))
}

/// Get filesystem stats (total, free) for a path using statvfs.
fn get_fs_stats(path: &str) -> (u64, u64) {
    use std::ffi::CString;
    let c_path = match CString::new(path) {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };

    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
            let total = stat.f_blocks as u64 * stat.f_frsize as u64;
            let free = stat.f_bavail as u64 * stat.f_frsize as u64;
            (total, free)
        } else {
            (0, 0)
        }
    }
}
