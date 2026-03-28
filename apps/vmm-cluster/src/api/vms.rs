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
    /// Optional: viSwitch to attach the VM to.
    /// When set, auto-allocates a port and configures bridge mode.
    pub viswitch_id: Option<i64>,
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
    if let Some(viswitch_id) = body.viswitch_id {
        // viSwitch mode: allocate a port and connect via viSwitch bridge
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let _port_index = crate::services::viswitch::ViSwitchService::assign_port(&db, viswitch_id, &vm_id, None)
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("Cannot allocate viSwitch port: {}", e)))?;
        let bridge_name = crate::services::viswitch::ViSwitchService::bridge_name(viswitch_id);
        drop(db);

        if let Some(obj) = config.as_object_mut() {
            obj.insert("net_mode".into(), serde_json::json!("bridge"));
            obj.insert("net_host_nic".into(), serde_json::json!(bridge_name));
            obj.insert("net_enabled".into(), serde_json::json!(true));
        }
    } else if let Some(network_id) = body.network_id {
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

    // Step 2: Determine target host — explicit or via scheduler (Best Fit)
    let target_host_id = if let Some(host_id) = &body.host_id {
        host_id.clone()
    } else {
        // Auto-placement: use scheduler to pick the best host
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let ram_mb = config.get("ram_mb").and_then(|v| v.as_u64()).unwrap_or(2048) as u32;
        let cpu_cores = config.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
        crate::engine::scheduler::Scheduler::select_host(&db, &body.cluster_id, ram_mb, cpu_cores, None)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?
            .ok_or_else(|| AppError(StatusCode::CONFLICT,
                "No host has enough resources for this VM. Free up resources or add hosts.".into()))?
    };

    // Step 3: Provision on the target host
    {
        let node = state.nodes.get(&target_host_id)
            .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Target host not found or offline".into()))?;

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
        VmService::assign_host(&db, &vm_id, &target_host_id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;

        let host_name = node.hostname.clone();
        EventService::log(&db, "info", "vm",
            &format!("VM '{}' placed on host '{}'", body.name, host_name),
            Some("vm"), Some(&vm_id), Some(&target_host_id));
    }

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

    // Resolve host — auto-place if unassigned
    let (existing_host_id, needs_placement) = {
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        let vm = VmService::get(&db, &id).map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        match vm.host_id {
            Some(hid) => (Some(hid), None),
            None => {
                let hid = crate::engine::scheduler::Scheduler::select_host(&db, &vm.cluster_id, vm.ram_mb, vm.cpu_cores, None)
                    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?
                    .ok_or_else(|| AppError(StatusCode::CONFLICT,
                        "No host has enough resources for this VM".into()))?;
                let config = VmService::get_config(&db, &id)
                    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
                (None, Some((hid, vm.name.clone(), config)))
            }
        }
    };
    // DB lock is dropped here — safe to await

    let host_id = if let Some((hid, vm_name, config)) = needs_placement {
        // Provision on auto-selected host
        let node = state.nodes.get(&hid)
            .ok_or_else(|| AppError(StatusCode::BAD_GATEWAY, "Selected host not connected".into()))?;
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true).build()
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let provision_req = vmm_core::cluster::ProvisionVmRequest { vm_id: id.clone(), config };
        let resp = client.post(format!("{}/agent/vms/provision", &node.address))
            .header("X-Agent-Token", &node.agent_token)
            .json(&provision_req).send().await
            .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Cannot reach host: {}", e)))?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(AppError(StatusCode::BAD_GATEWAY, format!("Provisioning failed: {}", err)));
        }
        let db = state.db.lock().map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock error".into()))?;
        VmService::assign_host(&db, &id, &hid)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        EventService::log(&db, "info", "vm",
            &format!("VM '{}' auto-placed on '{}' at start", vm_name, node.hostname),
            Some("vm"), Some(&id), Some(&hid));
        hid
    } else {
        existing_host_id.unwrap()
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
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        tracing::error!("VM start failed on host {}: {} {}", host_id, status, err);
        return Err(AppError(StatusCode::BAD_GATEWAY, format!("Start failed on host: {} {}", status, err)));
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

    // Release viSwitch port if assigned
    let viswitch_ids: Vec<i64> = {
        let mut stmt = db.prepare("SELECT DISTINCT viswitch_id FROM viswitch_ports WHERE vm_id = ?1").ok();
        stmt.as_mut().map(|s| {
            s.query_map(rusqlite::params![&id], |row| row.get::<_, i64>(0))
                .ok().map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        }).unwrap_or_default()
    };
    for vs_id in viswitch_ids {
        let _ = crate::services::viswitch::ViSwitchService::release_port(&db, vs_id, &id);
    }

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
