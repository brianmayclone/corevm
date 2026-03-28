//! Disk service — all database operations for claimed_disks.

use rusqlite::Connection;

pub struct DiskService;

#[derive(Debug, Clone)]
pub struct ClaimedDiskInfo {
    pub id: String,
    pub device_path: String,
    pub device_uuid: String,
    pub mount_path: String,
    pub fs_type: String,
    pub model: String,
    pub serial: String,
    pub size_bytes: u64,
    pub status: String,
    pub backend_id: String,
}

impl DiskService {
    pub fn create(db: &Connection, id: &str, device_path: &str, mount_path: &str, fs_type: &str,
                  model: &str, serial: &str, size_bytes: u64) -> Result<(), String> {
        db.execute(
            "INSERT INTO claimed_disks (id, device_path, mount_path, fs_type, model, serial, size_bytes, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'formatting')",
            rusqlite::params![id, device_path, mount_path, fs_type, model, serial, size_bytes],
        ).map_err(|e| format!("{}", e))?;
        Ok(())
    }

    pub fn set_mounted(db: &Connection, id: &str, device_uuid: &str, backend_id: &str) {
        log_err!(db.execute(
            "UPDATE claimed_disks SET device_uuid = ?1, status = 'mounted', backend_id = ?2 WHERE id = ?3",
            rusqlite::params![device_uuid, backend_id, id],
        ), "DiskService::set_mounted");
    }

    pub fn set_error(db: &Connection, id: &str) {
        log_err!(db.execute("UPDATE claimed_disks SET status = 'error' WHERE id = ?1",
            rusqlite::params![id]), "DiskService::set_error");
    }

    pub fn set_released(db: &Connection, id: &str) {
        log_err!(db.execute("UPDATE claimed_disks SET status = 'released' WHERE id = ?1",
            rusqlite::params![id]), "DiskService::set_released");
    }

    pub fn get_mounted(db: &Connection, device_path: &str) -> Option<ClaimedDiskInfo> {
        db.query_row(
            "SELECT id, device_path, device_uuid, mount_path, fs_type, model, serial, size_bytes, status, backend_id
             FROM claimed_disks WHERE device_path = ?1 AND status = 'mounted'",
            rusqlite::params![device_path],
            |row| Ok(ClaimedDiskInfo {
                id: row.get(0)?, device_path: row.get(1)?, device_uuid: row.get(2)?,
                mount_path: row.get(3)?, fs_type: row.get(4)?, model: row.get(5)?,
                serial: row.get(6)?, size_bytes: row.get(7)?, status: row.get(8)?,
                backend_id: row.get(9)?,
            }),
        ).ok()
    }

    pub fn list_claimed(db: &Connection) -> Vec<(String, String, String)> {
        let mut stmt = db.prepare(
            "SELECT id, device_path, mount_path FROM claimed_disks WHERE status != 'released'"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn list_mounted_paths(db: &Connection) -> Vec<String> {
        let mut stmt = db.prepare(
            "SELECT mount_path FROM claimed_disks WHERE status = 'mounted'"
        ).unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn count_available(db: &Connection) -> u32 {
        // This is a rough count — actual discovery uses lsblk
        db.query_row(
            "SELECT COUNT(*) FROM claimed_disks WHERE status = 'mounted'",
            [], |row| row.get(0),
        ).unwrap_or(0)
    }
}
