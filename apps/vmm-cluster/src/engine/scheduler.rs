//! VM placement scheduler — decides which host a new VM should run on.
//!
//! Uses simple bin-packing: pick the online host with the most free RAM
//! that can accommodate the VM's resource requirements.

use crate::services::host::{HostService, HostInfo};
use rusqlite::Connection;

pub struct Scheduler;

impl Scheduler {
    /// Find the best host for a new VM with given resource requirements.
    /// Returns the host ID, or None if no suitable host is available.
    pub fn select_host(
        db: &Connection,
        cluster_id: &str,
        required_ram_mb: u32,
        required_cpu_cores: u32,
        _preferred_host_id: Option<&str>,
    ) -> Result<Option<String>, String> {
        let hosts = HostService::list(db)?;

        // Filter candidates
        let mut candidates: Vec<&HostInfo> = hosts.iter()
            .filter(|h| h.cluster_id == cluster_id)
            .filter(|h| h.status == "online")
            .filter(|h| !h.maintenance_mode)
            .filter(|h| h.free_ram_mb >= required_ram_mb as i64)
            .filter(|h| h.cpu_cores >= required_cpu_cores as i32)
            .collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        // Sort by most free RAM (greedy bin-packing)
        candidates.sort_by(|a, b| b.free_ram_mb.cmp(&a.free_ram_mb));

        Ok(Some(candidates[0].id.clone()))
    }

    /// Calculate a host's resource utilization score (0.0 = idle, 1.0 = fully loaded).
    pub fn host_utilization(host: &HostInfo) -> f64 {
        let ram_util = if host.total_ram_mb > 0 {
            1.0 - (host.free_ram_mb as f64 / host.total_ram_mb as f64)
        } else {
            0.0
        };
        let cpu_util = host.cpu_usage_pct / 100.0;
        // Weighted average: RAM counts more than CPU
        ram_util * 0.6 + cpu_util * 0.4
    }
}
