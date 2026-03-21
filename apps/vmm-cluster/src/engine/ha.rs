//! HA (High Availability) Engine — automatic VM restart on host failure.
//!
//! When a host goes offline, HA-protected VMs are restarted on remaining hosts.
//! VMs are prioritized by ha_restart_priority (high → medium → low).

use std::sync::Arc;
use crate::state::ClusterState;
use crate::node_client::NodeClient;
use crate::services::vm::VmService;
use crate::services::host::HostService;
use crate::services::task::TaskService;
use crate::services::event::EventService;

/// Trigger HA restart for all protected VMs on a failed host.
pub async fn handle_host_failure(state: &Arc<ClusterState>, failed_host_id: &str) {
    tracing::warn!("HA: Handling host failure for {}", failed_host_id);

    let ha_vms = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };
        VmService::list_ha_protected_by_host(&db, failed_host_id).unwrap_or_default()
    };

    if ha_vms.is_empty() {
        tracing::info!("HA: No HA-protected VMs on failed host");
        return;
    }

    tracing::warn!("HA: {} VMs need restart", ha_vms.len());

    // Sort by priority: high first, then medium, then low
    let mut sorted_vms = ha_vms;
    sorted_vms.sort_by(|a, b| {
        let priority_ord = |p: &str| match p { "high" => 0, "medium" => 1, _ => 2 };
        priority_ord(&a.ha_restart_priority).cmp(&priority_ord(&b.ha_restart_priority))
    });

    for vm in &sorted_vms {
        restart_vm_on_available_host(state, vm, failed_host_id).await;
    }
}

async fn restart_vm_on_available_host(
    state: &Arc<ClusterState>,
    vm: &crate::services::vm::ClusterVm,
    failed_host_id: &str,
) {
    // Find a suitable target host
    let target = find_target_host(state, vm, failed_host_id);
    let target_host_id = match target {
        Some(id) => id,
        None => {
            tracing::error!("HA: No suitable host for VM '{}' — marking as orphaned", vm.name);
            if let Ok(db) = state.db.lock() {
                VmService::update_state(&db, &vm.id, "orphaned").ok();
                EventService::log(&db, "error", "ha",
                    &format!("VM '{}' cannot be restarted — no suitable host", vm.name),
                    Some("vm"), Some(&vm.id), None);
            }
            return;
        }
    };

    // Create a task for tracking
    let task_id = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };
        let details = serde_json::json!({
            "vm_name": vm.name,
            "source_host": failed_host_id,
            "target_host": target_host_id,
        });
        TaskService::create(&db, "vm.ha_restart", "vm", &vm.id, None,
            Some(&details.to_string())).unwrap_or_default()
    };

    // Get target node connection
    let node = match state.nodes.get(&target_host_id) {
        Some(n) => n.clone(),
        None => return,
    };

    let client = match NodeClient::new(&node.address, &node.agent_token) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Provision VM on target
    let config = match state.db.lock().ok().and_then(|db| VmService::get_config(&db, &vm.id).ok()) {
        Some(c) => c,
        None => return,
    };

    let provision_req = vmm_core::cluster::ProvisionVmRequest {
        vm_id: vm.id.clone(),
        config,
    };

    tracing::info!("HA: Provisioning VM '{}' on host '{}'", vm.name, node.hostname);

    match client.provision_vm(&provision_req).await {
        Ok(resp) if resp.success => {
            // Start the VM
            match client.start_vm(&vm.id).await {
                Ok(start_resp) if start_resp.success => {
                    if let Ok(db) = state.db.lock() {
                        VmService::assign_host(&db, &vm.id, &target_host_id).ok();
                        VmService::update_state(&db, &vm.id, "running").ok();
                        TaskService::complete(&db, &task_id).ok();
                        EventService::log(&db, "warning", "ha",
                            &format!("VM '{}' restarted on '{}' (HA)", vm.name, node.hostname),
                            Some("vm"), Some(&vm.id), Some(&target_host_id));
                    }
                    tracing::info!("HA: VM '{}' successfully restarted on '{}'", vm.name, node.hostname);
                }
                Ok(resp) => {
                    if let Ok(db) = state.db.lock() {
                        TaskService::fail(&db, &task_id, &resp.error.unwrap_or_default()).ok();
                    }
                }
                Err(e) => {
                    if let Ok(db) = state.db.lock() {
                        TaskService::fail(&db, &task_id, &e).ok();
                    }
                }
            }
        }
        Ok(resp) => {
            tracing::error!("HA: Provision failed for VM '{}': {:?}", vm.name, resp.error);
            if let Ok(db) = state.db.lock() {
                TaskService::fail(&db, &task_id, &resp.error.unwrap_or_default()).ok();
            }
        }
        Err(e) => {
            tracing::error!("HA: Provision failed for VM '{}': {}", vm.name, e);
            if let Ok(db) = state.db.lock() {
                TaskService::fail(&db, &task_id, &e).ok();
            }
        }
    }
}

/// Find a suitable host for HA VM restart.
fn find_target_host(state: &Arc<ClusterState>, vm: &crate::services::vm::ClusterVm, exclude_host: &str) -> Option<String> {
    let db = state.db.lock().ok()?;
    let hosts = HostService::list(&db).ok()?;

    // Filter: online, not in maintenance, not the failed host, enough RAM
    let mut candidates: Vec<_> = hosts.into_iter()
        .filter(|h| h.id != exclude_host)
        .filter(|h| h.status == "online")
        .filter(|h| !h.maintenance_mode)
        .filter(|h| h.free_ram_mb >= vm.ram_mb as i64)
        .collect();

    // Sort by most free RAM (simple bin-packing)
    candidates.sort_by(|a, b| b.free_ram_mb.cmp(&a.free_ram_mb));
    candidates.first().map(|h| h.id.clone())
}
