//! File-to-backend mapping — metadata index queries.

use rusqlite::Connection;

/// Find the local filesystem path of a synced replica for a given file.
/// Returns the full path (backend_path + rel_path) if a local replica exists.
pub fn find_local_replica(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
    node_id: &str,
) -> Option<String> {
    db.query_row(
        "SELECT b.path || '/' || fm.rel_path
         FROM file_map fm
         JOIN file_replicas fr ON fr.file_id = fm.id
         JOIN backends b ON b.id = fr.backend_id
         WHERE fm.volume_id = ?1 AND fm.rel_path = ?2
           AND b.node_id = ?3 AND fr.state = 'synced'
         LIMIT 1",
        rusqlite::params![volume_id, rel_path, node_id],
        |row| row.get(0),
    ).ok()
}

/// Find any synced replica (local or remote) and return (node_id, backend_path, rel_path).
pub fn find_any_replica(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
) -> Option<(String, String, String)> {
    db.query_row(
        "SELECT b.node_id, b.path, fm.rel_path
         FROM file_map fm
         JOIN file_replicas fr ON fr.file_id = fm.id
         JOIN backends b ON b.id = fr.backend_id
         WHERE fm.volume_id = ?1 AND fm.rel_path = ?2 AND fr.state = 'synced'
         LIMIT 1",
        rusqlite::params![volume_id, rel_path],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).ok()
}

/// Find a synced replica on a REMOTE node (excluding local node).
/// Returns the remote node_id that holds a copy.
pub fn find_remote_replica(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
    local_node_id: &str,
) -> Option<String> {
    db.query_row(
        "SELECT b.node_id
         FROM file_map fm
         JOIN file_replicas fr ON fr.file_id = fm.id
         JOIN backends b ON b.id = fr.backend_id
         WHERE fm.volume_id = ?1 AND fm.rel_path = ?2
           AND fr.state = 'synced' AND b.node_id != ?3
         LIMIT 1",
        rusqlite::params![volume_id, rel_path, local_node_id],
        |row| row.get(0),
    ).ok()
}

/// Get files that are under-replicated (fewer synced replicas than required).
pub fn find_under_replicated(db: &Connection) -> Vec<UnderReplicatedFile> {
    let mut stmt = db.prepare(
        "SELECT fm.id, fm.volume_id, fm.rel_path, v.replica_count,
                COUNT(CASE WHEN fr.state = 'synced' THEN 1 END) AS synced_count
         FROM file_map fm
         JOIN volumes v ON v.id = fm.volume_id
         LEFT JOIN file_replicas fr ON fr.file_id = fm.id
         GROUP BY fm.id
         HAVING synced_count < v.replica_count
         ORDER BY v.replica_count - synced_count DESC, fm.size_bytes ASC"
    ).unwrap();

    stmt.query_map([], |row| {
        Ok(UnderReplicatedFile {
            file_id: row.get(0)?,
            volume_id: row.get(1)?,
            rel_path: row.get(2)?,
            desired_replicas: row.get(3)?,
            current_synced: row.get(4)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect()
}

/// Get files with stale replicas that need re-syncing.
pub fn find_stale_replicas(db: &Connection) -> Vec<StaleReplica> {
    let mut stmt = db.prepare(
        "SELECT fr.file_id, fr.backend_id, fm.volume_id, fm.rel_path, b.node_id, b.path
         FROM file_replicas fr
         JOIN file_map fm ON fm.id = fr.file_id
         JOIN backends b ON b.id = fr.backend_id
         WHERE fr.state = 'stale'
         ORDER BY fm.size_bytes ASC"
    ).unwrap();

    stmt.query_map([], |row| {
        Ok(StaleReplica {
            file_id: row.get(0)?,
            backend_id: row.get(1)?,
            volume_id: row.get(2)?,
            rel_path: row.get(3)?,
            node_id: row.get(4)?,
            backend_path: row.get(5)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect()
}

pub struct UnderReplicatedFile {
    pub file_id: i64,
    pub volume_id: String,
    pub rel_path: String,
    pub desired_replicas: u32,
    pub current_synced: u32,
}

pub struct StaleReplica {
    pub file_id: i64,
    pub backend_id: String,
    pub volume_id: String,
    pub rel_path: String,
    pub node_id: String,
    pub backend_path: String,
}
