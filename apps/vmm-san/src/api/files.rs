//! File operations — list, read, write, delete files on volumes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use crate::state::CoreSanState;
use crate::storage::chunk::deterministic_file_id;

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
    let db = state.db.read();

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
        let db = state.db.read();
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
        let db = state.db.read();
        crate::storage::chunk::read_chunk_data(
            &db, file_id, 0, file_size,
            &volume_id, &state.node_id, chunk_size,
        )
    };

    // Check if we got valid data (no zero-filled gaps from missing chunks)
    if let Ok(data) = local_result {
        // Verify we have local chunk replicas (not just zero-filled)
        let has_local_chunks = {
            let db = state.db.read();
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
        let db = state.db.read();
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
        let db = state.db.read();
        db.query_row(
            "SELECT id FROM backends WHERE node_id = ?1 AND status = 'online' LIMIT 1",
            rusqlite::params![&state.node_id],
            |row| row.get::<_, String>(0),
        ).map_err(|_| (StatusCode::NOT_FOUND,
            "No local backend available for this volume".into()))?;
    }

    // Chunk-based atomic write: lease → chunk split → write to backends → DB update → write_log
    let new_version = {
        let db = state.db.write();
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
            let db = state.db.read();
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
/// Propagates deletion to all online peers (unless this is a peer-originated delete).
pub async fn delete(
    headers: axum::http::HeaderMap,
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, rel_path)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let is_peer_request = headers.get(crate::auth::PEER_SECRET_HEADER).is_some();

    // 1. Delete local chunk files from disk
    {
        let db = state.db.write();

        let file_id: Option<i64> = db.query_row(
            "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![&volume_id, &rel_path], |row| row.get(0),
        ).ok();

        if let Some(fid) = file_id {
            let chunk_files: Vec<(u32, String, Option<String>)> = {
                let mut stmt = db.prepare(
                    "SELECT fc.chunk_index, b.path, fc.dedup_sha256 FROM chunk_replicas cr
                     JOIN file_chunks fc ON fc.id = cr.chunk_id
                     JOIN backends b ON b.id = cr.backend_id
                     WHERE fc.file_id = ?1 AND cr.node_id = ?2"
                ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
                let rows = stmt.query_map(
                    rusqlite::params![fid, &state.node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
                rows.filter_map(|r| r.ok()).collect()
            };

            for (chunk_index, backend_path, dedup_sha) in &chunk_files {
                let path = if let Some(ref dsha) = dedup_sha {
                    // Decrement ref_count; only delete the dedup file if ref_count reaches 0
                    db.execute(
                        "UPDATE dedup_store SET ref_count = ref_count - 1 WHERE sha256 = ?1 AND volume_id = ?2",
                        rusqlite::params![dsha, &volume_id],
                    ).ok();
                    let rc: i64 = db.query_row(
                        "SELECT COALESCE(MIN(ref_count), 0) FROM dedup_store WHERE sha256 = ?1 AND volume_id = ?2",
                        rusqlite::params![dsha, &volume_id], |row| row.get(0),
                    ).unwrap_or(1);
                    if rc <= 0 {
                        crate::storage::chunk::dedup_chunk_path(backend_path, &volume_id, dsha)
                    } else {
                        continue; // Other files still reference this dedup chunk
                    }
                } else {
                    crate::storage::chunk::chunk_path(backend_path, &volume_id, fid, *chunk_index)
                };
                let _ = std::fs::remove_file(&path);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::remove_dir(parent); // only succeeds if empty
                }
            }
        }

        // 2. Delete from local DB atomically
        crate::services::file::FileService::delete(&db, &volume_id, &rel_path)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
    }

    tracing::info!("Deleted file {}/{} (local chunks removed)", volume_id, rel_path);

    // 3. Propagate deletion to all online peers (not for peer-originated requests)
    if !is_peer_request {
        let client = crate::peer::client::PeerClient::new(&state.config.peer.secret);
        let online_peers: Vec<(String, String)> = state.peers.iter()
            .filter(|p| p.status == crate::state::PeerStatus::Online)
            .map(|p| (p.node_id.clone(), p.address.clone()))
            .collect();

        for (peer_id, peer_addr) in &online_peers {
            match client.delete_file(peer_addr, &volume_id, &rel_path).await {
                Ok(_) => tracing::info!("Delete propagated to peer {}", peer_id),
                Err(e) => tracing::warn!("Delete propagation to {} failed: {}", peer_id, e),
            }
        }
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

use serde::Deserialize;

#[derive(Deserialize)]
pub struct MkdirRequest {
    pub path: String,
}

/// POST /api/volumes/{id}/mkdir — create a directory inside a volume.
/// Directories are virtual — they exist as a zero-byte marker file in file_map.
/// The browse/readdir logic derives directories from file path prefixes.
pub async fn mkdir(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
    Json(body): Json<MkdirRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Create a .dir marker entry in file_map so the directory is visible
    // even when empty (files with this prefix would also make it visible)
    let marker_path = format!("{}/.keep", body.path.trim_end_matches('/'));
    let db = state.db.write();
    let now = chrono::Utc::now().to_rfc3339();
    let file_id = deterministic_file_id(&volume_id, &marker_path);
    db.execute(
        "INSERT OR IGNORE INTO file_map (id, volume_id, rel_path, size_bytes, version, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, 0, ?4, ?4)",
        rusqlite::params![file_id, &volume_id, &marker_path, &now],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    tracing::info!("Created directory {}/{}", volume_id, body.path);
    Ok(Json(serde_json::json!({ "success": true, "path": body.path })))
}

// ── Disk Allocation ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AllocateDiskRequest {
    pub path: String,
    pub size_gb: u64,
}

/// POST /api/volumes/{id}/allocate-disk — create a pre-allocated raw disk image.
/// Writes the full size as zero-chunks server-side. This is the correct way to create
/// VM disk images on CoreSAN — sparse files (ftruncate) don't work reliably over FUSE.
pub async fn allocate_disk(
    State(state): State<Arc<CoreSanState>>,
    Path(volume_id): Path<String>,
    Json(body): Json<AllocateDiskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let size_bytes = body.size_gb * 1024 * 1024 * 1024;
    if size_bytes == 0 || body.size_gb > 4096 {
        return Err((StatusCode::BAD_REQUEST, "size_gb must be between 1 and 4096".into()));
    }

    // Check quorum
    let quorum = *state.quorum_status.read().unwrap();
    if quorum == crate::state::QuorumStatus::Fenced || quorum == crate::state::QuorumStatus::Sanitizing {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "Node not available for writes".into()));
    }

    // Verify backend exists and check capacity
    {
        let db = state.db.read();
        db.query_row(
            "SELECT id FROM backends WHERE node_id = ?1 AND status = 'online' AND claimed_disk_id != '' LIMIT 1",
            rusqlite::params![&state.node_id], |row| row.get::<_, String>(0),
        ).map_err(|_| (StatusCode::NOT_FOUND, "No claimed disks available".into()))?;

        // Check if volume has a max_size_bytes limit
        let (vol_max, vol_used): (u64, u64) = db.query_row(
            "SELECT v.max_size_bytes, COALESCE(SUM(fm.size_bytes), 0)
             FROM volumes v LEFT JOIN file_map fm ON fm.volume_id = v.id
             WHERE v.id = ?1 GROUP BY v.id",
            rusqlite::params![&volume_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or((0, 0));

        if vol_max > 0 && vol_used + size_bytes > vol_max {
            return Err((StatusCode::BAD_REQUEST,
                format!("Volume size limit exceeded. Max: {} bytes, used: {} bytes, requested: {} bytes",
                    vol_max, vol_used, size_bytes)));
        }

        // Check physical disk capacity
        let local_raid: String = db.query_row(
            "SELECT local_raid FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id], |row| row.get(0),
        ).unwrap_or_else(|_| "stripe".into());

        let free: u64 = {
            let backends: Vec<(u64, u64)> = db.prepare(
                "SELECT total_bytes, free_bytes FROM backends WHERE node_id = ?1 AND status = 'online' AND claimed_disk_id != ''"
            ).unwrap().query_map(rusqlite::params![&state.node_id], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap().filter_map(|r| r.ok()).collect();
            match local_raid.as_str() {
                "mirror" => backends.iter().map(|(_, f)| *f).min().unwrap_or(0),
                "stripe_mirror" => backends.iter().map(|(_, f)| *f).sum::<u64>() / 2,
                _ => backends.iter().map(|(_, f)| *f).sum(),
            }
        };

        if size_bytes > free {
            return Err((StatusCode::BAD_REQUEST,
                format!("Not enough disk space. Requested: {} GB, available: {} GB ({} RAID)",
                    body.size_gb, free / 1024 / 1024 / 1024, local_raid)));
        }
    }

    tracing::info!("Allocating disk {}/{} ({} GB)...", volume_id, body.path, body.size_gb);

    // Register the disk with its full logical size in the metadata.
    // Chunks are created as metadata-only entries — no physical zero-bytes written.
    // Reads to unwritten chunks return zeros (sparse/thin provisioning at chunk level).
    // This is instant regardless of disk size and doesn't waste storage space.
    let file_id = {
        let db = state.db.write();

        let chunk_size: u64 = db.query_row(
            "SELECT chunk_size_bytes FROM volumes WHERE id = ?1",
            rusqlite::params![&volume_id], |row| row.get(0),
        ).map_err(|_| (StatusCode::NOT_FOUND, "Volume not found".into()))?;

        let now = chrono::Utc::now().to_rfc3339();
        let chunk_count = ((size_bytes + chunk_size - 1) / chunk_size) as u32;

        // Create file_map entry with the full logical size
        let file_id = deterministic_file_id(&volume_id, &body.path);
        db.execute(
            "INSERT INTO file_map (id, volume_id, rel_path, size_bytes, sha256, version, chunk_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, '', 1, ?5, ?6, ?6)
             ON CONFLICT(volume_id, rel_path) DO UPDATE SET
                size_bytes = excluded.size_bytes, version = version + 1,
                chunk_count = excluded.chunk_count, updated_at = excluded.updated_at",
            rusqlite::params![file_id, &volume_id, &body.path, size_bytes as i64, chunk_count, &now],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("file_map: {}", e)))?;

        // True thin provisioning: NO file_chunks entries created upfront.
        // Chunks are created on-demand when the VM actually writes to them.
        // This makes allocate-disk instant regardless of disk size.

        tracing::info!("Allocated disk {}/{}: {} bytes (thin provisioned, instant)",
            volume_id, body.path, size_bytes);

        file_id
    };

    // Trigger metadata replication to peers (no chunk data to push — thin provisioned)
    let version = {
        let db = state.db.read();
        db.query_row("SELECT version FROM file_map WHERE id = ?1",
            rusqlite::params![file_id], |row| row.get::<_, i64>(0)).unwrap_or(1)
    };
    let _ = state.write_tx.send(crate::engine::push_replicator::WriteEvent {
        volume_id: volume_id.clone(),
        rel_path: body.path.clone(),
        file_id,
        version,
        writer_node_id: state.node_id.clone(),
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "path": format!("/vmm/san/{}/{}", volume_id, body.path),
        "size_bytes": size_bytes,
    })))
}

/// GET /api/volumes/{id}/browse/{*path} — list directory contents inside a volume.
pub async fn browse(
    State(state): State<Arc<CoreSanState>>,
    Path((volume_id, dir_path)): Path<(String, String)>,
) -> Result<Json<Vec<BrowseEntry>>, (StatusCode, String)> {
    let db = state.db.read();

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

    // file_map is the single source of truth — no disk scanning
    // (chunk data lives under .coresan/<volume_id>/<file_id>/chunk_XXXXXX,
    //  which is internal structure and must NOT be exposed to users)

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
    pub deduplicated: bool,
    pub dedup_sha256: Option<String>,
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
    let db = state.db.read();

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
                    COALESCE(fc.sha256, ''),
                    -- pick best state per (chunk, node): synced > syncing > stale > error
                    CASE MIN(CASE cr.state
                        WHEN 'synced'  THEN 1
                        WHEN 'syncing' THEN 2
                        WHEN 'stale'   THEN 3
                        ELSE 4 END)
                      WHEN 1 THEN 'synced'
                      WHEN 2 THEN 'syncing'
                      WHEN 3 THEN 'stale'
                      ELSE 'error' END AS best_state,
                    MIN(cr.backend_id), COALESCE(MIN(b.path), 'remote'),
                    cr.node_id,
                    COALESCE(MIN(p.hostname), cr.node_id),
                    fc.dedup_sha256
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             LEFT JOIN backends b ON b.id = cr.backend_id
             LEFT JOIN peers p ON p.node_id = cr.node_id
             WHERE fm.volume_id = ?1
             GROUP BY cr.chunk_id, cr.node_id
             ORDER BY fc.file_id, fc.chunk_index, cr.node_id"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&volume_id], |row| {
            let node_id: String = row.get(8)?;
            let mut hostname: String = row.get(9)?;
            // For local node, use our hostname
            if node_id == state.node_id {
                hostname = state.hostname.clone();
            }
            let dedup_sha256: Option<String> = row.get(10).ok().flatten();
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
                deduplicated: dedup_sha256.is_some(),
                dedup_sha256,
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
