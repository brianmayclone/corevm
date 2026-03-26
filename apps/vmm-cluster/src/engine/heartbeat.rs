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

                    // Sync CoreSAN status into hosts table (auto-discovery)
                    match &status.san {
                        Some(san) if san.running => {
                            // Check if this is a newly discovered SAN host (was not enabled before)
                            let was_san_enabled: bool = db.query_row(
                                "SELECT san_enabled FROM hosts WHERE id = ?1",
                                rusqlite::params![&node_id],
                                |row| row.get::<_, i64>(0),
                            ).map(|v| v == 1).unwrap_or(false);

                            let _ = db.execute(
                                "UPDATE hosts SET san_enabled = 1, san_node_id = ?1,
                                    san_address = ?2, san_volumes = ?3, san_peers = ?4
                                 WHERE id = ?5",
                                rusqlite::params![
                                    &san.node_id, &san.address,
                                    san.volumes.len() as i64, san.peer_count as i64,
                                    &node_id
                                ],
                            );

                            // Auto peer registration: if new SAN host or has no peers yet,
                            // register it with all other SAN hosts
                            if !was_san_enabled || san.peer_count == 0 {
                                let other_san_hosts: Vec<(String, String, String)> = {
                                    let mut stmt = db.prepare(
                                        "SELECT san_node_id, san_address, hostname FROM hosts
                                         WHERE san_enabled = 1 AND san_address != '' AND id != ?1"
                                    ).unwrap();
                                    stmt.query_map(rusqlite::params![&node_id], |row| {
                                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
                                    }).unwrap().filter_map(|r| r.ok()).collect()
                                };

                                if !other_san_hosts.is_empty() {
                                    let new_san_addr = san.address.clone();
                                    let new_san_node_id = san.node_id.clone();
                                    let new_hostname = node.hostname.clone();
                                    let new_node_id = node_id.clone();
                                    tokio::spawn(async move {
                                        let client = crate::san_client::SanClient::new(&new_san_addr);
                                        for (other_node_id, other_addr, other_hostname) in &other_san_hosts {
                                            // Register the new host on the existing host
                                            let other_client = crate::san_client::SanClient::new(other_addr);
                                            let join_body = serde_json::json!({
                                                "address": new_san_addr,
                                                "node_id": new_san_node_id,
                                                "hostname": new_hostname,
                                            });
                                            if let Err(e) = other_client.join_peer(&join_body).await {
                                                tracing::warn!("Auto-peer: failed to register {} on {}: {}",
                                                    new_hostname, other_hostname, e);
                                            } else {
                                                tracing::info!("Auto-peer: registered {} on {}",
                                                    new_hostname, other_hostname);
                                            }

                                            // Register the existing host on the new host
                                            let reverse_body = serde_json::json!({
                                                "address": other_addr,
                                                "node_id": other_node_id,
                                                "hostname": other_hostname,
                                            });
                                            if let Err(e) = client.join_peer(&reverse_body).await {
                                                tracing::warn!("Auto-peer: failed to register {} on {}: {}",
                                                    other_hostname, new_hostname, e);
                                            } else {
                                                tracing::info!("Auto-peer: registered {} on {}",
                                                    other_hostname, new_hostname);
                                            }
                                        }
                                        tracing::info!("Auto-peer registration complete for {} ({}) with {} peers",
                                            new_hostname, new_node_id, other_san_hosts.len());
                                    });

                                    EventService::log(&db, "info", "san",
                                        &format!("CoreSAN peer auto-registered: {}", node.hostname),
                                        Some("host"), Some(&node_id), Some(&node_id));
                                }
                            }

                            // Also update vsan-type datastores with volume capacity
                            for vol in &san.volumes {
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
                        _ => {
                            // CoreSAN not running on this host
                            let _ = db.execute(
                                "UPDATE hosts SET san_enabled = 0, san_node_id = '',
                                    san_address = '', san_volumes = 0, san_peers = 0
                                 WHERE id = ?1",
                                rusqlite::params![&node_id],
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
