//! Cluster management API handlers — CRUD for logical clusters.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::cluster::ClusterService;
use crate::services::audit::AuditService;

#[derive(Deserialize)]
pub struct CreateClusterRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
pub struct UpdateClusterRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub drs_enabled: Option<bool>,
    pub ha_enabled: Option<bool>,
    pub ha_vm_restart_priority: Option<String>,
    pub ha_admission_control: Option<bool>,
    pub ha_failover_hosts: Option<i32>,
}

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let clusters = ClusterService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(clusters).unwrap()))
}

pub async fn get(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let cluster = ClusterService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(cluster).unwrap()))
}

pub async fn create(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateClusterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let id = ClusterService::create(&db, &body.name, &body.description)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;

    // Auto-create a default viSwitch for the new cluster
    let vs_id = crate::services::viswitch::ViSwitchService::create_viswitch(
        &db, &id, "Default Switch", "Auto-created default virtual switch",
        1024, 128, 1500, "failover",
    ).ok();
    if let Some(vs_id) = vs_id {
        let _ = db.execute(
            "UPDATE clusters SET default_viswitch_id = ?1 WHERE id = ?2",
            rusqlite::params![vs_id, &id],
        );
        tracing::info!("Auto-created 'Default Switch' (id={}) for cluster '{}'", vs_id, body.name);
    }

    AuditService::log(&db, user.id, "cluster.create", "cluster", &id, Some(&body.name));
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn update(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateClusterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    ClusterService::update(
        &db, &id, body.name.as_deref(), body.description.as_deref(),
        body.drs_enabled, body.ha_enabled, body.ha_vm_restart_priority.as_deref(),
        body.ha_admission_control, body.ha_failover_hosts,
    ).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "cluster.update", "cluster", &id, None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    ClusterService::delete(&db, &id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "cluster.delete", "cluster", &id, None);
    Ok(Json(serde_json::json!({"ok": true})))
}
