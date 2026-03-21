//! Host management service — registration, status tracking, maintenance mode.
//!
//! The cluster is the authority. Hosts are registered by the cluster admin,
//! and the cluster pushes configuration to them.

use rusqlite::Connection;
use serde::Serialize;

pub struct HostService;

#[derive(Debug, Serialize, Clone)]
pub struct HostInfo {
    pub id: String,
    pub hostname: String,
    pub address: String,
    pub cluster_id: String,
    pub cpu_model: String,
    pub cpu_cores: i32,
    pub cpu_threads: i32,
    pub total_ram_mb: i64,
    pub free_ram_mb: i64,
    pub cpu_usage_pct: f64,
    pub hw_virtualization: bool,
    pub status: String,
    pub maintenance_mode: bool,
    pub connection_state: String,
    pub last_heartbeat: Option<String>,
    pub version: String,
    pub vm_count: i64,
    pub registered_at: String,
}

impl HostService {
    pub fn list(db: &Connection) -> Result<Vec<HostInfo>, String> {
        let mut stmt = db.prepare(
            "SELECT h.id, h.hostname, h.address, h.cluster_id,
                    h.cpu_model, h.cpu_cores, h.cpu_threads, h.total_ram_mb,
                    h.free_ram_mb, h.cpu_usage_pct, h.hw_virtualization,
                    h.status, h.maintenance_mode, h.connection_state,
                    h.last_heartbeat, h.version, h.registered_at,
                    (SELECT COUNT(*) FROM vms v WHERE v.host_id = h.id) as vm_count
             FROM hosts h ORDER BY h.hostname"
        ).map_err(|e| e.to_string())?;

        let hosts = stmt.query_map([], |row| {
            Ok(HostInfo {
                id: row.get(0)?, hostname: row.get(1)?, address: row.get(2)?,
                cluster_id: row.get(3)?, cpu_model: row.get(4)?,
                cpu_cores: row.get(5)?, cpu_threads: row.get(6)?,
                total_ram_mb: row.get(7)?, free_ram_mb: row.get(8)?,
                cpu_usage_pct: row.get(9)?, hw_virtualization: row.get::<_, i32>(10)? != 0,
                status: row.get(11)?, maintenance_mode: row.get::<_, i32>(12)? != 0,
                connection_state: row.get(13)?, last_heartbeat: row.get(14)?,
                version: row.get(15)?, registered_at: row.get(16)?,
                vm_count: row.get(17)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(hosts)
    }

    pub fn get(db: &Connection, id: &str) -> Result<HostInfo, String> {
        db.query_row(
            "SELECT h.id, h.hostname, h.address, h.cluster_id,
                    h.cpu_model, h.cpu_cores, h.cpu_threads, h.total_ram_mb,
                    h.free_ram_mb, h.cpu_usage_pct, h.hw_virtualization,
                    h.status, h.maintenance_mode, h.connection_state,
                    h.last_heartbeat, h.version, h.registered_at,
                    (SELECT COUNT(*) FROM vms v WHERE v.host_id = h.id)
             FROM hosts h WHERE h.id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(HostInfo {
                    id: row.get(0)?, hostname: row.get(1)?, address: row.get(2)?,
                    cluster_id: row.get(3)?, cpu_model: row.get(4)?,
                    cpu_cores: row.get(5)?, cpu_threads: row.get(6)?,
                    total_ram_mb: row.get(7)?, free_ram_mb: row.get(8)?,
                    cpu_usage_pct: row.get(9)?, hw_virtualization: row.get::<_, i32>(10)? != 0,
                    status: row.get(11)?, maintenance_mode: row.get::<_, i32>(12)? != 0,
                    connection_state: row.get(13)?, last_heartbeat: row.get(14)?,
                    version: row.get(15)?, registered_at: row.get(16)?,
                    vm_count: row.get(17)?,
                })
            },
        ).map_err(|_| "Host not found".to_string())
    }

    /// Insert a new host record after successful registration.
    pub fn insert(db: &Connection, id: &str, hostname: &str, address: &str,
                  cluster_id: &str, agent_token: &str, version: &str) -> Result<(), String> {
        db.execute(
            "INSERT INTO hosts (id, hostname, address, cluster_id, agent_token, version) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, hostname, address, cluster_id, agent_token, version],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Host already registered".into() }
            else { e.to_string() }
        })?;
        Ok(())
    }

    /// Remove a host record (deregistration).
    pub fn delete(db: &Connection, id: &str) -> Result<(), String> {
        let vm_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM vms WHERE host_id = ?1 AND state != 'stopped'",
            rusqlite::params![id], |row| row.get(0),
        ).unwrap_or(0);
        if vm_count > 0 {
            return Err(format!("Cannot remove host: {} VMs still running", vm_count));
        }

        let affected = db.execute("DELETE FROM hosts WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("Host not found".into()) } else { Ok(()) }
    }

    /// Update host hardware and status from heartbeat data.
    pub fn update_heartbeat(db: &Connection, node_id: &str, status: &vmm_core::cluster::HostStatus) -> Result<(), String> {
        db.execute(
            "UPDATE hosts SET
                cpu_model = ?1, cpu_cores = ?2, cpu_threads = ?3,
                total_ram_mb = ?4, hw_virtualization = ?5,
                free_ram_mb = ?6, cpu_usage_pct = ?7,
                hostname = ?8, version = ?9,
                status = CASE WHEN maintenance_mode = 1 THEN 'maintenance' ELSE 'online' END,
                connection_state = 'connected',
                last_heartbeat = datetime('now')
             WHERE id = ?10",
            rusqlite::params![
                status.hardware.cpu_model,
                status.hardware.cpu_cores,
                status.hardware.cpu_threads,
                status.hardware.total_ram_mb,
                status.hardware.hw_virtualization as i32,
                status.free_ram_mb,
                status.cpu_usage_pct as f64,
                status.hostname,
                status.version,
                node_id,
            ],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Set maintenance mode for a host.
    pub fn set_maintenance(db: &Connection, id: &str, enabled: bool) -> Result<(), String> {
        let status = if enabled { "maintenance" } else { "online" };
        db.execute(
            "UPDATE hosts SET maintenance_mode = ?1, status = ?2, updated_at = datetime('now') WHERE id = ?3",
            rusqlite::params![enabled as i32, status, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Mark a host as offline (missed heartbeats).
    pub fn mark_offline(db: &Connection, id: &str) -> Result<(), String> {
        db.execute(
            "UPDATE hosts SET status = 'offline', connection_state = 'disconnected' WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get the agent token for a host (for RMM communication).
    pub fn get_agent_token(db: &Connection, id: &str) -> Result<String, String> {
        db.query_row(
            "SELECT agent_token FROM hosts WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).map_err(|_| "Host not found".to_string())
    }

    /// Import existing VMs from a newly registered host via Agent API.
    /// Uses VmService for DB operations — no direct DB access here.
    pub async fn import_vms_from_agent(
        state: &std::sync::Arc<crate::state::ClusterState>,
        address: &str,
        agent_token: &str,
        cluster_id: &str,
        node_id: &str,
    ) -> i32 {
        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
        {
            Ok(c) => c,
            Err(_) => return 0,
        };

        let resp = match client.get(format!("{}/agent/vms", address))
            .header("X-Agent-Token", agent_token)
            .send().await
        {
            Ok(r) => r,
            Err(_) => return 0,
        };

        let agent_vms: Vec<vmm_core::cluster::AgentVmStatus> = match resp.json().await {
            Ok(v) => v,
            Err(_) => return 0,
        };

        let mut imported = 0;
        if let Ok(db) = state.db.lock() {
            for agent_vm in &agent_vms {
                // Use VmService to check existence
                if crate::services::vm::VmService::get(&db, &agent_vm.id).is_ok() {
                    continue; // Already known
                }

                let config_json = serde_json::json!({
                    "uuid": agent_vm.id,
                    "name": agent_vm.id,
                    "ram_mb": agent_vm.ram_used_mb,
                    "cpu_cores": 1,
                    "guest_os": "other", "guest_arch": "x64",
                    "disk_images": [], "iso_image": "",
                    "boot_order": "diskfirst", "bios_type": "seabios",
                    "gpu_model": "stdvga", "vram_mb": 16,
                    "nic_model": "e1000", "net_enabled": false,
                    "net_mode": "usermode", "net_host_nic": "",
                    "mac_mode": "dynamic", "mac_address": "",
                    "audio_enabled": false, "usb_tablet": false,
                    "ram_alloc": "ondemand", "diagnostics": false,
                    "disk_cache_mb": 0, "disk_cache_mode": "none",
                }).to_string();

                // Use VmService to create
                if crate::services::vm::VmService::create(
                    &db, &agent_vm.id, &agent_vm.id, "",
                    cluster_id, &config_json, 0,
                ).is_ok() {
                    // Assign to host
                    let _ = crate::services::vm::VmService::assign_host(&db, &agent_vm.id, node_id);
                    let _ = crate::services::vm::VmService::update_state(&db, &agent_vm.id, &agent_vm.state);
                    imported += 1;
                }
            }

            if imported > 0 {
                crate::services::event::EventService::log(&db, "info", "host",
                    &format!("Imported {} VMs from host", imported),
                    Some("host"), Some(node_id), Some(node_id));
            }
        }
        imported
    }
}
