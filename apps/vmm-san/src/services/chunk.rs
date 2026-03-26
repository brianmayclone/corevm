//! Chunk service — all database operations for file_chunks and chunk_replicas.

use rusqlite::Connection;

pub struct ChunkService;

impl ChunkService {
    pub fn create_chunk(db: &Connection, file_id: i64, chunk_index: u32, offset: u64, size: u64) -> Option<i64> {
        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![file_id, chunk_index, offset, size],
        ).ok();
        db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, chunk_index], |row| row.get(0),
        ).ok()
    }

    pub fn get_chunk_id(db: &Connection, file_id: i64, chunk_index: u32) -> Option<i64> {
        db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, chunk_index], |row| row.get(0),
        ).ok()
    }

    pub fn count_chunks(db: &Connection, file_id: i64) -> u32 {
        db.query_row("SELECT COUNT(*) FROM file_chunks WHERE file_id = ?1", rusqlite::params![file_id], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn update_chunk_count(db: &Connection, file_id: i64, count: u32) {
        db.execute("UPDATE file_map SET chunk_count = ?1 WHERE id = ?2", rusqlite::params![count, file_id]).ok();
    }

    pub fn update_chunk_sha256(db: &Connection, file_id: i64, chunk_index: u32, sha256: &str) {
        db.execute(
            "UPDATE file_chunks SET sha256 = ?1 WHERE file_id = ?2 AND chunk_index = ?3",
            rusqlite::params![sha256, file_id, chunk_index],
        ).ok();
    }

    pub fn add_replica(db: &Connection, chunk_id: i64, backend_id: &str, node_id: &str, state: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        let synced_at = if state == "synced" { Some(now.as_str()) } else { None };
        db.execute(
            "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![chunk_id, backend_id, node_id, state, synced_at],
        ).ok();
    }

    pub fn mark_replica_synced(db: &Connection, chunk_id: i64, backend_id: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "UPDATE chunk_replicas SET state = 'synced', synced_at = ?1 WHERE chunk_id = ?2 AND backend_id = ?3",
            rusqlite::params![&now, chunk_id, backend_id],
        ).ok();
    }

    pub fn mark_replica_error(db: &Connection, file_id: i64, chunk_index: u32, backend_id: &str) {
        db.execute(
            "UPDATE chunk_replicas SET state = 'error'
             WHERE chunk_id = (SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2)
               AND backend_id = ?3",
            rusqlite::params![file_id, chunk_index, backend_id],
        ).ok();
    }

    pub fn mark_stale_on_other_nodes(db: &Connection, chunk_id: i64, node_id: &str) {
        db.execute(
            "UPDATE chunk_replicas SET state = 'stale' WHERE chunk_id = ?1 AND node_id != ?2",
            rusqlite::params![chunk_id, node_id],
        ).ok();
    }

    pub fn mark_all_error_on_backend(db: &Connection, backend_id: &str) -> usize {
        db.execute(
            "UPDATE chunk_replicas SET state = 'error' WHERE backend_id = ?1 AND state = 'synced'",
            rusqlite::params![backend_id],
        ).unwrap_or(0)
    }

    /// Get local replicas for a chunk (all backends on this node).
    pub fn get_local_replicas(db: &Connection, file_id: i64, chunk_index: u32, node_id: &str) -> Vec<(String, String)> {
        let mut stmt = db.prepare(
            "SELECT cr.backend_id, b.path FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE fc.file_id = ?1 AND fc.chunk_index = ?2
               AND cr.node_id = ?3 AND cr.state = 'synced'"
        ).unwrap();
        stmt.query_map(rusqlite::params![file_id, chunk_index, node_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Get all replicas for a chunk write (including non-synced, for mirror writes).
    pub fn get_write_replicas(db: &Connection, file_id: i64, chunk_index: u32, node_id: &str) -> Vec<(i64, String, String)> {
        let mut stmt = db.prepare(
            "SELECT fc.id, cr.backend_id, b.path FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE fc.file_id = ?1 AND fc.chunk_index = ?2 AND cr.node_id = ?3"
        ).unwrap();
        stmt.query_map(rusqlite::params![file_id, chunk_index, node_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Count distinct nodes with synced replicas for a chunk.
    pub fn synced_node_count(db: &Connection, chunk_id: i64) -> u32 {
        db.query_row(
            "SELECT COUNT(DISTINCT node_id) FROM chunk_replicas WHERE chunk_id = ?1 AND state = 'synced'",
            rusqlite::params![chunk_id], |row| row.get(0),
        ).unwrap_or(0)
    }

    /// Get nodes that have a synced copy of a chunk.
    pub fn nodes_with_chunk(db: &Connection, chunk_id: i64) -> Vec<String> {
        let mut stmt = db.prepare(
            "SELECT DISTINCT node_id FROM chunk_replicas WHERE chunk_id = ?1 AND state = 'synced'"
        ).unwrap();
        stmt.query_map(rusqlite::params![chunk_id], |row| row.get(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Find under-replicated chunks (fewer synced nodes than FTT+1).
    pub fn find_under_replicated(db: &Connection, limit: u32) -> Vec<(i64, i64, u32, String, u32, u32)> {
        let mut stmt = db.prepare(
            "SELECT fc.id, fc.file_id, fc.chunk_index, fm.volume_id, v.ftt,
                    (SELECT COUNT(DISTINCT cr.node_id) FROM chunk_replicas cr
                     WHERE cr.chunk_id = fc.id AND cr.state = 'synced') AS synced_nodes
             FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN volumes v ON v.id = fm.volume_id
             WHERE synced_nodes < (v.ftt + 1)
             LIMIT ?1"
        ).unwrap();
        stmt.query_map(rusqlite::params![limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Find chunks on degraded/draining/offline backends (for local rebalancer).
    pub fn find_chunks_on_bad_backends(db: &Connection, node_id: &str, limit: u32) -> Vec<(i64, i64, u32, String, String, String)> {
        let mut stmt = db.prepare(
            "SELECT cr.chunk_id, fc.file_id, fc.chunk_index, cr.backend_id, b.path, fm.volume_id
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN backends b ON b.id = cr.backend_id
             WHERE cr.node_id = ?1 AND b.status IN ('offline', 'degraded', 'draining')
               AND cr.state = 'synced'
             LIMIT ?2"
        ).unwrap();
        stmt.query_map(rusqlite::params![node_id, limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Check if any chunks remain on a backend.
    pub fn backend_has_chunks(db: &Connection, backend_id: &str) -> bool {
        db.query_row(
            "SELECT COUNT(*) FROM chunk_replicas WHERE backend_id = ?1",
            rusqlite::params![backend_id], |row| row.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false)
    }

    /// Total/synced/stale chunk counts for a volume.
    pub fn volume_chunk_counts(db: &Connection, volume_id: &str) -> (u64, u64, u64) {
        let total: u64 = db.query_row(
            "SELECT COUNT(*) FROM file_chunks fc JOIN file_map fm ON fm.id = fc.file_id WHERE fm.volume_id = ?1",
            rusqlite::params![volume_id], |row| row.get(0),
        ).unwrap_or(0);
        let synced: u64 = db.query_row(
            "SELECT COUNT(DISTINCT fc.id) FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN chunk_replicas cr ON cr.chunk_id = fc.id
             WHERE fm.volume_id = ?1 AND cr.state = 'synced'",
            rusqlite::params![volume_id], |row| row.get(0),
        ).unwrap_or(0);
        let stale: u64 = db.query_row(
            "SELECT COUNT(DISTINCT fc.id) FROM file_chunks fc
             JOIN file_map fm ON fm.id = fc.file_id
             JOIN chunk_replicas cr ON cr.chunk_id = fc.id
             WHERE fm.volume_id = ?1 AND cr.state IN ('stale', 'syncing')",
            rusqlite::params![volume_id], |row| row.get(0),
        ).unwrap_or(0);
        (total, synced, stale)
    }

    /// Protected/degraded file counts for a volume.
    pub fn volume_protection_counts(db: &Connection, volume_id: &str) -> (u64, u64) {
        let protected: u64 = db.query_row(
            "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1 AND protection_status = 'protected'",
            rusqlite::params![volume_id], |row| row.get(0),
        ).unwrap_or(0);
        let degraded: u64 = db.query_row(
            "SELECT COUNT(*) FROM file_map WHERE volume_id = ?1 AND protection_status = 'degraded'",
            rusqlite::params![volume_id], |row| row.get(0),
        ).unwrap_or(0);
        (protected, degraded)
    }
}
