//! Chunk-level API endpoints — receive/serve individual chunks from peers.
//!
//! These endpoints are the core of chunk-based replication. Peers push/pull
//! individual chunks rather than whole files, enabling efficient RAID-aware
//! distribution across claimed disks.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use std::sync::Arc;
use crate::state::CoreSanState;
use crate::storage::chunk;

/// PUT /api/chunks/{volume_id}/{file_id}/{chunk_index} — receive a chunk from a peer.
/// Stores the chunk on a local backend and updates chunk_replicas.
pub async fn write_chunk(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, file_id, chunk_index)): Path<(String, i64, u32)>,
    body: axum::body::Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    // Find a local backend to store the chunk
    let (backend_id, backend_path) = {
        let db = state.db.lock().unwrap();

        // Get volume's local_raid policy to decide placement
        let local_raid: String = db.query_row(
            "SELECT local_raid FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id], |row| row.get(0),
        ).unwrap_or_else(|_| "stripe".into());

        let placements = chunk::place_chunk(&db, &volume_id, &state.node_id, chunk_index, &local_raid);
        if placements.is_empty() {
            return Err((StatusCode::NOT_FOUND, "No local backend available".into()));
        }
        placements[0].clone()
    };

    // Write chunk to disk
    let path = chunk::chunk_path(&backend_path, &volume_id, file_id, chunk_index);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {}", e)))?;
    }

    // Atomic write: temp + fsync + rename
    let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, &body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write: {}", e)))?;
    if let Ok(f) = std::fs::File::open(&tmp) {
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, &path).map_err(|e| {
        std::fs::remove_file(&tmp).ok();
        (StatusCode::INTERNAL_SERVER_ERROR, format!("rename: {}", e))
    })?;

    // Update database: ensure file_chunks entry exists, then add/update chunk_replica
    {
        let db = state.db.lock().unwrap();

        // Ensure file_chunks row exists
        let chunk_size = body.len() as u64;
        let offset = chunk_index as u64 * {
            db.query_row(
                "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
                rusqlite::params![&volume_id], |row| row.get::<_, u64>(0),
            ).unwrap_or(chunk::DEFAULT_CHUNK_SIZE)
        };

        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![file_id, chunk_index, offset, chunk_size],
        ).ok();

        // Get chunk_id
        if let Ok(chunk_id) = db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, chunk_index], |row| row.get::<_, i64>(0),
        ) {
            // Compute sha256
            use sha2::{Sha256, Digest};
            let sha256 = format!("{:x}", Sha256::digest(&body));
            db.execute(
                "UPDATE file_chunks SET sha256 = ?1, size_bytes = ?2 WHERE id = ?3",
                rusqlite::params![&sha256, chunk_size, chunk_id],
            ).ok();

            // Add/update chunk replica
            let now = chrono::Utc::now().to_rfc3339();
            db.execute(
                "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                 VALUES (?1, ?2, ?3, 'synced', ?4)",
                rusqlite::params![chunk_id, &backend_id, &state.node_id, &now],
            ).ok();
        }
    }

    tracing::debug!("Received chunk {}/{}/idx{} ({} bytes)", volume_id, file_id, chunk_index, body.len());
    Ok(StatusCode::OK)
}

/// GET /api/chunks/{volume_id}/{file_id}/{chunk_index} — serve a chunk to a peer.
/// Reads from local backends and returns the raw chunk data.
pub async fn read_chunk(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, file_id, chunk_index)): Path<(String, i64, u32)>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    // Find a local synced replica for this chunk
    let replicas: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT b.path FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
               AND cr.node_id = ?3 AND cr.state = 'synced'"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        stmt.query_map(
            rusqlite::params![file_id, chunk_index, &state.node_id],
            |row| row.get(0),
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
         .filter_map(|r| r.ok()).collect()
    };

    for backend_path in &replicas {
        let path = chunk::chunk_path(backend_path, &volume_id, file_id, chunk_index);
        if let Ok(data) = std::fs::read(&path) {
            return Ok(data);
        }
    }

    Err((StatusCode::NOT_FOUND, format!("Chunk {}/{}/idx{} not found locally", volume_id, file_id, chunk_index)))
}

/// POST /api/file-meta/sync — receive file metadata from a peer (leader or writer).
/// Creates/updates file_map and file_chunks entries so this node knows about the file.
pub async fn sync_file_meta(
    State(state): State<Arc<CoreSanState>>,
    Json(meta): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, String)> {
    let volume_id = meta["volume_id"].as_str()
        .ok_or((StatusCode::BAD_REQUEST, "missing volume_id".into()))?;
    let rel_path = meta["rel_path"].as_str()
        .ok_or((StatusCode::BAD_REQUEST, "missing rel_path".into()))?;
    let file_id = meta["file_id"].as_i64()
        .ok_or((StatusCode::BAD_REQUEST, "missing file_id".into()))?;
    let size_bytes = meta["size_bytes"].as_u64().unwrap_or(0);
    let sha256 = meta["sha256"].as_str().unwrap_or("");
    let version = meta["version"].as_i64().unwrap_or(0);
    let chunk_count = meta["chunk_count"].as_u64().unwrap_or(0) as u32;
    let chunk_size_bytes = meta["chunk_size_bytes"].as_u64()
        .unwrap_or(chunk::DEFAULT_CHUNK_SIZE);

    let db = state.db.lock().unwrap();
    let now = chrono::Utc::now().to_rfc3339();

    // Ensure file_map entry exists with correct metadata
    // Use the REMOTE file_id as a hint but our own DB assigns IDs
    db.execute(
        "INSERT INTO file_map (volume_id, rel_path, size_bytes, sha256, version, chunk_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(volume_id, rel_path) DO UPDATE SET
            size_bytes = MAX(file_map.size_bytes, excluded.size_bytes),
            sha256 = CASE WHEN excluded.version > file_map.version THEN excluded.sha256 ELSE file_map.sha256 END,
            version = MAX(file_map.version, excluded.version),
            chunk_count = MAX(file_map.chunk_count, excluded.chunk_count),
            updated_at = excluded.updated_at",
        rusqlite::params![volume_id, rel_path, size_bytes, sha256, version, chunk_count, &now],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("file_map sync: {}", e)))?;

    // Ensure file_chunks entries exist for all chunks
    let local_file_id: i64 = db.query_row(
        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![volume_id, rel_path], |row| row.get(0),
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("get file_id: {}", e)))?;

    for ci in 0..chunk_count {
        let offset = ci as u64 * chunk_size_bytes;
        let size = chunk_size_bytes.min(size_bytes.saturating_sub(offset));
        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![local_file_id, ci, offset, size],
        ).ok();
    }

    tracing::debug!("Synced file metadata: {}/{} v{} ({} chunks)",
        volume_id, rel_path, version, chunk_count);
    Ok(StatusCode::OK)
}
