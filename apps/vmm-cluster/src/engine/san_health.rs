//! SAN health monitor — periodic health checks of all CoreSAN hosts.
//!
//! Polls each SAN-enabled host's `/api/status` every 30 seconds and stores
//! an aggregated health snapshot. Logs events on state changes (degraded,
//! offline, recovered).

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::san_client::{SanClient, get_san_hosts};
use crate::services::event::EventService;
use crate::state::ClusterState;

const HEALTH_INTERVAL_SECS: u64 = 30;

/// Spawn the SAN health monitor as a background task.
pub fn spawn(state: Arc<ClusterState>) {
    tokio::spawn(async move {
        // Wait for initial heartbeats to populate SAN addresses
        tokio::time::sleep(Duration::from_secs(15)).await;

        let mut tick = interval(Duration::from_secs(HEALTH_INTERVAL_SECS));
        loop {
            tick.tick().await;
            check_all_san_hosts(&state).await;
        }
    });
}

async fn check_all_san_hosts(state: &ClusterState) {
    let hosts = {
        let db = state.db.lock().unwrap();
        get_san_hosts(&db)
    };

    if hosts.is_empty() {
        return;
    }

    let mut host_results = Vec::new();

    let futures: Vec<_> = hosts.iter().map(|h| {
        let client = SanClient::new(&h.san_address);
        let host_id = h.host_id.clone();
        let hostname = h.hostname.clone();
        let san_address = h.san_address.clone();
        async move {
            match client.get_status().await {
                Ok(status) => {
                    let volumes = status.get("volumes").and_then(|v| v.as_array());

                    let degraded_volumes: Vec<String> = volumes.map(|vols| {
                        vols.iter().filter_map(|v| {
                            let vol_status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                            if vol_status == "degraded" || v.get("degraded_files").and_then(|f| f.as_u64()).unwrap_or(0) > 0 {
                                Some(v.get("volume_name").and_then(|n| n.as_str()).unwrap_or("?").to_string())
                            } else {
                                None
                            }
                        }).collect()
                    }).unwrap_or_default();

                    let peer_count = status.get("peer_count").and_then(|p| p.as_u64()).unwrap_or(0);
                    let backend_count = volumes.map(|vols| {
                        vols.first().and_then(|v| v.get("backend_count").and_then(|b| b.as_u64())).unwrap_or(0)
                    }).unwrap_or(0);

                    let health = if !degraded_volumes.is_empty() {
                        "degraded"
                    } else {
                        "healthy"
                    };

                    serde_json::json!({
                        "host_id": host_id,
                        "hostname": hostname,
                        "san_address": san_address,
                        "reachable": true,
                        "health": health,
                        "peer_count": peer_count,
                        "backend_count": backend_count,
                        "degraded_volumes": degraded_volumes,
                        "uptime_secs": status.get("uptime_secs").and_then(|u| u.as_u64()).unwrap_or(0),
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "host_id": host_id,
                        "hostname": hostname,
                        "san_address": san_address,
                        "reachable": false,
                        "health": "offline",
                        "error": e,
                    })
                }
            }
        }
    }).collect();

    let results = futures::future::join_all(futures).await;
    for r in &results {
        host_results.push(r.clone());
    }

    // Detect state changes and log events
    {
        let previous = state.san_health.read().unwrap();
        let prev_hosts = previous.get("hosts").and_then(|h| h.as_array());

        for host in &host_results {
            let host_id = host.get("host_id").and_then(|h| h.as_str()).unwrap_or("");
            let hostname = host.get("hostname").and_then(|h| h.as_str()).unwrap_or("");
            let reachable = host.get("reachable").and_then(|r| r.as_bool()).unwrap_or(false);
            let health = host.get("health").and_then(|h| h.as_str()).unwrap_or("unknown");

            let prev_reachable = prev_hosts.and_then(|hosts| {
                hosts.iter().find(|h| h.get("host_id").and_then(|id| id.as_str()) == Some(host_id))
            }).and_then(|h| h.get("reachable").and_then(|r| r.as_bool())).unwrap_or(true);

            let prev_health = prev_hosts.and_then(|hosts| {
                hosts.iter().find(|h| h.get("host_id").and_then(|id| id.as_str()) == Some(host_id))
            }).and_then(|h| h.get("health").and_then(|s| s.as_str())).unwrap_or("unknown");

            // Log state transitions
            if prev_reachable && !reachable {
                let db = state.db.lock().unwrap();
                EventService::log(&db, "error", "san",
                    &format!("CoreSAN on {} is unreachable", hostname),
                    Some("host"), Some(host_id), Some(host_id));
                tracing::error!("SAN health: {} ({}) is unreachable", hostname, host_id);
            } else if !prev_reachable && reachable {
                let db = state.db.lock().unwrap();
                EventService::log(&db, "info", "san",
                    &format!("CoreSAN on {} recovered", hostname),
                    Some("host"), Some(host_id), Some(host_id));
                tracing::info!("SAN health: {} ({}) recovered", hostname, host_id);
            } else if prev_health != "degraded" && health == "degraded" {
                let degraded = host.get("degraded_volumes").and_then(|d| d.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let db = state.db.lock().unwrap();
                EventService::log(&db, "warning", "san",
                    &format!("CoreSAN on {} has degraded volumes: {}", hostname, degraded),
                    Some("host"), Some(host_id), Some(host_id));
                tracing::warn!("SAN health: {} has degraded volumes: {}", hostname, degraded);
            }
        }
    }

    // Update snapshot
    let snapshot = serde_json::json!({
        "hosts": host_results,
        "checked_at": chrono::Utc::now().to_rfc3339(),
    });

    *state.san_health.write().unwrap() = snapshot;
}
