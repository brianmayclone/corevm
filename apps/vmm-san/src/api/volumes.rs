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
    /// Maximum volume size in bytes (required).
    pub max_size_bytes: u64,
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
    pub max_size_bytes: u64,
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

    db.execute(
        "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, max_size_bytes, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'online')",
        rusqlite::params![&id, &body.name, body.ftt, body.chunk_size_bytes, &body.local_raid, body.max_size_bytes],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create volume: {}", e)))?;

    // Create FUSE mount point for this volume
    let fuse_path = state.config.data.fuse_root.join(&body.name);
    if !fuse_path.exists() {
        std::fs::create_dir_all(&fuse_path).ok();
    }

    // Backends are ONLY claimed disks (/vmm/san-disks/*).
    // The root filesystem must NEVER be used as a storage backend.

    tracing::info!("Created volume '{}' (id={}, ftt={}, local_raid={}, chunk={}MB)",
        body.name, id, body.ftt, body.local_raid, body.chunk_size_bytes / (1024 * 1024));

    drop(db);

    // Mount FUSE for the new volume
    {
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

    Ok((StatusCode::CREATED, Json(VolumeResponse {
        id,
        name: body.name,
        ftt: body.ftt,
        local_raid: body.local_raid,
        chunk_size_bytes: body.chunk_size_bytes,
        max_size_bytes: body.max_size_bytes,
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

    let backend_count: u32 = db.query_row(
        "SELECT COUNT(*) FROM backends WHERE status = 'online' AND claimed_disk_id != ''",
        [], |row| row.get(0),
    ).unwrap_or(0);

    let mut stmt = db.prepare(
        "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at, max_size_bytes
         FROM volumes ORDER BY name"
    ).unwrap();

    let volumes = stmt.query_map([], |row| {
        let local_raid: String = row.get(3)?;
        // Calculate RAID-corrected capacity from claimed disks only
        let (total_bytes, free_bytes) = usable_capacity_for_raid_static(&db, &local_raid);
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
        "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at, max_size_bytes
         FROM volumes WHERE id = ?1",
        rusqlite::params![&id],
        |row| Ok(VolumeResponse {
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
        let client = crate::peer::client::PeerClient::new(&secret);
        for addr in peers {
            client.delete_volume(&addr, &vol_id).await.ok();
        }
    });

    Ok(Json(serde_json::json!({ "success": true })))
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

    tracing::info!("Synced volume '{}' from peer (id={}, ftt={}, raid={})",
        body.name, body.id, body.ftt, body.local_raid);

    Ok(Json(serde_json::json!({ "synced": true, "already_exists": false })))
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
