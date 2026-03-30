//! Host management API handlers — registration, status, maintenance.
//!
//! Host registration is a multi-step process:
//! 1. Admin provides host URL + admin credentials
//! 2. Cluster verifies credentials against the host
//! 3. Cluster registers the host via the Agent API
//! 4. Host switches to managed mode

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::{ClusterState, NodeConnection, NodeStatus};
use crate::auth::middleware::{AuthUser, AppError, require_admin};
use crate::services::host::HostService;
use crate::services::audit::AuditService;
use crate::services::event::EventService;

#[derive(Deserialize)]
pub struct RegisterHostRequest {
    /// URL of the vmm-server to register (e.g. "https://192.168.1.10:8443")
    pub address: String,
    /// Cluster to assign the host to
    pub cluster_id: String,
    /// Admin username on the vmm-server (for verification)
    pub admin_username: String,
    /// Admin password on the vmm-server (for verification)
    pub admin_password: String,
}

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let hosts = HostService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(hosts).unwrap()))
}

pub async fn get(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let host = HostService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::to_value(host).unwrap()))
}

pub async fn register(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<RegisterHostRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;

    let address = body.address.trim_end_matches('/').to_string();
    let node_id = uuid::Uuid::new_v4().to_string();
    let agent_token = generate_agent_token();

    // Step 1: Verify admin credentials on the target host
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let login_resp = client.post(format!("{}/api/auth/login", &address))
        .json(&serde_json::json!({
            "username": body.admin_username,
            "password": body.admin_password,
        }))
        .send().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot reach host: {}", e)))?;

    if !login_resp.status().is_success() {
        return Err(AppError(StatusCode::UNPROCESSABLE_ENTITY, "Invalid admin credentials on target host".into()));
    }

    // Step 2: Check host isn't already managed
    let info_resp = client.get(format!("{}/api/system/info", &address))
        .send().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot query host info: {}", e)))?;

    let info_json: serde_json::Value = info_resp.json().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    if info_json.get("mode").and_then(|v| v.as_str()) == Some("managed") {
        return Err(AppError(StatusCode::CONFLICT, "Host is already managed by a cluster".into()));
    }

    let hostname = info_json.get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let version = info_json.get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Step 3: Register via Agent API
    let cluster_url = format!("http://{}:{}", state.config.server.bind, state.config.server.port);
    let register_req = vmm_core::cluster::AgentRegisterRequest {
        cluster_id: body.cluster_id.clone(),
        cluster_url,
        agent_token: agent_token.clone(),
        node_id: node_id.clone(),
    };

    let reg_resp = client.post(format!("{}/agent/register", &address))
        .json(&register_req)
        .send().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Registration failed: {}", e)))?;

    if !reg_resp.status().is_success() {
        let err_text = reg_resp.text().await.unwrap_or_default();
        return Err(AppError(StatusCode::BAD_GATEWAY, format!("Host rejected registration: {}", err_text)));
    }

    // Step 4: Save host in cluster DB
    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        HostService::insert(&db, &node_id, &hostname, &address, &body.cluster_id, &agent_token, &version)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        AuditService::log(&db, user.id, "host.register", "host", &node_id, Some(&hostname));
        EventService::log(&db, "info", "host", &format!("Host '{}' registered", hostname),
            Some("host"), Some(&node_id), Some(&node_id));
    }

    // Step 5: Add to in-memory node connections
    let agent_token_clone = agent_token.clone();
    let address_clone = address.clone();
    state.nodes.insert(node_id.clone(), NodeConnection {
        node_id: node_id.clone(),
        hostname: hostname.clone(),
        address: address.clone(),
        agent_token,
        status: NodeStatus::Connecting,
        missed_heartbeats: 0,
    });

    // Step 6: Import existing VMs from the host via services (NO direct DB access)
    let imported_vms = HostService::import_vms_from_agent(
        &state, &address_clone, &agent_token_clone, &body.cluster_id, &node_id,
    ).await;

    // Step 7: Deploy all cluster viSwitches to the new host
    {
        let viswitches_to_deploy: Vec<i64> = {
            let db = state.db.lock().ok();
            db.and_then(|db| {
                let mut stmt = db.prepare(
                    "SELECT id FROM viswitches WHERE cluster_id = ?1 AND enabled = 1"
                ).ok()?;
                let rows = stmt.query_map(rusqlite::params![&body.cluster_id], |row| row.get::<_, i64>(0)).ok()?;
                Some(rows.filter_map(|r| r.ok()).collect())
            }).unwrap_or_default()
        };

        if let Ok(nc) = crate::node_client::NodeClient::new(&address_clone, &agent_token_clone) {
            for vs_id in viswitches_to_deploy {
                let setup_req = {
                    let db = state.db.lock().ok();
                    db.and_then(|db| crate::api::viswitch::build_setup_request_pub(&db, vs_id).ok())
                };
                if let Some(req) = setup_req {
                    match nc.setup_viswitch(&req).await {
                        Ok(resp) if resp.success => {
                            tracing::info!("viSwitch '{}' deployed to new host {}", req.bridge_name, hostname);
                        }
                        _ => {
                            tracing::warn!("Failed to deploy viSwitch to new host {}", hostname);
                        }
                    }
                }
            }
        }
    }

    Ok(Json(serde_json::json!({
        "id": node_id,
        "hostname": hostname,
        "status": "connecting",
        "imported_vms": imported_vms,
    })))
}

pub async fn deregister(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;

    // Get host info before removing
    let (address, agent_token) = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let host = HostService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        (host.address, HostService::get_agent_token(&db, &id).unwrap_or_default())
    };

    // Tell the node to deregister
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let _ = client.post(format!("{}/agent/deregister", &address))
        .header("X-Agent-Token", &agent_token)
        .send().await;

    // Remove from DB and in-memory
    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        HostService::delete(&db, &id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
        AuditService::log(&db, user.id, "host.deregister", "host", &id, None);
    }
    state.nodes.remove(&id);

    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct MaintenanceRequest {
    /// "migrate" = move VMs to other hosts (cold migration), "shutdown" = just stop VMs
    #[serde(default = "default_maintenance_mode")]
    pub mode: String,
}
fn default_maintenance_mode() -> String { "migrate".into() }

pub async fn enter_maintenance(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
    body: Option<Json<MaintenanceRequest>>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let mode = body.map(|b| b.mode.clone()).unwrap_or_else(|| "migrate".into());

    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    HostService::set_maintenance(&db, &id, true).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "host.maintenance.enter", "host", &id, Some(&format!("mode: {}", mode)));
    EventService::log(&db, "warning", "host",
        &format!("Host entering maintenance mode ({})", mode),
        Some("host"), Some(&id), Some(&id));
    drop(db);

    // Update in-memory status
    if let Some(mut node) = state.nodes.get_mut(&id) {
        node.status = NodeStatus::Maintenance;
    }

    // Trigger evacuation or shutdown in background
    let state_evac = state.clone();
    let host_evac = id.clone();
    if mode == "migrate" {
        tokio::spawn(async move {
            crate::engine::maintenance::evacuate_host(&state_evac, &host_evac).await;
        });
    } else {
        // Shutdown mode: just stop all running VMs on this host
        tokio::spawn(async move {
            crate::engine::maintenance::shutdown_host_vms(&state_evac, &host_evac).await;
        });
    }

    Ok(Json(serde_json::json!({"ok": true, "mode": mode})))
}

pub async fn exit_maintenance(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_admin(&user)?;
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    HostService::set_maintenance(&db, &id, false).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "host.maintenance.exit", "host", &id, None);
    EventService::log(&db, "info", "host", "Host exited maintenance mode",
        Some("host"), Some(&id), Some(&id));

    if let Some(mut node) = state.nodes.get_mut(&id) {
        node.status = NodeStatus::Online;
    }

    Ok(Json(serde_json::json!({"ok": true})))
}

/// Generate a cryptographically secure agent token.
fn generate_agent_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
