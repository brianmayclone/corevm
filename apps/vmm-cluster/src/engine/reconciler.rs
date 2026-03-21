//! State reconciler — syncs cluster DB with node reality after reconnect.

use std::sync::Arc;
use crate::state::ClusterState;
use crate::node_client::NodeClient;
use crate::services::vm::VmService;
use crate::services::event::EventService;

enum ReconcileAction {
    ForceStop(String),     // VM ID to stop on this host
    Reclaim(String),       // VM ID to reclaim from orphaned
}

/// Reconcile state when a host reconnects after being offline.
pub async fn reconcile_host(state: &Arc<ClusterState>, host_id: &str) {
    tracing::info!("Reconciler: Starting state reconciliation for host {}", host_id);

    let node = match state.nodes.get(host_id) {
        Some(n) => n.clone(),
        None => return,
    };

    let client = match NodeClient::new(&node.address, &node.agent_token) {
        Ok(c) => c,
        Err(_) => return,
    };

    let agent_status = match client.get_status().await {
        Ok(s) => s,
        Err(_) => return,
    };

    // Phase 1: Determine actions needed (DB lock held, no awaits)
    let actions: Vec<ReconcileAction> = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(_) => return,
        };

        let mut actions = Vec::new();
        for agent_vm in &agent_status.vms {
            match VmService::get(&db, &agent_vm.id) {
                Ok(cluster_vm) => {
                    if let Some(ref db_host) = cluster_vm.host_id {
                        if db_host != host_id && agent_vm.state == "running" {
                            tracing::warn!("Reconciler: VM '{}' running on {} but DB says {} — will stop",
                                agent_vm.id, host_id, db_host);
                            actions.push(ReconcileAction::ForceStop(agent_vm.id.clone()));
                        }
                    }
                    if cluster_vm.state == "orphaned" && agent_vm.state == "running" {
                        actions.push(ReconcileAction::Reclaim(agent_vm.id.clone()));
                    }
                }
                Err(_) => {
                    if agent_vm.state == "running" {
                        actions.push(ReconcileAction::ForceStop(agent_vm.id.clone()));
                    }
                }
            }
        }
        actions
    }; // DB lock dropped here

    // Phase 2: Execute actions (async, no DB lock needed for network calls)
    let mut stopped = 0;
    let mut reclaimed = 0;

    for action in &actions {
        match action {
            ReconcileAction::ForceStop(vm_id) => {
                let _ = client.force_stop_vm(vm_id).await;
                stopped += 1;
            }
            ReconcileAction::Reclaim(vm_id) => {
                if let Ok(db) = state.db.lock() {
                    let _ = VmService::assign_host(&db, vm_id, host_id);
                    let _ = VmService::update_state(&db, vm_id, "running");
                }
                reclaimed += 1;
            }
        }
    }

    if stopped > 0 || reclaimed > 0 {
        if let Ok(db) = state.db.lock() {
            EventService::log(&db, "info", "host",
                &format!("Reconciliation: {} stopped, {} reclaimed", stopped, reclaimed),
                Some("host"), Some(host_id), Some(host_id));
        }
        tracing::info!("Reconciler: Host {} — {} stopped, {} reclaimed", host_id, stopped, reclaimed);
    }
}
