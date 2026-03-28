//! Chunk-level API endpoints — receive/serve individual chunks from peers.
//!
//! These endpoints are the core of chunk-based replication. Peers push/pull
//! individual chunks rather than whole files, enabling efficient RAID-aware
//! distribution across claimed disks.
//!
//! Safety guarantees:
//! - Atomic write (tmp + fsync + rename) prevents partial chunks on disk
//! - SHA256 verification on write: sender can provide expected hash via header
//! - Read-back verification after write confirms data integrity
//! - DB updates wrapped in transactions to prevent inconsistency
//! - No re-replication (peer requests are not forwarded)

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use sha2::{Sha256, Digest};
use std::sync::Arc;
use crate::state::CoreSanState;
use crate::storage::chunk;

/// Custom header for sender to provide the expected SHA256 of the chunk.
const EXPECTED_SHA256_HEADER: &str = "X-CoreSAN-Chunk-SHA256";

/// PUT /api/chunks/{volume_id}/{file_id}/{chunk_index} — receive a chunk from a peer.
/// Stores the chunk on a local backend and updates chunk_replicas.
/// If the sender provides X-CoreSAN-Chunk-SHA256, the received data is verified against it.
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
            tracing::warn!("Chunk write REJECTED: SHA256 mismatch for {}/{}/idx{} expected={} actual={}",
                volume_id, file_id, chunk_index, expected_str, &actual_sha256[..8]);
            return Err((StatusCode::CONFLICT,
                format!("SHA256 mismatch: expected {} got {}", expected_str, actual_sha256)));
        }
    }

    // 2. Find a local backend to store the chunk
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

    // 3. Atomic write: temp + fsync + rename
    let path = chunk::chunk_path(&backend_path, &volume_id, file_id, chunk_index);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {}", e)))?;
    }

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

    // 4. Read-back verification — ensure what we wrote is actually on disk
    let readback_sha = match std::fs::read(&path) {
        Ok(d) => format!("{:x}", Sha256::digest(&d)),
        Err(e) => {
            std::fs::remove_file(&path).ok();
            return Err((StatusCode::INTERNAL_SERVER_ERROR,
                format!("readback failed: {}", e)));
        }
    };

    if readback_sha != actual_sha256 {
        std::fs::remove_file(&path).ok();
        tracing::error!("Chunk write: readback SHA256 mismatch! Disk may be failing. {}/{}/idx{}",
            volume_id, file_id, chunk_index);
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
            "Readback verification failed — possible disk error".into()));
    }

    // 5. Update database in a transaction
    {
        let db = state.db.lock().unwrap();
        let chunk_size = body.len() as u64;
        let vol_chunk_size = db.query_row(
            "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id], |row| row.get::<_, u64>(0),
        ).unwrap_or(chunk::DEFAULT_CHUNK_SIZE);
        let offset = chunk_index as u64 * vol_chunk_size;
        let now = chrono::Utc::now().to_rfc3339();

        db.execute("BEGIN IMMEDIATE", []).ok();

        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![file_id, chunk_index, offset, chunk_size],
        ).ok();

        if let Ok(chunk_id) = db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, chunk_index], |row| row.get::<_, i64>(0),
        ) {
            db.execute(
                "UPDATE file_chunks SET sha256 = ?1, size_bytes = ?2 WHERE id = ?3",
                rusqlite::params![&actual_sha256, chunk_size, chunk_id],
            ).ok();

            db.execute(
                "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                 VALUES (?1, ?2, ?3, 'synced', ?4)",
                rusqlite::params![chunk_id, &backend_id, &state.node_id, &now],
            ).ok();
        }

        db.execute("COMMIT", []).ok();
    }

    tracing::debug!("Received chunk {}/{}/idx{} ({} bytes, sha256={})",
        volume_id, file_id, chunk_index, body.len(), &actual_sha256[..8]);
    Ok(StatusCode::OK)
}

/// GET /api/chunks/{volume_id}/{file_id}/{chunk_index} — serve a chunk to a peer.
/// Reads from local backends, verifies SHA256 before serving.
pub async fn read_chunk(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, file_id, chunk_index)): Path<(String, i64, u32)>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    // Find local synced replica(s) with expected SHA256
    let replicas: Vec<(String, String, String)> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT b.path, cr.backend_id, COALESCE(fc.sha256, '') FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
               AND cr.node_id = ?3 AND cr.state = 'synced'"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        stmt.query_map(
            rusqlite::params![file_id, chunk_index, &state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
         .filter_map(|r| r.ok()).collect()
    };

    for (backend_path, backend_id, expected_sha256) in &replicas {
        let path = chunk::chunk_path(backend_path, &volume_id, file_id, chunk_index);
        if let Ok(data) = std::fs::read(&path) {
            // Verify SHA256 before serving — never send corrupt data to peers
            if !expected_sha256.is_empty() {
                let actual = format!("{:x}", Sha256::digest(&data));
                if actual != *expected_sha256 {
                    tracing::warn!("read_chunk: SHA256 mismatch on {}, marking error", path.display());
                    let db = state.db.lock().unwrap();
                    db.execute(
                        "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = (
                            SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2
                        ) AND backend_id = ?3",
                        rusqlite::params![file_id, chunk_index, backend_id],
                    ).ok();
                    continue; // Try next replica
                }
            }
            return Ok(data);
        }
    }

    Err((StatusCode::NOT_FOUND, format!("Chunk {}/{}/idx{} not found locally", volume_id, file_id, chunk_index)))
}

/// POST /api/file-meta/sync — receive file metadata from a peer (leader or writer).
/// Creates/updates file_map and file_chunks entries so this node knows about the file.
/// Uses transactions to ensure atomicity.
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
    let now = chrono::Utc::now().to_rfc3339();

    db.execute("BEGIN IMMEDIATE", []).ok();

    // Upsert file_map — only update if incoming version is newer
    db.execute(
        "INSERT INTO file_map (volume_id, rel_path, size_bytes, sha256, version, chunk_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(volume_id, rel_path) DO UPDATE SET
            size_bytes = CASE WHEN excluded.version > file_map.version THEN excluded.size_bytes ELSE file_map.size_bytes END,
            sha256 = CASE WHEN excluded.version > file_map.version THEN excluded.sha256 ELSE file_map.sha256 END,
            version = MAX(file_map.version, excluded.version),
            chunk_count = CASE WHEN excluded.version > file_map.version THEN excluded.chunk_count ELSE MAX(file_map.chunk_count, excluded.chunk_count) END,
            updated_at = CASE WHEN excluded.version > file_map.version THEN excluded.updated_at ELSE file_map.updated_at END",
        rusqlite::params![volume_id, rel_path, size_bytes, sha256, version, chunk_count, &now],
    ).map_err(|e| {
        db.execute("ROLLBACK", []).ok();
        (StatusCode::INTERNAL_SERVER_ERROR, format!("file_map sync: {}", e))
    })?;

    // Ensure file_chunks entries exist for all chunks
    let local_file_id: i64 = db.query_row(
        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![volume_id, rel_path], |row| row.get(0),
    ).map_err(|e| {
        db.execute("ROLLBACK", []).ok();
        (StatusCode::INTERNAL_SERVER_ERROR, format!("get file_id: {}", e))
    })?;

    for ci in 0..chunk_count {
        let offset = ci as u64 * chunk_size_bytes;
        let size = chunk_size_bytes.min(size_bytes.saturating_sub(offset));
        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![local_file_id, ci, offset, size],
        ).ok();
    }

    db.execute("COMMIT", []).ok();

    tracing::debug!("Synced file metadata: {}/{} v{} ({} chunks)",
        volume_id, rel_path, version, chunk_count);
    Ok(StatusCode::OK)
}
