//! Storage management API — pools, disk images, ISOs.

use axum::{extract::{Path, State, Multipart}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError, require_admin, require_operator};

fn db_err(e: impl std::fmt::Display) -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ── Storage Pools ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StoragePool {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub pool_type: String,
    pub shared: bool,
    pub mount_source: Option<String>,
    pub mount_opts: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Deserialize)]
pub struct CreatePoolRequest {
    pub name: String,
    pub path: String,
    /// "local", "nfs", "cephfs", "glusterfs"
    #[serde(default = "default_pool_type")]
    pub pool_type: String,
    /// NFS: "server:/export", CephFS: "mon1,mon2:/path"
    pub mount_source: Option<String>,
    /// Mount options (e.g. "vers=4,noatime")
    pub mount_opts: Option<String>,
}
fn default_pool_type() -> String { "local".into() }

fn get_disk_space(path: &str) -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            let c_path = std::ffi::CString::new(path).unwrap_or_default();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let total = stat.f_blocks as u64 * stat.f_frsize as u64;
                let free = stat.f_bavail as u64 * stat.f_frsize as u64;
                return (total, free);
            }
        }
    }
    (0, 0)
}

/// GET /api/storage/pools
pub async fn list_pools(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<StoragePool>>, AppError> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, path, pool_type, shared, mount_source, mount_opts FROM storage_pools ORDER BY name"
    ).map_err(db_err)?;
    let pools: Vec<StoragePool> = stmt.query_map([], |row| {
        Ok((
            row.get::<_,i64>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?,
            row.get::<_,String>(3)?, row.get::<_,bool>(4)?,
            row.get::<_,Option<String>>(5)?, row.get::<_,Option<String>>(6)?,
        ))
    }).map_err(db_err)?
    .filter_map(|r| r.ok())
    .map(|(id, name, path, pool_type, shared, mount_source, mount_opts)| {
        let (total_bytes, free_bytes) = get_disk_space(&path);
        StoragePool { id, name, path, pool_type, shared, mount_source, mount_opts, total_bytes, free_bytes }
    }).collect();
    Ok(Json(pools))
}

/// POST /api/storage/pools
pub async fn create_pool(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;

    let valid_types = ["local", "nfs", "cephfs", "glusterfs"];
    if !valid_types.contains(&req.pool_type.as_str()) {
        return Err(AppError(StatusCode::BAD_REQUEST,
            format!("Invalid pool_type. Must be one of: {}", valid_types.join(", "))));
    }

    let shared = req.pool_type != "local";

    // For shared storage, mount_source is required
    if shared && req.mount_source.is_none() {
        return Err(AppError(StatusCode::BAD_REQUEST,
            "mount_source is required for shared storage (e.g. \"server:/export\")".into()));
    }

    let path = std::path::Path::new(&req.path);
    if !path.exists() {
        std::fs::create_dir_all(path)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Cannot create directory: {}", e)))?;
    }

    let db = state.db.lock().unwrap();
    db.execute(
        "INSERT INTO storage_pools (name, path, pool_type, shared, mount_source, mount_opts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![&req.name, &req.path, &req.pool_type, shared, &req.mount_source, &req.mount_opts],
    ).map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError(StatusCode::CONFLICT, "Pool path already registered".into())
        } else { db_err(e) }
    })?;
    let id = db.last_insert_rowid();
    Ok(Json(serde_json::json!({
        "id": id, "name": req.name, "path": req.path,
        "pool_type": req.pool_type, "shared": shared
    })))
}

/// DELETE /api/storage/pools/:id
pub async fn delete_pool(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(pool_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    // Check no images reference this pool
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM disk_images WHERE pool_id = ?1", rusqlite::params![pool_id], |r| r.get(0),
    ).map_err(db_err)?;
    if count > 0 {
        return Err(AppError(StatusCode::CONFLICT, format!("{} disk images still in this pool", count)));
    }
    let affected = db.execute("DELETE FROM storage_pools WHERE id = ?1", rusqlite::params![pool_id])
        .map_err(db_err)?;
    if affected == 0 { return Err(AppError(StatusCode::NOT_FOUND, "Pool not found".into())); }
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Disk Images ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DiskImage {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size_bytes: i64,
    pub format: String,
    pub pool_id: Option<i64>,
    pub vm_id: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateDiskRequest {
    pub name: String,
    pub size_gb: u64,
    pub pool_id: i64,
}

#[derive(Deserialize)]
pub struct ResizeDiskRequest {
    pub size_gb: u64,
}

/// GET /api/storage/images
pub async fn list_images(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DiskImage>>, AppError> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT id, name, path, size_bytes, format, pool_id, vm_id, created_at FROM disk_images ORDER BY name"
    ).map_err(db_err)?;
    let images: Vec<DiskImage> = stmt.query_map([], |row| {
        Ok(DiskImage {
            id: row.get(0)?, name: row.get(1)?, path: row.get(2)?,
            size_bytes: row.get(3)?, format: row.get(4)?,
            pool_id: row.get(5)?, vm_id: row.get(6)?, created_at: row.get(7)?,
        })
    }).map_err(db_err)?
    .filter_map(|r| r.ok()).collect();
    Ok(Json(images))
}

/// POST /api/storage/images — create a new raw disk image.
pub async fn create_image(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateDiskRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;

    let size_bytes = req.size_gb * 1024 * 1024 * 1024;
    if req.size_gb > state.config.storage.max_disk_size_gb {
        return Err(AppError(StatusCode::BAD_REQUEST,
            format!("Max disk size is {} GB", state.config.storage.max_disk_size_gb)));
    }

    // Get pool path
    let db = state.db.lock().unwrap();
    let pool_path: String = db.query_row(
        "SELECT path FROM storage_pools WHERE id = ?1", rusqlite::params![req.pool_id], |r| r.get(0),
    ).map_err(|_| AppError(StatusCode::NOT_FOUND, "Storage pool not found".into()))?;

    let filename = format!("{}.raw", req.name.replace(' ', "_").to_lowercase());
    let disk_path = std::path::Path::new(&pool_path).join(&filename);
    if disk_path.exists() {
        return Err(AppError(StatusCode::CONFLICT, "Disk image already exists".into()));
    }

    // Create sparse file
    let file = std::fs::File::create(&disk_path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Create failed: {}", e)))?;
    file.set_len(size_bytes)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Allocate failed: {}", e)))?;

    let path_str = disk_path.to_string_lossy().to_string();
    db.execute(
        "INSERT INTO disk_images (name, path, size_bytes, format, pool_id) VALUES (?1, ?2, ?3, 'raw', ?4)",
        rusqlite::params![&req.name, &path_str, size_bytes as i64, req.pool_id],
    ).map_err(db_err)?;
    let id = db.last_insert_rowid();

    Ok(Json(serde_json::json!({"id": id, "name": req.name, "path": path_str, "size_bytes": size_bytes})))
}

/// DELETE /api/storage/images/:id
pub async fn delete_image(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();

    // Check not attached to a running VM
    let result = db.query_row(
        "SELECT path, vm_id FROM disk_images WHERE id = ?1", rusqlite::params![image_id],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,Option<String>>(1)?)),
    ).map_err(|_| AppError(StatusCode::NOT_FOUND, "Disk image not found".into()))?;
    let (path, vm_id) = result;

    if let Some(ref vid) = vm_id {
        if let Some(vm) = state.vms.get(vid) {
            if vm.state != crate::state::VmState::Stopped {
                return Err(AppError(StatusCode::CONFLICT, "Disk is attached to a running VM".into()));
            }
        }
    }

    // Delete file
    let _ = std::fs::remove_file(&path);
    db.execute("DELETE FROM disk_images WHERE id = ?1", rusqlite::params![image_id]).map_err(db_err)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /api/storage/images/:id/resize
pub async fn resize_image(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<i64>,
    Json(req): Json<ResizeDiskRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let new_size = req.size_gb * 1024 * 1024 * 1024;

    let db = state.db.lock().unwrap();
    let (path, current_size): (String, i64) = db.query_row(
        "SELECT path, size_bytes FROM disk_images WHERE id = ?1", rusqlite::params![image_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|_| AppError(StatusCode::NOT_FOUND, "Disk image not found".into()))?;

    if (new_size as i64) < current_size {
        return Err(AppError(StatusCode::BAD_REQUEST, "Cannot shrink disk image (data loss risk)".into()));
    }

    let file = std::fs::OpenOptions::new().write(true).open(&path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Open failed: {}", e)))?;
    file.set_len(new_size)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Resize failed: {}", e)))?;

    db.execute(
        "UPDATE disk_images SET size_bytes = ?1 WHERE id = ?2",
        rusqlite::params![new_size as i64, image_id],
    ).map_err(db_err)?;

    Ok(Json(serde_json::json!({"ok": true, "size_bytes": new_size})))
}

// ── ISOs ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Iso {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size_bytes: i64,
    pub uploaded_at: String,
}

/// GET /api/storage/isos
pub async fn list_isos(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Iso>>, AppError> {
    let db = state.db.lock().unwrap();
    let mut stmt = db.prepare("SELECT id, name, path, size_bytes, uploaded_at FROM isos ORDER BY name")
        .map_err(db_err)?;
    let isos: Vec<Iso> = stmt.query_map([], |row| {
        Ok(Iso {
            id: row.get(0)?, name: row.get(1)?, path: row.get(2)?,
            size_bytes: row.get(3)?, uploaded_at: row.get(4)?,
        })
    }).map_err(db_err)?
    .filter_map(|r| r.ok()).collect();
    Ok(Json(isos))
}

/// POST /api/storage/isos/upload — multipart upload.
pub async fn upload_iso(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            let filename = field.file_name()
                .unwrap_or("upload.iso")
                .to_string();
            let iso_dir = &state.config.storage.iso_pool;
            let iso_path = iso_dir.join(&filename);

            if iso_path.exists() {
                return Err(AppError(StatusCode::CONFLICT, "ISO already exists".into()));
            }

            // Stream to disk
            let data = field.bytes().await
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Upload error: {}", e)))?;
            std::fs::write(&iso_path, &data)
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Write error: {}", e)))?;

            let size = data.len() as i64;
            let path_str = iso_path.to_string_lossy().to_string();

            let db = state.db.lock().unwrap();
            db.execute(
                "INSERT INTO isos (name, path, size_bytes) VALUES (?1, ?2, ?3)",
                rusqlite::params![&filename, &path_str, size],
            ).map_err(db_err)?;
            let id = db.last_insert_rowid();

            return Ok(Json(serde_json::json!({"id": id, "name": filename, "size_bytes": size})));
        }
    }

    Err(AppError(StatusCode::BAD_REQUEST, "No file field in upload".into()))
}

/// DELETE /api/storage/isos/:id
pub async fn delete_iso(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(iso_id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    let path: String = db.query_row(
        "SELECT path FROM isos WHERE id = ?1", rusqlite::params![iso_id], |r| r.get(0),
    ).map_err(|_| AppError(StatusCode::NOT_FOUND, "ISO not found".into()))?;

    let _ = std::fs::remove_file(&path);
    db.execute("DELETE FROM isos WHERE id = ?1", rusqlite::params![iso_id]).map_err(db_err)?;
    Ok(Json(serde_json::json!({"ok": true})))
}
