//! Volume CRUD endpoints with per-volume resilience policy.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use crate::state::CoreSanState;
use crate::services::chunk::ChunkService;
use crate::peer::client::PeerClient;

#[derive(Deserialize)]
pub struct CreateVolumeRequest {
    pub name: String,
    /// Maximum volume size in bytes (required).
    pub max_size_bytes: u64,
    #[serde(default = "default_ftt")]
    pub ftt: u32,
    #[serde(default = "default_local_raid")]
    pub local_raid: String,
    #[serde(default = "default_chunk_size")]
    pub chunk_size_bytes: u64,
    #[serde(default = "default_access_protocols")]
    pub access_protocols: Vec<String>,
}

fn default_ftt() -> u32 { 1 }
fn default_local_raid() -> String { "stripe".into() }
fn default_chunk_size() -> u64 { crate::storage::chunk::DEFAULT_CHUNK_SIZE }
fn default_access_protocols() -> Vec<String> { vec!["fuse".into()] }

#[derive(Deserialize)]
pub struct UpdateVolumeRequest {
    pub ftt: Option<u32>,
    pub local_raid: Option<String>,
    pub access_protocols: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct VolumeResponse {
    pub id: String,
    pub name: String,
    pub ftt: u32,
    pub local_raid: String,
    pub chunk_size_bytes: u64,
    pub max_size_bytes: u64,
    pub status: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub backend_count: u32,
    pub created_at: String,
    pub access_protocols: Vec<String>,
}

/// POST /api/volumes — create a new volume with resilience policy.
pub async fn create(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateVolumeRequest>,
) -> Result<(StatusCode, Json<VolumeResponse>), (StatusCode, String)> {
    // Validate access_protocols
    let valid_protocols = ["fuse", "s3"];
    for proto in &body.access_protocols {
        if !valid_protocols.contains(&proto.as_str()) {
            return Err((StatusCode::BAD_REQUEST,
                format!("Unknown access protocol '{}'. Valid: {:?}", proto, valid_protocols)));
        }
    }
    if body.access_protocols.is_empty() {
        return Err((StatusCode::BAD_REQUEST,
            "access_protocols must contain at least one protocol".into()));
    }

    // Validate FTT
    if body.ftt > 2 {
        return Err((StatusCode::BAD_REQUEST, "ftt must be 0, 1, or 2".into()));
    }
    if !["stripe", "mirror", "stripe_mirror"].contains(&body.local_raid.as_str()) {
        return Err((StatusCode::BAD_REQUEST,
            "local_raid must be 'stripe', 'mirror', or 'stripe_mirror'".into()));
    }

    // Validate claimed disks and RAID requirements
    {
        let db = state.db.lock().unwrap();
        let claimed_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM backends WHERE node_id = ?1 AND status = 'online' AND claimed_disk_id != ''",
            rusqlite::params![&state.node_id], |row| row.get(0),
        ).unwrap_or(0);

        if claimed_count == 0 {
            return Err((StatusCode::BAD_REQUEST,
                "No disks claimed. Claim at least one disk before creating a volume.".into()));
        }

        // RAID-specific disk count requirements
        let min_disks: i64 = match body.local_raid.as_str() {
            "mirror" => 2,
            "stripe_mirror" => 4,
            _ => 1, // stripe needs at least 1
        };

        if claimed_count < min_disks {
            return Err((StatusCode::BAD_REQUEST,
                format!("{} RAID requires at least {} claimed disks, but only {} available.",
                    body.local_raid, min_disks, claimed_count)));
        }
    }

    // Validate volume size
    if body.max_size_bytes == 0 {
        return Err((StatusCode::BAD_REQUEST,
            "max_size_bytes is required and must be > 0".into()));
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

    // Check available capacity on local claimed disks (RAID-corrected)
    let (usable_total, usable_free) = usable_capacity_for_raid(&db, &state.node_id, &body.local_raid);

    if usable_free < body.max_size_bytes {
        return Err((StatusCode::BAD_REQUEST,
            format!("Not enough storage. Volume needs {} but only {} usable ({} RAID, {} claimed disks).",
                format_size(body.max_size_bytes), format_size(usable_free),
                body.local_raid, format_size(usable_total))));
    }

    let protocols_json = serde_json::to_string(&body.access_protocols).unwrap_or_else(|_| "[\"fuse\"]".into());

    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, max_size_bytes, access_protocols, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'online')",
        rusqlite::params![&id, &body.name, body.ftt, body.chunk_size_bytes, &body.local_raid, body.max_size_bytes, &protocols_json],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create volume: {}", e)))?;

    // Create FUSE mount point for this volume (only if FUSE protocol enabled)
    if body.access_protocols.contains(&"fuse".into()) {
        let fuse_path = state.config.data.fuse_root.join(&body.name);
        if !fuse_path.exists() {
            std::fs::create_dir_all(&fuse_path).ok();
        }
    }

    // Backends are ONLY claimed disks (/vmm/san-disks/*).
    // The root filesystem must NEVER be used as a storage backend.

    tracing::info!("Created volume '{}' (id={}, ftt={}, local_raid={}, chunk={}MB, protocols={:?})",
        body.name, id, body.ftt, body.local_raid, body.chunk_size_bytes / (1024 * 1024), body.access_protocols);

    drop(db);

    // Mount FUSE for the new volume (only if FUSE protocol enabled)
    if body.access_protocols.contains(&"fuse".into()) {
        let fuse_path = state.config.data.fuse_root.join(&body.name);
        let rt = tokio::runtime::Handle::current();
        let state_clone = Arc::clone(&state);
        let vol_id = id.clone();
        let vol_name = body.name.clone();

        let allow_other = std::fs::read_to_string("/etc/fuse.conf")
            .map(|c| c.lines().any(|l| l.trim() == "user_allow_other" && !l.starts_with('#')))
            .unwrap_or(false) || unsafe { libc::getuid() } == 0;

        let mount_path = fuse_path.clone();
        std::thread::spawn(move || {
            let fs_name = format!("coresan:{}", vol_name);
            let mut options = vec![fuser::MountOption::FSName(fs_name)];
            if allow_other {
                options.push(fuser::MountOption::AllowOther);
                options.push(fuser::MountOption::AutoUnmount);
            }
            match fuser::mount2(
                crate::engine::fuse_mount::CoreSanFS::new(state_clone, vol_id, rt),
                &mount_path, &options,
            ) {
                Ok(_) => tracing::info!("FUSE unmounted: {}", mount_path.display()),
                Err(e) => tracing::error!("FUSE mount failed for {}: {}", mount_path.display(), e),
            }
        });
        tracing::info!("FUSE mounted: /vmm/san/{}", body.name);
    }

    // Start disk server for this volume (direct VM I/O via UDS)
    crate::engine::disk_server::spawn_volume_listener(
        Arc::clone(&state), id.clone(), body.name.clone(),
    );

    // Sync volume to all peers so they mount it too
    let vol_json = serde_json::json!({
        "id": &id, "name": &body.name, "ftt": body.ftt,
        "chunk_size_bytes": body.chunk_size_bytes, "local_raid": &body.local_raid,
        "max_size_bytes": body.max_size_bytes,
        "access_protocols": &body.access_protocols,
    });
    let peers: Vec<String> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .map(|p| p.address.clone())
        .collect();

    let secret = state.config.peer.secret.clone();
    tokio::spawn(async move {
        let client = crate::peer::client::PeerClient::new(&secret);
        for addr in peers {
            if let Err(e) = client.sync_volume(&addr, &vol_json).await {
                tracing::warn!("Failed to sync volume to {}: {}", addr, e);
            }
        }
    });

    // Start object server listener for this volume if S3 protocol enabled
    if body.access_protocols.contains(&"s3".into()) {
        crate::engine::object_server::spawn_volume_listener(
            Arc::clone(&state), id.clone(), body.name.clone(),
        );
    }

    Ok((StatusCode::CREATED, Json(VolumeResponse {
        id,
        name: body.name.clone(),
        ftt: body.ftt,
        local_raid: body.local_raid,
        chunk_size_bytes: body.chunk_size_bytes,
        max_size_bytes: body.max_size_bytes,
        status: "online".into(),
        total_bytes: 0,
        free_bytes: 0,
        backend_count: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        access_protocols: body.access_protocols,
    })))
}

/// GET /api/volumes — list all volumes with capacity stats.
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
) -> Json<Vec<VolumeResponse>> {
    let db = state.db.lock().unwrap();

    let backend_count: u32 = db.query_row(
        "SELECT COUNT(*) FROM backends WHERE status = 'online' AND claimed_disk_id != ''",
        [], |row| row.get(0),
    ).unwrap_or(0);

    let mut stmt = db.prepare(
        "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at, max_size_bytes, access_protocols
         FROM volumes ORDER BY name"
    ).unwrap();

    let volumes = stmt.query_map([], |row| {
        let local_raid: String = row.get(3)?;
        // Calculate RAID-corrected capacity from claimed disks only
        let (total_bytes, free_bytes) = usable_capacity_for_raid_static(&db, &local_raid);
        let protocols_json: String = row.get::<_, String>(8).unwrap_or_else(|_| "[\"fuse\"]".into());
        let access_protocols: Vec<String> = serde_json::from_str(&protocols_json).unwrap_or_else(|_| vec!["fuse".into()]);
        Ok(VolumeResponse {
            id: row.get(0)?,
            name: row.get(1)?,
            ftt: row.get(2)?,
            local_raid,
            chunk_size_bytes: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
            max_size_bytes: row.get(7)?,
            total_bytes,
            free_bytes,
            backend_count,
            access_protocols,
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

    let total_bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(total_bytes), 0) FROM backends WHERE status = 'online'",
        [], |row| row.get(0),
    ).unwrap_or(0);
    let free_bytes: u64 = db.query_row(
        "SELECT COALESCE(SUM(free_bytes), 0) FROM backends WHERE status = 'online'",
        [], |row| row.get(0),
    ).unwrap_or(0);
    let backend_count: u32 = db.query_row(
        "SELECT COUNT(*) FROM backends WHERE status = 'online'",
        [], |row| row.get(0),
    ).unwrap_or(0);

    let vol = db.query_row(
        "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at, max_size_bytes, access_protocols
         FROM volumes WHERE id = ?1",
        rusqlite::params![&id],
        |row| {
            let protocols_json: String = row.get::<_, String>(8).unwrap_or_else(|_| "[\"fuse\"]".into());
            let access_protocols: Vec<String> = serde_json::from_str(&protocols_json).unwrap_or_else(|_| vec!["fuse".into()]);
            Ok(VolumeResponse {
                id: row.get(0)?,
                name: row.get(1)?,
                ftt: row.get(2)?,
                local_raid: row.get(3)?,
                chunk_size_bytes: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                max_size_bytes: row.get(7)?,
                total_bytes,
                free_bytes,
                backend_count,
                access_protocols,
            })
        },
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
    if let Some(ref protocols) = body.access_protocols {
        let valid_protocols = ["fuse", "s3"];
        for proto in protocols {
            if !valid_protocols.contains(&proto.as_str()) {
                return Err((StatusCode::BAD_REQUEST,
                    format!("Unknown access protocol '{}'. Valid: {:?}", proto, valid_protocols)));
            }
        }
        if protocols.is_empty() {
            return Err((StatusCode::BAD_REQUEST,
                "access_protocols must contain at least one protocol".into()));
        }
        let protocols_json = serde_json::to_string(protocols).unwrap_or_else(|_| "[\"fuse\"]".into());
        db.execute("UPDATE volumes SET access_protocols = ?1 WHERE id = ?2",
            rusqlite::params![&protocols_json, &id]).ok();
    }

    tracing::info!("Updated volume {} policy", id);

    Ok(Json(serde_json::json!({ "success": true, "rebalance_triggered": true })))
}

/// Query parameters for DELETE /api/volumes/{id}.
#[derive(Deserialize)]
pub struct DeleteVolumeQuery {
    /// If true, force-delete all files, chunks, and replicas before removing the volume.
    #[serde(default)]
    pub force: bool,
}

/// DELETE /api/volumes/{id}?force=true|false — remove volume.
/// Default (force=false): rejects if the volume still has files.
/// With force=true: deletes all files, chunks, chunk_replicas, and on-disk chunk data first.
pub async fn delete(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
    Query(query): Query<DeleteVolumeQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Get volume name for FUSE cleanup
    let name: Option<String> = db.query_row(
        "SELECT name FROM volumes WHERE id = ?1",
        rusqlite::params![&id], |row| row.get(0),
    ).ok();

    if name.is_none() {
        return Err((StatusCode::NOT_FOUND, "Volume not found".into()));
    }

    if query.force {
        // Force delete: remove all file data for this volume

        // Collect backend paths so we can delete chunk files on disk
        let backend_paths: Vec<String> = {
            let mut stmt = db.prepare(
                "SELECT path FROM backends WHERE status = 'online' AND claimed_disk_id != ''"
            ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
            let rows = stmt.query_map([], |row| row.get(0)).unwrap();
            rows.filter_map(|r| r.ok()).collect()
        };

        // Delete chunk_replicas for all chunks belonging to this volume
        db.execute(
            "DELETE FROM chunk_replicas WHERE chunk_id IN (
                SELECT fc.id FROM file_chunks fc
                JOIN file_map fm ON fm.id = fc.file_id
                WHERE fm.volume_id = ?1
            )",
            rusqlite::params![&id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Delete replicas failed: {}", e)))?;

        // Delete file_chunks for all files in this volume
        db.execute(
            "DELETE FROM file_chunks WHERE file_id IN (
                SELECT id FROM file_map WHERE volume_id = ?1
            )",
            rusqlite::params![&id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Delete chunks failed: {}", e)))?;

        // Delete file_map entries
        let deleted_files: usize = db.execute(
            "DELETE FROM file_map WHERE volume_id = ?1",
            rusqlite::params![&id],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Delete files failed: {}", e)))?;

        tracing::info!("Force-delete volume {}: removed {} file records", id, deleted_files);

        // Delete chunk files on disk: .coresan/{volume_id}/ on each backend
        for bp in &backend_paths {
            let vol_dir = std::path::Path::new(bp).join(".coresan").join(&id);
            if vol_dir.exists() {
                match std::fs::remove_dir_all(&vol_dir) {
                    Ok(_) => tracing::info!("Removed chunk directory: {}", vol_dir.display()),
                    Err(e) => tracing::warn!("Failed to remove {}: {}", vol_dir.display(), e),
                }
            }
        }
    } else {
        // Default: reject if volume has files
        let file_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1",
            rusqlite::params![&id], |row| row.get(0),
        ).unwrap_or(0);

        if file_count > 0 {
            return Err((StatusCode::CONFLICT,
                format!("Volume has {} files, remove them first (or use ?force=true)", file_count)));
        }
    }

    db.execute("DELETE FROM volumes WHERE id = ?1", rusqlite::params![&id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Delete failed: {}", e)))?;

    // Clean up FUSE mount directory
    if let Some(ref name) = name {
        let fuse_path = state.config.data.fuse_root.join(name);
        // Unmount FUSE first
        std::process::Command::new("fusermount3")
            .args(["-u", &fuse_path.to_string_lossy()])
            .output().ok();
        std::fs::remove_dir(&fuse_path).ok();
        tracing::info!("Deleted volume '{}'", name);
    }

    drop(db);

    // Notify peers to delete this volume too
    let vol_id = id.clone();
    let peers: Vec<String> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .map(|p| p.address.clone())
        .collect();
    let secret = state.config.peer.secret.clone();

    tokio::spawn(async move {
        let client = PeerClient::new(&secret);
        for addr in peers {
            client.delete_volume(&addr, &vol_id).await.ok();
        }
    });

    Ok(Json(serde_json::json!({ "success": true, "force": query.force })))
}

/// Receive a volume definition from a peer (sync).
#[derive(Deserialize)]
pub struct SyncVolumeRequest {
    pub id: String,
    pub name: String,
    #[serde(default = "default_ftt")]
    pub ftt: u32,
    #[serde(default = "default_chunk_size")]
    pub chunk_size_bytes: u64,
    #[serde(default = "default_local_raid")]
    pub local_raid: String,
    #[serde(default)]
    pub max_size_bytes: u64,
}

/// POST /api/volumes/sync — receive a volume definition from a peer.
/// Creates the volume locally if it doesn't exist and mounts the FUSE endpoint.
pub async fn sync(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<SyncVolumeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Check if volume already exists
    let exists: bool = db.query_row(
        "SELECT COUNT(*) FROM volumes WHERE id = ?1",
        rusqlite::params![&body.id], |row| row.get::<_, i64>(0),
    ).map(|c| c > 0).unwrap_or(false);

    if exists {
        // Already synced
        return Ok(Json(serde_json::json!({ "synced": true, "already_exists": true })));
    }

    // Create the volume locally
    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, max_size_bytes, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'online')",
        rusqlite::params![&body.id, &body.name, body.ftt, body.chunk_size_bytes, &body.local_raid, body.max_size_bytes],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;

    // Create FUSE mount directory
    let fuse_path = state.config.data.fuse_root.join(&body.name);
    std::fs::create_dir_all(&fuse_path).ok();

    // Backends are ONLY claimed disks — no root-FS backend for synced volumes either.

    drop(db);

    // Mount FUSE for this new volume
    let rt = tokio::runtime::Handle::current();
    let state_clone = Arc::clone(&state);
    let vol_id = body.id.clone();
    let vol_name = body.name.clone();

    std::thread::spawn(move || {
        let fuse_path_clone = state_clone.config.data.fuse_root.join(&vol_name);

        let allow_other = std::fs::read_to_string("/etc/fuse.conf")
            .map(|c| c.lines().any(|l| l.trim() == "user_allow_other" && !l.starts_with('#')))
            .unwrap_or(false) || unsafe { libc::getuid() } == 0;

        let fs_name = format!("coresan:{}", vol_name);
        let mut options = vec![
            fuser::MountOption::FSName(fs_name),
        ];
        if allow_other {
            options.push(fuser::MountOption::AllowOther);
            options.push(fuser::MountOption::AutoUnmount);
        }

        match fuser::mount2(
            crate::engine::fuse_mount::CoreSanFS::new(state_clone, vol_id, rt),
            &fuse_path_clone, &options,
        ) {
            Ok(_) => tracing::info!("FUSE unmounted (synced volume): {}", fuse_path_clone.display()),
            Err(e) => tracing::error!("FUSE mount failed for synced volume {}: {}", fuse_path_clone.display(), e),
        }
    });

    // Start disk server for this synced volume (direct VM I/O via UDS)
    crate::engine::disk_server::spawn_volume_listener(
        Arc::clone(&state), body.id.clone(), body.name.clone(),
    );

    tracing::info!("Synced volume '{}' from peer (id={}, ftt={}, raid={})",
        body.name, body.id, body.ftt, body.local_raid);

    Ok(Json(serde_json::json!({ "synced": true, "already_exists": false })))
}

// ── Volume Health Analysis ───────────────────────────────────

#[derive(Serialize)]
pub struct VolumeHealthResponse {
    pub volume_id: String,
    pub overall_status: String, // healthy, degraded, critical
    pub file_protection_summary: FileProtectionSummary,
    pub affected_files: Vec<AffectedFile>,
    pub chunk_statistics: ChunkStatistics,
    pub integrity_failures: Vec<IntegrityFailure>,
}

#[derive(Serialize)]
pub struct FileProtectionSummary {
    pub protected: u64,
    pub degraded: u64,
    pub unprotected: u64,
    pub total: u64,
}

#[derive(Serialize)]
pub struct AffectedFile {
    pub file_id: i64,
    pub rel_path: String,
    pub size_bytes: u64,
    pub protection_status: String,
    pub synced_nodes: u64,
}

#[derive(Serialize)]
pub struct ChunkStatistics {
    pub synced: u64,
    pub syncing: u64,
    pub stale: u64,
    pub error: u64,
    pub total: u64,
}

#[derive(Serialize)]
pub struct IntegrityFailure {
    pub id: i64,
    pub file_id: i64,
    pub rel_path: String,
    pub backend_id: String,
    pub expected_sha256: String,
    pub actual_sha256: String,
    pub checked_at: String,
}

/// GET /api/volumes/{id}/health — volume health analysis.
pub async fn health(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<Json<VolumeHealthResponse>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Verify volume exists
    let exists: bool = db.query_row(
        "SELECT COUNT(*) FROM volumes WHERE id = ?1",
        rusqlite::params![&id], |row| row.get::<_, i64>(0),
    ).map(|c| c > 0).unwrap_or(false);

    if !exists {
        return Err((StatusCode::NOT_FOUND, "Volume not found".into()));
    }

    // File protection breakdown
    let mut protection = FileProtectionSummary {
        protected: 0, degraded: 0, unprotected: 0, total: 0,
    };
    {
        let mut stmt = db.prepare(
            "SELECT protection_status, COUNT(*) FROM file_map WHERE volume_id = ?1 GROUP BY protection_status"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        for row in rows.flatten() {
            match row.0.as_str() {
                "protected" => protection.protected = row.1,
                "degraded" => protection.degraded = row.1,
                _ => protection.unprotected += row.1,
            }
        }
        protection.total = protection.protected + protection.degraded + protection.unprotected;
    }

    // Affected files (degraded or worse)
    let affected_files: Vec<AffectedFile> = {
        let mut stmt = db.prepare(
            "SELECT fm.id, fm.rel_path, fm.size_bytes, fm.protection_status,
                    (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                     JOIN file_chunks fc ON fc.id = cr.chunk_id
                     WHERE fc.file_id = fm.id AND cr.state = 'synced') as synced_nodes
             FROM file_map fm WHERE fm.volume_id = ?1 AND fm.protection_status != 'protected'"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&id], |row| {
            Ok(AffectedFile {
                file_id: row.get(0)?,
                rel_path: row.get(1)?,
                size_bytes: row.get(2)?,
                protection_status: row.get(3)?,
                synced_nodes: row.get(4)?,
            })
        }).unwrap();
        rows.flatten().collect()
    };

    // Chunk state counts
    let mut chunks = ChunkStatistics {
        synced: 0, syncing: 0, stale: 0, error: 0, total: 0,
    };
    {
        let mut stmt = db.prepare(
            "SELECT cr.state, COUNT(*) FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             WHERE fm.volume_id = ?1 GROUP BY cr.state"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        for row in rows.flatten() {
            match row.0.as_str() {
                "synced" => chunks.synced = row.1,
                "syncing" => chunks.syncing = row.1,
                "stale" => chunks.stale = row.1,
                "error" => chunks.error = row.1,
                _ => {}
            }
        }
        chunks.total = chunks.synced + chunks.syncing + chunks.stale + chunks.error;
    }

    // Integrity failures (last 50)
    let integrity_failures: Vec<IntegrityFailure> = {
        let mut stmt = db.prepare(
            "SELECT il.id, il.file_id, fm.rel_path, il.backend_id, il.expected_sha256, il.actual_sha256, il.checked_at
             FROM integrity_log il
             JOIN file_map fm ON fm.id = il.file_id
             WHERE fm.volume_id = ?1 AND il.passed = 0
             ORDER BY il.checked_at DESC LIMIT 50"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        let rows = stmt.query_map(rusqlite::params![&id], |row| {
            Ok(IntegrityFailure {
                id: row.get(0)?,
                file_id: row.get(1)?,
                rel_path: row.get(2)?,
                backend_id: row.get(3)?,
                expected_sha256: row.get(4)?,
                actual_sha256: row.get(5)?,
                checked_at: row.get(6)?,
            })
        }).unwrap();
        rows.flatten().collect()
    };

    // Determine overall status
    let overall_status = if !integrity_failures.is_empty() || chunks.error > 0 {
        "critical".to_string()
    } else if protection.degraded > 0 || chunks.stale > 0 || protection.unprotected > 0 {
        "degraded".to_string()
    } else {
        "healthy".to_string()
    };

    Ok(Json(VolumeHealthResponse {
        volume_id: id,
        overall_status,
        file_protection_summary: protection,
        affected_files,
        chunk_statistics: chunks,
        integrity_failures,
    }))
}

// ── Feature: Manual Repair Trigger ──────────────────────────────

#[derive(Serialize)]
pub struct RepairResponse {
    pub repaired: u32,
    pub remaining: u32,
    pub errors: Vec<String>,
}

/// POST /api/volumes/{id}/repair — trigger immediate repair for this volume.
/// Finds all under-replicated chunks for this volume and repairs them inline.
pub async fn trigger_repair(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<Json<RepairResponse>, (StatusCode, String)> {
    // Verify volume exists
    {
        let db = state.db.lock().unwrap();
        let exists: bool = db.query_row(
            "SELECT COUNT(*) FROM volumes WHERE id = ?1",
            rusqlite::params![&id], |row| row.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false);
        if !exists {
            return Err((StatusCode::NOT_FOUND, "Volume not found".into()));
        }
    }

    let client = PeerClient::new(&state.config.peer.secret);
    let mut repaired = 0u32;
    let mut errors: Vec<String> = Vec::new();

    // Query under-replicated chunks for THIS volume specifically
    let under_replicated = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, fc.chunk_index, fm.volume_id, v.ftt,
                    (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                     WHERE cr.chunk_id = fc.id AND cr.state = 'synced') AS synced_nodes
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN volumes v ON v.id = fm.volume_id
             WHERE fm.volume_id = ?1
               AND synced_nodes < (v.ftt + 1)
               AND synced_nodes > 0
             LIMIT 1000"
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
        let rows: Vec<(i64, i64, u32, String, u32, u32)> = stmt.query_map(rusqlite::params![&id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        }).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?
          .filter_map(|r| r.ok()).collect();
        rows
    };

    let total_under = under_replicated.len() as u32;

    for (chunk_id, file_id, chunk_index, volume_id, ftt, synced_nodes) in &under_replicated {
        let needed = (ftt + 1).saturating_sub(*synced_nodes);
        for _ in 0..needed {
            match repair_single_chunk_inline(&state, &client, *chunk_id, *file_id, *chunk_index, volume_id).await {
                Ok(true) => repaired += 1,
                Ok(false) => {
                    // No progress but no hard error (e.g., no available peer)
                }
                Err(e) => {
                    errors.push(format!("chunk {} (file {}, idx {}): {}", chunk_id, file_id, chunk_index, e));
                }
            }
        }
    }

    let remaining = total_under.saturating_sub(repaired);

    tracing::info!("Manual repair for volume {}: repaired={}, remaining={}, errors={}",
        id, repaired, remaining, errors.len());

    Ok(Json(RepairResponse { repaired, remaining, errors }))
}

/// Inline repair of a single chunk — reuses the same logic as engine/repair.rs
/// but returns Result instead of bool for better error reporting.
async fn repair_single_chunk_inline(
    state: &CoreSanState,
    client: &PeerClient,
    chunk_id: i64,
    file_id: i64,
    chunk_index: u32,
    volume_id: &str,
) -> Result<bool, String> {
    let source_node_id = {
        let db = state.db.lock().unwrap();
        ChunkService::find_chunk_source(&db, chunk_id)
    };

    let source_node_id = match source_node_id {
        Some(id) => id,
        None => return Err("no source node with synced replica".into()),
    };

    let nodes_with_chunk: Vec<String> = {
        let db = state.db.lock().unwrap();
        ChunkService::nodes_with_chunk(&db, chunk_id)
    };

    // Try local node first (pull from peer if we don't have it)
    if !nodes_with_chunk.contains(&state.node_id) {
        if source_node_id == state.node_id {
            return Err("source is local but we don't have the chunk".into());
        }

        let peer_addr = match state.peers.get(&source_node_id) {
            Some(p) => p.address.clone(),
            None => return Err(format!("source peer {} not found", source_node_id)),
        };

        let data = client.pull_chunk(&peer_addr, volume_id, file_id, chunk_index).await
            .map_err(|e| format!("pull from {}: {}", source_node_id, e))?;

        let expected_sha = {
            let db = state.db.lock().unwrap();
            ChunkService::get_chunk_sha256(&db, chunk_id)
        };

        use sha2::{Sha256, Digest};
        let actual_sha = format!("{:x}", Sha256::digest(&data));

        if let Some(ref expected) = expected_sha {
            if *expected != actual_sha {
                return Err(format!("SHA256 mismatch from peer {}", source_node_id));
            }
        }

        let local_backend = {
            let db = state.db.lock().unwrap();
            let local_raid: String = db.query_row(
                "SELECT local_raid FROM volumes WHERE id = ?1",
                rusqlite::params![volume_id], |row| row.get(0),
            ).unwrap_or_else(|_| "stripe".into());
            let placements = crate::storage::chunk::place_chunk(&db, volume_id, &state.node_id, chunk_index, &local_raid);
            placements.into_iter().next()
        };

        if let Some((backend_id, backend_path)) = local_backend {
            let dst = crate::storage::chunk::chunk_path(&backend_path, volume_id, file_id, chunk_index);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).ok();
            }

            let tmp = dst.with_extension(format!("tmp.{}", Uuid::new_v4()));
            std::fs::write(&tmp, &data).map_err(|e| format!("write tmp: {}", e))?;
            if let Ok(f) = std::fs::File::open(&tmp) {
                f.sync_all().ok();
            }
            std::fs::rename(&tmp, &dst).map_err(|e| {
                std::fs::remove_file(&tmp).ok();
                format!("rename: {}", e)
            })?;

            let db = state.db.lock().unwrap();
            ChunkService::set_replica_synced(&db, chunk_id, &backend_id, &state.node_id)
                .map_err(|e| format!("set_replica_synced: {}", e))?;
            if expected_sha.is_none() {
                ChunkService::update_chunk_sha256_by_id(&db, chunk_id, &actual_sha)
                    .map_err(|e| format!("update sha256: {}", e))?;
            }

            tracing::info!("Repair(manual): pulled chunk {} (file {}, idx {}) from {}",
                chunk_id, file_id, chunk_index, source_node_id);
            return Ok(true);
        }

        return Err("no local backend available for placement".into());
    }

    // We have it locally — push to a peer that doesn't
    let local_chunk_info = {
        let db = state.db.lock().unwrap();
        ChunkService::find_local_chunk_path(&db, chunk_id, &state.node_id)
    };

    let (backend_path, expected_sha) = match local_chunk_info {
        Some(info) => info,
        None => return Err("local chunk path not found".into()),
    };

    let src = crate::storage::chunk::chunk_path(&backend_path, volume_id, file_id, chunk_index);
    let data = std::fs::read(&src).map_err(|e| format!("read local chunk: {}", e))?;

    if !expected_sha.is_empty() {
        use sha2::{Sha256, Digest};
        let local_sha = format!("{:x}", Sha256::digest(&data));
        if local_sha != expected_sha {
            return Err(format!("local chunk {} is corrupt", chunk_id));
        }
    }

    let target_peer = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .find(|p| !nodes_with_chunk.contains(&p.node_id))
        .map(|p| (p.node_id.clone(), p.address.clone()));

    if let Some((target_node_id, target_addr)) = target_peer {
        let rel_path = {
            let db = state.db.lock().unwrap();
            db.query_row("SELECT rel_path FROM file_map WHERE id = ?1",
                rusqlite::params![file_id], |row| row.get::<_, String>(0))
                .unwrap_or_default()
        };

        client.push_chunk_full(&target_addr, volume_id, file_id, chunk_index, data, &rel_path, &state.node_id).await
            .map_err(|e| format!("push to {}: {}", target_node_id, e))?;

        let db = state.db.lock().unwrap();
        ChunkService::track_remote_replica(&db, chunk_id, &target_node_id)
            .map_err(|e| format!("track_remote_replica: {}", e))?;

        tracing::info!("Repair(manual): pushed chunk {} (file {}, idx {}) to {}",
            chunk_id, file_id, chunk_index, target_node_id);
        return Ok(true);
    }

    Err("no available peer to push chunk to".into())
}

// ── Feature: Remove Host from Volume ────────────────────────────

#[derive(Deserialize)]
pub struct RemoveHostRequest {
    pub node_id: String,
}

#[derive(Serialize)]
pub struct RemoveHostResponse {
    pub success: bool,
    pub replicas_removed: u64,
    pub message: String,
}

/// POST /api/volumes/{id}/remove-host — remove a host's participation in a volume.
/// Checks if sync is complete and FTT won't be violated, then removes chunk_replicas
/// for that node from this volume.
pub async fn remove_host_from_volume(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
    Json(body): Json<RemoveHostRequest>,
) -> Result<Json<RemoveHostResponse>, (StatusCode, String)> {
    let db = state.db.lock().unwrap();

    // Verify volume exists and get FTT
    let ftt: u32 = db.query_row(
        "SELECT ftt FROM volumes WHERE id = ?1",
        rusqlite::params![&id], |row| row.get(0),
    ).map_err(|_| (StatusCode::NOT_FOUND, "Volume not found".into()))?;

    let required_nodes = ftt + 1;

    // Check for pending syncs (chunks in 'syncing' state for this node in this volume)
    let syncing_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM chunk_replicas cr
         JOIN file_chunks fc ON fc.id = cr.chunk_id
         JOIN file_map fm ON fm.id = fc.file_id
         WHERE fm.volume_id = ?1 AND cr.node_id = ?2 AND cr.state = 'syncing'",
        rusqlite::params![&id, &body.node_id], |row| row.get(0),
    ).unwrap_or(0);

    if syncing_count > 0 {
        return Err((StatusCode::CONFLICT,
            format!("Node {} has {} chunks still syncing for this volume. Wait for sync to complete.",
                body.node_id, syncing_count)));
    }

    // Check if removing this node would violate FTT for any chunk.
    // For each chunk in this volume, count distinct synced nodes EXCLUDING the target node.
    // If any chunk would have fewer than required_nodes, reject.
    let violation_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (
            SELECT fc.id AS chunk_id,
                   (SELECT COUNT(DISTINCT cr2.node_id) FROM chunk_replicas cr2
                    WHERE cr2.chunk_id = fc.id AND cr2.state = 'synced' AND cr2.node_id != ?3) AS remaining_nodes
            FROM file_chunks fc
            JOIN file_map fm ON fm.id = fc.file_id
            WHERE fm.volume_id = ?1
              AND fc.id IN (SELECT chunk_id FROM chunk_replicas WHERE node_id = ?2 AND state = 'synced')
        ) sub WHERE sub.remaining_nodes < ?4",
        rusqlite::params![&id, &body.node_id, &body.node_id, required_nodes],
        |row| row.get(0),
    ).unwrap_or(0);

    if violation_count > 0 {
        return Err((StatusCode::CONFLICT,
            format!(
                "Removing node {} would violate FTT={} for {} chunk(s). \
                 Ensure sufficient replicas exist on other nodes before removing this host.",
                body.node_id, ftt, violation_count
            )));
    }

    // Safe to remove: delete all chunk_replicas for this node for chunks in this volume
    let removed: usize = db.execute(
        "DELETE FROM chunk_replicas WHERE node_id = ?1 AND chunk_id IN (
            SELECT fc.id FROM file_chunks fc
            JOIN file_map fm ON fm.id = fc.file_id
            WHERE fm.volume_id = ?2
        )",
        rusqlite::params![&body.node_id, &id],
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Delete replicas failed: {}", e)))?;

    tracing::info!("Removed host {} from volume {}: {} replica records deleted",
        body.node_id, id, removed);

    Ok(Json(RemoveHostResponse {
        success: true,
        replicas_removed: removed as u64,
        message: format!("Removed node {} from volume. {} replica records deleted. \
                          Chunk files on the remote node can be cleaned up separately.",
            body.node_id, removed),
    }))
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    }
}

/// Calculate usable capacity based on RAID policy, only from claimed disks.
/// Used by volume create (validation), volume list (display), and allocate-disk (pre-check).
fn usable_capacity_for_raid(db: &rusqlite::Connection, node_id: &str, local_raid: &str) -> (u64, u64) {
    let backends: Vec<(u64, u64)> = {
        let mut stmt = db.prepare(
            "SELECT total_bytes, free_bytes FROM backends
             WHERE node_id = ?1 AND status = 'online' AND claimed_disk_id != ''
             ORDER BY total_bytes"
        ).unwrap();
        stmt.query_map(rusqlite::params![node_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };
    compute_raid_capacity(&backends, local_raid)
}

/// Same as above but without node_id filter — for use inside query_map closures
/// where the connection is already borrowed.
fn usable_capacity_for_raid_static(db: &rusqlite::Connection, local_raid: &str) -> (u64, u64) {
    let backends: Vec<(u64, u64)> = {
        let mut stmt = db.prepare(
            "SELECT total_bytes, free_bytes FROM backends
             WHERE status = 'online' AND claimed_disk_id != ''
             ORDER BY total_bytes"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    };
    compute_raid_capacity(&backends, local_raid)
}

fn compute_raid_capacity(backends: &[(u64, u64)], local_raid: &str) -> (u64, u64) {
    if backends.is_empty() {
        return (0, 0);
    }
    match local_raid {
        "mirror" => {
            // Mirror: usable = smallest disk (data mirrored to all)
            let min_total = backends.iter().map(|(t, _)| *t).min().unwrap_or(0);
            let min_free = backends.iter().map(|(_, f)| *f).min().unwrap_or(0);
            (min_total, min_free)
        }
        "stripe_mirror" => {
            // Stripe-mirror: usable = sum / 2
            let sum_total: u64 = backends.iter().map(|(t, _)| *t).sum();
            let sum_free: u64 = backends.iter().map(|(_, f)| *f).sum();
            (sum_total / 2, sum_free / 2)
        }
        _ => {
            // Stripe: usable = sum of all
            let sum_total: u64 = backends.iter().map(|(t, _)| *t).sum();
            let sum_free: u64 = backends.iter().map(|(_, f)| *f).sum();
            (sum_total, sum_free)
        }
    }
}
