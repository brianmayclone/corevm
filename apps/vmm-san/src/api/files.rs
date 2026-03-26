//! File operations — list, read, write, delete files on volumes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use crate::state::CoreSanState;
use crate::storage::file_map;

#[derive(Serialize)]
pub struct FileEntry {
    pub rel_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at: String,
    pub updated_at: String,
    pub replica_count: u32,
    pub synced_count: u32,
}

/// GET /api/volumes/{id}/files — list all files in a volume.
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
) -> Result<Json<Vec<FileEntry>>, StatusCode> {
    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare(
        "SELECT fm.rel_path, fm.size_bytes, fm.sha256, fm.created_at, fm.updated_at,
                COUNT(fr.backend_id) AS replica_count,
                SUM(CASE WHEN fr.state = 'synced' THEN 1 ELSE 0 END) AS synced_count
         FROM file_map fm
         LEFT JOIN file_replicas fr ON fr.file_id = fm.id
         WHERE fm.volume_id = ?1
         GROUP BY fm.id
         ORDER BY fm.rel_path"
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let files = stmt.query_map(rusqlite::params![&volume_id], |row| {
        Ok(FileEntry {
            rel_path: row.get(0)?,
            size_bytes: row.get(1)?,
            sha256: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            replica_count: row.get(5)?,
            synced_count: row.get(6)?,
        })
    }).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
      .filter_map(|r| r.ok()).collect();

    Ok(Json(files))
}

/// GET /api/volumes/{id}/files/*path — read/stream a file.
pub async fn read(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let local_path = {
        let db = state.db.lock().unwrap();
        file_map::find_local_replica(&db, &volume_id, &rel_path, &state.node_id)
    };

    match local_path {
        Some(path) => {
            tokio::fs::read(&path).await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Read error: {}", e)))
        }
        None => {
            // TODO: Fetch from peer node (Phase 2)
            Err((StatusCode::NOT_FOUND, "File not found on local node".into()))
        }
    }
}

/// PUT /api/volumes/{id}/files/*path — write a file (creates or overwrites).
/// Uses atomic write with write-lease acquisition and immediate push replication.
pub async fn write(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<FileEntry>, (StatusCode, String)> {
    // Check quorum — fenced nodes reject writes
    let quorum = *state.quorum_status.read().unwrap();
    if quorum == crate::state::QuorumStatus::Fenced {
        return Err((StatusCode::SERVICE_UNAVAILABLE,
            "node is fenced (no quorum) — writes are not allowed".into()));
    }

    // Check volume sync_mode — 'quorum' mode not yet implemented
    {
        let db = state.db.lock().unwrap();
        let sync_mode: String = db.query_row(
            "SELECT sync_mode FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id],
            |row| row.get(0),
        ).unwrap_or_else(|_| "async".into());
        if sync_mode == "quorum" {
            return Err((StatusCode::NOT_IMPLEMENTED,
                "sync_mode 'quorum' is not yet implemented".into()));
        }
    }

    // Select the best local backend
    let (backend_id, backend_path) = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT id, path FROM backends
             WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'
             ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![&volume_id, &state.node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).map_err(|_| (StatusCode::NOT_FOUND,
            "No local backend available for this volume".into()))?
    };

    // Atomic write: lease → temp file → fsync → rename → DB update → write_log
    let new_version = {
        let db = state.db.lock().unwrap();
        crate::engine::write_lease::atomic_write(
            &db, &volume_id, &rel_path, &state.node_id,
            &backend_id, &backend_path, &body, None, quorum,
        ).map_err(|e| (StatusCode::CONFLICT, e))?
    };

    let size = body.len() as u64;
    use sha2::{Sha256, Digest};
    let sha256 = format!("{:x}", Sha256::digest(&body));
    let now = chrono::Utc::now().to_rfc3339();

    // Push to peers immediately (non-blocking channel send)
    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.clone(),
        rel_path: rel_path.clone(),
        version: new_version,
        data: std::sync::Arc::new(body.to_vec()),
        writer_node_id: state.node_id.clone(),
    });

    tracing::debug!("Wrote file {}/{} v{} ({} bytes)", volume_id, rel_path, new_version, size);

    Ok(Json(FileEntry {
        rel_path,
        size_bytes: size,
        sha256,
        created_at: now.clone(),
        updated_at: now,
        replica_count: 1,
        synced_count: 1,
    }))
}

/// DELETE /api/volumes/{id}/files/*path — delete a file and all replicas.
pub async fn delete(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Find all local replicas and delete the physical files
    let mut stmt = db.prepare(
        "SELECT b.path FROM file_replicas fr
         JOIN backends b ON b.id = fr.backend_id
         JOIN file_map fm ON fm.id = fr.file_id
         WHERE fm.volume_id = ?1 AND fm.rel_path = ?2 AND b.node_id = ?3"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    let paths: Vec<String> = stmt.query_map(
        rusqlite::params![&volume_id, &rel_path, &state.node_id],
        |row| row.get(0),
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
     .filter_map(|r| r.ok()).collect();

    for backend_path in paths {
        let full_path = std::path::Path::new(&backend_path).join(&rel_path);
        std::fs::remove_file(&full_path).ok();
    }

    // Remove from database — delete replicas first to avoid FK constraint
    let file_id: Option<i64> = db.query_row(
        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![&volume_id, &rel_path], |row| row.get(0),
    ).ok();
    if let Some(fid) = file_id {
        db.execute("DELETE FROM integrity_log WHERE file_id = ?1", rusqlite::params![fid]).ok();
        db.execute("DELETE FROM file_replicas WHERE file_id = ?1", rusqlite::params![fid]).ok();
        db.execute("DELETE FROM write_log WHERE file_id = ?1", rusqlite::params![fid]).ok();
    }
    db.execute(
        "DELETE FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![&volume_id, &rel_path],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    tracing::info!("Deleted file {}/{}", volume_id, rel_path);

    Ok(Json(serde_json::json!({ "success": true })))
}

use serde::Deserialize;

#[derive(Deserialize)]
pub struct MkdirRequest {
    pub path: String,
}

/// POST /api/volumes/{id}/mkdir — create a directory inside a volume.
pub async fn mkdir(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
    Json(body): Json<MkdirRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Create the directory on all local backends
    let backends: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT path FROM backends WHERE node_id = ?1 AND status = 'online'"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        let result = stmt.query_map(rusqlite::params![&state.node_id], |row| row.get(0))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
            .filter_map(|r| r.ok()).collect();
        result
    };

    for bp in &backends {
        let dir = std::path::Path::new(bp)
            .join(".coresan").join(&volume_id).join(&body.path);
        std::fs::create_dir_all(&dir).ok();
    }

    tracing::info!("Created directory {}/{}", volume_id, body.path);

    Ok(Json(serde_json::json!({ "success": true, "path": body.path })))
}

/// GET /api/volumes/{id}/browse/{*path} — list directory contents inside a volume.
pub async fn browse(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, dir_path)): Path<(String, String)>,
) -> Result<Json<Vec<BrowseEntry>>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // List files in file_map that are direct children of dir_path
    let prefix = if dir_path.is_empty() || dir_path == "/" {
        String::new()
    } else {
        format!("{}/", dir_path.trim_end_matches('/'))
    };

    let pattern = format!("{}%", prefix);

    let mut stmt = db.prepare(
        "SELECT rel_path, size_bytes, updated_at FROM file_map
         WHERE volume_id = ?1 AND rel_path LIKE ?2"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    let all_paths: Vec<(String, u64, String)> = stmt.query_map(
        rusqlite::params![&volume_id, &pattern],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
     .filter_map(|r| r.ok()).collect();

    let mut entries = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for (path, size, updated) in all_paths {
        let suffix = if prefix.is_empty() {
            path.as_str()
        } else {
            match path.strip_prefix(&prefix) {
                Some(s) => s,
                None => continue,
            }
        };

        if let Some(slash) = suffix.find('/') {
            let dir_name = &suffix[..slash];
            if seen_dirs.insert(dir_name.to_string()) {
                entries.push(BrowseEntry {
                    name: dir_name.to_string(),
                    is_dir: true,
                    size_bytes: 0,
                    updated_at: String::new(),
                });
            }
        } else if !suffix.is_empty() {
            entries.push(BrowseEntry {
                name: suffix.to_string(),
                is_dir: false,
                size_bytes: size,
                updated_at: updated,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });

    Ok(Json(entries))
}

#[derive(Serialize)]
pub struct BrowseEntry {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub updated_at: String,
}

/// GET /api/volumes/{id}/browse — browse root directory of a volume.
pub async fn browse_root(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
) -> Result<Json<Vec<BrowseEntry>>, (StatusCode, String)> {
    browse(State(state), Path((volume_id, String::new()))).await
}
