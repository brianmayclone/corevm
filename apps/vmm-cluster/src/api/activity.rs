//! Activity/audit and network compat endpoints.
//!
//! These endpoints mirror vmm-server's API so the existing UI Dashboard
//! works without changes when connected to vmm-cluster.

use axum::{Json, extract::{State, Query}};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::services::audit::AuditService;

// ── Activity Feed (mirrors vmm-server's /api/system/activity) ───────────

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

// ── Network Stats (stub — cluster aggregates from hosts) ────────────────

#[derive(Serialize)]
pub struct NetworkStats {
    pub total_interfaces: i32,
    pub active_interfaces: i32,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}

pub async fn network_stats(
    _auth: AuthUser,
) -> Json<NetworkStats> {
    // Cluster doesn't have direct network interfaces — return stub
    // Real implementation would aggregate from host heartbeats
    Json(NetworkStats {
        total_interfaces: 0,
        active_interfaces: 0,
        total_rx_bytes: 0,
        total_tx_bytes: 0,
    })
}

pub async fn network_interfaces(
    _auth: AuthUser,
) -> Json<Vec<serde_json::Value>> {
    Json(Vec::new())
}

// ── Resource Groups (cluster has its own) ───────────────────────────────

pub async fn list_resource_groups(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT rg.id, rg.name, rg.description, rg.is_default, rg.created_at,
                (SELECT COUNT(*) FROM vms WHERE resource_group_id = rg.id) as vm_count
         FROM resource_groups rg ORDER BY rg.name"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let groups: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, i64>(0)?,
            "name": row.get::<_, String>(1)?,
            "description": row.get::<_, String>(2)?,
            "is_default": row.get::<_, i32>(3)? != 0,
            "created_at": row.get::<_, String>(4)?,
            "vm_count": row.get::<_, i64>(5)?,
            "permissions": []
        }))
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .filter_map(|r| r.ok()).collect();

    Ok(Json(serde_json::Value::Array(groups)))
}

// ── Storage Pools compat (maps datastores to pool-like responses) ───────

pub async fn list_storage_pools(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT id, name, mount_path, store_type, 1, mount_source, mount_opts, total_bytes, free_bytes \
         FROM datastores ORDER BY name"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let pools: Vec<serde_json::Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "path": row.get::<_, String>(2)?,
            "pool_type": row.get::<_, String>(3)?,
            "shared": row.get::<_, i32>(4)? != 0,
            "mount_source": row.get::<_, String>(5)?,
            "mount_opts": row.get::<_, String>(6)?,
            "total_bytes": row.get::<_, i64>(7)?,
            "free_bytes": row.get::<_, i64>(8)?,
        }))
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .filter_map(|r| r.ok()).collect();

    Ok(Json(serde_json::Value::Array(pools)))
}

pub async fn storage_stats(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let total_pools: i64 = db.query_row("SELECT COUNT(*) FROM datastores", [], |r| r.get(0)).unwrap_or(0);
    let online_pools: i64 = db.query_row("SELECT COUNT(*) FROM datastores WHERE status = 'online'", [], |r| r.get(0)).unwrap_or(0);
    let total_bytes: i64 = db.query_row("SELECT COALESCE(SUM(total_bytes), 0) FROM datastores", [], |r| r.get(0)).unwrap_or(0);
    let free_bytes: i64 = db.query_row("SELECT COALESCE(SUM(free_bytes), 0) FROM datastores", [], |r| r.get(0)).unwrap_or(0);
    let total_images: i64 = db.query_row("SELECT COUNT(*) FROM disk_images", [], |r| r.get(0)).unwrap_or(0);
    let total_isos: i64 = db.query_row("SELECT COUNT(*) FROM isos", [], |r| r.get(0)).unwrap_or(0);

    Ok(Json(serde_json::json!({
        "total_pools": total_pools,
        "online_pools": online_pools,
        "total_bytes": total_bytes,
        "used_bytes": total_bytes - free_bytes,
        "free_bytes": free_bytes,
        "vm_disk_bytes": 0,
        "total_images": total_images,
        "total_isos": total_isos,
        "orphaned_images": 0
    })))
}
