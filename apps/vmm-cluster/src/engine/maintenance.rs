//! Maintenance engine — VM evacuation when a host enters maintenance mode.
//!
//! Stops VMs on the maintenance host and migrates them (cold migration)
//! to other available hosts in the same cluster.

use std::sync::Arc;
use crate::state::ClusterState;
use crate::node_client::NodeClient;
use crate::services::vm::VmService;
use crate::services::host::HostService;
use crate::services::task::TaskService;
use crate::services::event::EventService;
use crate::engine::scheduler::Scheduler;

/// Evacuate all VMs from a host entering maintenance mode.
pub async fn evacuate_host(state: &Arc<ClusterState>, host_id: &str) {
    tracing::info!("Maintenance: Starting evacuation of host {}", host_id);

    let vms = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };
        VmService::list_by_host(&db, host_id).unwrap_or_default()
    };

    if vms.is_empty() {
        tracing::info!("Maintenance: No VMs to evacuate");
        return;
    }

    let mut migrated = 0;
    let mut failed = 0;

    for vm in &vms {
        match migrate_vm_from_host(state, vm, host_id).await {
            Ok(()) => migrated += 1,
            Err(e) => {
                tracing::error!("Maintenance: Failed to migrate VM '{}': {}", vm.name, e);
                failed += 1;
            }
        }
    }

    if let Ok(db) = state.db.lock() {
        EventService::log(&db, "info", "host",
            &format!("Evacuation complete: {} migrated, {} failed", migrated, failed),
            Some("host"), Some(host_id), Some(host_id));
    }

    tracing::info!("Maintenance: Evacuation done — {} migrated, {} failed", migrated, failed);
}

async fn migrate_vm_from_host(
    state: &Arc<ClusterState>,
    vm: &crate::services::vm::ClusterVm,
    source_host_id: &str,
) -> Result<(), String> {
    // Find target host
    let target_host_id = {
        let db = state.db.lock().map_err(|_| "DB lock error".to_string())?;
        Scheduler::select_host(&db, &vm.cluster_id, vm.ram_mb, vm.cpu_cores, None)?
            .ok_or_else(|| format!("No suitable host for VM '{}' ({}MB RAM)", vm.name, vm.ram_mb))?
    };

    // Create migration task
    let task_id = {
        let db = state.db.lock().map_err(|_| "DB lock error".to_string())?;
        let details = serde_json::json!({
            "vm_name": vm.name, "source_host": source_host_id,
            "target_host": target_host_id, "reason": "maintenance",
        });
        TaskService::create(&db, "vm.migrate", "vm", &vm.id, None, Some(&details.to_string()))?
    };

    // Step 1: Stop VM on source host (if running)
    if vm.state == "running" {
        let source_node = state.nodes.get(source_host_id)
            .ok_or("Source host not connected")?;
        let source_client = NodeClient::new(&source_node.address, &source_node.agent_token)?;

        tracing::info!("Maintenance: Stopping VM '{}' on source host", vm.name);
        let stop_resp = source_client.stop_vm(&vm.id).await?;
        if !stop_resp.success {
            // Try force stop
            let _ = source_client.force_stop_vm(&vm.id).await;
        }

        // Wait for VM to stop (poll for up to 30 seconds)
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            match source_client.get_status().await {
                Ok(status) => {
                    if let Some(agent_vm) = status.vms.iter().find(|v| v.id == vm.id) {
                        if agent_vm.state == "stopped" { break; }
                    } else {
                        break; // VM no longer listed
                    }
                }
                Err(_) => break,
            }
        }
    }

    // Step 2: Provision VM on target host
    let target_node = state.nodes.get(&target_host_id)
        .ok_or("Target host not connected")?;
    let target_client = NodeClient::new(&target_node.address, &target_node.agent_token)?;

    let config = {
        let db = state.db.lock().map_err(|_| "DB lock error".to_string())?;
        VmService::get_config(&db, &vm.id)?
    };

    let provision_req = vmm_core::cluster::ProvisionVmRequest {
        vm_id: vm.id.clone(),
        config: config.clone(),
    };

    tracing::info!("Maintenance: Provisioning VM '{}' on target host '{}'", vm.name, target_node.hostname);
    let provision_resp = target_client.provision_vm(&provision_req).await?;
    if !provision_resp.success {
        let err = provision_resp.error.unwrap_or_default();
        if let Ok(db) = state.db.lock() { TaskService::fail(&db, &task_id, &err).ok(); }
        return Err(err);
    }

    // Step 3: Destroy VM on source host (don't delete disk files — shared storage)
    if let Some(source_node) = state.nodes.get(source_host_id) {
        let source_client = NodeClient::new(&source_node.address, &source_node.agent_token)
            .map_err(|e| e.to_string())?;
        let _ = source_client.destroy_vm(&vm.id).await;
    }

    // Step 4: Start VM on target host
    tracing::info!("Maintenance: Starting VM '{}' on target host", vm.name);
    let start_resp = target_client.start_vm(&vm.id).await?;

    // Step 5: Update cluster DB
    {
        let db = state.db.lock().map_err(|_| "DB lock error".to_string())?;
        VmService::assign_host(&db, &vm.id, &target_host_id)?;
        let new_state = if start_resp.success { "running" } else { "stopped" };
        VmService::update_state(&db, &vm.id, new_state)?;

        // Record migration
        db.execute(
            "INSERT INTO migrations (vm_id, vm_name, source_host_id, target_host_id, migration_type, reason, status) \
             VALUES (?1, ?2, ?3, ?4, 'cold', 'maintenance', 'completed')",
            rusqlite::params![&vm.id, &vm.name, source_host_id, &target_host_id],
        ).ok();

        TaskService::complete(&db, &task_id)?;
        EventService::log(&db, "info", "vm",
            &format!("VM '{}' migrated to '{}' (maintenance)", vm.name, target_node.hostname),
            Some("vm"), Some(&vm.id), Some(&target_host_id));
    }

    Ok(())
}
