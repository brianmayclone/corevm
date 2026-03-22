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
    use crate::services::validation;
    // Validate inputs
    if body.name.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "Network name is required".into()));
    }
    validation::validate_cidr(&body.subnet)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    validation::validate_ipv4(&body.gateway)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    validation::validate_ip_in_subnet(&body.gateway, &body.subnet)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    if let Some(vlan) = body.vlan_id {
        validation::validate_vlan(vlan)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    }
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
    use crate::services::validation;

    // Validate DHCP range vs gateway if both are being set
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let current = NetworkService::get_network(&db, id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;

    let subnet = body.get("subnet").and_then(|v| v.as_str()).unwrap_or(&current.subnet);
    let gateway = body.get("gateway").and_then(|v| v.as_str()).unwrap_or(&current.gateway);
    let dhcp_start = body.get("dhcp_range_start").and_then(|v| v.as_str()).unwrap_or(&current.dhcp_range_start);
    let dhcp_end = body.get("dhcp_range_end").and_then(|v| v.as_str()).unwrap_or(&current.dhcp_range_end);

    // Validate subnet/gateway if changed
    if body.get("subnet").is_some() {
        validation::validate_cidr(subnet).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    }
    if body.get("gateway").is_some() {
        validation::validate_ipv4(gateway).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        validation::validate_ip_in_subnet(gateway, subnet).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    }

    // Validate DHCP range — check gateway collision
    if !dhcp_start.is_empty() && !dhcp_end.is_empty() {
        if body.get("dhcp_range_start").is_some() || body.get("dhcp_range_end").is_some() || body.get("gateway").is_some() {
            validation::validate_dhcp_range(dhcp_start, dhcp_end, subnet, gateway)
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        }
    }

    if let Some(vlan) = body.get("vlan_id").and_then(|v| v.as_i64()) {
        validation::validate_vlan(vlan as i32).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    }

    NetworkService::update_network(&db, id, &body).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "network.update", "network", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Static DHCP Reservations ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateReservationRequest {
    pub mac_address: String,
    pub ip_address: String,
    pub hostname: Option<String>,
}

pub async fn create_reservation(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(network_id): Path<i64>,
    Json(body): Json<CreateReservationRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    use crate::services::validation;
    validation::validate_mac(&body.mac_address).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    validation::validate_ipv4(&body.ip_address).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;

    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = NetworkService::create_static_reservation(&db, network_id, &body.mac_address, &body.ip_address, body.hostname.as_deref())
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn delete_reservation(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path((_network_id, reservation_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NetworkService::delete_reservation(&db, reservation_id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── DNS Records CRUD ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDnsRecordRequest {
    pub record_type: String,
    pub name: String,
    pub value: String,
    #[serde(default = "default_ttl")]
    pub ttl: i64,
}
fn default_ttl() -> i64 { 3600 }

pub async fn create_dns_record(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(network_id): Path<i64>,
    Json(body): Json<CreateDnsRecordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let id = NetworkService::create_dns_record(&db, network_id, &body.record_type, &body.name, &body.value, body.ttl)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({"id": id})))
}

pub async fn delete_dns_record(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path((_network_id, record_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    NetworkService::delete_dns_record(&db, record_id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
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
