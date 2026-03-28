//! viSwitch API — manage virtual switches with uplinks, teaming, and traffic types.

use axum::{Json, extract::{State, Path, Query}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::viswitch::ViSwitchService;
use crate::services::audit::AuditService;

#[derive(Deserialize)]
pub struct ViSwitchQuery {
    pub cluster_id: Option<String>,
}

// ── List / Get ──────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Query(q): Query<ViSwitchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let switches = ViSwitchService::list_viswitches(&db, q.cluster_id.as_deref())
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(switches).unwrap()))
}

pub async fn get(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let vs = ViSwitchService::get_viswitch(&db, id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    let uplinks = ViSwitchService::list_uplinks(&db, id).unwrap_or_default();
    let ports = ViSwitchService::list_ports(&db, id).unwrap_or_default();
    Ok(Json(serde_json::json!({ "viswitch": vs, "uplinks": uplinks, "ports": ports })))
}

// ── Create ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateViSwitchRequest {
    pub cluster_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_max_ports")]
    pub max_ports: i32,
    #[serde(default = "default_max_uplinks")]
    pub max_uplinks: i32,
    #[serde(default = "default_mtu")]
    pub mtu: i32,
    #[serde(default = "default_policy")]
    pub uplink_policy: String,
}
fn default_max_ports() -> i32 { 1024 }
fn default_max_uplinks() -> i32 { 128 }
fn default_mtu() -> i32 { 1500 }
fn default_policy() -> String { "failover".into() }

pub async fn create(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateViSwitchRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    if body.name.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "viSwitch name is required".into()));
    }
    if !["roundrobin", "failover", "rulebased"].contains(&body.uplink_policy.as_str()) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid uplink_policy (roundrobin, failover, rulebased)".into()));
    }
    if body.max_ports < 1 || body.max_ports > 1024 {
        return Err(AppError(StatusCode::BAD_REQUEST, "max_ports must be 1–1024".into()));
    }
    if body.max_uplinks < 1 || body.max_uplinks > 128 {
        return Err(AppError(StatusCode::BAD_REQUEST, "max_uplinks must be 1–128".into()));
    }

    let id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let id = ViSwitchService::create_viswitch(
            &db, &body.cluster_id, &body.name, &body.description,
            body.max_ports, body.max_uplinks, body.mtu, &body.uplink_policy,
        ).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        AuditService::log(&db, user.id, "viswitch.create", "viswitch", &id.to_string(), Some(&body.name));
        id
    };

    // Deploy empty viSwitch (bridge only, no uplinks yet) to all online nodes
    let bridge_name = ViSwitchService::bridge_name(id);
    let setup_req = vmm_core::cluster::SetupViSwitchRequest {
        viswitch_id: id,
        bridge_name: bridge_name.clone(),
        mtu: body.mtu as u32,
        uplink_policy: body.uplink_policy.clone(),
        uplink_rules: "[]".into(),
        uplinks: vec![],
    };
    deploy_viswitch_to_nodes(&state, &body.cluster_id, &setup_req).await;

    Ok(Json(serde_json::json!({"id": id, "bridge": bridge_name})))
}

// ── Update ──────────────────────────────────────────────────────────────

pub async fn update(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    if let Some(policy) = body.get("uplink_policy").and_then(|v| v.as_str()) {
        if !["roundrobin", "failover", "rulebased"].contains(&policy) {
            return Err(AppError(StatusCode::BAD_REQUEST, "Invalid uplink_policy".into()));
        }
    }

    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    ViSwitchService::update_viswitch(&db, id, &body).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "viswitch.update", "viswitch", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Delete ──────────────────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    let cluster_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let vs = ViSwitchService::get_viswitch(&db, id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        vs.cluster_id.clone()
    };

    // Teardown on all nodes
    let bridge_name = ViSwitchService::bridge_name(id);
    let teardown_req = vmm_core::cluster::TeardownViSwitchRequest {
        viswitch_id: id,
        bridge_name,
    };
    teardown_viswitch_from_nodes(&state, &cluster_id, &teardown_req).await;

    // Delete from DB (cascade removes uplinks + ports)
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    ViSwitchService::delete_viswitch(&db, id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "viswitch.delete", "viswitch", &id.to_string(), None);
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Uplinks ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddUplinkRequest {
    pub uplink_type: String,
    #[serde(default)]
    pub physical_nic: String,
    pub network_id: Option<i64>,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_vm_traffic")]
    pub traffic_types: String,
}
fn default_true() -> bool { true }
fn default_vm_traffic() -> String { "vm".into() }

pub async fn add_uplink(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(viswitch_id): Path<i64>,
    Json(body): Json<AddUplinkRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    if !["physical", "virtual"].contains(&body.uplink_type.as_str()) {
        return Err(AppError(StatusCode::BAD_REQUEST, "uplink_type must be 'physical' or 'virtual'".into()));
    }
    if body.uplink_type == "physical" && body.physical_nic.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "physical_nic is required for physical uplinks".into()));
    }
    if body.uplink_type == "virtual" && body.network_id.is_none() {
        return Err(AppError(StatusCode::BAD_REQUEST, "network_id is required for virtual uplinks".into()));
    }
    // Validate traffic_types
    for tt in body.traffic_types.split(',') {
        let tt = tt.trim();
        if !["vm", "san", "migration", "management", "backup"].contains(&tt) {
            return Err(AppError(StatusCode::BAD_REQUEST, format!("Unknown traffic type: '{}'", tt)));
        }
    }

    let (uplink_id, cluster_id) = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let vs = ViSwitchService::get_viswitch(&db, viswitch_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        let uid = ViSwitchService::add_uplink(
            &db, viswitch_id, &body.uplink_type, &body.physical_nic,
            body.network_id, body.active, &body.traffic_types,
        ).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        AuditService::log(&db, user.id, "viswitch.uplink.add", "viswitch", &viswitch_id.to_string(), None);
        (uid, vs.cluster_id)
    };

    // Redeploy viSwitch with updated uplinks to all nodes
    redeploy_viswitch(&state, viswitch_id, &cluster_id).await;

    Ok(Json(serde_json::json!({"id": uplink_id})))
}

pub async fn remove_uplink(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path((viswitch_id, uplink_id)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    let cluster_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let vs = ViSwitchService::get_viswitch(&db, viswitch_id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        ViSwitchService::remove_uplink(&db, uplink_id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        AuditService::log(&db, user.id, "viswitch.uplink.remove", "viswitch", &viswitch_id.to_string(), None);
        vs.cluster_id
    };

    redeploy_viswitch(&state, viswitch_id, &cluster_id).await;

    Ok(Json(serde_json::json!({"ok": true})))
}

// ── Ports ───────────────────────────────────────────────────────────────

pub async fn list_ports(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
    let ports = ViSwitchService::list_ports(&db, id)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(ports).unwrap()))
}

// ── Host NIC Discovery ──────────────────────────────────────────────────

/// GET /api/viswitches/host-nics — list physical NICs from all online cluster hosts.
pub async fn host_nics(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let nodes: Vec<(String, String, String, String)> = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let mut stmt = db.prepare(
            "SELECT id, hostname, address, agent_token FROM hosts"
        ).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?))
        }).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut results = Vec::new();
    for (node_id, hostname, address, token) in &nodes {
        let is_online = state.nodes.get(node_id)
            .map(|n| n.status == crate::state::NodeStatus::Online)
            .unwrap_or(false);
        if !is_online { continue; }

        if let Ok(client) = crate::node_client::NodeClient::new(address, token) {
            if let Ok(ifaces) = client.get_network_interfaces().await {
                results.push(serde_json::json!({
                    "host_id": node_id,
                    "hostname": hostname,
                    "nics": ifaces,
                }));
            }
        }
    }
    Ok(Json(serde_json::to_value(results).unwrap()))
}

// ── Deployment Helpers ──────────────────────────────────────────────────

/// Build a SetupViSwitchRequest from DB state and deploy to all nodes.
async fn redeploy_viswitch(state: &Arc<ClusterState>, viswitch_id: i64, cluster_id: &str) {
    let setup_req = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };
        match build_setup_request(&db, viswitch_id) {
            Ok(req) => req,
            Err(e) => {
                tracing::warn!("Cannot build viSwitch setup request: {}", e);
                return;
            }
        }
    };
    deploy_viswitch_to_nodes(state, cluster_id, &setup_req).await;
}

/// Build a SetupViSwitchRequest — public for use by host registration.
pub fn build_setup_request_pub(
    db: &rusqlite::Connection,
    viswitch_id: i64,
) -> Result<vmm_core::cluster::SetupViSwitchRequest, String> {
    build_setup_request(db, viswitch_id)
}

/// Build a full SetupViSwitchRequest from current DB state.
fn build_setup_request(
    db: &rusqlite::Connection,
    viswitch_id: i64,
) -> Result<vmm_core::cluster::SetupViSwitchRequest, String> {
    let vs = ViSwitchService::get_viswitch(db, viswitch_id)?;
    let uplink_rows = ViSwitchService::list_uplinks(db, viswitch_id)?;

    let uplinks: Vec<vmm_core::cluster::ViSwitchUplink> = uplink_rows.iter().map(|u| {
        let vxlan = if u.uplink_type == "virtual" {
            u.network_id.map(|nid| vmm_core::cluster::VxlanConfig {
                vni: nid as u32 + 10000,
                group: format!("239.1.{}.{}", (nid / 256) % 256, nid % 256),
                port: 4789,
                local_ip: String::new(),
            })
        } else {
            None
        };

        vmm_core::cluster::ViSwitchUplink {
            uplink_index: u.uplink_index as u32,
            uplink_type: u.uplink_type.clone(),
            physical_nic: u.physical_nic.clone(),
            network_id: u.network_id,
            vxlan,
            active: u.active,
            traffic_types: u.traffic_types.clone(),
        }
    }).collect();

    Ok(vmm_core::cluster::SetupViSwitchRequest {
        viswitch_id,
        bridge_name: ViSwitchService::bridge_name(viswitch_id),
        mtu: vs.mtu as u32,
        uplink_policy: vs.uplink_policy,
        uplink_rules: vs.uplink_rules,
        uplinks,
    })
}

/// Deploy a viSwitch to all online nodes in the cluster.
async fn deploy_viswitch_to_nodes(
    state: &Arc<ClusterState>,
    cluster_id: &str,
    req: &vmm_core::cluster::SetupViSwitchRequest,
) {
    let nodes: Vec<(String, String, String)> = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };
        let mut stmt = match db.prepare("SELECT id, address, agent_token FROM hosts WHERE cluster_id = ?1") {
            Ok(s) => s,
            Err(_) => return,
        };
        let rows = match stmt.query_map(rusqlite::params![cluster_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }) {
            Ok(r) => r,
            Err(_) => return,
        };
        rows.filter_map(|r| r.ok()).collect()
    };

    for (node_id, address, token) in &nodes {
        let is_online = state.nodes.get(node_id)
            .map(|n| n.status == crate::state::NodeStatus::Online)
            .unwrap_or(false);
        if !is_online { continue; }

        // Fill in VXLAN local_ip per node
        let mut node_req = req.clone();
        for uplink in &mut node_req.uplinks {
            if let Some(ref mut vxlan) = uplink.vxlan {
                if vxlan.local_ip.is_empty() {
                    if let Some(ip) = extract_ip_from_url(address) {
                        vxlan.local_ip = ip;
                    }
                }
            }
        }

        match crate::node_client::NodeClient::new(address, token) {
            Ok(client) => {
                match client.setup_viswitch(&node_req).await {
                    Ok(resp) if resp.success => {
                        tracing::info!("viSwitch '{}' deployed to node {}", req.bridge_name, node_id);
                    }
                    Ok(resp) => {
                        tracing::warn!("viSwitch deploy to node {} failed: {:?}", node_id, resp.error);
                    }
                    Err(e) => {
                        tracing::warn!("viSwitch deploy to node {} failed: {}", node_id, e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Cannot connect to node {} for viSwitch deploy: {}", node_id, e);
            }
        }
    }
}

/// Tear down a viSwitch from all online nodes in the cluster.
async fn teardown_viswitch_from_nodes(
    state: &Arc<ClusterState>,
    cluster_id: &str,
    req: &vmm_core::cluster::TeardownViSwitchRequest,
) {
    let nodes: Vec<(String, String, String)> = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };
        let mut stmt = match db.prepare("SELECT id, address, agent_token FROM hosts WHERE cluster_id = ?1") {
            Ok(s) => s,
            Err(_) => return,
        };
        let rows = match stmt.query_map(rusqlite::params![cluster_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }) {
            Ok(r) => r,
            Err(_) => return,
        };
        rows.filter_map(|r| r.ok()).collect()
    };

    for (node_id, address, token) in &nodes {
        let is_online = state.nodes.get(node_id)
            .map(|n| n.status == crate::state::NodeStatus::Online)
            .unwrap_or(false);
        if !is_online { continue; }

        if let Ok(client) = crate::node_client::NodeClient::new(address, token) {
            match client.teardown_viswitch(req).await {
                Ok(_) => tracing::info!("viSwitch '{}' removed from node {}", req.bridge_name, node_id),
                Err(e) => tracing::warn!("viSwitch teardown on node {} failed: {}", node_id, e),
            }
        }
    }
}

fn extract_ip_from_url(url: &str) -> Option<String> {
    let url = url.trim_start_matches("https://").trim_start_matches("http://");
    let host = url.split(':').next()?;
    if host.parse::<std::net::Ipv4Addr>().is_ok() { Some(host.to_string()) } else { None }
}
