//! VM management service — the cluster is the AUTHORITY for all VMs.
//!
//! Unlike vmm-server where VMs live locally, the cluster owns all VM state.
//! VMs are created in the cluster DB first, then provisioned on nodes.

use rusqlite::Connection;
use serde::Serialize;

pub struct VmService;

#[derive(Debug, Serialize, Clone)]
pub struct ClusterVm {
    pub id: String,
    pub name: String,
    pub description: String,
    pub cluster_id: String,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub state: String,
    pub guest_os: String,
    pub ram_mb: u32,
    pub cpu_cores: u32,
    pub ha_protected: bool,
    pub ha_restart_priority: String,
    pub drs_automation: String,
    pub resource_group_id: Option<i64>,
    pub owner_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

impl VmService {
    /// List all VMs across the entire cluster.
    pub fn list(db: &Connection) -> Result<Vec<ClusterVm>, String> {
        let mut stmt = db.prepare(
            "SELECT v.id, v.name, v.description, v.cluster_id, v.host_id,
                    h.hostname, v.state, v.config_json,
                    v.ha_protected, v.ha_restart_priority, v.drs_automation,
                    v.resource_group_id, v.owner_id, v.created_at, v.updated_at
             FROM vms v
             LEFT JOIN hosts h ON v.host_id = h.id
             ORDER BY v.name"
        ).map_err(|e| e.to_string())?;

        let vms = stmt.query_map([], |row| {
            let config_json: String = row.get(7)?;
            let config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or_default();
            Ok(ClusterVm {
                id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                cluster_id: row.get(3)?, host_id: row.get(4)?,
                host_name: row.get(5)?, state: row.get(6)?,
                guest_os: config.get("guest_os").and_then(|v| v.as_str()).unwrap_or("other").to_string(),
                ram_mb: config.get("ram_mb").and_then(|v| v.as_u64()).unwrap_or(256) as u32,
                cpu_cores: config.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                ha_protected: row.get::<_, i32>(8)? != 0,
                ha_restart_priority: row.get(9)?,
                drs_automation: row.get(10)?,
                resource_group_id: row.get(11)?,
                owner_id: row.get(12)?,
                created_at: row.get(13)?, updated_at: row.get(14)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(vms)
    }

    /// Get a single VM by ID.
    pub fn get(db: &Connection, id: &str) -> Result<ClusterVm, String> {
        db.query_row(
            "SELECT v.id, v.name, v.description, v.cluster_id, v.host_id,
                    h.hostname, v.state, v.config_json,
                    v.ha_protected, v.ha_restart_priority, v.drs_automation,
                    v.resource_group_id, v.owner_id, v.created_at, v.updated_at
             FROM vms v
             LEFT JOIN hosts h ON v.host_id = h.id
             WHERE v.id = ?1",
            rusqlite::params![id],
            |row| {
                let config_json: String = row.get(7)?;
                let config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or_default();
                Ok(ClusterVm {
                    id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                    cluster_id: row.get(3)?, host_id: row.get(4)?,
                    host_name: row.get(5)?, state: row.get(6)?,
                    guest_os: config.get("guest_os").and_then(|v| v.as_str()).unwrap_or("other").to_string(),
                    ram_mb: config.get("ram_mb").and_then(|v| v.as_u64()).unwrap_or(256) as u32,
                    cpu_cores: config.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                    ha_protected: row.get::<_, i32>(8)? != 0,
                    ha_restart_priority: row.get(9)?,
                    drs_automation: row.get(10)?,
                    resource_group_id: row.get(11)?,
                    owner_id: row.get(12)?,
                    created_at: row.get(13)?, updated_at: row.get(14)?,
                })
            },
        ).map_err(|_| "VM not found".to_string())
    }

    /// Get the full VM config JSON.
    pub fn get_config(db: &Connection, id: &str) -> Result<serde_json::Value, String> {
        let json: String = db.query_row(
            "SELECT config_json FROM vms WHERE id = ?1",
            rusqlite::params![id], |row| row.get(0),
        ).map_err(|_| "VM not found".to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    /// Create a VM in the cluster DB. This does NOT provision it on a node yet.
    pub fn create(db: &Connection, id: &str, name: &str, description: &str,
                  cluster_id: &str, config_json: &str, owner_id: i64) -> Result<(), String> {
        db.execute(
            "INSERT INTO vms (id, name, description, cluster_id, config_json, owner_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, name, description, cluster_id, config_json, owner_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Assign a VM to a host (after provisioning on the node).
    pub fn assign_host(db: &Connection, vm_id: &str, host_id: &str) -> Result<(), String> {
        db.execute(
            "UPDATE vms SET host_id = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![host_id, vm_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Update VM state (synced from node heartbeat or after commands).
    pub fn update_state(db: &Connection, vm_id: &str, state: &str) -> Result<(), String> {
        db.execute(
            "UPDATE vms SET state = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![state, vm_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Update VM config (cluster authority pushes new config to node).
    pub fn update_config(db: &Connection, vm_id: &str, config_json: &str) -> Result<(), String> {
        db.execute(
            "UPDATE vms SET config_json = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![config_json, vm_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Delete a VM from the cluster DB.
    pub fn delete(db: &Connection, vm_id: &str) -> Result<(), String> {
        let affected = db.execute("DELETE FROM vms WHERE id = ?1", rusqlite::params![vm_id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("VM not found".into()) } else { Ok(()) }
    }

    /// List VMs on a specific host (for heartbeat reconciliation and HA).
    pub fn list_by_host(db: &Connection, host_id: &str) -> Result<Vec<ClusterVm>, String> {
        let mut stmt = db.prepare(
            "SELECT v.id, v.name, v.description, v.cluster_id, v.host_id,
                    h.hostname, v.state, v.config_json,
                    v.ha_protected, v.ha_restart_priority, v.drs_automation,
                    v.resource_group_id, v.owner_id, v.created_at, v.updated_at
             FROM vms v
             LEFT JOIN hosts h ON v.host_id = h.id
             WHERE v.host_id = ?1
             ORDER BY v.name"
        ).map_err(|e| e.to_string())?;

        let vms = stmt.query_map(rusqlite::params![host_id], |row| {
            let config_json: String = row.get(7)?;
            let config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or_default();
            Ok(ClusterVm {
                id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                cluster_id: row.get(3)?, host_id: row.get(4)?,
                host_name: row.get(5)?, state: row.get(6)?,
                guest_os: config.get("guest_os").and_then(|v| v.as_str()).unwrap_or("other").to_string(),
                ram_mb: config.get("ram_mb").and_then(|v| v.as_u64()).unwrap_or(256) as u32,
                cpu_cores: config.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                ha_protected: row.get::<_, i32>(8)? != 0,
                ha_restart_priority: row.get(9)?,
                drs_automation: row.get(10)?,
                resource_group_id: row.get(11)?,
                owner_id: row.get(12)?,
                created_at: row.get(13)?, updated_at: row.get(14)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(vms)
    }

    /// List HA-protected VMs on a specific host (for HA engine).
    pub fn list_ha_protected_by_host(db: &Connection, host_id: &str) -> Result<Vec<ClusterVm>, String> {
        let all = Self::list_by_host(db, host_id)?;
        Ok(all.into_iter().filter(|vm| vm.ha_protected && vm.state == "running").collect())
    }
}
