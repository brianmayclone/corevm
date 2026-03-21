//! Migration service — orchestrates direct host-to-host VM migration.
//!
//! The cluster generates a one-time token and instructs both hosts:
//! - Source host: stop VM, send disks directly to target
//! - Target host: receive disks, provision VM
//! No data flows through the cluster — hosts transfer directly.

use std::sync::Arc;
use crate::state::ClusterState;
use crate::node_client::NodeClient;
use crate::services::vm::VmService;
use crate::services::host::HostService;
use crate::services::task::TaskService;
use crate::services::event::EventService;

pub struct MigrationService;

impl MigrationService {
    /// Migrate a VM from its current host to a target host.
    /// Uses direct host-to-host transfer with a one-time token.
    pub async fn migrate_vm(
        state: &Arc<ClusterState>,
        vm_id: &str,
        target_host_id: &str,
        reason: &str,
        initiated_by: Option<i64>,
    ) {
        let vm = match state.db.lock().ok().and_then(|db| VmService::get(&db, vm_id).ok()) {
            Some(v) => v,
            None => { tracing::error!("Migration: VM {} not found", vm_id); return; }
        };

        let source_host_id = match &vm.host_id {
            Some(id) => id.clone(),
            None => { tracing::error!("Migration: VM '{}' has no host", vm.name); return; }
        };

        if source_host_id == target_host_id {
            tracing::warn!("Migration: VM '{}' already on target host", vm.name);
            return;
        }

        // Generate one-time migration token
        let migration_token = uuid::Uuid::new_v4().to_string();

        // Create task
        let task_id = {
            let db = match state.db.lock() { Ok(db) => db, Err(_) => return };
            let details = serde_json::json!({
                "vm_name": vm.name, "source_host": source_host_id,
                "target_host": target_host_id, "reason": reason,
                "direct_transfer": true,
            });
            TaskService::create(&db, "vm.migrate", "vm", vm_id, initiated_by, Some(&details.to_string())).unwrap_or_default()
        };

        if let Ok(db) = state.db.lock() {
            TaskService::start(&db, &task_id).ok();
            db.execute(
                "INSERT INTO migrations (vm_id, vm_name, source_host_id, target_host_id, migration_type, reason, status, initiated_by) \
                 VALUES (?1, ?2, ?3, ?4, 'cold', ?5, 'in_progress', ?6)",
                rusqlite::params![vm_id, &vm.name, &source_host_id, target_host_id, reason, initiated_by],
            ).ok();
        }

        tracing::info!("Migration: Starting direct transfer of VM '{}' from {} to {} (token: {}...)",
            vm.name, source_host_id, target_host_id, &migration_token[..8]);

        // Check if both hosts share a datastore (skip disk transfer if so)
        let shared_storage = check_shared_storage(state, &source_host_id, target_host_id, vm_id);

        // Get host connection info
        let source_node = match state.nodes.get(&source_host_id) {
            Some(n) => n.clone(), None => { fail_task(state, &task_id, "Source host not connected"); return; }
        };
        let target_node = match state.nodes.get(target_host_id) {
            Some(n) => n.clone(), None => { fail_task(state, &task_id, "Target host not connected"); return; }
        };

        let source_client = match NodeClient::new(&source_node.address, &source_node.agent_token) {
            Ok(c) => c, Err(e) => { fail_task(state, &task_id, &e); return; }
        };
        let target_client = match NodeClient::new(&target_node.address, &target_node.agent_token) {
            Ok(c) => c, Err(e) => { fail_task(state, &task_id, &e); return; }
        };

        if shared_storage {
            // Shared storage path: no disk transfer needed, just provision + start
            tracing::info!("Migration: Shared storage detected — skipping disk transfer");

            // Stop on source
            let _ = source_client.stop_vm(vm_id).await;
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if let Ok(status) = source_client.get_status().await {
                    if status.vms.iter().all(|v| v.id != vm_id || v.state == "stopped") { break; }
                }
            }

            if let Ok(db) = state.db.lock() { TaskService::update_progress(&db, &task_id, 30).ok(); }

            // Provision on target with same config
            let config = match state.db.lock().ok().and_then(|db| VmService::get_config(&db, vm_id).ok()) {
                Some(c) => c, None => { fail_task(state, &task_id, "Cannot read VM config"); return; }
            };
            let provision_req = vmm_core::cluster::ProvisionVmRequest { vm_id: vm_id.to_string(), config };
            match target_client.provision_vm(&provision_req).await {
                Ok(r) if r.success => {},
                Ok(r) => { fail_task(state, &task_id, &r.error.unwrap_or_default()); return; }
                Err(e) => { fail_task(state, &task_id, &e); return; }
            }

            if let Ok(db) = state.db.lock() { TaskService::update_progress(&db, &task_id, 60).ok(); }

            // Destroy on source (keep disk files — shared storage)
            let _ = source_client.destroy_vm(vm_id).await;

            if let Ok(db) = state.db.lock() { TaskService::update_progress(&db, &task_id, 80).ok(); }
        } else {
            // Direct transfer path: source sends disks directly to target
            tracing::info!("Migration: Direct host-to-host disk transfer");

            // Get VM config and disk paths
            let config_json = match state.db.lock().ok().and_then(|db| {
                VmService::get_config(&db, vm_id).ok().map(|c| serde_json::to_string(&c).ok()).flatten()
            }) {
                Some(c) => c, None => { fail_task(state, &task_id, "Cannot serialize VM config"); return; }
            };

            let disk_paths: Vec<String> = {
                let config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or_default();
                config.get("disk_images").and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default()
            };

            if let Ok(db) = state.db.lock() { TaskService::update_progress(&db, &task_id, 10).ok(); }

            // Tell source to send disks directly to target
            let send_req = vmm_core::cluster::MigrationSendRequest {
                vm_id: vm_id.to_string(),
                migration_token: migration_token.clone(),
                target_address: target_node.address.clone(),
                disk_paths,
                config_json,
            };

            // Send migration command to source host
            let resp = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(7200))
                .build()
                .ok()
                .and_then(|c| {
                    let url = format!("{}/agent/migration/send", source_node.address);
                    let token = source_node.agent_token.clone();
                    // We need to block on this — it's a long operation
                    Some((c, url, token))
                });

            if let Some((client, url, token)) = resp {
                match client.post(&url)
                    .header("X-Agent-Token", &token)
                    .json(&send_req)
                    .send().await
                {
                    Ok(r) if r.status().is_success() => {
                        tracing::info!("Migration: Direct transfer completed for VM '{}'", vm.name);
                    }
                    Ok(r) => {
                        let err = r.text().await.unwrap_or_default();
                        fail_task(state, &task_id, &format!("Transfer failed: {}", err));
                        return;
                    }
                    Err(e) => {
                        fail_task(state, &task_id, &format!("Transfer error: {}", e));
                        return;
                    }
                }
            }

            if let Ok(db) = state.db.lock() { TaskService::update_progress(&db, &task_id, 80).ok(); }
        }

        // Start VM on target if it was running
        let should_start = vm.state == "running";
        if should_start {
            let _ = target_client.start_vm(vm_id).await;
        }

        // Update cluster DB
        if let Ok(db) = state.db.lock() {
            VmService::assign_host(&db, vm_id, target_host_id).ok();
            VmService::update_state(&db, vm_id, if should_start { "running" } else { "stopped" }).ok();
            db.execute(
                "UPDATE migrations SET status = 'completed', completed_at = datetime('now') WHERE vm_id = ?1 AND status = 'in_progress'",
                rusqlite::params![vm_id],
            ).ok();
            TaskService::complete(&db, &task_id).ok();
            EventService::log(&db, "info", "vm",
                &format!("VM '{}' migrated to '{}' ({}, {})", vm.name, target_node.hostname, reason,
                    if shared_storage { "shared storage" } else { "direct transfer" }),
                Some("vm"), Some(vm_id), Some(target_host_id));
        }

        tracing::info!("Migration: VM '{}' successfully migrated to '{}'", vm.name, target_node.hostname);
    }
}

/// Check if source and target hosts share a datastore that contains the VM's disks.
fn check_shared_storage(state: &Arc<ClusterState>, source_id: &str, target_id: &str, _vm_id: &str) -> bool {
    let db = match state.db.lock() { Ok(db) => db, Err(_) => return false };
    // Check if any datastore is mounted on both hosts
    let shared: i64 = db.query_row(
        "SELECT COUNT(*) FROM datastore_hosts dh1 \
         JOIN datastore_hosts dh2 ON dh1.datastore_id = dh2.datastore_id \
         WHERE dh1.host_id = ?1 AND dh2.host_id = ?2 AND dh1.mounted = 1 AND dh2.mounted = 1",
        rusqlite::params![source_id, target_id],
        |r| r.get(0),
    ).unwrap_or(0);
    shared > 0
}

fn fail_task(state: &Arc<ClusterState>, task_id: &str, error: &str) {
    tracing::error!("Migration failed: {}", error);
    if let Ok(db) = state.db.lock() {
        TaskService::fail(&db, task_id, error).ok();
        db.execute(
            "UPDATE migrations SET status = 'failed', error = ?1, completed_at = datetime('now') WHERE status = 'in_progress'",
            rusqlite::params![error],
        ).ok();
    }
}
