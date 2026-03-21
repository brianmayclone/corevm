//! SDN Network API — manage virtual networks with DHCP, DNS, PXE.

use axum::{Json, extract::{State, Path, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::network::NetworkService;
use crate::services::audit::AuditService;

#[derive(Deserialize)]
pub struct NetworkQuery {
    pub cluster_id: Option<String>,
}

pub async fn list_networks(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<NetworkQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let networks = NetworkService::list_networks(&db, q.cluster_id.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(networks).unwrap()))
}

pub async fn get_network(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let net = NetworkService::get_network(&db, id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    let leases = NetworkService::list_leases(&db, id).unwrap_or_default();
    let dns = NetworkService::list_dns_records(&db, id).unwrap_or_default();
    Ok(Json(serde_json::json!({ "network": net, "leases": leases, "dns_records": dns })))
}

#[derive(Deserialize)]
pub struct CreateNetworkRequest {
    pub cluster_id: String,
    pub name: String,
    pub subnet: String,
    pub gateway: String,
    pub vlan_id: Option<i32>,
}

pub async fn create_network(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateNetworkRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = NetworkService::create_network(&db, &body.cluster_id, &body.name, &body.subnet,
        &body.gateway, body.vlan_id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "network.create", "network", &id.to_string(), Some(&body.name));
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn update_network(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NetworkService::update_network(&db, id, &body).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "network.update", "network", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn delete_network(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NetworkService::delete_network(&db, id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "network.delete", "network", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}
