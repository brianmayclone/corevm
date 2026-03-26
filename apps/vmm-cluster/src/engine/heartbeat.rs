//! Heartbeat monitor — polls all registered nodes every 10 seconds.
//!
//! Updates host status, VM states, and datastore capacities in the cluster DB.
//! Detects host failures and triggers HA if needed.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::{ClusterState, NodeStatus};
use crate::node_client::NodeClient;
use crate::services::host::HostService;
use crate::services::vm::VmService;
use crate::services::event::EventService;

const HEARTBEAT_INTERVAL_SECS: u64 = 10;
const MAX_MISSED_HEARTBEATS: u32 = 3;

/// Spawn the heartbeat monitor as a background task.
pub fn spawn(state: Arc<ClusterState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        loop {
            tick.tick().await;
            poll_all_nodes(&state).await;
        }
    });
}

async fn poll_all_nodes(state: &Arc<ClusterState>) {
    // Snapshot current node IDs to avoid holding the DashMap lock
    let node_ids: Vec<String> = state.nodes.iter().map(|n| n.node_id.clone()).collect();

    if node_ids.is_empty() {
        return;
    }

    for node_id in node_ids {
        let node = match state.nodes.get(&node_id) {
            Some(n) => n.clone(),
            None => continue,
        };

        // Skip nodes in maintenance (they're still polled for status but not for HA)
        let client = match NodeClient::new(&node.address, &node.agent_token) {
            Ok(c) => c,
            Err(_) => continue,
        };

        match client.get_status().await {
            Ok(status) => {
                // Heartbeat succeeded — update DB
                if let Ok(db) = state.db.lock() {
                    let _ = HostService::update_heartbeat(&db, &node_id, &status);

                    // Sync VM states from agent report
                    for agent_vm in &status.vms {
                        let _ = VmService::update_state(&db, &agent_vm.id, &agent_vm.state);
                    }

                    // Sync datastore mount status
                    for ds in &status.datastores {
                        let _ = crate::services::datastore::DatastoreService::update_host_mount(
                            &db, &ds.datastore_id, &node_id,
                            ds.mounted, if ds.mounted { "mounted" } else { "error" },
                            ds.total_bytes as i64, ds.free_bytes as i64,
                        );
                    }

                    // Sync CoreSAN status — update vsan-type datastores with SAN volume capacity
                    if let Some(ref san) = status.san {
                        for vol in &san.volumes {
                            // Find matching datastore with store_type "vsan" and matching mount path
                            let _ = db.execute(
                                "UPDATE datastores SET total_bytes = ?1, free_bytes = ?2,
                                    status = CASE WHEN ?3 = 'online' THEN 'online'
                                                  WHEN ?3 = 'degraded' THEN 'degraded'
                                                  ELSE 'offline' END
                                 WHERE store_type = 'vsan' AND name = ?4",
                                rusqlite::params![
                                    vol.total_bytes as i64, vol.free_bytes as i64,
                                    &vol.status, &vol.volume_name
                                ],
                            );
                        }
                    }
                }

                // Reset missed heartbeat counter, mark online
                let was_offline = if let Some(mut n) = state.nodes.get_mut(&node_id) {
                    n.missed_heartbeats = 0;
                    let was = n.status == NodeStatus::Offline;
                    if n.status == NodeStatus::Connecting || n.status == NodeStatus::Offline {
                        n.status = NodeStatus::Online;
                        if let Ok(db) = state.db.lock() {
                            EventService::log(&db, "info", "host",
                                &format!("Host '{}' is now online", n.hostname),
                                Some("host"), Some(&node_id), Some(&node_id));
                        }
                    }
                    was
                } else { false };

                // Reconcile state if host was previously offline (prevent dual-running VMs)
                if was_offline {
                    let state_clone = Arc::clone(state);
                    let node_id_clone = node_id.clone();
                    tokio::spawn(async move {
                        crate::engine::reconciler::reconcile_host(&state_clone, &node_id_clone).await;
                    });
                }
            }
            Err(_) => {
                // Heartbeat failed
                let should_trigger_ha = if let Some(mut n) = state.nodes.get_mut(&node_id) {
                    n.missed_heartbeats += 1;
                    if n.missed_heartbeats >= MAX_MISSED_HEARTBEATS && n.status != NodeStatus::Offline {
                        n.status = NodeStatus::Offline;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                if should_trigger_ha {
                    tracing::warn!("Host {} is OFFLINE (missed {} heartbeats)", node_id, MAX_MISSED_HEARTBEATS);
                    if let Ok(db) = state.db.lock() {
                        let _ = HostService::mark_offline(&db, &node_id);
                        EventService::log(&db, "critical", "ha",
                            &format!("Host offline — {} missed heartbeats", MAX_MISSED_HEARTBEATS),
                            Some("host"), Some(&node_id), Some(&node_id));
                    }
                    // Trigger HA engine to restart protected VMs
                    let state_clone = Arc::clone(state);
                    let node_id_clone = node_id.clone();
                    tokio::spawn(async move {
                        crate::engine::ha::handle_host_failure(&state_clone, &node_id_clone).await;
                    });
                }
            }
        }
    }
}
