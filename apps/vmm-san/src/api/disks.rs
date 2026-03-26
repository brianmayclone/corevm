//! Disk management API — discover, claim, release, and reset physical disks.

use axum::extract::State;
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
        disk::DiskStatus::OsDisk => {
            return Err((StatusCode::FORBIDDEN,
                "Cannot claim the OS disk".into()));
        }
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
    }

    let disk_id = Uuid::new_v4().to_string();
    let mount_path = format!("/vmm/san-disks/{}", disk_id);

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
        // Delete file replicas on this backend
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
