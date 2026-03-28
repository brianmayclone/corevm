//! Chunk-level API endpoints — receive/serve individual chunks from peers.
//!
//! Safety guarantees:
//! - Atomic write (tmp + fsync + rename) prevents partial chunks on disk
//! - SHA256 verification on write (sender provides expected hash via header)
//! - Read-back verification after write confirms disk integrity
//! - DB updates via ChunkService with proper error handling
//! - No re-replication (peer requests are not forwarded)

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use sha2::{Sha256, Digest};
use std::sync::Arc;
use crate::state::CoreSanState;
use crate::storage::chunk;
use crate::services::chunk::ChunkService;
use crate::services::file::FileService;

const EXPECTED_SHA256_HEADER: &str = "X-CoreSAN-Chunk-SHA256";

/// PUT /api/chunks/{volume_id}/{file_id}/{chunk_index} — receive a chunk from a peer.
pub async fn write_chunk(
    headers: HeaderMap,
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, file_id, chunk_index)): Path<(String, i64, u32)>,
    body: axum::body::Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    // 1. Verify SHA256 if sender provided expected hash
    let actual_sha256 = format!("{:x}", Sha256::digest(&body));
    if let Some(expected) = headers.get(EXPECTED_SHA256_HEADER) {
        let expected_str = expected.to_str().unwrap_or("");
        if !expected_str.is_empty() && expected_str != actual_sha256 {
            tracing::warn!("Chunk write REJECTED: SHA256 mismatch {}/{}/idx{}", volume_id, file_id, chunk_index);
            return Err((StatusCode::CONFLICT,
                format!("SHA256 mismatch: expected {} got {}", expected_str, actual_sha256)));
        }
    }

    // 2. Find a local backend
    let (backend_id, backend_path) = {
        let db = state.db.lock().unwrap();
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

    // 3. Atomic write to disk: temp + fsync + rename
    let path = chunk::chunk_path(&backend_path, &volume_id, file_id, chunk_index);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {}", e)))?;
    }

    let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, &body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("write: {}", e)))?;
    if let Ok(f) = std::fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        (StatusCode::INTERNAL_SERVER_ERROR, format!("rename: {}", e))
    })?;

    // 4. Read-back verification
    let readback_sha = match std::fs::read(&path) {
        Ok(d) => format!("{:x}", Sha256::digest(&d)),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("readback failed: {}", e)));
        }
    };
    if readback_sha != actual_sha256 {
        let _ = std::fs::remove_file(&path);
        tracing::error!("write_chunk: readback SHA256 mismatch! Disk error? {}/{}/idx{}", volume_id, file_id, chunk_index);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "Readback verification failed".into()));
    }

    // 5. Update DB via ChunkService
    {
        let db = state.db.lock().unwrap();
        let vol_chunk_size = db.query_row(
            "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id], |row| row.get::<_, u64>(0),
        ).unwrap_or(chunk::DEFAULT_CHUNK_SIZE);

        if let Err(e) = ChunkService::receive_chunk(
            &db, file_id, chunk_index, body.len() as u64,
            vol_chunk_size, &actual_sha256, &backend_id, &state.node_id,
        ) {
            tracing::error!("write_chunk DB error: {}", e);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("DB: {}", e)));
        }
    }

    tracing::info!("Received chunk {}/{}/idx{} ({} bytes, sha256={})",
        volume_id, file_id, chunk_index, body.len(), &actual_sha256[..8]);
    Ok(StatusCode::OK)
}

/// GET /api/chunks/{volume_id}/{file_id}/{chunk_index} — serve a chunk to a peer.
pub async fn read_chunk(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, file_id, chunk_index)): Path<(String, i64, u32)>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let replicas: Vec<(String, String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT b.path, cr.backend_id, COALESCE(fc.sha256, '') FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
               AND cr.node_id = ?3 AND cr.state = 'synced'"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        let rows = stmt.query_map(
            rusqlite::params![file_id, chunk_index, &state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for (backend_path, backend_id, expected_sha256) in &replicas {
        let path = chunk::chunk_path(backend_path, &volume_id, file_id, chunk_index);
        if let Ok(data) = std::fs::read(&path) {
            // Verify SHA256 before serving
            if !expected_sha256.is_empty() {
                let actual = format!("{:x}", Sha256::digest(&data));
                if actual != *expected_sha256 {
                    tracing::warn!("read_chunk: SHA256 mismatch on {}, marking error", path.display());
                    let db = state.db.lock().unwrap();
                    log_err!(ChunkService::mark_replica_error_by_backend(&db, file_id, chunk_index, backend_id),
                        "read_chunk: mark error");
                    continue;
                }
            }
            return Ok(data);
        }
    }

    Err((StatusCode::NOT_FOUND, format!("Chunk {}/{}/idx{} not found locally", volume_id, file_id, chunk_index)))
}

/// POST /api/file-meta/sync — receive file metadata from a peer.
pub async fn sync_file_meta(
    State(state): State<Arc<CoreSanState>>,
    Json(meta): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, String)> {
    let volume_id = meta["volume_id"].as_str()
        .ok_or((StatusCode::BAD_REQUEST, "missing volume_id".into()))?;
    let rel_path = meta["rel_path"].as_str()
        .ok_or((StatusCode::BAD_REQUEST, "missing rel_path".into()))?;
    let size_bytes = meta["size_bytes"].as_u64().unwrap_or(0);
    let sha256 = meta["sha256"].as_str().unwrap_or("");
    let version = meta["version"].as_i64().unwrap_or(0);
    let chunk_count = meta["chunk_count"].as_u64().unwrap_or(0) as u32;
    let chunk_size_bytes = meta["chunk_size_bytes"].as_u64()
        .unwrap_or(chunk::DEFAULT_CHUNK_SIZE);

    let db = state.db.lock().unwrap();

    FileService::sync_metadata(
        &db, volume_id, rel_path, size_bytes, sha256, version, chunk_count, chunk_size_bytes,
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    tracing::info!("Synced file metadata: {}/{} v{} ({} chunks)", volume_id, rel_path, version, chunk_count);
    Ok(StatusCode::OK)
}
