//! File service — all database operations for file_map, write leases, and protection status.

use rusqlite::Connection;

pub struct FileService;

impl FileService {
    pub fn get_id(db: &Connection, volume_id: &str, rel_path: &str) -> Option<i64> {
        db.query_row(
            "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![volume_id, rel_path], |row| row.get(0),
        ).ok()
    }

    pub fn create(db: &Connection, volume_id: &str, rel_path: &str) -> Option<i64> {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT OR IGNORE INTO file_map (volume_id, rel_path, size_bytes, version, created_at, updated_at)
             VALUES (?1, ?2, 0, 0, ?3, ?3)",
            rusqlite::params![volume_id, rel_path, &now],
        ).ok();
        Self::get_id(db, volume_id, rel_path)
    }

    pub fn get_size(db: &Connection, file_id: i64) -> u64 {
        db.query_row("SELECT size_bytes FROM file_map WHERE id = ?1", rusqlite::params![file_id], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn update_size(db: &Connection, file_id: i64, new_size: u64) {
        db.execute(
            "UPDATE file_map SET size_bytes = MAX(size_bytes, ?1), updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![new_size as i64, file_id],
        ).ok();
    }

    pub fn increment_version(db: &Connection, file_id: i64) {
        db.execute(
            "UPDATE file_map SET version = version + 1, updated_at = datetime('now') WHERE id = ?1",
            rusqlite::params![file_id],
        ).ok();
    }

    pub fn update_protection_status(db: &Connection, file_id: i64, status: &str) {
        db.execute(
            "UPDATE file_map SET protection_status = ?1 WHERE id = ?2",
            rusqlite::params![status, file_id],
        ).ok();
    }

    pub fn delete(db: &Connection, volume_id: &str, rel_path: &str) -> Result<(), String> {
        let file_id = Self::get_id(db, volume_id, rel_path);
        if let Some(fid) = file_id {
            db.execute("DELETE FROM integrity_log WHERE file_id = ?1", rusqlite::params![fid]).ok();
            db.execute("DELETE FROM chunk_replicas WHERE chunk_id IN (SELECT id FROM file_chunks WHERE file_id = ?1)", rusqlite::params![fid]).ok();
            db.execute("DELETE FROM file_chunks WHERE file_id = ?1", rusqlite::params![fid]).ok();
            db.execute("DELETE FROM file_replicas WHERE file_id = ?1", rusqlite::params![fid]).ok();
            db.execute("DELETE FROM write_log WHERE file_id = ?1", rusqlite::params![fid]).ok();
        }
        db.execute(
            "DELETE FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![volume_id, rel_path],
        ).map_err(|e| format!("{}", e))?;
        Ok(())
    }

    /// Acquire or renew a write lease. Returns (acquired: bool, version: i64).
    pub fn acquire_lease(db: &Connection, volume_id: &str, rel_path: &str, node_id: &str, quorum: crate::state::QuorumStatus) -> Result<i64, String> {
        if quorum == crate::state::QuorumStatus::Fenced {
            return Err("node is fenced (no quorum)".into());
        }
        let now = chrono::Utc::now();
        let until = (now + chrono::Duration::seconds(30)).to_rfc3339();
        let now_str = now.to_rfc3339();

        let current = db.query_row(
            "SELECT write_owner, write_lease_until, version FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![volume_id, rel_path],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)),
        );

        match current {
            Ok((owner, lease_until, version)) => {
                if owner.is_empty() || owner == node_id || lease_until < now_str {
                    db.execute(
                        "UPDATE file_map SET write_owner = ?1, write_lease_until = ?2 WHERE volume_id = ?3 AND rel_path = ?4",
                        rusqlite::params![node_id, &until, volume_id, rel_path],
                    ).ok();
                    Ok(version)
                } else {
                    Err(format!("File owned by node {}", owner))
                }
            }
            Err(_) => Ok(0) // File doesn't exist yet
        }
    }

    pub fn release_lease(db: &Connection, volume_id: &str, rel_path: &str, node_id: &str) {
        db.execute(
            "UPDATE file_map SET write_owner = '', write_lease_until = '' WHERE volume_id = ?1 AND rel_path = ?2 AND write_owner = ?3",
            rusqlite::params![volume_id, rel_path, node_id],
        ).ok();
    }

    pub fn release_all_leases_for_node(db: &Connection, node_id: &str) {
        let count = db.execute(
            "UPDATE file_map SET write_owner = '', write_lease_until = '' WHERE write_owner = ?1",
            rusqlite::params![node_id],
        ).unwrap_or(0);
        if count > 0 {
            tracing::info!("Released {} write leases for node {}", count, node_id);
        }
    }

    pub fn list_files_with_ftt(db: &Connection) -> Vec<(i64, String, u32)> {
        let mut stmt = db.prepare(
            "SELECT fm.id, fm.volume_id, v.ftt FROM file_map fm JOIN volumes v ON v.id = fm.volume_id WHERE fm.chunk_count > 0"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    }
}
