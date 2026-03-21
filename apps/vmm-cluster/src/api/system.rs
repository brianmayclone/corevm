//! System API handlers — cluster info and aggregated stats.

use axum::{Json, extract::State};
use axum::http::StatusCode;
use serde::Serialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};

#[derive(Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub mode: String,
    pub cluster_name: Option<String>,
    pub uptime_secs: u64,
    pub total_hosts: usize,
    pub online_hosts: usize,
}

#[derive(Serialize)]
pub struct ClusterStats {
    pub total_hosts: i64,
    pub online_hosts: i64,
    pub maintenance_hosts: i64,
    pub offline_hosts: i64,
    pub total_vms: i64,
    pub running_vms: i64,
    pub stopped_vms: i64,
    pub total_ram_mb: i64,
    pub used_ram_mb: i64,
    pub total_disk_bytes: i64,
    pub used_disk_bytes: i64,
    pub ha_protected_vms: i64,
}

pub async fn info(
    State(state): State<Arc<ClusterState>>,
) -> Json<SystemInfo> {
    let online = state.nodes.iter()
        .filter(|n| n.status == crate::state::NodeStatus::Online)
        .count();

    Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        mode: "cluster".to_string(),
        cluster_name: None, // TODO: first cluster name
        uptime_secs: state.started_at.elapsed().as_secs(),
        total_hosts: state.nodes.len(),
        online_hosts: online,
    })
}

pub async fn stats(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<ClusterStats>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;

    let total_hosts: i64 = db.query_row("SELECT COUNT(*) FROM hosts", [], |r| r.get(0)).unwrap_or(0);
    let online_hosts: i64 = db.query_row("SELECT COUNT(*) FROM hosts WHERE status = 'online'", [], |r| r.get(0)).unwrap_or(0);
    let maintenance_hosts: i64 = db.query_row("SELECT COUNT(*) FROM hosts WHERE status = 'maintenance'", [], |r| r.get(0)).unwrap_or(0);
    let offline_hosts: i64 = db.query_row("SELECT COUNT(*) FROM hosts WHERE status = 'offline'", [], |r| r.get(0)).unwrap_or(0);

    let total_vms: i64 = db.query_row("SELECT COUNT(*) FROM vms", [], |r| r.get(0)).unwrap_or(0);
    let running_vms: i64 = db.query_row("SELECT COUNT(*) FROM vms WHERE state = 'running'", [], |r| r.get(0)).unwrap_or(0);
    let stopped_vms: i64 = db.query_row("SELECT COUNT(*) FROM vms WHERE state = 'stopped'", [], |r| r.get(0)).unwrap_or(0);

    let total_ram_mb: i64 = db.query_row("SELECT COALESCE(SUM(total_ram_mb), 0) FROM hosts", [], |r| r.get(0)).unwrap_or(0);
    let free_ram_mb: i64 = db.query_row("SELECT COALESCE(SUM(free_ram_mb), 0) FROM hosts", [], |r| r.get(0)).unwrap_or(0);

    let total_disk_bytes: i64 = db.query_row("SELECT COALESCE(SUM(total_bytes), 0) FROM datastores", [], |r| r.get(0)).unwrap_or(0);
    let free_disk_bytes: i64 = db.query_row("SELECT COALESCE(SUM(free_bytes), 0) FROM datastores", [], |r| r.get(0)).unwrap_or(0);

    let ha_protected_vms: i64 = db.query_row("SELECT COUNT(*) FROM vms WHERE ha_protected = 1", [], |r| r.get(0)).unwrap_or(0);

    Ok(Json(ClusterStats {
        total_hosts, online_hosts, maintenance_hosts, offline_hosts,
        total_vms, running_vms, stopped_vms,
        total_ram_mb, used_ram_mb: total_ram_mb - free_ram_mb,
        total_disk_bytes, used_disk_bytes: total_disk_bytes - free_disk_bytes,
        ha_protected_vms,
    }))
}
