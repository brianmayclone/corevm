//! File service — all database operations for file_map and write leases.
//!
//! This is the ONLY module that writes to the file_map table.
//! All engines, API handlers, and FUSE operations go through this service.

use rusqlite::Connection;
use crate::db::{DbResult, DbContext, db_transaction};

pub struct FileService;

impl FileService {
    pub fn get_id(db: &Connection, volume_id: &str, rel_path: &str) -> Option<i64> {
        db.query_row(
            "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![volume_id, rel_path], |row| row.get(0),
        ).ok()
    }

    pub fn create(db: &Connection, volume_id: &str, rel_path: &str) -> DbResult<i64> {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT OR IGNORE INTO file_map (volume_id, rel_path, size_bytes, version, created_at, updated_at)
             VALUES (?1, ?2, 0, 0, ?3, ?3)",
            rusqlite::params![volume_id, rel_path, &now],
        ).ctx("FileService::create INSERT")?;

        db.query_row(
            "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
            rusqlite::params![volume_id, rel_path], |row| row.get(0),
        ).ctx("FileService::create SELECT id")
    }

    pub fn get_size(db: &Connection, file_id: i64) -> u64 {
        db.query_row("SELECT size_bytes FROM file_map WHERE id = ?1",
            rusqlite::params![file_id], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn update_size(db: &Connection, file_id: i64, new_size: u64) -> DbResult<()> {
        db.execute(
            "UPDATE file_map SET size_bytes = MAX(size_bytes, ?1), updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![new_size as i64, file_id],
        ).ctx("FileService::update_size")?;
        Ok(())
    }

    pub fn increment_version(db: &Connection, file_id: i64) -> DbResult<()> {
        db.execute(
            "UPDATE file_map SET version = version + 1, updated_at = datetime('now') WHERE id = ?1",
            rusqlite::params![file_id],
        ).ctx("FileService::increment_version")?;
        Ok(())
    }

    pub fn update_protection_status(db: &Connection, file_id: i64, status: &str) -> DbResult<()> {
        db.execute(
            "UPDATE file_map SET protection_status = ?1 WHERE id = ?2",
            rusqlite::params![status, file_id],
        ).ctx("FileService::update_protection_status")?;
        Ok(())
    }

    /// Delete a file and all its chunks/replicas atomically.
    pub fn delete(db: &Connection, volume_id: &str, rel_path: &str) -> DbResult<()> {
        db_transaction(db, "FileService::delete", || {
            if let Some(fid) = Self::get_id(db, volume_id, rel_path) {
                db.execute("DELETE FROM integrity_log WHERE file_id = ?1", rusqlite::params![fid])
                    .ctx("delete: integrity_log")?;
                db.execute("DELETE FROM chunk_replicas WHERE chunk_id IN (SELECT id FROM file_chunks WHERE file_id = ?1)",
                    rusqlite::params![fid])
                    .ctx("delete: chunk_replicas")?;
                db.execute("DELETE FROM file_chunks WHERE file_id = ?1", rusqlite::params![fid])
                    .ctx("delete: file_chunks")?;
                db.execute("DELETE FROM file_replicas WHERE file_id = ?1", rusqlite::params![fid])
                    .ctx("delete: file_replicas")?;
                db.execute("DELETE FROM write_log WHERE file_id = ?1", rusqlite::params![fid])
                    .ctx("delete: write_log")?;
            }
            db.execute(
                "DELETE FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![volume_id, rel_path],
            ).ctx("delete: file_map")?;
            Ok(())
        })
    }

    /// Sync file metadata from a peer (leader or writer). Idempotent upsert.
    pub fn sync_metadata(
        db: &Connection,
        volume_id: &str,
        rel_path: &str,
        size_bytes: u64,
        sha256: &str,
        version: i64,
        chunk_count: u32,
        chunk_size_bytes: u64,
    ) -> DbResult<i64> {
        let now = chrono::Utc::now().to_rfc3339();

        db_transaction(db, "FileService::sync_metadata", || {
            db.execute(
                "INSERT INTO file_map (volume_id, rel_path, size_bytes, sha256, version, chunk_count, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(volume_id, rel_path) DO UPDATE SET
                    size_bytes = CASE WHEN excluded.version > file_map.version THEN excluded.size_bytes ELSE file_map.size_bytes END,
                    sha256 = CASE WHEN excluded.version > file_map.version THEN excluded.sha256 ELSE file_map.sha256 END,
                    version = MAX(file_map.version, excluded.version),
                    chunk_count = CASE WHEN excluded.version > file_map.version THEN excluded.chunk_count ELSE MAX(file_map.chunk_count, excluded.chunk_count) END,
                    updated_at = CASE WHEN excluded.version > file_map.version THEN excluded.updated_at ELSE file_map.updated_at END",
                rusqlite::params![volume_id, rel_path, size_bytes, sha256, version, chunk_count, &now],
            ).ctx("sync_metadata: UPSERT file_map")?;

            let local_file_id: i64 = db.query_row(
                "SELECT id FROM file_map WHERE volume_id = ?1 AND rel_path = ?2",
                rusqlite::params![volume_id, rel_path], |row| row.get(0),
            ).ctx("sync_metadata: get file_id")?;

            for ci in 0..chunk_count {
                let offset = ci as u64 * chunk_size_bytes;
                let size = chunk_size_bytes.min(size_bytes.saturating_sub(offset));
                db.execute(
                    "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![local_file_id, ci, offset, size],
                ).ctx("sync_metadata: INSERT file_chunks")?;
            }

            Ok(local_file_id)
        })
    }

    // ── Write Leases ─────────────────────────────────────────────────

    pub fn acquire_lease(db: &Connection, volume_id: &str, rel_path: &str, node_id: &str, quorum: crate::state::QuorumStatus) -> Result<i64, String> {
        if quorum == crate::state::QuorumStatus::Fenced || quorum == crate::state::QuorumStatus::Sanitizing {
            return Err("node is fenced or sanitizing".into());
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
                    ).map_err(|e| format!("lease update: {}", e))?;
                    Ok(version)
                } else {
                    Err(format!("File owned by node {}", owner))
                }
            }
            Err(_) => Ok(0)
        }
    }

    pub fn release_lease(db: &Connection, volume_id: &str, rel_path: &str, node_id: &str) {
        log_err!(db.execute(
            "UPDATE file_map SET write_owner = '', write_lease_until = '' WHERE volume_id = ?1 AND rel_path = ?2 AND write_owner = ?3",
            rusqlite::params![volume_id, rel_path, node_id],
        ), "FileService::release_lease");
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

    /// Get all files with their FTT for protection status updates.
    pub fn list_files_with_ftt(db: &Connection) -> Vec<(i64, String, u32)> {
        let mut stmt = db.prepare(
            "SELECT fm.id, fm.volume_id, v.ftt FROM file_map fm JOIN volumes v ON v.id = fm.volume_id WHERE fm.chunk_count > 0"
        ).unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    }
}
