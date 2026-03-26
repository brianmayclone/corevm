//! Volume service — all database operations for volumes.

use rusqlite::Connection;

pub struct VolumeService;

#[derive(Debug, Clone)]
pub struct VolumeInfo {
    pub id: String,
    pub name: String,
    pub ftt: u32,
    pub local_raid: String,
    pub chunk_size_bytes: u64,
    pub status: String,
    pub created_at: String,
}

impl VolumeService {
    pub fn create(db: &Connection, id: &str, name: &str, ftt: u32, chunk_size: u64, local_raid: &str) -> Result<(), String> {
        db.execute(
            "INSERT INTO volumes (id, name, ftt, chunk_size_bytes, local_raid, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'online')",
            rusqlite::params![id, name, ftt, chunk_size, local_raid],
        ).map_err(|e| format!("Failed to create volume: {}", e))?;
        Ok(())
    }

    pub fn get(db: &Connection, id: &str) -> Option<VolumeInfo> {
        db.query_row(
            "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at FROM volumes WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok(VolumeInfo {
                id: row.get(0)?, name: row.get(1)?, ftt: row.get(2)?,
                local_raid: row.get(3)?, chunk_size_bytes: row.get(4)?,
                status: row.get(5)?, created_at: row.get(6)?,
            }),
        ).ok()
    }

    pub fn list(db: &Connection) -> Vec<VolumeInfo> {
        let mut stmt = db.prepare(
            "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at FROM volumes ORDER BY name"
        ).unwrap();
        stmt.query_map([], |row| Ok(VolumeInfo {
            id: row.get(0)?, name: row.get(1)?, ftt: row.get(2)?,
            local_raid: row.get(3)?, chunk_size_bytes: row.get(4)?,
            status: row.get(5)?, created_at: row.get(6)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn list_online(db: &Connection) -> Vec<VolumeInfo> {
        let mut stmt = db.prepare(
            "SELECT id, name, ftt, local_raid, chunk_size_bytes, status, created_at FROM volumes WHERE status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| Ok(VolumeInfo {
            id: row.get(0)?, name: row.get(1)?, ftt: row.get(2)?,
            local_raid: row.get(3)?, chunk_size_bytes: row.get(4)?,
            status: row.get(5)?, created_at: row.get(6)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn delete(db: &Connection, id: &str) -> Result<(), String> {
        db.execute("DELETE FROM volumes WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("Delete failed: {}", e))?;
        Ok(())
    }

    pub fn update_ftt(db: &Connection, id: &str, ftt: u32) {
        db.execute("UPDATE volumes SET ftt = ?1 WHERE id = ?2", rusqlite::params![ftt, id]).ok();
    }

    pub fn update_local_raid(db: &Connection, id: &str, raid: &str) {
        db.execute("UPDATE volumes SET local_raid = ?1 WHERE id = ?2", rusqlite::params![raid, id]).ok();
    }

    pub fn update_status(db: &Connection, id: &str, status: &str) {
        db.execute("UPDATE volumes SET status = ?1 WHERE id = ?2", rusqlite::params![status, id]).ok();
    }

    pub fn exists(db: &Connection, id: &str) -> bool {
        db.query_row("SELECT COUNT(*) FROM volumes WHERE id = ?1", rusqlite::params![id], |row| row.get::<_, i64>(0))
            .map(|c| c > 0).unwrap_or(false)
    }

    pub fn file_count(db: &Connection, volume_id: &str) -> i64 {
        db.query_row("SELECT COUNT(*) FROM file_map WHERE volume_id = ?1", rusqlite::params![volume_id], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn list_as_json(db: &Connection) -> Vec<serde_json::Value> {
        let mut stmt = db.prepare(
            "SELECT id, name, ftt, chunk_size_bytes, local_raid FROM volumes WHERE status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "ftt": row.get::<_, u32>(2)?,
                "chunk_size_bytes": row.get::<_, u64>(3)?,
                "local_raid": row.get::<_, String>(4)?,
            }))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }
}
