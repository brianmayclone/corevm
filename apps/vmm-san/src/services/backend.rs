//! Backend service — all database operations for backends (claimed disk mounts).

use rusqlite::Connection;

pub struct BackendService;

#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub id: String,
    pub node_id: String,
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub status: String,
    pub claimed_disk_id: String,
    pub last_check: Option<String>,
}

impl BackendService {
    pub fn create(db: &Connection, id: &str, node_id: &str, path: &str, total: u64, free: u64, disk_id: &str) -> Result<(), String> {
        db.execute(
            "INSERT INTO backends (id, node_id, path, total_bytes, free_bytes, status, last_check, claimed_disk_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'online', datetime('now'), ?6)",
            rusqlite::params![id, node_id, path, total, free, disk_id],
        ).map_err(|e| format!("{}", e))?;
        Ok(())
    }

    pub fn list_for_node(db: &Connection, node_id: &str) -> Vec<BackendInfo> {
        let mut stmt = db.prepare(
            "SELECT id, node_id, path, total_bytes, free_bytes, status, claimed_disk_id, last_check
             FROM backends WHERE node_id = ?1 ORDER BY path"
        ).unwrap();
        stmt.query_map(rusqlite::params![node_id], |row| Ok(BackendInfo {
            id: row.get(0)?, node_id: row.get(1)?, path: row.get(2)?,
            total_bytes: row.get(3)?, free_bytes: row.get(4)?, status: row.get(5)?,
            claimed_disk_id: row.get(6)?, last_check: row.get(7)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn list_online(db: &Connection) -> Vec<BackendInfo> {
        let mut stmt = db.prepare(
            "SELECT id, node_id, path, total_bytes, free_bytes, status, claimed_disk_id, last_check
             FROM backends WHERE status = 'online' ORDER BY free_bytes DESC"
        ).unwrap();
        stmt.query_map([], |row| Ok(BackendInfo {
            id: row.get(0)?, node_id: row.get(1)?, path: row.get(2)?,
            total_bytes: row.get(3)?, free_bytes: row.get(4)?, status: row.get(5)?,
            claimed_disk_id: row.get(6)?, last_check: row.get(7)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn list_online_for_node(db: &Connection, node_id: &str) -> Vec<BackendInfo> {
        let mut stmt = db.prepare(
            "SELECT id, node_id, path, total_bytes, free_bytes, status, claimed_disk_id, last_check
             FROM backends WHERE node_id = ?1 AND status = 'online' ORDER BY free_bytes DESC"
        ).unwrap();
        stmt.query_map(rusqlite::params![node_id], |row| Ok(BackendInfo {
            id: row.get(0)?, node_id: row.get(1)?, path: row.get(2)?,
            total_bytes: row.get(3)?, free_bytes: row.get(4)?, status: row.get(5)?,
            claimed_disk_id: row.get(6)?, last_check: row.get(7)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn list_paths_for_node(db: &Connection, node_id: &str) -> Vec<String> {
        let mut stmt = db.prepare(
            "SELECT path FROM backends WHERE node_id = ?1 AND status = 'online'"
        ).unwrap();
        stmt.query_map(rusqlite::params![node_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn count_online(db: &Connection) -> u32 {
        db.query_row("SELECT COUNT(*) FROM backends WHERE status = 'online'", [], |row| row.get(0)).unwrap_or(0)
    }

    pub fn count_online_for_node(db: &Connection, node_id: &str) -> u32 {
        db.query_row(
            "SELECT COUNT(*) FROM backends WHERE node_id = ?1 AND status = 'online'",
            rusqlite::params![node_id], |row| row.get(0),
        ).unwrap_or(0)
    }

    pub fn total_capacity(db: &Connection) -> (u64, u64) {
        let total: u64 = db.query_row(
            "SELECT COALESCE(SUM(total_bytes), 0) FROM backends WHERE status = 'online'",
            [], |row| row.get(0),
        ).unwrap_or(0);
        let free: u64 = db.query_row(
            "SELECT COALESCE(SUM(free_bytes), 0) FROM backends WHERE status = 'online'",
            [], |row| row.get(0),
        ).unwrap_or(0);
        (total, free)
    }

    pub fn update_stats(db: &Connection, id: &str, total: u64, free: u64, status: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        log_err!(db.execute(
            "UPDATE backends SET total_bytes = ?1, free_bytes = ?2, status = ?3, last_check = ?4 WHERE id = ?5",
            rusqlite::params![total, free, status, &now, id],
        ), "BackendService::update_stats");
    }

    pub fn set_status(db: &Connection, id: &str, status: &str) {
        log_err!(db.execute("UPDATE backends SET status = ?1 WHERE id = ?2", rusqlite::params![status, id]), "BackendService::set_status");
    }

    pub fn delete(db: &Connection, id: &str) {
        log_err!(db.execute("DELETE FROM backends WHERE id = ?1", rusqlite::params![id]), "BackendService::delete");
    }

    pub fn get_best_for_node(db: &Connection, node_id: &str) -> Option<(String, String)> {
        db.query_row(
            "SELECT id, path FROM backends WHERE node_id = ?1 AND status = 'online' ORDER BY free_bytes DESC LIMIT 1",
            rusqlite::params![node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok()
    }
}
