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
    // Check if VM's disks are on shared storage
    let has_shared_storage = {
        let db = match state.db.lock() { Ok(db) => db, Err(_) => return };
        // Check if any datastore mounted on OTHER hosts contains this VM's disks
        let shared_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM datastore_hosts dh \
             JOIN hosts h ON dh.host_id = h.id \
             WHERE dh.mounted = 1 AND h.id != ?1 AND h.status = 'online'",
            rusqlite::params![failed_host_id],
            |r| r.get(0),
        ).unwrap_or(0);
        shared_count > 0
    };

    if !has_shared_storage {
        tracing::warn!("HA: VM '{}' has no shared storage — cannot restart on another host", vm.name);
        if let Ok(db) = state.db.lock() {
            VmService::update_state(&db, &vm.id, "orphaned").ok();
            EventService::log(&db, "warning", "ha",
                &format!("VM '{}' cannot be HA-restarted — no shared storage accessible from other hosts", vm.name),
                Some("vm"), Some(&vm.id), None);
        }
        return;
    }

    // Find a suitable target host (with retry on cascading failure)
    let target_host_id = {
        let mut attempts = 0;
        let max_attempts = 3;
        let mut target = None;

        while attempts < max_attempts {
            target = find_target_host(state, vm, failed_host_id);
            if target.is_some() { break; }
            attempts += 1;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        match target {
            Some(id) => id,
            None => {
                tracing::error!("HA: No suitable host for VM '{}' after {} attempts — marking as orphaned", vm.name, max_attempts);
                if let Ok(db) = state.db.lock() {
                    VmService::update_state(&db, &vm.id, "orphaned").ok();
                    EventService::log(&db, "error", "ha",
                        &format!("VM '{}' cannot be restarted — no suitable host (tried {} times)", vm.name, max_attempts),
                        Some("vm"), Some(&vm.id), None);
                }
                return;
            }
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
