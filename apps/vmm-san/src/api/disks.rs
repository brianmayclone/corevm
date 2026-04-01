//! Disk management API — discover, claim, release, and reset physical disks.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;
use crate::state::CoreSanState;
use crate::storage::{disk, disk_ops};

/// GET /api/disks — list all block devices with their availability status.
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
) -> Json<Vec<disk::DiscoveredDisk>> {
    let db = state.db.lock().unwrap();
    Json(disk::discover_disks(&db))
}

/// GET /api/disks/{device_name}/smart — full SMART detail for a specific disk.
pub async fn smart_detail(
    State(state): State<Arc<CoreSanState>>,
    Path(device_name): Path<String>,
) -> Result<Json<crate::storage::smart::SmartData>, (StatusCode, String)> {
    let device_path = format!("/dev/{}", device_name);

    // Try DB first (cached data from smart_monitor)
    let cached = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT device_path, supported, health_passed, transport, power_on_hours, temperature_c,
                    reallocated_sectors, pending_sectors, uncorrectable_sectors, wear_leveling_pct,
                    media_errors, percentage_used, model, serial, firmware, raw_json, collected_at
             FROM smart_data WHERE device_path = ?1",
            rusqlite::params![&device_path],
            |row| {
                Ok(crate::storage::smart::SmartData {
                    device_path: row.get(0)?,
                    supported: row.get::<_, i32>(1)? != 0,
                    health_passed: row.get::<_, Option<i32>>(2)?.map(|v| v != 0),
                    transport: row.get(3)?,
                    power_on_hours: row.get(4)?,
                    temperature_celsius: row.get(5)?,
                    reallocated_sectors: row.get(6)?,
                    pending_sectors: row.get(7)?,
                    uncorrectable_sectors: row.get(8)?,
                    wear_leveling_pct: row.get::<_, Option<i64>>(9)?.map(|v| v as u8),
                    media_errors: row.get(10)?,
                    percentage_used: row.get::<_, Option<i64>>(11)?.map(|v| v as u8),
                    model: row.get(12)?,
                    serial: row.get(13)?,
                    firmware: row.get(14)?,
                    raw_json: row.get(15)?,
                    collected_at: row.get(16)?,
                })
            },
        ).ok()
    };

    if let Some(data) = cached {
        return Ok(Json(data));
    }

    // Not in DB yet — read directly (blocks, but only on demand)
    let path = device_path.clone();
    let data = tokio::task::spawn_blocking(move || {
        crate::storage::smart::read_smart(&path)
    }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?;

    Ok(Json(data))
}

#[derive(Deserialize)]
pub struct ClaimDiskRequest {
    pub device_path: String,
    #[serde(default = "default_ext4")]
    pub fs_type: String,
    #[serde(default)]
    pub confirm_format: bool,
}
fn default_ext4() -> String { "ext4".into() }

/// POST /api/disks/claim — claim a disk: format, mount, create backend.
pub async fn claim(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<ClaimDiskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Validate filesystem type
    if !["ext4", "xfs"].contains(&body.fs_type.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "fs_type must be 'ext4' or 'xfs'".into()));
    }

    // Discover disks and find the requested one
    let disk_info = {
        let db = state.db.lock().unwrap();
        let disks = disk::discover_disks(&db);
        disks.into_iter().find(|d| d.device.path == body.device_path)
    };

    let disk_info = match disk_info {
        Some(d) => d,
        None => return Err((StatusCode::NOT_FOUND,
            format!("Device not found: {}", body.device_path))),
    };

    // Check availability
    match &disk_info.status {
        disk::DiskStatus::InUse { mountpoint } => {
            return Err((StatusCode::CONFLICT,
                format!("Disk is mounted at {} — unmount it first", mountpoint)));
        }
        disk::DiskStatus::Claimed { disk_id, .. } => {
            return Err((StatusCode::CONFLICT,
                format!("Disk is already claimed (id={})", disk_id)));
        }
        disk::DiskStatus::HasData { fs_type } => {
            if !body.confirm_format {
                return Err((StatusCode::CONFLICT, serde_json::json!({
                    "error": "Disk has existing data",
                    "fs_type": fs_type,
                    "model": disk_info.device.model,
                    "size_bytes": disk_info.device.size_bytes,
                    "hint": "Set confirm_format=true to wipe and format the disk"
                }).to_string()));
            }
        }
        disk::DiskStatus::Available => { /* Good to go */ }
        disk::DiskStatus::OsDisk => {
            return Err((StatusCode::FORBIDDEN, "Cannot claim the OS disk".into()));
        }
    }

    let disk_id = Uuid::new_v4().to_string();
    let mount_path = format!("/vmm/san-disks/{}", disk_id);

    // Remove any old released/error entry for this device_path so re-claim works
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "DELETE FROM claimed_disks WHERE device_path = ?1 AND status IN ('released', 'error')",
            rusqlite::params![&body.device_path],
        ).ok();
    }

    // Record claim in DB (status: formatting)
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO claimed_disks (id, device_path, mount_path, fs_type, model, serial, size_bytes, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'formatting')",
            rusqlite::params![
                &disk_id, &body.device_path, &mount_path, &body.fs_type,
                &disk_info.device.model, &disk_info.device.serial,
                disk_info.device.size_bytes
            ],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
    }

    // Format the disk (blocking operation)
    let device = body.device_path.clone();
    let fs = body.fs_type.clone();
    let mp = mount_path.clone();

    let format_result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let uuid = disk_ops::format_disk(&device, &fs)?;
        disk_ops::mount_disk(&device, &mp)?;
        disk_ops::create_mount_unit(&mp, &uuid, &fs)?;
        Ok(uuid)
    }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task error: {}", e)))?;

    let device_uuid = match format_result {
        Ok(uuid) => uuid,
        Err(e) => {
            // Mark as error in DB
            let db = state.db.lock().unwrap();
            db.execute("UPDATE claimed_disks SET status = 'error' WHERE id = ?1",
                rusqlite::params![&disk_id]).ok();
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Format failed: {}", e)));
        }
    };

    // Create backend for this disk
    let backend_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let (total_bytes, free_bytes) = crate::storage::backend::refresh_stats(&mount_path);

    {
        let db = state.db.lock().unwrap();

        // Update claimed_disks with UUID and status
        db.execute(
            "UPDATE claimed_disks SET device_uuid = ?1, status = 'mounted', backend_id = ?2 WHERE id = ?3",
            rusqlite::params![&device_uuid, &backend_id, &disk_id],
        ).ok();

        // Create backend
        db.execute(
            "INSERT INTO backends (id, node_id, path, total_bytes, free_bytes, status, last_check, claimed_disk_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'online', ?6, ?7)",
            rusqlite::params![
                &backend_id, &state.node_id, &mount_path,
                total_bytes, free_bytes, &now, &disk_id
            ],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Backend creation: {}", e)))?;
    }

    tracing::info!("Claimed disk {} → {} (backend={})",
        body.device_path, mount_path, backend_id);

    Ok(Json(serde_json::json!({
        "disk_id": disk_id,
        "backend_id": backend_id,
        "device_path": body.device_path,
        "mount_path": mount_path,
        "device_uuid": device_uuid,
        "fs_type": body.fs_type,
    })))
}

#[derive(Deserialize)]
pub struct CreateFileDiskRequest {
    /// Size in bytes for the virtual disk file.
    pub size_bytes: u64,
    #[serde(default = "default_ext4")]
    pub fs_type: String,
    /// Optional custom name (used in filename). Defaults to UUID.
    #[serde(default)]
    pub name: String,
}

/// POST /api/disks/create-file — create a file-backed virtual disk for development/testing.
///
/// Creates a sparse file, attaches it as a loop device, formats it, mounts it,
/// and registers it as a backend — identical to a physical claimed disk from that point on.
pub async fn create_file(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateFileDiskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !["ext4", "xfs"].contains(&body.fs_type.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "fs_type must be 'ext4' or 'xfs'".into()));
    }
    if body.size_bytes < 64 * 1024 * 1024 {
        return Err((StatusCode::BAD_REQUEST, "Minimum size is 64 MB".into()));
    }

    let disk_id = Uuid::new_v4().to_string();
    let label = if body.name.is_empty() { disk_id.clone() } else { body.name.clone() };
    let file_dir = state.config.data.data_dir.join("file-disks");
    let file_path = file_dir.join(format!("{}.img", label));
    let file_path_str = file_path.to_string_lossy().to_string();
    let mount_path = format!("/vmm/san-disks/{}", disk_id);

    // Record in DB as formatting
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO claimed_disks (id, device_path, mount_path, fs_type, model, serial, size_bytes, status)
             VALUES (?1, ?2, ?3, ?4, 'file-disk', ?5, ?6, 'formatting')",
            rusqlite::params![
                &disk_id, &file_path_str, &mount_path, &body.fs_type,
                &label, body.size_bytes
            ],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", e)))?;
    }

    // Create file, attach loop, format, mount (blocking)
    let fs = body.fs_type.clone();
    let fp = file_path_str.clone();
    let mp = mount_path.clone();
    let size = body.size_bytes;

    let result = tokio::task::spawn_blocking(move || -> Result<(String, String), String> {
        let (loop_dev, uuid) = disk_ops::create_file_disk(&fp, size, &fs)?;
        disk_ops::mount_disk(&loop_dev, &mp)?;
        Ok((loop_dev, uuid))
    }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task error: {}", e)))?;

    let (loop_device, device_uuid) = match result {
        Ok(r) => r,
        Err(e) => {
            let db = state.db.lock().unwrap();
            db.execute("UPDATE claimed_disks SET status = 'error' WHERE id = ?1",
                rusqlite::params![&disk_id]).ok();
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {}", e)));
        }
    };

    // Create backend
    let backend_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let (total_bytes, free_bytes) = crate::storage::backend::refresh_stats(&mount_path);

    {
        let db = state.db.lock().unwrap();

        db.execute(
            "UPDATE claimed_disks SET device_uuid = ?1, status = 'mounted', backend_id = ?2, device_path = ?3 WHERE id = ?4",
            rusqlite::params![&device_uuid, &backend_id, &loop_device, &disk_id],
        ).ok();

        db.execute(
            "INSERT INTO backends (id, node_id, path, total_bytes, free_bytes, status, last_check, claimed_disk_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'online', ?6, ?7)",
            rusqlite::params![
                &backend_id, &state.node_id, &mount_path,
                total_bytes, free_bytes, &now, &disk_id
            ],
        ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Backend creation: {}", e)))?;
    }

    tracing::info!("Created file-disk {} → {} → {} (backend={})",
        file_path_str, loop_device, mount_path, backend_id);

    Ok(Json(serde_json::json!({
        "disk_id": disk_id,
        "backend_id": backend_id,
        "file_path": file_path_str,
        "loop_device": loop_device,
        "mount_path": mount_path,
        "device_uuid": device_uuid,
        "fs_type": body.fs_type,
        "size_bytes": body.size_bytes,
    })))
}

#[derive(Deserialize)]
pub struct ReleaseDiskRequest {
    pub device_path: String,
    #[serde(default)]
    pub wipe: bool,
}

/// POST /api/disks/release — release a claimed disk.
pub async fn release(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<ReleaseDiskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (disk_id, mount_path, backend_id) = {
        let db = state.db.lock().unwrap();
        db.query_row(
            "SELECT id, mount_path, backend_id FROM claimed_disks WHERE device_path = ?1 AND status = 'mounted'",
            rusqlite::params![&body.device_path],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).map_err(|_| (StatusCode::NOT_FOUND, format!("Disk {} is not claimed", body.device_path)))?
    };

    // Remove backend (triggers drain if it has files)
    {
        let db = state.db.lock().unwrap();
        // Delete chunk replicas on this backend
        db.execute(
            "DELETE FROM chunk_replicas WHERE backend_id = ?1",
            rusqlite::params![&backend_id],
        ).ok();
        // Also clean up legacy file_replicas
        db.execute(
            "DELETE FROM file_replicas WHERE backend_id = ?1",
            rusqlite::params![&backend_id],
        ).ok();
        db.execute(
            "DELETE FROM backends WHERE id = ?1",
            rusqlite::params![&backend_id],
        ).ok();
    }

    // Unmount and remove mount unit
    let mp = mount_path.clone();
    let dev = body.device_path.clone();
    let do_wipe = body.wipe;

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        disk_ops::unmount_disk(&mp)?;
        disk_ops::remove_mount_unit(&mp)?;
        if do_wipe {
            disk_ops::wipe_disk(&dev)?;
        }
        Ok(())
    }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
      .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Update DB
    {
        let db = state.db.lock().unwrap();
        db.execute(
            "UPDATE claimed_disks SET status = 'released' WHERE id = ?1",
            rusqlite::params![&disk_id],
        ).ok();
    }

    tracing::info!("Released disk {} (wipe={})", body.device_path, body.wipe);

    Ok(Json(serde_json::json!({ "success": true, "wiped": body.wipe })))
}

#[derive(Deserialize)]
pub struct ResetDiskRequest {
    pub device_path: String,
}

/// POST /api/disks/reset — reset a disk with existing data (not mounted, not OS).
pub async fn reset(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<ResetDiskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Check disk status
    let disk_info = {
        let db = state.db.lock().unwrap();
        let disks = disk::discover_disks(&db);
        disks.into_iter().find(|d| d.device.path == body.device_path)
    };

    let disk_info = match disk_info {
        Some(d) => d,
        None => return Err((StatusCode::NOT_FOUND,
            format!("Device not found: {}", body.device_path))),
    };

    match &disk_info.status {
        disk::DiskStatus::OsDisk => {
            return Err((StatusCode::FORBIDDEN, "Cannot reset the OS disk".into()));
        }
        disk::DiskStatus::InUse { mountpoint } => {
            return Err((StatusCode::CONFLICT,
                format!("Disk is mounted at {} — unmount it first", mountpoint)));
        }
        disk::DiskStatus::Claimed { .. } => {
            return Err((StatusCode::CONFLICT,
                "Disk is claimed by CoreSAN — release it first".into()));
        }
        disk::DiskStatus::HasData { .. } | disk::DiskStatus::Available => {
            // OK to reset
        }
    }

    let dev = body.device_path.clone();
    tokio::task::spawn_blocking(move || disk_ops::reset_disk(&dev))
        .await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}", e)))?
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    tracing::info!("Reset disk {} (signatures wiped, partition table destroyed)", body.device_path);

    Ok(Json(serde_json::json!({
        "success": true,
        "device_path": body.device_path,
        "status": "available"
    })))
}
