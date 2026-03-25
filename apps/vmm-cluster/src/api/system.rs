//! System API handlers — cluster info and aggregated stats.
//!
//! Thin handlers delegating to StatsService.

use axum::{Json, extract::State};
use axum::http::StatusCode;
use serde::Serialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError};
use crate::services::stats::StatsService;

#[derive(Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub build_sha: String,
    pub build_time: String,
    pub mode: String,
    pub cluster_name: Option<String>,
    pub uptime_secs: u64,
    pub total_hosts: usize,
    pub online_hosts: usize,
}

pub async fn info(
    State(state): State<Arc<ClusterState>>,
) -> Json<SystemInfo> {
    let online = state.nodes.iter()
        .filter(|n| n.status == crate::state::NodeStatus::Online)
        .count();

    Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_sha: env!("COREVM_GIT_SHA").to_string(),
        build_time: env!("COREVM_BUILD_TIMESTAMP").to_string(),
        mode: "cluster".to_string(),
        cluster_name: None,
        uptime_secs: state.started_at.elapsed().as_secs(),
        total_hosts: state.nodes.len(),
        online_hosts: online,
    })
}

pub async fn stats(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let stats = StatsService::cluster_stats(&db);
    Ok(Json(serde_json::to_value(stats).unwrap()))
}
