//! File operations — list, read, write, delete files on volumes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use crate::state::CoreSanState;

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
                (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 WHERE fc.file_id = fm.id) AS replica_count,
                (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 WHERE fc.file_id = fm.id AND cr.state = 'synced') AS synced_count
         FROM file_map fm
         WHERE fm.volume_id = ?1
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

/// GET /api/volumes/{id}/files/*path — read a file by assembling its chunks.
/// If chunks are not on this node, transparently fetches from a peer that has them.
/// Peer-originated requests (identified by X-CoreSAN-Secret header) only check locally
/// to prevent recursive peer-to-peer fetch loops.
pub async fn read(
    headers: axum::http::HeaderMap,
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let is_peer_request = headers.get(crate::auth::PEER_SECRET_HEADER).is_some();

    // 1. Look up file in file_map
    let file_info = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT fm.id, fm.size_bytes, v.chunk_size_bytes FROM file_map fm
             JOIN volumes v ON v.id = fm.volume_id
             WHERE fm.volume_id = ?1 AND fm.rel_path = ?2",
            rusqlite::params![&volume_id, &rel_path],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?)),
        ).ok()
    };

    let (file_id, file_size, chunk_size) = match file_info {
        Some(info) => info,
        None => {
            if is_peer_request {
                return Err((StatusCode::NOT_FOUND, "File not found locally".into()));
            }
            // File not in our DB — ask peers
            return read_from_peers(state.clone(), volume_id, rel_path).await;
        }
    };

    if file_size == 0 {
        return Ok(Vec::new());
    }

    // 2. Try to read all chunks locally
    let local_result = {
        let db = state.db.lock().unwrap();
        crate::storage::chunk::read_chunk_data(
            &db, file_id, 0, file_size,
            &volume_id, &state.node_id, chunk_size,
        )
    };

    // Check if we got valid data (no zero-filled gaps from missing chunks)
    if let Ok(data) = local_result {
        // Verify we have local chunk replicas (not just zero-filled)
        let has_local_chunks = {
            let db = state.db.lock().unwrap();
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM chunk_replicas cr
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 WHERE fc.file_id = ?1 AND cr.node_id = ?2 AND cr.state = 'synced'",
                rusqlite::params![file_id, &state.node_id], |row| row.get(0),
            ).unwrap_or(0);
            count > 0
        };

        if has_local_chunks {
            return Ok(data);
        }
    }

    // Peer-originated requests stop here — no recursive fetching
    if is_peer_request {
        return Err((StatusCode::NOT_FOUND, "File not found locally".into()));
    }

    // 3. No local chunks — fetch whole file from a peer that has it
    read_from_peers(state.clone(), volume_id, rel_path).await
}

/// Try to read a file from online peers.
async fn read_from_peers(
    state: Arc<CoreSanState>,
    volume_id: String,
    rel_path: String,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let client = crate::peer::client::PeerClient::new(&state.config.peer.secret);

    let online_peers: Vec<(String, String)> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online && p.node_id != state.node_id)
        .map(|p| (p.node_id.clone(), p.address.clone()))
        .collect();

    for (peer_id, peer_addr) in &online_peers {
        match client.pull_file(peer_addr, &volume_id, &rel_path).await {
            Ok(data) => {
                tracing::debug!("Peer-fetched {}/{} ({} bytes) from {}",
                    volume_id, rel_path, data.len(), peer_id);
                return Ok(data);
            }
            Err(_) => continue,
        }
    }

    Err((StatusCode::NOT_FOUND, "File not found on any node".into()))
}

/// PUT /api/volumes/{id}/files/*path — write a file (creates or overwrites).
/// Uses chunk-based storage with write-lease acquisition and immediate push replication.
pub async fn write(
    headers: axum::http::HeaderMap,
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<FileEntry>, (StatusCode, String)> {
    // Detect if this write comes from a peer (replication push) — do NOT re-replicate
    let is_peer_write = headers.get(crate::auth::PEER_SECRET_HEADER).is_some();
    // Check quorum — fenced nodes reject writes
    let quorum = *state.quorum_status.read().unwrap();
    if quorum == crate::state::QuorumStatus::Fenced {
        return Err((StatusCode::SERVICE_UNAVAILABLE,
            "node is fenced (no quorum) — writes are not allowed".into()));
    }
    if quorum == crate::state::QuorumStatus::Sanitizing {
        return Err((StatusCode::SERVICE_UNAVAILABLE,
            "node is sanitizing (startup integrity check) — writes are not yet allowed".into()));
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

    // Verify at least one local backend exists
    {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT id FROM backends WHERE node_id = ?1 AND status = 'online' LIMIT 1",
            rusqlite::params![&state.node_id],
            |row| row.get::<_, String>(0),
        ).map_err(|_| (StatusCode::NOT_FOUND,
            "No local backend available for this volume".into()))?;
    }

    // Chunk-based atomic write: lease → chunk split → write to backends → DB update → write_log
    let new_version = {
        let db = state.db.lock().unwrap();
        crate::engine::write_lease::atomic_write(
            &db, &volume_id, &rel_path, &state.node_id,
            "", "", &body, None, quorum,
        ).map_err(|e| (StatusCode::CONFLICT, e))?
    };

    let size = body.len() as u64;
    use sha2::{Sha256, Digest};
    let sha256 = format!("{:x}", Sha256::digest(&body));
    let now = chrono::Utc::now().to_rfc3339();

    // Push chunks to peers — only for original writes, NOT for peer-replicated writes
    if !is_peer_write {
        // Get file_id and chunk info for replication
        let file_id = {
            let db = state.db.lock().unwrap();
            db.query_row(
                "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![&volume_id, &rel_path], |row| row.get::<_, i64>(0),
            ).ok()
        };

        if let Some(file_id) = file_id {
            let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
                volume_id: volume_id.clone(),
                rel_path: rel_path.clone(),
                file_id,
                version: new_version,
                writer_node_id: state.node_id.clone(),
            });
        }
        tracing::info!("Wrote file {}/{} v{} ({} bytes, chunk-based)", volume_id, rel_path, new_version, size);
    } else {
        tracing::info!("Received replica {}/{} v{} ({} bytes, chunk-based)", volume_id, rel_path, new_version, size);
    }

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

/// DELETE /api/volumes/{id}/files/*path — delete a file and all chunk replicas.
pub async fn delete(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    let file_id: Option<i64> = db.query_row(
        "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
        rusqlite::params![&volume_id, &rel_path], |row| row.get(0),
    ).ok();

    if let Some(fid) = file_id {
        // Delete physical chunk files on all local backends
        let chunk_files: Vec<(u32, String)> = {
            let mut stmt = db.prepare(
                "SELECT fc.chunk_index, b.path FROM chunk_replicas cr
                 JOIN file_chunks fc ON fc.id = cr.chunk_id
                 JOIN backends b ON b.id = cr.backend_id
                 WHERE fc.file_id = ?1 AND cr.node_id = ?2"
            ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
            let rows = stmt.query_map(
                rusqlite::params![fid, &state.node_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for (chunk_index, backend_path) in &chunk_files {
            let path = crate::storage::chunk::chunk_path(backend_path, &volume_id, fid, *chunk_index);
            std::fs::remove_file(&path).ok();
            // Clean up empty parent dirs
            if let Some(parent) = path.parent() {
                std::fs::remove_dir(parent).ok(); // only succeeds if empty
            }
        }

    }

    // Delete file + all chunks/replicas atomically via FileService
    crate::services::file::FileService::delete(&db, &volume_id, &rel_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    tracing::info!("Deleted file {}/{} (chunks removed)", volume_id, rel_path);

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

    // Also scan backend directories for empty folders (not in file_map)
    {
        let backend_paths: Vec<String> = db.prepare(
            "SELECT path FROM backends WHERE node_id = ?1 AND status = 'online'"
        ).ok().map(|mut stmt| {
            stmt.query_map(rusqlite::params![&state.node_id], |row| row.get(0))
                .ok().map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        }).unwrap_or_default();

        for bp in &backend_paths {
            let scan_dir = std::path::Path::new(bp)
                .join(".coresan").join(&volume_id).join(&dir_path);
            if let Ok(read) = std::fs::read_dir(&scan_dir) {
                for entry in read.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if seen_dirs.insert(name.clone()) {
                            entries.push(BrowseEntry {
                                name,
                                is_dir: true,
                                size_bytes: 0,
                                updated_at: String::new(),
                            });
                        }
                    }
                }
            }
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

// ── Chunk Map (Allocation Details) ──────────────────────────────────

#[derive(Serialize)]
pub struct ChunkMapEntry {
    pub chunk_index: u32,
    pub file_id: i64,
    pub rel_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub state: String,       // "synced", "stale", "error", "empty"
    pub backend_id: String,
    pub backend_path: String,
    pub node_id: String,
    pub node_hostname: String,
}

#[derive(Serialize)]
pub struct ChunkMapResponse {
    pub volume_id: String,
    pub volume_name: String,
    pub chunk_size_bytes: u64,
    pub total_chunks: u64,
    pub total_capacity_bytes: u64,
    pub used_bytes: u64,
    pub backends: Vec<ChunkMapBackend>,
    pub chunks: Vec<ChunkMapEntry>,
}

#[derive(Serialize)]
pub struct ChunkMapBackend {
    pub backend_id: String,
    pub node_id: String,
    pub node_hostname: String,
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub status: String,
}

/// GET /api/volumes/{id}/chunk-map — full chunk allocation map for visualization.
/// Returns every chunk with its placement, status, and file association.
pub async fn chunk_map(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
) -> Result<Json<ChunkMapResponse>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Volume info
    let (vol_name, chunk_size, total_cap, free_cap): (String, u64, u64, u64) = db.query_row(
        "SELECT v.name, v.chunk_size_bytes,
                COALESCE((SELECT SUM(b.total_bytes) FROM backends b WHERE b.node_id IN (
                    SELECT DISTINCT cr.node_id FROM chunk_replicas cr
                    JOIN file_chunks fc ON fc.id = cr.chunk_id
                    JOIN file_map fm ON fm.id = fc.file_id
                    WHERE fm.volume_id = v.id
                ) OR b.node_id = ?2), 0),
                COALESCE((SELECT SUM(b.free_bytes) FROM backends b WHERE b.node_id IN (
                    SELECT DISTINCT cr.node_id FROM chunk_replicas cr
                    JOIN file_chunks fc ON fc.id = cr.chunk_id
                    JOIN file_map fm ON fm.id = fc.file_id
                    WHERE fm.volume_id = v.id
                ) OR b.node_id = ?2), 0)
         FROM volumes v WHERE v.id = ?1",
        rusqlite::params![&volume_id, &state.node_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).map_err(|_| (StatusCode::NOT_FOUND, "Volume not found".into()))?;

    // All backends that participate in this volume
    let backends: Vec<ChunkMapBackend> = {
        let mut stmt = db.prepare(
            "SELECT b.id, b.node_id, b.path, b.total_bytes, b.free_bytes, b.status,
                    COALESCE(p.hostname, ?2)
             FROM backends b
             LEFT JOIN peers p ON p.node_id = b.node_id
             WHERE b.status != 'released'
             ORDER BY b.node_id, b.path"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&state.node_id, &state.hostname], |row| {
            Ok(ChunkMapBackend {
                backend_id: row.get(0)?,
                node_id: row.get(1)?,
                path: row.get(2)?,
                total_bytes: row.get(3)?,
                free_bytes: row.get(4)?,
                status: row.get(5)?,
                node_hostname: row.get(6)?,
            })
        }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // All chunks with their replicas (including remote replicas tracked locally)
    let chunks: Vec<ChunkMapEntry> = {
        let mut stmt = db.prepare(
            "SELECT fc.chunk_index, fc.file_id, fm.rel_path, fc.size_bytes,
                    COALESCE(fc.sha256, ''), cr.state,
                    cr.backend_id, COALESCE(b.path, 'remote'), cr.node_id,
                    COALESCE(p.hostname, cr.node_id)
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             LEFT JOIN backends b ON b.id = cr.backend_id
             LEFT JOIN peers p ON p.node_id = cr.node_id
             WHERE fm.volume_id = ?1
             ORDER BY fc.file_id, fc.chunk_index, cr.node_id"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&volume_id], |row| {
            let node_id: String = row.get(8)?;
            let mut hostname: String = row.get(9)?;
            // For local node, use our hostname
            if node_id == state.node_id {
                hostname = state.hostname.clone();
            }
            Ok(ChunkMapEntry {
                chunk_index: row.get(0)?,
                file_id: row.get(1)?,
                rel_path: row.get(2)?,
                size_bytes: row.get(3)?,
                sha256: row.get(4)?,
                state: row.get(5)?,
                backend_id: row.get(6)?,
                backend_path: row.get(7)?,
                node_id,
                node_hostname: hostname,
            })
        }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let total_chunks = chunks.len() as u64;
    let used_bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(fm.size_bytes), 0) FROM file_map fm WHERE fm.volume_id = ?1",
        rusqlite::params![&volume_id], |row| row.get(0),
    ).unwrap_or(0);

    Ok(Json(ChunkMapResponse {
        volume_id,
        volume_name: vol_name,
        chunk_size_bytes: chunk_size,
        total_chunks,
        total_capacity_bytes: total_cap,
        used_bytes,
        backends,
        chunks,
    }))
}
