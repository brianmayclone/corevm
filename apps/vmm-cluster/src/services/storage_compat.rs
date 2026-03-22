//! StorageCompatService — compatibility layer mapping cluster datastores to
//! the vmm-server storage pool API format, so the existing UI works unchanged.

use rusqlite::Connection;
use crate::services::base::BaseService;

pub struct StorageCompatService;

impl StorageCompatService {
    /// List datastores as storage pools. Optionally filtered to only those
    /// mounted on ALL hosts in the given cluster.
    pub fn list_pools(db: &Connection, cluster_id: Option<&str>) -> Result<Vec<serde_json::Value>, String> {
        if let Some(cid) = cluster_id {
            // Show ALL datastores for the cluster (not just fully-mounted ones)
            let mut stmt = db.prepare(
                "SELECT d.id, d.name, d.mount_path, d.store_type, 1, d.mount_source, d.mount_opts,
                        d.total_bytes, d.free_bytes
                 FROM datastores d
                 WHERE d.cluster_id = ?1
                 ORDER BY d.name"
            ).map_err(|e| e.to_string())?;

            let rows = stmt.query_map(rusqlite::params![cid], |row| {
                Self::row_to_pool(row)
            }).map_err(|e| e.to_string())?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        } else {
            let mut stmt = db.prepare(
                "SELECT id, name, mount_path, store_type, 1, mount_source, mount_opts, total_bytes, free_bytes \
                 FROM datastores ORDER BY name"
            ).map_err(|e| e.to_string())?;

            let rows = stmt.query_map([], |row| {
                Self::row_to_pool(row)
            }).map_err(|e| e.to_string())?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        }
    }

    /// Browse files in a datastore (disk images + ISOs from DB).
    pub fn browse(db: &Connection, datastore_id: &str, ext_filter: Option<&str>) -> Result<Vec<serde_json::Value>, String> {
        let mount_path: String = db.query_row(
            "SELECT mount_path FROM datastores WHERE id = ?1",
            rusqlite::params![datastore_id], |r| r.get(0),
        ).map_err(|_| "Datastore not found".to_string())?;

        let mut files = Vec::new();
        let ext = ext_filter.unwrap_or("");

        if ext.is_empty() || ext == ".raw" || ext == ".qcow2" {
            let mut stmt = db.prepare(
                "SELECT name, path, size_bytes FROM disk_images WHERE datastore_id = ?1 ORDER BY name"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(rusqlite::params![datastore_id], |row| {
                let mp = mount_path.clone();
                Ok(serde_json::json!({
                    "name": row.get::<_, String>(0)?,
                    "path": format!("{}/{}", mp, row.get::<_, String>(1)?),
                    "size_bytes": row.get::<_, i64>(2)?,
                    "is_dir": false,
                }))
            }).map_err(|e| e.to_string())?;
            files.extend(rows.filter_map(|r| r.ok()));
        }

        if ext.is_empty() || ext == ".iso" {
            let mut stmt = db.prepare(
                "SELECT name, path, size_bytes FROM isos WHERE datastore_id = ?1 ORDER BY name"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(rusqlite::params![datastore_id], |row| {
                let mp = mount_path.clone();
                Ok(serde_json::json!({
                    "name": row.get::<_, String>(0)?,
                    "path": format!("{}/{}", mp, row.get::<_, String>(1)?),
                    "size_bytes": row.get::<_, i64>(2)?,
                    "is_dir": false,
                }))
            }).map_err(|e| e.to_string())?;
            files.extend(rows.filter_map(|r| r.ok()));
        }

        Ok(files)
    }

    /// List all disk images.
    pub fn list_images(db: &Connection) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = db.prepare(
            "SELECT di.id, di.name, di.path, di.size_bytes, di.format, di.datastore_id, di.vm_id, \
                    v.name, di.created_at \
             FROM disk_images di LEFT JOIN vms v ON di.vm_id = v.id ORDER BY di.name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "path": row.get::<_, String>(2)?,
                "size_bytes": row.get::<_, i64>(3)?,
                "format": row.get::<_, String>(4)?,
                "pool_id": row.get::<_, Option<String>>(5)?,
                "vm_id": row.get::<_, Option<String>>(6)?,
                "vm_name": row.get::<_, Option<String>>(7)?,
                "created_at": row.get::<_, String>(8)?,
            }))
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// List all ISOs.
    pub fn list_isos(db: &Connection) -> Result<Vec<serde_json::Value>, String> {
        let mut stmt = db.prepare(
            "SELECT id, name, path, size_bytes, uploaded_at FROM isos ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "path": row.get::<_, String>(2)?,
                "size_bytes": row.get::<_, i64>(3)?,
                "uploaded_at": row.get::<_, String>(4)?,
            }))
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn row_to_pool(row: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "path": row.get::<_, String>(2)?,
            "pool_type": row.get::<_, String>(3)?,
            "shared": row.get::<_, i32>(4)? != 0,
            "mount_source": row.get::<_, String>(5)?,
            "mount_opts": row.get::<_, String>(6)?,
            "total_bytes": row.get::<_, i64>(7)?,
            "free_bytes": row.get::<_, i64>(8)?,
        }))
    }
}
