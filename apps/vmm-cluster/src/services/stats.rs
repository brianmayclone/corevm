//! StatsService — aggregated cluster statistics.
//!
//! Provides all dashboard metrics. Uses BaseService helpers for common queries.

use rusqlite::Connection;
use serde::Serialize;
use crate::services::base::BaseService;

pub struct StatsService;

#[derive(Debug, Serialize, Clone)]
pub struct ClusterStats {
    pub total_hosts: i64,
    pub online_hosts: i64,
    pub maintenance_hosts: i64,
    pub offline_hosts: i64,
    pub total_vms: i64,
    pub running_vms: i64,
    pub stopped_vms: i64,
    pub total_ram_mb: i64,
    pub used_ram_mb: i64,
    pub total_disk_bytes: i64,
    pub used_disk_bytes: i64,
    pub ha_protected_vms: i64,
}

#[derive(Debug, Serialize)]
pub struct StorageStats {
    pub total_pools: i64,
    pub online_pools: i64,
    pub total_bytes: i64,
    pub used_bytes: i64,
    pub free_bytes: i64,
    pub vm_disk_bytes: i64,
    pub total_images: i64,
    pub total_isos: i64,
    pub orphaned_images: i64,
}

impl StatsService {
    /// Get aggregated cluster statistics for the dashboard.
    pub fn cluster_stats(db: &Connection) -> ClusterStats {
        let total_hosts = BaseService::count(db, "hosts", "", &[]);
        let online_hosts = BaseService::count(db, "hosts", "status = 'online'", &[]);
        let maintenance_hosts = BaseService::count(db, "hosts", "status = 'maintenance'", &[]);
        let offline_hosts = BaseService::count(db, "hosts", "status = 'offline'", &[]);

        let total_vms = BaseService::count(db, "vms", "", &[]);
        let running_vms = BaseService::count(db, "vms", "state = 'running'", &[]);
        let stopped_vms = BaseService::count(db, "vms", "state = 'stopped'", &[]);

        let total_ram_mb = BaseService::sum_i64(db, "hosts", "total_ram_mb", "", &[]);
        let free_ram_mb = BaseService::sum_i64(db, "hosts", "free_ram_mb", "", &[]);

        let total_disk_bytes = BaseService::sum_i64(db, "datastores", "total_bytes", "", &[]);
        let free_disk_bytes = BaseService::sum_i64(db, "datastores", "free_bytes", "", &[]);

        let ha_protected_vms = BaseService::count(db, "vms", "ha_protected = 1", &[]);

        ClusterStats {
            total_hosts, online_hosts, maintenance_hosts, offline_hosts,
            total_vms, running_vms, stopped_vms,
            total_ram_mb, used_ram_mb: total_ram_mb - free_ram_mb,
            total_disk_bytes, used_disk_bytes: total_disk_bytes - free_disk_bytes,
            ha_protected_vms,
        }
    }

    /// Get storage aggregate statistics.
    pub fn storage_stats(db: &Connection) -> StorageStats {
        let total_pools = BaseService::count(db, "datastores", "", &[]);
        let online_pools = BaseService::count(db, "datastores", "status = 'online'", &[]);
        let total_bytes = BaseService::sum_i64(db, "datastores", "total_bytes", "", &[]);
        let free_bytes = BaseService::sum_i64(db, "datastores", "free_bytes", "", &[]);
        let total_images = BaseService::count(db, "disk_images", "", &[]);
        let total_isos = BaseService::count(db, "isos", "", &[]);
        let orphaned_images = BaseService::count(db, "disk_images", "vm_id IS NULL", &[]);

        StorageStats {
            total_pools, online_pools, total_bytes,
            used_bytes: total_bytes - free_bytes, free_bytes,
            vm_disk_bytes: 0, total_images, total_isos, orphaned_images,
        }
    }
}
