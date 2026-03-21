//! Migration service — orchestrates cold VM migration between hosts.

use std::sync::Arc;
use crate::state::ClusterState;
use crate::node_client::NodeClient;
use crate::services::vm::VmService;
use crate::services::task::TaskService;
use crate::services::event::EventService;

pub struct MigrationService;

impl MigrationService {
    /// Migrate a VM from its current host to a target host (cold migration).
    /// This is an async operation — runs in the background.
    pub async fn migrate_vm(
        state: &Arc<ClusterState>,
        vm_id: &str,
        target_host_id: &str,
        reason: &str,
        initiated_by: Option<i64>,
    ) {
        let vm = match state.db.lock().ok().and_then(|db| VmService::get(&db, vm_id).ok()) {
            Some(v) => v,
            None => {
                tracing::error!("Migration: VM {} not found", vm_id);
                return;
            }
        };

        let source_host_id = match &vm.host_id {
            Some(id) => id.clone(),
            None => {
                tracing::error!("Migration: VM '{}' has no host assigned", vm.name);
                return;
            }
        };

        if source_host_id == target_host_id {
            tracing::warn!("Migration: VM '{}' already on target host", vm.name);
            return;
        }

        // Create task
        let task_id = {
            let db = match state.db.lock() { Ok(db) => db, Err(_) => return };
            let details = serde_json::json!({
                "vm_name": vm.name, "source_host": source_host_id,
                "target_host": target_host_id, "reason": reason,
            });
            TaskService::create(&db, "vm.migrate", "vm", vm_id, initiated_by,
                Some(&details.to_string())).unwrap_or_default()
        };

        if let Ok(db) = state.db.lock() {
            TaskService::start(&db, &task_id).ok();
            // Record migration
            db.execute(
                "INSERT INTO migrations (vm_id, vm_name, source_host_id, target_host_id, migration_type, reason, status, initiated_by) \
                 VALUES (?1, ?2, ?3, ?4, 'cold', ?5, 'in_progress', ?6)",
                rusqlite::params![vm_id, &vm.name, &source_host_id, target_host_id, reason, initiated_by],
            ).ok();
        }

        tracing::info!("Migration: Starting cold migration of VM '{}' from {} to {}", vm.name, source_host_id, target_host_id);

        // Step 1: Stop VM on source (if running)
        if vm.state == "running" {
            if let Some(source_node) = state.nodes.get(&source_host_id) {
                if let Ok(client) = NodeClient::new(&source_node.address, &source_node.agent_token) {
                    let stop_resp = client.stop_vm(vm_id).await;
                    if stop_resp.is_err() {
                        let _ = client.force_stop_vm(vm_id).await;
                    }
                    // Wait for stop
                    for _ in 0..30 {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        if let Ok(status) = client.get_status().await {
                            if let Some(agent_vm) = status.vms.iter().find(|v| v.id == vm_id) {
                                if agent_vm.state == "stopped" { break; }
                            } else { break; }
                        }
                    }
                }
            }
            if let Ok(db) = state.db.lock() {
                TaskService::update_progress(&db, &task_id, 30).ok();
            }
        }

        // Step 2: Provision VM on target host
        let config = match state.db.lock().ok().and_then(|db| VmService::get_config(&db, vm_id).ok()) {
            Some(c) => c,
            None => {
                if let Ok(db) = state.db.lock() { TaskService::fail(&db, &task_id, "Cannot read VM config").ok(); }
                return;
            }
        };

        let target_node = match state.nodes.get(target_host_id) {
            Some(n) => n.clone(),
            None => {
                if let Ok(db) = state.db.lock() { TaskService::fail(&db, &task_id, "Target host not connected").ok(); }
                return;
            }
        };

        let target_client = match NodeClient::new(&target_node.address, &target_node.agent_token) {
            Ok(c) => c,
            Err(e) => {
                if let Ok(db) = state.db.lock() { TaskService::fail(&db, &task_id, &e).ok(); }
                return;
            }
        };

        let provision_req = vmm_core::cluster::ProvisionVmRequest {
            vm_id: vm_id.to_string(),
            config: config.clone(),
        };

        match target_client.provision_vm(&provision_req).await {
            Ok(resp) if resp.success => {}
            Ok(resp) => {
                let err = resp.error.unwrap_or_default();
                if let Ok(db) = state.db.lock() { TaskService::fail(&db, &task_id, &err).ok(); }
                return;
            }
            Err(e) => {
                if let Ok(db) = state.db.lock() { TaskService::fail(&db, &task_id, &e).ok(); }
                return;
            }
        }

        if let Ok(db) = state.db.lock() {
            TaskService::update_progress(&db, &task_id, 60).ok();
        }

        // Step 3: Destroy VM on source (keep disk files — shared storage)
        if let Some(source_node) = state.nodes.get(&source_host_id) {
            if let Ok(client) = NodeClient::new(&source_node.address, &source_node.agent_token) {
                let _ = client.destroy_vm(vm_id).await;
            }
        }

        if let Ok(db) = state.db.lock() {
            TaskService::update_progress(&db, &task_id, 80).ok();
        }

        // Step 4: Start VM on target (if it was running before)
        let should_start = vm.state == "running";
        if should_start {
            let _ = target_client.start_vm(vm_id).await;
        }

        // Step 5: Update cluster DB
        if let Ok(db) = state.db.lock() {
            VmService::assign_host(&db, vm_id, target_host_id).ok();
            let new_state = if should_start { "running" } else { "stopped" };
            VmService::update_state(&db, vm_id, new_state).ok();

            // Update migration record
            db.execute(
                "UPDATE migrations SET status = 'completed', completed_at = datetime('now') \
                 WHERE vm_id = ?1 AND status = 'in_progress'",
                rusqlite::params![vm_id],
            ).ok();

            TaskService::complete(&db, &task_id).ok();
            EventService::log(&db, "info", "vm",
                &format!("VM '{}' migrated to '{}' ({})", vm.name, target_node.hostname, reason),
                Some("vm"), Some(vm_id), Some(target_host_id));
        }

        tracing::info!("Migration: VM '{}' successfully migrated to '{}'", vm.name, target_node.hostname);
    }
}
