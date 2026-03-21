//! VM service — CRUD + lifecycle operations.

use rusqlite::Connection;
use vmm_core::config::VmConfig;

pub struct VmService;

#[derive(Debug, serde::Serialize)]
pub struct VmRecord {
    pub id: String,
    pub name: String,
    pub config: VmConfig,
    pub owner_id: i64,
    pub resource_group_id: i64,
    pub created_at: String,
}

impl VmService {
    pub fn list(db: &Connection) -> Result<Vec<VmRecord>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, config_json, owner_id, created_at, resource_group_id FROM vms ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let vms = stmt.query_map([], |row| {
            let config_json: String = row.get(2)?;
            Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, config_json, row.get::<_,i64>(3)?, row.get::<_,String>(4)?, row.get::<_,i64>(5)?))
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .map(|(id, name, config_json, owner_id, created_at, resource_group_id)| {
            let config: VmConfig = serde_json::from_str(&config_json).unwrap_or_default();
            VmRecord { id, name, config, owner_id, resource_group_id, created_at }
        }).collect();
        Ok(vms)
    }

    pub fn get(db: &Connection, vm_id: &str) -> Result<VmRecord, String> {
        db.query_row(
            "SELECT id, name, config_json, owner_id, created_at, resource_group_id FROM vms WHERE id = ?1",
            rusqlite::params![vm_id],
            |row| {
                let config_json: String = row.get(2)?;
                Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, config_json, row.get::<_,i64>(3)?, row.get::<_,String>(4)?, row.get::<_,i64>(5)?))
            },
        ).map(|(id, name, config_json, owner_id, created_at, resource_group_id)| {
            let config: VmConfig = serde_json::from_str(&config_json).unwrap_or_default();
            VmRecord { id, name, config, owner_id, resource_group_id, created_at }
        }).map_err(|_| "VM not found".into())
    }

    pub fn create(db: &Connection, config: &VmConfig, owner_id: i64) -> Result<(), String> {
        let config_json = serde_json::to_string(config)
            .map_err(|e| format!("Serialization error: {}", e))?;
        db.execute(
            "INSERT INTO vms (id, name, config_json, owner_id) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![&config.uuid, &config.name, &config_json, owner_id],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "VM already exists".into() }
            else { e.to_string() }
        })?;
        Ok(())
    }

    pub fn update(db: &Connection, vm_id: &str, config: &VmConfig) -> Result<(), String> {
        let config_json = serde_json::to_string(config)
            .map_err(|e| format!("Serialization error: {}", e))?;
        let affected = db.execute(
            "UPDATE vms SET name = ?1, config_json = ?2, updated_at = datetime('now') WHERE id = ?3",
            rusqlite::params![&config.name, &config_json, vm_id],
        ).map_err(|e| e.to_string())?;
        if affected == 0 { Err("VM not found".into()) } else { Ok(()) }
    }

    pub fn delete(db: &Connection, vm_id: &str) -> Result<(), String> {
        let affected = db.execute("DELETE FROM vms WHERE id = ?1", rusqlite::params![vm_id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("VM not found".into()) } else { Ok(()) }
    }
}
