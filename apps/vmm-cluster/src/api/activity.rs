//! Compatibility endpoints — mirrors vmm-server APIs so the existing UI works.
//!
//! All handlers are thin — they delegate to the service layer.

use axum::{Json, extract::{State, Query, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::services::audit::AuditService;
use crate::services::stats::StatsService;
use crate::services::storage_compat::StorageCompatService;
use crate::services::resource_group::ResourceGroupService;

// ── Activity Feed ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ActivityQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
}
fn default_limit() -> u32 { 20 }

pub async fn activity(
    _auth: AuthUser,
    State(state): State<Arc<ClusterState>>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let entries = AuditService::recent(&db, q.limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(entries).unwrap()))
}

// ── Resource Group permissions list (compat) ────────────────────────────

pub async fn resource_group_permissions_list(
    _auth: AuthUser,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "permissions": [
            "vm.create", "vm.edit", "vm.delete", "vm.start_stop", "vm.console",
            "network.edit", "storage.edit", "snapshots.manage"
        ],
        "categories": {
            "Virtual Machines": ["vm.create", "vm.edit", "vm.delete", "vm.start_stop", "vm.console"],
            "Infrastructure": ["network.edit", "storage.edit", "snapshots.manage"]
        }
    }))
}

// ── Settings (compat stubs) ─────────────────────────────────────────────

pub async fn settings_server(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Json<serde_json::Value> {
    let uptime = state.started_at.elapsed().as_secs();
    Json(serde_json::json!({
        "bind": state.config.server.bind,
        "port": state.config.server.port,
        "session_timeout_hours": state.config.auth.session_timeout_hours,
        "max_disk_size_gb": 2048,
        "log_level": state.config.logging.level,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime,
    }))
}

pub async fn settings_time(_auth: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "current_time": chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "timezone": "UTC", "ntp_enabled": false, "ntp_servers": [],
    }))
}

pub async fn settings_security(_auth: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "max_login_attempts": 5, "lockout_duration_secs": 300,
        "password_min_length": 6, "require_uppercase": false,
        "require_numbers": false, "api_keys_enabled": false,
    }))
}

pub async fn list_settings_groups(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let groups = crate::services::group::GroupService::list(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::Value::Array(groups)))
}

pub async fn create_settings_group(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let role = body.get("role").and_then(|v| v.as_str()).unwrap_or("viewer");
    let desc = body.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = crate::services::group::GroupService::create(&db, name, role, desc)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn delete_settings_group(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    crate::services::group::GroupService::delete(&db, id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Network (stubs) ─────────────────────────────────────────────────────

pub async fn network_stats(_auth: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "total_interfaces": 0, "active_interfaces": 0, "total_rx_bytes": 0, "total_tx_bytes": 0 }))
}

pub async fn network_interfaces(_auth: AuthUser) -> Json<Vec<serde_json::Value>> {
    Json(Vec::new())
}

// ── Resource Groups ─────────────────────────────────────────────────────

pub async fn list_resource_groups(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let groups = ResourceGroupService::list(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::Value::Array(groups)))
}

// ── Storage Pools (compat — maps datastores) ────────────────────────────

#[derive(Deserialize)]
pub struct PoolsQuery {
    pub cluster_id: Option<String>,
}

pub async fn list_storage_pools(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
    Query(q): Query<PoolsQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    // Treat empty string as None (no filter)
    let cid = q.cluster_id.as_deref().filter(|s| !s.is_empty());
    let pools = StorageCompatService::list_pools(&db, cid)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::Value::Array(pools)))
}

pub async fn delete_storage_pool(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    crate::services::datastore::DatastoreService::delete(&db, &id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub ext: Option<String>,
}

pub async fn browse_storage_pool(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<BrowseQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let files = StorageCompatService::browse(&db, &id, q.ext.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::Value::Array(files)))
}

pub async fn storage_stats(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let stats = StatsService::storage_stats(&db);
    Ok(Json(serde_json::to_value(stats).unwrap()))
}

// ── Disk Images & ISOs ──────────────────────────────────────────────────

pub async fn list_images(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let images = StorageCompatService::list_images(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::Value::Array(images)))
}

pub async fn list_isos(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let isos = StorageCompatService::list_isos(&db)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::Value::Array(isos)))
}
