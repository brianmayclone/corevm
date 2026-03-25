//! VM management API handlers — cluster is the AUTHORITY.
//!
//! VMs are created in the cluster DB first, then provisioned on nodes.
//! All operations go through the service layer, never directly to DB.

use axum::{Json, extract::{State, Path}};
use axum::http::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use crate::state::ClusterState;
use crate::auth::middleware::{AuthUser, AppError, require_operator};
use crate::services::vm::VmService;
use crate::services::audit::AuditService;
use crate::services::event::EventService;

#[derive(Deserialize)]
pub struct CreateVmRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub cluster_id: String,
    /// Optional: specific host to place on. If omitted, scheduler decides.
    pub host_id: Option<String>,
    pub config: serde_json::Value,
    /// Optional: SDN network to attach the VM to.
    /// When set, the cluster enriches the config with bridge mode + sdn_config.
    pub network_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateVmRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Option<serde_json::Value>,
    pub ha_protected: Option<bool>,
    pub ha_restart_priority: Option<String>,
    pub drs_automation: Option<String>,
}

pub async fn list(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let vms = VmService::list(&db).map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(vms).unwrap()))
}

pub async fn get(
    State(state): State<Arc<ClusterState>>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    let vm = VmService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
    let config = VmService::get_config(&db, &id).unwrap_or_default();
    Ok(Json(serde_json::json!({
        "vm": vm,
        "config": config,
    })))
}

pub async fn create(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Json(body): Json<CreateVmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    let vm_id = uuid::Uuid::new_v4().to_string().replace("-", "");

    // Enrich VM config with SDN network details (bridge mode + sdn_config)
    let mut config = body.config.clone();
    if let Some(network_id) = body.network_id {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let net = crate::services::network::NetworkService::get_network(&db, network_id)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Network not found: {}", e)))?;
        drop(db);

        // Parse subnet to build SdnNetConfig
        let bridge_name = format!("sdn{}", network_id);
        let sdn_config = build_sdn_config(&net)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Invalid network config: {}", e)))?;

        // Override network settings in the VM config
        if let Some(obj) = config.as_object_mut() {
            obj.insert("net_mode".into(), serde_json::json!("bridge"));
            obj.insert("net_host_nic".into(), serde_json::json!(bridge_name));
            obj.insert("net_enabled".into(), serde_json::json!(true));
            obj.insert("sdn_config".into(), serde_json::to_value(&sdn_config)
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?);
        }
    }

    let config_json = serde_json::to_string(&config)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;

    // Step 1: Create VM in cluster DB (authority)
    {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        VmService::create(&db, &vm_id, &body.name, &body.description, &body.cluster_id, &config_json, user.id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        AuditService::log(&db, user.id, "vm.create", "vm", &vm_id, Some(&body.name));
        EventService::log(&db, "info", "vm", &format!("VM '{}' created", body.name),
            Some("vm"), Some(&vm_id), None);
    }

    // Step 2: Provision on host (if host specified or scheduler picks one)
    if let Some(host_id) = &body.host_id {
        // Provision on specified host via Agent API
        let node = state.nodes.get(host_id)
            .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Host not found or offline".into()))?;

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let provision_req = vmm_core::cluster::ProvisionVmRequest {
            vm_id: vm_id.clone(),
            config: config.clone(),
        };

        let resp = client.post(format!("{}/agent/vms/provision", &node.address))
            .header("X-Agent-Token", &node.agent_token)
            .json(&provision_req)
            .send().await
            .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot reach host: {}", e)))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(AppError(StatusCode::BAD_GATEWAY, format!("Provisioning failed: {}", err)));
        }

        // Assign host in cluster DB
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        VmService::assign_host(&db, &vm_id, host_id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }
    // else: VM stays unplaced, scheduler will handle it (Phase 5)

    Ok(Json(serde_json::json!({
        "id": vm_id,
        "name": body.name,
        "state": "stopped",
    })))
}

pub async fn start(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    let host_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let vm = VmService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        vm.host_id.ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "VM is not assigned to a host".into()))?
    };

    // Send start command to host
    let node = state.nodes.get(&host_id)
        .ok_or_else(|| AppError(StatusCode::BAD_GATEWAY, "Host not connected".into()))?;

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let resp = client.post(format!("{}/agent/vms/{}/start", &node.address, &id))
        .header("X-Agent-Token", &node.agent_token)
        .send().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot reach host: {}", e)))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(AppError(StatusCode::BAD_GATEWAY, format!("Start failed: {}", err)));
    }

    // Update state in cluster DB
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    VmService::update_state(&db, &id, "starting")
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    AuditService::log(&db, user.id, "vm.start", "vm", &id, None);

    Ok(Json(serde_json::json!({"ok": true, "state": "starting"})))
}

pub async fn stop(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    let host_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let vm = VmService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        vm.host_id.ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "VM is not assigned to a host".into()))?
    };

    let node = state.nodes.get(&host_id)
        .ok_or_else(|| AppError(StatusCode::BAD_GATEWAY, "Host not connected".into()))?;

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let resp = client.post(format!("{}/agent/vms/{}/stop", &node.address, &id))
        .header("X-Agent-Token", &node.agent_token)
        .send().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot reach host: {}", e)))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(AppError(StatusCode::BAD_GATEWAY, format!("Stop failed: {}", err)));
    }

    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    VmService::update_state(&db, &id, "stopping")
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    AuditService::log(&db, user.id, "vm.stop", "vm", &id, None);

    Ok(Json(serde_json::json!({"ok": true, "state": "stopping"})))
}

pub async fn force_stop(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    let host_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let vm = VmService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        vm.host_id.ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "VM is not assigned to a host".into()))?
    };

    let node = state.nodes.get(&host_id)
        .ok_or_else(|| AppError(StatusCode::BAD_GATEWAY, "Host not connected".into()))?;

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let resp = client.post(format!("{}/agent/vms/{}/force-stop", &node.address, &id))
        .header("X-Agent-Token", &node.agent_token)
        .send().await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot reach host: {}", e)))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(AppError(StatusCode::BAD_GATEWAY, format!("Force-stop failed: {}", err)));
    }

    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    VmService::update_state(&db, &id, "stopped")
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    AuditService::log(&db, user.id, "vm.force_stop", "vm", &id, None);

    Ok(Json(serde_json::json!({"ok": true, "state": "stopped"})))
}

pub async fn delete(
    State(state): State<Arc<ClusterState>>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_operator(&user)?;

    // If VM is assigned to a host, destroy it there first
    let host_id = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let vm = VmService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        vm.host_id
    };

    if let Some(host_id) = host_id {
        if let Some(node) = state.nodes.get(&host_id) {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let _ = client.post(format!("{}/agent/vms/{}/destroy", &node.address, &id))
                .header("X-Agent-Token", &node.agent_token)
                .send().await;
        }
    }

    // Remove from cluster DB (authoritative deletion)
    let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
    VmService::delete(&db, &id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    AuditService::log(&db, user.id, "vm.delete", "vm", &id, None);

    Ok(Json(serde_json::json!({"ok": true})))
}

// ── SDN Config Builder ──────────────────────────────────────────────────

/// Build an SdnNetConfig from a VirtualNetwork definition.
/// Parses the subnet CIDR and gateway to create the byte-level config
/// that SLIRP (and the DHCP server) needs.
fn build_sdn_config(net: &crate::services::network::VirtualNetwork) -> Result<vmm_core::config::SdnNetConfig, String> {
    // Parse gateway IP (e.g. "10.0.50.1")
    let gw: std::net::Ipv4Addr = net.gateway.parse()
        .map_err(|_| format!("Invalid gateway IP: {}", net.gateway))?;
    let gw_octets = gw.octets();

    // Parse subnet CIDR (e.g. "10.0.50.0/24")
    let (subnet_ip, prefix_len) = parse_cidr(&net.subnet)?;
    let sub_octets = subnet_ip.octets();

    // Calculate netmask from prefix length
    let mask_bits: u32 = if prefix_len == 0 { 0 } else { !((1u32 << (32 - prefix_len)) - 1) };
    let netmask = mask_bits.to_be_bytes();

    // Guest IP: DHCP range start (or gateway + 100 as fallback)
    let guest_ip = if !net.dhcp_range_start.is_empty() {
        net.dhcp_range_start.parse::<std::net::Ipv4Addr>()
            .map(|ip| ip.octets())
            .unwrap_or([sub_octets[0], sub_octets[1], sub_octets[2], gw_octets[3].wrapping_add(100)])
    } else {
        [sub_octets[0], sub_octets[1], sub_octets[2], gw_octets[3].wrapping_add(100)]
    };

    // DNS IP: same as gateway (the SLIRP internal DNS relay)
    let dns_ip = gw_octets;

    // Upstream DNS
    let upstream_dns: Vec<String> = if net.dns_upstream.is_empty() {
        Vec::new()
    } else {
        net.dns_upstream.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    };

    // PXE next-server
    let pxe_next_server = if net.pxe_next_server.is_empty() {
        [0, 0, 0, 0]
    } else {
        net.pxe_next_server.parse::<std::net::Ipv4Addr>()
            .map(|ip| ip.octets())
            .unwrap_or([0, 0, 0, 0])
    };

    Ok(vmm_core::config::SdnNetConfig {
        net_prefix: [sub_octets[0], sub_octets[1], sub_octets[2]],
        gateway_ip: gw_octets,
        dns_ip,
        guest_ip,
        netmask,
        upstream_dns,
        dns_domain: net.dns_domain.clone(),
        pxe_boot_file: net.pxe_boot_file.clone(),
        pxe_next_server,
    })
}

/// Parse a CIDR notation string like "10.0.50.0/24".
fn parse_cidr(cidr: &str) -> Result<(std::net::Ipv4Addr, u8), String> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid CIDR: {}", cidr));
    }
    let ip: std::net::Ipv4Addr = parts[0].parse()
        .map_err(|_| format!("Invalid IP in CIDR: {}", parts[0]))?;
    let prefix: u8 = parts[1].parse()
        .map_err(|_| format!("Invalid prefix length: {}", parts[1]))?;
    if prefix > 32 {
        return Err(format!("Prefix length {} > 32", prefix));
    }
    Ok((ip, prefix))
}
