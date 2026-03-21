//! DRS (Distributed Resource Scheduler) Engine — generates placement recommendations.
//!
//! Runs periodically, reads DRS rules from the database, analyzes load imbalance
//! across hosts, and creates recommendations for VM migrations.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::ClusterState;
use crate::services::host::HostService;
use crate::services::vm::VmService;
use crate::services::cluster::ClusterService;
use crate::services::drs_service::DrsService;
use crate::engine::scheduler::Scheduler;

const DRS_INTERVAL_SECS: u64 = 300; // 5 minutes
const DEFAULT_IMBALANCE_THRESHOLD: f64 = 0.3;

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

    // Get all clusters and process each
    let clusters = match ClusterService::list(&db) {
        Ok(c) => c,
        Err(_) => return,
    };

    for cluster in &clusters {
        if !cluster.drs_enabled { continue; }

        // Load DRS rules for this cluster
        let rules = DrsService::active_rules(&db, &cluster.id).unwrap_or_default();

        // If no rules configured, use defaults
        let cpu_threshold = rules.iter()
            .find(|r| r.metric == "cpu_usage")
            .map(|r| r.threshold)
            .unwrap_or(80.0);
        let ram_threshold = rules.iter()
            .find(|r| r.metric == "ram_usage")
            .map(|r| r.threshold)
            .unwrap_or(90.0);
        let rule_priority = rules.first()
            .map(|r| r.priority.as_str())
            .unwrap_or("medium");

        let hosts = match HostService::list(&db) {
            Ok(h) => h,
            Err(_) => continue,
        };

        let online_hosts: Vec<_> = hosts.iter()
            .filter(|h| h.cluster_id == cluster.id)
            .filter(|h| h.status == "online" && !h.maintenance_mode)
            .collect();

        if online_hosts.len() < 2 { continue; }

        // Calculate utilization scores
        let scores: Vec<f64> = online_hosts.iter()
            .map(|h| Scheduler::host_utilization(h))
            .collect();

        let mean = scores.iter().sum::<f64>() / scores.len() as f64;
        let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / scores.len() as f64;
        let std_dev = variance.sqrt();

        if std_dev < DEFAULT_IMBALANCE_THRESHOLD { continue; }

        // Find overloaded hosts based on configured rules
        for (i, host) in online_hosts.iter().enumerate() {
            if scores[i] <= 0.7 { continue; }

            let cpu_overloaded = host.cpu_usage_pct > cpu_threshold;
            let ram_pct = if host.total_ram_mb > 0 {
                (1.0 - host.free_ram_mb as f64 / host.total_ram_mb as f64) * 100.0
            } else { 0.0 };
            let ram_overloaded = ram_pct > ram_threshold;

            if !cpu_overloaded && !ram_overloaded { continue; }

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

            // Load DRS exclusions for this cluster
            let exclusions: Vec<String> = db.prepare(
                "SELECT target_id FROM drs_exclusions WHERE cluster_id = ?1"
            ).ok().map(|mut stmt| {
                stmt.query_map(rusqlite::params![&cluster.id], |r| r.get::<_, String>(0))
                    .ok().map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default()
            }).unwrap_or_default();

            // Pick smallest running VM that would fit — exclude DRS-excluded VMs
            let candidate = vms.iter()
                .filter(|v| v.state == "running")
                .filter(|v| v.ram_mb as i64 <= target_host.free_ram_mb)
                .filter(|v| !exclusions.contains(&v.id))  // Skip excluded VMs
                .filter(|v| !v.resource_group_id.map(|rg| exclusions.contains(&rg.to_string())).unwrap_or(false))  // Skip excluded resource groups
                .min_by_key(|v| v.ram_mb);

            if let Some(vm) = candidate {
                let reason = if cpu_overloaded {
                    format!("Host '{}' CPU at {:.0}% (threshold: {:.0}%), Host '{}' at {:.0}%",
                        host.hostname, host.cpu_usage_pct, cpu_threshold,
                        target_host.hostname, target_host.cpu_usage_pct)
                } else {
                    format!("Host '{}' RAM at {:.0}% (threshold: {:.0}%), Host '{}' has {}MB free",
                        host.hostname, ram_pct, ram_threshold,
                        target_host.hostname, target_host.free_ram_mb)
                };

                // Expire old pending recommendations for this VM
                let _ = db.execute(
                    "UPDATE drs_recommendations SET status = 'expired' WHERE vm_id = ?1 AND status = 'pending'",
                    rusqlite::params![&vm.id],
                );

                let _ = db.execute(
                    "INSERT INTO drs_recommendations (cluster_id, vm_id, source_host_id, target_host_id, reason, priority) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![&cluster.id, &vm.id, &host.id, &target_host.id, &reason, rule_priority],
                );

                tracing::info!("DRS: Recommend moving VM '{}' from '{}' to '{}': {}",
                    vm.name, host.hostname, target_host.hostname, reason);
            }
        }
    }
}
