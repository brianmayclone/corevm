//! Activity/audit and network compat endpoints.
//!
//! These endpoints mirror vmm-server's API so the existing UI Dashboard
//! works without changes when connected to vmm-cluster.

use axum::{Json, extract::{State, Query, Path}};
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
// Optional query param ?cluster_id=... filters to datastores accessible by all nodes in that cluster.

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

    let pools: Vec<serde_json::Value> = if let Some(cluster_id) = &q.cluster_id {
        // Only return datastores where ALL hosts in the cluster have them mounted
        let host_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM hosts WHERE cluster_id = ?1 AND status != 'offline'",
            rusqlite::params![cluster_id], |r| r.get(0),
        ).unwrap_or(0);

        if host_count == 0 {
            return Ok(Json(serde_json::Value::Array(Vec::new())));
        }

        let mut stmt = db.prepare(
            "SELECT d.id, d.name, d.mount_path, d.store_type, 1, d.mount_source, d.mount_opts,
                    d.total_bytes, d.free_bytes
             FROM datastores d
             WHERE d.cluster_id = ?1
               AND d.status = 'online'
               AND (SELECT COUNT(*) FROM datastore_hosts dh
                    JOIN hosts h ON dh.host_id = h.id
                    WHERE dh.datastore_id = d.id AND dh.mounted = 1
                      AND h.cluster_id = ?1 AND h.status != 'offline') = ?2
             ORDER BY d.name"
        ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let rows = stmt.query_map(rusqlite::params![cluster_id, host_count], |row| {
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
        }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        // No filter — return all datastores
        let mut stmt = db.prepare(
            "SELECT id, name, mount_path, store_type, 1, mount_source, mount_opts, total_bytes, free_bytes \
             FROM datastores ORDER BY name"
        ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let rows = stmt.query_map([], |row| {
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
        }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    Ok(Json(serde_json::Value::Array(pools)))
}

// ── Disk Images compat ──────────────────────────────────────────────────

pub async fn list_images(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT di.id, di.name, di.path, di.size_bytes, di.format, di.datastore_id, di.vm_id, \
                v.name as vm_name, di.created_at \
         FROM disk_images di LEFT JOIN vms v ON di.vm_id = v.id ORDER BY di.name"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "path": row.get::<_, String>(2)?,
            "size_bytes": row.get::<_, i64>(3)?,
            "format": row.get::<_, String>(4)?,
            "pool_id": row.get::<_, Option<String>>(5)?,
            "vm_id": row.get::<_, Option<String>>(6)?,
            "vm_name": row.get::<_, Option<String>>(7)?,
            "created_at": row.get::<_, String>(8)?,
        }))
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let images: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Ok(Json(serde_json::Value::Array(images)))
}

pub async fn list_isos(
    State(state): State<Arc<ClusterState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let mut stmt = db.prepare(
        "SELECT id, name, path, size_bytes, uploaded_at FROM isos ORDER BY name"
    ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "path": row.get::<_, String>(2)?,
            "size_bytes": row.get::<_, i64>(3)?,
            "uploaded_at": row.get::<_, String>(4)?,
        }))
    }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let isos: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
    Ok(Json(serde_json::Value::Array(isos)))
}

// ── Browse a datastore's files ──────────────────────────────────────────

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

    // Get the datastore's mount path
    let mount_path: String = db.query_row(
        "SELECT mount_path FROM datastores WHERE id = ?1",
        rusqlite::params![&id], |r| r.get(0),
    ).map_err(|_| AppError(StatusCode::NOT_FOUND, "Datastore not found".into()))?;

    // We can't browse remote filesystems directly from the cluster.
    // Instead, list disk_images and isos from the DB that belong to this datastore.
    let mut files: Vec<serde_json::Value> = Vec::new();

    let ext_filter = q.ext.as_deref().unwrap_or("");

    if ext_filter.is_empty() || ext_filter == ".raw" || ext_filter == ".qcow2" {
        let mut stmt = db.prepare(
            "SELECT name, path, size_bytes FROM disk_images WHERE datastore_id = ?1 ORDER BY name"
        ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let disks: Vec<serde_json::Value> = stmt.query_map(rusqlite::params![&id], |row| {
            Ok(serde_json::json!({
                "name": row.get::<_, String>(0)?,
                "path": format!("{}/{}", mount_path, row.get::<_, String>(1)?),
                "size_bytes": row.get::<_, i64>(2)?,
                "is_dir": false,
            }))
        }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok()).collect();
        files.extend(disks);
    }

    if ext_filter.is_empty() || ext_filter == ".iso" {
        let mut stmt = db.prepare(
            "SELECT name, path, size_bytes FROM isos WHERE datastore_id = ?1 ORDER BY name"
        ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let isos: Vec<serde_json::Value> = stmt.query_map(rusqlite::params![&id], |row| {
            Ok(serde_json::json!({
                "name": row.get::<_, String>(0)?,
                "path": format!("{}/{}", mount_path, row.get::<_, String>(1)?),
                "size_bytes": row.get::<_, i64>(2)?,
                "is_dir": false,
            }))
        }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .filter_map(|r| r.ok()).collect();
        files.extend(isos);
    }

    Ok(Json(serde_json::Value::Array(files)))
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
