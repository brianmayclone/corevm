//! Cluster management service — CRUD for logical clusters.

use rusqlite::Connection;
use serde::Serialize;

pub struct ClusterService;

#[derive(Debug, Serialize)]
pub struct ClusterInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub drs_enabled: bool,
    pub ha_enabled: bool,
    pub ha_vm_restart_priority: String,
    pub ha_admission_control: bool,
    pub ha_failover_hosts: i32,
    pub host_count: i64,
    pub vm_count: i64,
    pub total_ram_mb: i64,
    pub free_ram_mb: i64,
    pub created_at: String,
}

impl ClusterService {
    pub fn list(db: &Connection) -> Result<Vec<ClusterInfo>, String> {
        let mut stmt = db.prepare(
            "SELECT c.id, c.name, c.description, c.drs_enabled, c.ha_enabled,
                    c.ha_vm_restart_priority, c.ha_admission_control, c.ha_failover_hosts,
                    c.created_at,
                    (SELECT COUNT(*) FROM hosts h WHERE h.cluster_id = c.id) as host_count,
                    (SELECT COUNT(*) FROM vms v WHERE v.cluster_id = c.id) as vm_count,
                    COALESCE((SELECT SUM(h2.total_ram_mb) FROM hosts h2 WHERE h2.cluster_id = c.id), 0) as total_ram,
                    COALESCE((SELECT SUM(h3.free_ram_mb) FROM hosts h3 WHERE h3.cluster_id = c.id), 0) as free_ram
             FROM clusters c ORDER BY c.name"
        ).map_err(|e| e.to_string())?;

        let clusters = stmt.query_map([], |row| {
            Ok(ClusterInfo {
                id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                drs_enabled: row.get::<_, i32>(3)? != 0,
                ha_enabled: row.get::<_, i32>(4)? != 0,
                ha_vm_restart_priority: row.get(5)?,
                ha_admission_control: row.get::<_, i32>(6)? != 0,
                ha_failover_hosts: row.get(7)?,
                created_at: row.get(8)?,
                host_count: row.get(9)?, vm_count: row.get(10)?,
                total_ram_mb: row.get(11)?, free_ram_mb: row.get(12)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(clusters)
    }

    pub fn get(db: &Connection, id: &str) -> Result<ClusterInfo, String> {
        db.query_row(
            "SELECT c.id, c.name, c.description, c.drs_enabled, c.ha_enabled,
                    c.ha_vm_restart_priority, c.ha_admission_control, c.ha_failover_hosts,
                    c.created_at,
                    (SELECT COUNT(*) FROM hosts h WHERE h.cluster_id = c.id),
                    (SELECT COUNT(*) FROM vms v WHERE v.cluster_id = c.id),
                    COALESCE((SELECT SUM(h2.total_ram_mb) FROM hosts h2 WHERE h2.cluster_id = c.id), 0),
                    COALESCE((SELECT SUM(h3.free_ram_mb) FROM hosts h3 WHERE h3.cluster_id = c.id), 0)
             FROM clusters c WHERE c.id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(ClusterInfo {
                    id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                    drs_enabled: row.get::<_, i32>(3)? != 0,
                    ha_enabled: row.get::<_, i32>(4)? != 0,
                    ha_vm_restart_priority: row.get(5)?,
                    ha_admission_control: row.get::<_, i32>(6)? != 0,
                    ha_failover_hosts: row.get(7)?,
                    created_at: row.get(8)?,
                    host_count: row.get(9)?, vm_count: row.get(10)?,
                    total_ram_mb: row.get(11)?, free_ram_mb: row.get(12)?,
                })
            },
        ).map_err(|_| "Cluster not found".to_string())
    }

    pub fn create(db: &Connection, name: &str, description: &str) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        db.execute(
            "INSERT INTO clusters (id, name, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![&id, name, description],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Cluster name already exists".into() }
            else { e.to_string() }
        })?;
        Ok(id)
    }

    pub fn update(db: &Connection, id: &str, name: Option<&str>, description: Option<&str>,
                  drs_enabled: Option<bool>, ha_enabled: Option<bool>,
                  ha_vm_restart_priority: Option<&str>, ha_admission_control: Option<bool>,
                  ha_failover_hosts: Option<i32>) -> Result<(), String> {
        if let Some(n) = name {
            db.execute("UPDATE clusters SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![n, id]).map_err(|e| e.to_string())?;
        }
        if let Some(d) = description {
            db.execute("UPDATE clusters SET description = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![d, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = drs_enabled {
            db.execute("UPDATE clusters SET drs_enabled = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![v as i32, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = ha_enabled {
            db.execute("UPDATE clusters SET ha_enabled = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![v as i32, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = ha_vm_restart_priority {
            db.execute("UPDATE clusters SET ha_vm_restart_priority = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = ha_admission_control {
            db.execute("UPDATE clusters SET ha_admission_control = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![v as i32, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = ha_failover_hosts {
            db.execute("UPDATE clusters SET ha_failover_hosts = ?1, updated_at = datetime('now') WHERE id = ?2",
                rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete(db: &Connection, id: &str) -> Result<(), String> {
        // Check for hosts still assigned
        let host_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM hosts WHERE cluster_id = ?1",
            rusqlite::params![id], |row| row.get(0),
        ).unwrap_or(0);
        if host_count > 0 {
            return Err(format!("Cannot delete cluster: {} hosts still assigned", host_count));
        }

        let affected = db.execute("DELETE FROM clusters WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("Cluster not found".into()) } else { Ok(()) }
    }
}
