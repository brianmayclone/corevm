//! Storage management API endpoints — pools, disk images, ISOs.

use axum::{extract::{Path, State, Query, Multipart}, http::StatusCode, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::state::AppState;
use crate::auth::middleware::{AuthUser, AppError, require_admin, require_operator};
use crate::services::storage::StorageService;

// ── Storage Pools ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePoolRequest {
    pub name: String,
    pub path: String,
    #[serde(default = "default_pool_type")]
    pub pool_type: String,
    pub mount_source: Option<String>,
    pub mount_opts: Option<String>,
}
fn default_pool_type() -> String { "local".into() }

/// GET /api/storage/pools
pub async fn list_pools(_auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let pools = StorageService::list_pools(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(pools).unwrap()))
}

/// POST /api/storage/pools
pub async fn create_pool(auth: AuthUser, State(state): State<Arc<AppState>>, Json(req): Json<CreatePoolRequest>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    let id = StorageService::create_pool(&db, &req.name, &req.path, &req.pool_type, req.mount_source.as_deref(), req.mount_opts.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id, "name": req.name, "path": req.path, "pool_type": req.pool_type})))
}

/// DELETE /api/storage/pools/:id
pub async fn delete_pool(auth: AuthUser, State(state): State<Arc<AppState>>, Path(pool_id): Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&auth)?;
    let db = state.db.lock().unwrap();
    StorageService::delete_pool(&db, pool_id).map_err(|e| AppError(StatusCode::CONFLICT, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Disk Images ──────────────────────────────────────────────────────────

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
pub async fn list_images(_auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let images = StorageService::list_images(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(images).unwrap()))
}

/// POST /api/storage/images
pub async fn create_image(auth: AuthUser, State(state): State<Arc<AppState>>, Json(req): Json<CreateDiskRequest>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    let (id, path) = StorageService::create_image(&db, &req.name, req.size_gb, req.pool_id, state.config.storage.max_disk_size_gb)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id, "name": req.name, "path": path})))
}

/// DELETE /api/storage/images/:id
pub async fn delete_image(auth: AuthUser, State(state): State<Arc<AppState>>, Path(image_id): Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    StorageService::delete_image(&db, image_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /api/storage/images/:id/resize
pub async fn resize_image(auth: AuthUser, State(state): State<Arc<AppState>>, Path(image_id): Path<i64>, Json(req): Json<ResizeDiskRequest>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    StorageService::resize_image(&db, image_id, req.size_gb)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── ISOs ─────────────────────────────────────────────────────────────────

/// GET /api/storage/isos
pub async fn list_isos(_auth: AuthUser, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let isos = StorageService::list_isos(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(isos).unwrap()))
}

/// POST /api/storage/isos/upload — multipart upload.
pub async fn upload_iso(auth: AuthUser, State(state): State<Arc<AppState>>, mut multipart: Multipart) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            let filename = field.file_name().unwrap_or("upload.iso").to_string();
            let iso_path = state.config.storage.iso_pool.join(&filename);
            if iso_path.exists() {
                return Err(AppError(StatusCode::CONFLICT, "ISO already exists".into()));
            }
            let data = field.bytes().await
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Upload error: {}", e)))?;
            std::fs::write(&iso_path, &data)
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, format!("Write error: {}", e)))?;

            let path_str = iso_path.to_string_lossy().to_string();
            let db = state.db.lock().unwrap();
            let id = StorageService::save_iso(&db, &filename, &path_str, data.len() as i64)
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            return Ok(Json(serde_json::json!({"id": id, "name": filename, "size_bytes": data.len()})));
        }
    }
    Err(AppError(StatusCode::BAD_REQUEST, "No file field in upload".into()))
}

/// DELETE /api/storage/isos/:id
pub async fn delete_iso(auth: AuthUser, State(state): State<Arc<AppState>>, Path(iso_id): Path<i64>) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    StorageService::delete_iso(&db, iso_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Pool Browsing ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub ext: Option<String>,
}

/// GET /api/storage/pools/:id/browse — list files in a pool directory.
pub async fn browse_pool(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Path(pool_id): Path<i64>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let files = StorageService::browse_pool(&db, pool_id, q.ext.as_deref())
        .map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(files).unwrap()))
}

// ── Auto-create VM disk ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateVmDiskRequest {
    pub vm_name: String,
    pub vm_id: String,
    pub size_gb: u64,
    pub pool_id: i64,
}

/// POST /api/storage/vm-disk — create a disk automatically in pool/<vm_name>/disk.raw
pub async fn create_vm_disk(
    auth: AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateVmDiskRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&auth)?;
    let db = state.db.lock().unwrap();
    let (id, path) = StorageService::create_vm_disk(&db, &req.vm_name, &req.vm_id, req.size_gb, req.pool_id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id, "path": path})))
}

// ── Aggregate Stats ──────────────────────────────────────────────────────

/// GET /api/storage/stats — aggregate storage stats across all pools.
pub async fn storage_stats(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let stats = StorageService::aggregate_stats(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(stats).unwrap()))
}
