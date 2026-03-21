//! DRS (Distributed Resource Scheduler) Engine — generates placement recommendations.
//!
//! Runs periodically, analyzes load imbalance across hosts, and creates
//! recommendations for VM migrations. Admin decides whether to apply them.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::ClusterState;
use crate::services::host::HostService;
use crate::services::vm::VmService;
use crate::engine::scheduler::Scheduler;

const DRS_INTERVAL_SECS: u64 = 300; // 5 minutes
const IMBALANCE_THRESHOLD: f64 = 0.3; // Std dev threshold to trigger recommendations
const OVERLOADED_CPU_PCT: f64 = 80.0;
const OVERLOADED_RAM_PCT: f64 = 90.0;

/// Spawn the DRS engine as a background task.
pub fn spawn(state: Arc<ClusterState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(DRS_INTERVAL_SECS));
        loop {
            tick.tick().await;
            analyze_and_recommend(&state);
        }
    });
}

fn analyze_and_recommend(state: &Arc<ClusterState>) {
    let db = match state.db.lock() {
        Ok(db) => db,
        Err(_) => return,
    };

    let hosts = match HostService::list(&db) {
        Ok(h) => h,
        Err(_) => return,
    };

    let online_hosts: Vec<_> = hosts.iter()
        .filter(|h| h.status == "online" && !h.maintenance_mode)
        .collect();

    if online_hosts.len() < 2 {
        return; // Need at least 2 hosts for rebalancing
    }

    // Calculate utilization scores
    let scores: Vec<f64> = online_hosts.iter()
        .map(|h| Scheduler::host_utilization(h))
        .collect();

    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / scores.len() as f64;
    let std_dev = variance.sqrt();

    if std_dev < IMBALANCE_THRESHOLD {
        return; // Cluster is balanced enough
    }

    tracing::info!("DRS: Load imbalance detected (std_dev={:.2}), generating recommendations", std_dev);

    // Find overloaded and underloaded hosts
    for (i, host) in online_hosts.iter().enumerate() {
        if scores[i] <= 0.7 {
            continue; // Not overloaded
        }

        let is_cpu_overloaded = host.cpu_usage_pct > OVERLOADED_CPU_PCT;
        let ram_pct = if host.total_ram_mb > 0 {
            (1.0 - host.free_ram_mb as f64 / host.total_ram_mb as f64) * 100.0
        } else { 0.0 };
        let is_ram_overloaded = ram_pct > OVERLOADED_RAM_PCT;

        if !is_cpu_overloaded && !is_ram_overloaded {
            continue;
        }

        // Find VMs on this host that could be migrated
        let vms = VmService::list_by_host(&db, &host.id).unwrap_or_default();

        // Find best underloaded target
        let target = online_hosts.iter().enumerate()
            .filter(|(j, _)| *j != i)
            .min_by(|(a, _), (b, _)| scores[*a].partial_cmp(&scores[*b]).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, h)| *h);

        let target_host = match target {
            Some(h) => h,
            None => continue,
        };

        // Pick smallest VM that would make a difference
        let candidate = vms.iter()
            .filter(|v| v.state == "running")
            .filter(|v| v.ram_mb as i64 <= target_host.free_ram_mb)
            .min_by_key(|v| v.ram_mb);

        if let Some(vm) = candidate {
            let reason = if is_cpu_overloaded {
                format!("Host '{}' CPU at {:.0}%, Host '{}' at {:.0}%",
                    host.hostname, host.cpu_usage_pct, target_host.hostname, target_host.cpu_usage_pct)
            } else {
                format!("Host '{}' RAM at {:.0}%, Host '{}' has {}MB free",
                    host.hostname, ram_pct, target_host.hostname, target_host.free_ram_mb)
            };

            let priority = if scores[i] > 0.9 { "high" } else { "medium" };

            // Expire old pending recommendations for this VM
            let _ = db.execute(
                "UPDATE drs_recommendations SET status = 'expired' WHERE vm_id = ?1 AND status = 'pending'",
                rusqlite::params![&vm.id],
            );

            // Create new recommendation
            let _ = db.execute(
                "INSERT INTO drs_recommendations (cluster_id, vm_id, source_host_id, target_host_id, reason, priority) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![&vm.cluster_id, &vm.id, &host.id, &target_host.id, &reason, priority],
            );

            tracing::info!("DRS: Recommend moving VM '{}' from '{}' to '{}': {}",
                vm.name, host.hostname, target_host.hostname, reason);
        }
    }
}
