//! Datastore management service — cluster-wide storage authority.
//!
//! Datastores are defined centrally and mounted on all hosts in the cluster.

use rusqlite::Connection;
use serde::Serialize;

pub struct DatastoreService;

#[derive(Debug, Serialize, Clone)]
pub struct DatastoreInfo {
    pub id: String,
    pub name: String,
    pub store_type: String,
    pub mount_source: String,
    pub mount_opts: String,
    pub mount_path: String,
    pub cluster_id: String,
    pub total_bytes: i64,
    pub free_bytes: i64,
    pub status: String,
    pub host_mounts: Vec<DatastoreHostMount>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct DatastoreHostMount {
    pub host_id: String,
    pub hostname: String,
    pub mounted: bool,
    pub mount_status: String,
    pub total_bytes: i64,
    pub free_bytes: i64,
}

impl DatastoreService {
    pub fn list(db: &Connection) -> Result<Vec<DatastoreInfo>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, store_type, mount_source, mount_opts, mount_path, \
                    cluster_id, total_bytes, free_bytes, status, created_at \
             FROM datastores ORDER BY name"
        ).map_err(|e| e.to_string())?;

        let mut datastores: Vec<DatastoreInfo> = stmt.query_map([], |row| {
            Ok(DatastoreInfo {
                id: row.get(0)?, name: row.get(1)?, store_type: row.get(2)?,
                mount_source: row.get(3)?, mount_opts: row.get(4)?,
                mount_path: row.get(5)?, cluster_id: row.get(6)?,
                total_bytes: row.get(7)?, free_bytes: row.get(8)?,
                status: row.get(9)?, created_at: row.get(10)?,
                host_mounts: Vec::new(),
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();

        // Load host mounts for each datastore
        for ds in &mut datastores {
            ds.host_mounts = Self::get_host_mounts(db, &ds.id)?;
        }
        Ok(datastores)
    }

    pub fn get(db: &Connection, id: &str) -> Result<DatastoreInfo, String> {
        let mut ds = db.query_row(
            "SELECT id, name, store_type, mount_source, mount_opts, mount_path, \
                    cluster_id, total_bytes, free_bytes, status, created_at \
             FROM datastores WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(DatastoreInfo {
                    id: row.get(0)?, name: row.get(1)?, store_type: row.get(2)?,
                    mount_source: row.get(3)?, mount_opts: row.get(4)?,
                    mount_path: row.get(5)?, cluster_id: row.get(6)?,
                    total_bytes: row.get(7)?, free_bytes: row.get(8)?,
                    status: row.get(9)?, created_at: row.get(10)?,
                    host_mounts: Vec::new(),
                })
            },
        ).map_err(|_| "Datastore not found".to_string())?;

        ds.host_mounts = Self::get_host_mounts(db, &ds.id)?;
        Ok(ds)
    }

    /// Create a new datastore record. Does NOT mount it on hosts yet — that's done by the engine.
    pub fn create(db: &Connection, name: &str, store_type: &str, mount_source: &str,
                  mount_opts: &str, mount_path: &str, cluster_id: &str) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        db.execute(
            "INSERT INTO datastores (id, name, store_type, mount_source, mount_opts, mount_path, cluster_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![&id, name, store_type, mount_source, mount_opts, mount_path, cluster_id],
        ).map_err(|e| {
            if e.to_string().contains("UNIQUE") { "Datastore name already exists".into() }
            else { e.to_string() }
        })?;
        Ok(id)
    }

    pub fn delete(db: &Connection, id: &str) -> Result<(), String> {
        let disk_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM disk_images WHERE datastore_id = ?1",
            rusqlite::params![id], |row| row.get(0),
        ).unwrap_or(0);
        if disk_count > 0 {
            return Err(format!("Cannot delete datastore: {} disk images still present", disk_count));
        }

        let affected = db.execute("DELETE FROM datastores WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        if affected == 0 { Err("Datastore not found".into()) } else { Ok(()) }
    }

    /// Update datastore properties.
    pub fn update(db: &Connection, id: &str, name: Option<&str>, mount_source: Option<&str>,
                  mount_opts: Option<&str>, mount_path: Option<&str>) -> Result<(), String> {
        if let Some(v) = name {
            db.execute("UPDATE datastores SET name = ?1 WHERE id = ?2", rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = mount_source {
            db.execute("UPDATE datastores SET mount_source = ?1 WHERE id = ?2", rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = mount_opts {
            db.execute("UPDATE datastores SET mount_opts = ?1 WHERE id = ?2", rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        if let Some(v) = mount_path {
            db.execute("UPDATE datastores SET mount_path = ?1 WHERE id = ?2", rusqlite::params![v, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Update datastore status.
    pub fn update_status(db: &Connection, id: &str, status: &str) -> Result<(), String> {
        db.execute(
            "UPDATE datastores SET status = ?1 WHERE id = ?2",
            rusqlite::params![status, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Create a datastore_hosts entry for a specific host.
    pub fn add_host_mount(db: &Connection, datastore_id: &str, host_id: &str) -> Result<(), String> {
        db.execute(
            "INSERT OR REPLACE INTO datastore_hosts (datastore_id, host_id) VALUES (?1, ?2)",
            rusqlite::params![datastore_id, host_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Update mount status for a specific host.
    pub fn update_host_mount(db: &Connection, datastore_id: &str, host_id: &str,
                             mounted: bool, status: &str, total_bytes: i64, free_bytes: i64) -> Result<(), String> {
        db.execute(
            "UPDATE datastore_hosts SET mounted = ?1, mount_status = ?2, \
                    total_bytes = ?3, free_bytes = ?4, last_check = datetime('now') \
             WHERE datastore_id = ?5 AND host_id = ?6",
            rusqlite::params![mounted as i32, status, total_bytes, free_bytes, datastore_id, host_id],
        ).map_err(|e| e.to_string())?;

        // Update aggregate capacity on the datastore
        Self::recalculate_capacity(db, datastore_id)?;
        Ok(())
    }

    fn recalculate_capacity(db: &Connection, datastore_id: &str) -> Result<(), String> {
        // Use the max capacity reported by any host (shared storage = same underlying capacity)
        let (total, free): (i64, i64) = db.query_row(
            "SELECT COALESCE(MAX(total_bytes), 0), COALESCE(MAX(free_bytes), 0) \
             FROM datastore_hosts WHERE datastore_id = ?1 AND mounted = 1",
            rusqlite::params![datastore_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or((0, 0));

        db.execute(
            "UPDATE datastores SET total_bytes = ?1, free_bytes = ?2 WHERE id = ?3",
            rusqlite::params![total, free, datastore_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_host_mounts(db: &Connection, datastore_id: &str) -> Result<Vec<DatastoreHostMount>, String> {
        let mut stmt = db.prepare(
            "SELECT dh.host_id, h.hostname, dh.mounted, dh.mount_status, dh.total_bytes, dh.free_bytes \
             FROM datastore_hosts dh \
             JOIN hosts h ON dh.host_id = h.id \
             WHERE dh.datastore_id = ?1"
        ).map_err(|e| e.to_string())?;

        let mounts = stmt.query_map(rusqlite::params![datastore_id], |row| {
            Ok(DatastoreHostMount {
                host_id: row.get(0)?, hostname: row.get(1)?,
                mounted: row.get::<_, i32>(2)? != 0,
                mount_status: row.get(3)?,
                total_bytes: row.get(4)?, free_bytes: row.get(5)?,
            })
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();
        Ok(mounts)
    }
}
