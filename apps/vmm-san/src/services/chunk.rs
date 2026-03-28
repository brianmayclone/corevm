//! Chunk service — all database operations for file_chunks and chunk_replicas.
//!
//! This is the ONLY module that writes to file_chunks and chunk_replicas tables.
//! All engines, API handlers, and FUSE operations go through this service.

use rusqlite::Connection;
use crate::db::{DbResult, DbContext, db_transaction};

pub struct ChunkService;

// ── file_chunks CRUD ─────────────────────────────────────────────────

impl ChunkService {
    pub fn create_chunk(db: &Connection, file_id: i64, chunk_index: u32, offset: u64, size: u64) -> DbResult<i64> {
        db.execute(
            "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![file_id, chunk_index, offset, size],
        ).ctx("ChunkService::create_chunk INSERT")?;

        db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, chunk_index], |row| row.get(0),
        ).ctx("ChunkService::create_chunk SELECT id")
    }

    pub fn get_chunk_id(db: &Connection, file_id: i64, chunk_index: u32) -> DbResult<i64> {
        db.query_row(
            "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
            rusqlite::params![file_id, chunk_index], |row| row.get(0),
        ).ctx("ChunkService::get_chunk_id")
    }

    pub fn count_chunks(db: &Connection, file_id: i64) -> u32 {
        db.query_row("SELECT COUNT(*) FROM file_chunks WHERE file_id = ?1",
            rusqlite::params![file_id], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn update_chunk_sha256(db: &Connection, file_id: i64, chunk_index: u32, sha256: &str) -> DbResult<()> {
        db.execute(
            "UPDATE file_chunks SET sha256 = ?1 WHERE file_id = ?2 AND chunk_index = ?3",
            rusqlite::params![sha256, file_id, chunk_index],
        ).ctx("ChunkService::update_chunk_sha256")?;
        Ok(())
    }

    pub fn update_chunk_sha256_by_id(db: &Connection, chunk_id: i64, sha256: &str) -> DbResult<()> {
        db.execute(
            "UPDATE file_chunks SET sha256 = ?1 WHERE id = ?2",
            rusqlite::params![sha256, chunk_id],
        ).ctx("ChunkService::update_chunk_sha256_by_id")?;
        Ok(())
    }

    pub fn delete_chunks_for_file(db: &Connection, file_id: i64) -> DbResult<()> {
        db.execute(
            "DELETE FROM chunk_replicas WHERE chunk_id IN (SELECT id FROM file_chunks WHERE file_id = ?1)",
            rusqlite::params![file_id],
        ).ctx("ChunkService::delete_chunks_for_file replicas")?;
        db.execute(
            "DELETE FROM file_chunks WHERE file_id = ?1",
            rusqlite::params![file_id],
        ).ctx("ChunkService::delete_chunks_for_file chunks")?;
        Ok(())
    }

    // ── chunk_replicas CRUD ──────────────────────────────────────────

    /// Record that a chunk replica exists on a specific node.
    /// For local replicas, backend_id is the actual backend UUID.
    /// For remote-tracked replicas, backend_id can be empty.
    pub fn set_replica_synced(db: &Connection, chunk_id: i64, backend_id: &str, node_id: &str) -> DbResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
             VALUES (?1, ?2, ?3, 'synced', ?4)",
            rusqlite::params![chunk_id, backend_id, node_id, &now],
        ).ctx("ChunkService::set_replica_synced")?;
        Ok(())
    }

    /// Track that a remote peer now holds a chunk (after successful push).
    pub fn track_remote_replica(db: &Connection, chunk_id: i64, remote_node_id: &str) -> DbResult<()> {
        Self::set_replica_synced(db, chunk_id, "", remote_node_id)
    }

    pub fn mark_replica_error(db: &Connection, chunk_id: i64, node_id: &str) -> DbResult<()> {
        db.execute(
            "UPDATE chunk_replicas SET state = 'error' WHERE chunk_id = ?1 AND node_id = ?2",
            rusqlite::params![chunk_id, node_id],
        ).ctx("ChunkService::mark_replica_error")?;
        Ok(())
    }

    pub fn mark_replica_error_by_backend(db: &Connection, file_id: i64, chunk_index: u32, backend_id: &str) -> DbResult<()> {
        db.execute(
            "UPDATE chunk_replicas SET state = 'error'
             WHERE chunk_id = (SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2)
               AND backend_id = ?3",
            rusqlite::params![file_id, chunk_index, backend_id],
        ).ctx("ChunkService::mark_replica_error_by_backend")?;
        Ok(())
    }

    pub fn mark_stale_on_other_nodes(db: &Connection, chunk_id: i64, node_id: &str) -> DbResult<()> {
        db.execute(
            "UPDATE chunk_replicas SET state = 'stale' WHERE chunk_id = ?1 AND node_id != ?2",
            rusqlite::params![chunk_id, node_id],
        ).ctx("ChunkService::mark_stale_on_other_nodes")?;
        Ok(())
    }

    pub fn delete_replicas_on_backend(db: &Connection, backend_id: &str) -> DbResult<usize> {
        db.execute(
            "DELETE FROM chunk_replicas WHERE backend_id = ?1",
            rusqlite::params![backend_id],
        ).ctx("ChunkService::delete_replicas_on_backend")
    }

    // ── Queries ──────────────────────────────────────────────────────

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

    /// Get local synced replicas for a chunk (backend_id, backend_path).
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

    /// Find stale chunk replicas that need re-syncing.
    pub fn find_stale_replicas(db: &Connection, limit: u32) -> Vec<StaleChunkReplica> {
        let mut stmt = db.prepare(
            "SELECT cr.chunk_id, fc.file_id, fc.chunk_index, fm.volume_id,
                    cr.backend_id, COALESCE(b.path, ''), cr.node_id
             FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             JOIN file_map fm ON fm.id = fc.file_id
             LEFT JOIN backends b ON b.id = cr.backend_id
             WHERE cr.state = 'stale'
             ORDER BY fc.file_id, fc.chunk_index
             LIMIT ?1"
        ).unwrap();
        stmt.query_map(rusqlite::params![limit], |row| Ok(StaleChunkReplica {
            chunk_id: row.get(0)?,
            file_id: row.get(1)?,
            chunk_index: row.get(2)?,
            volume_id: row.get(3)?,
            backend_id: row.get(4)?,
            backend_path: row.get(5)?,
            node_id: row.get(6)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Find a source node that has a synced copy of a chunk.
    /// Prefers replicas with SHA256 stored (recently verified).
    pub fn find_chunk_source(db: &Connection, chunk_id: i64) -> Option<String> {
        db.query_row(
            "SELECT cr.node_id FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE cr.chunk_id = ?1 AND cr.state = 'synced'
             ORDER BY CASE WHEN fc.sha256 != '' THEN 0 ELSE 1 END
             LIMIT 1",
            rusqlite::params![chunk_id],
            |row| row.get::<_, String>(0),
        ).ok()
    }

    /// Get expected SHA256 for a chunk.
    pub fn get_chunk_sha256(db: &Connection, chunk_id: i64) -> Option<String> {
        db.query_row(
            "SELECT sha256 FROM file_chunks WHERE id = ?1",
            rusqlite::params![chunk_id], |row| row.get::<_, String>(0),
        ).ok().filter(|s| !s.is_empty())
    }

    /// Find local backend path for a chunk source.
    pub fn find_local_chunk_path(db: &Connection, chunk_id: i64, node_id: &str) -> Option<(String, String)> {
        db.query_row(
            "SELECT b.path, COALESCE(fc.sha256, '') FROM chunk_replicas cr
             JOIN backends b ON b.id = cr.backend_id
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE cr.chunk_id = ?1 AND cr.node_id = ?2 AND cr.state = 'synced'
             LIMIT 1",
            rusqlite::params![chunk_id, node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    }

    /// Find chunks on degraded/draining/offline backends for rebalancer.
    pub fn find_chunks_on_bad_backends(db: &Connection, node_id: &str, limit: u32) -> Vec<ChunkToMove> {
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
            Ok(ChunkToMove {
                chunk_id: row.get(0)?,
                file_id: row.get(1)?,
                chunk_index: row.get(2)?,
                backend_id: row.get(3)?,
                backend_path: row.get(4)?,
                volume_id: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Atomically move a chunk replica from one backend to another (for rebalancer).
    pub fn move_replica(db: &Connection, chunk_id: i64, old_backend_id: &str, new_backend_id: &str, node_id: &str) -> DbResult<()> {
        db_transaction(db, "ChunkService::move_replica", || {
            let now = chrono::Utc::now().to_rfc3339();
            db.execute(
                "UPDATE chunk_replicas SET backend_id = ?1, synced_at = ?2
                 WHERE chunk_id = ?3 AND node_id = ?4",
                rusqlite::params![new_backend_id, &now, chunk_id, node_id],
            ).ctx("move_replica UPDATE")?;
            Ok(())
        })
    }

    /// Count all replicas pending replication.
    pub fn count_pending_replicas(db: &Connection) -> u64 {
        db.query_row(
            "SELECT COUNT(*) FROM chunk_replicas WHERE state != 'synced'",
            [], |row| row.get(0),
        ).unwrap_or(0)
    }

    /// Receive a chunk from a peer: create file_chunks entry + chunk_replica in one transaction.
    pub fn receive_chunk(
        db: &Connection,
        file_id: i64,
        chunk_index: u32,
        chunk_size: u64,
        vol_chunk_size: u64,
        sha256: &str,
        backend_id: &str,
        node_id: &str,
    ) -> DbResult<()> {
        let offset = chunk_index as u64 * vol_chunk_size;
        let now = chrono::Utc::now().to_rfc3339();

        db_transaction(db, "ChunkService::receive_chunk", || {
            db.execute(
                "INSERT OR IGNORE INTO file_chunks (file_id, chunk_index, offset_bytes, size_bytes)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![file_id, chunk_index, offset, chunk_size],
            ).ctx("receive_chunk: INSERT file_chunks")?;

            let chunk_id: i64 = db.query_row(
                "SELECT id FROM file_chunks WHERE file_id = ?1 AND chunk_index = ?2",
                rusqlite::params![file_id, chunk_index], |row| row.get(0),
            ).ctx("receive_chunk: get chunk_id")?;

            db.execute(
                "UPDATE file_chunks SET sha256 = ?1, size_bytes = ?2 WHERE id = ?3",
                rusqlite::params![sha256, chunk_size, chunk_id],
            ).ctx("receive_chunk: UPDATE sha256")?;

            db.execute(
                "INSERT OR REPLACE INTO chunk_replicas (chunk_id, backend_id, node_id, state, synced_at)
                 VALUES (?1, ?2, ?3, 'synced', ?4)",
                rusqlite::params![chunk_id, backend_id, node_id, &now],
            ).ctx("receive_chunk: INSERT chunk_replica")?;

            Ok(())
        })
    }
}

// ── Types ────────────────────────────────────────────────────────────

pub struct StaleChunkReplica {
    pub chunk_id: i64,
    pub file_id: i64,
    pub chunk_index: u32,
    pub volume_id: String,
    pub backend_id: String,
    pub backend_path: String,
    pub node_id: String,
}

pub struct ChunkToMove {
    pub chunk_id: i64,
    pub file_id: i64,
    pub chunk_index: u32,
    pub backend_id: String,
    pub backend_path: String,
    pub volume_id: String,
}
