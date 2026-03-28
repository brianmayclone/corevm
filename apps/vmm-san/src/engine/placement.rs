//! File placement engine — decides which backend to write to and where to replicate.

use rusqlite::Connection;

/// Select the best local backend for a volume (most free space, online status).
pub fn select_local_backend(
    db: &Connection,
    volume_id: &str,
    node_id: &str,
) -> Option<(String, String)> {
    db.query_row(
        "SELECT id, path FROM backends
         WHERE volume_id = ?1 AND node_id = ?2 AND status = 'online'
         ORDER BY free_bytes DESC LIMIT 1",
        rusqlite::params![volume_id, node_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).ok()
}

/// Select backends for replica placement, preferring different nodes.
/// Returns a list of (backend_id, node_id, backend_path) tuples.
pub fn select_replica_backends(
    db: &Connection,
    volume_id: &str,
    exclude_backend_id: &str,
    desired_count: u32,
) -> Vec<(String, String, String)> {
    // First try backends on different nodes
    let mut stmt = db.prepare(
        "SELECT b.id, b.node_id, b.path FROM backends b
         WHERE b.id != ?1 AND b.status = 'online'
         AND b.node_id NOT IN (
             SELECT node_id FROM backends WHERE id = ?2
         )
         ORDER BY b.free_bytes DESC"
    ).unwrap();

    let mut results: Vec<(String, String, String)> = stmt.query_map(
        rusqlite::params![volume_id, exclude_backend_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap().filter_map(|r| r.ok()).collect();

    // If we don't have enough on different nodes, add same-node backends
    if (results.len() as u32) < desired_count {
        let mut fallback = db.prepare(
            "SELECT b.id, b.node_id, b.path FROM backends b
             WHERE b.id != ?1 AND b.status = 'online'
             AND b.id NOT IN (SELECT id FROM backends WHERE id IN (
                 SELECT ?2 UNION ALL SELECT ?3
             ))
             ORDER BY b.free_bytes DESC"
        ).unwrap();

        // Collect IDs we already selected
        let existing_ids: Vec<String> = results.iter().map(|(id, _, _)| id.clone()).collect();
        let existing_str = existing_ids.join(",");

        let more: Vec<(String, String, String)> = fallback.query_map(
            rusqlite::params![volume_id, exclude_backend_id, &existing_str],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap().filter_map(|r| r.ok())
         .filter(|(id, _, _)| !existing_ids.contains(id))
         .collect();

        results.extend(more);
    }

    results.truncate(desired_count as usize);
    results
}

/// Select the best backend for a new replica of a specific file,
/// taking into account which backends already have replicas.
pub fn select_new_replica_target(
    db: &Connection,
    file_id: i64,
    volume_id: &str,
) -> Option<(String, String, String)> {
    // Find backends that don't already have chunk replicas for this file,
    // preferring backends on different nodes from existing chunk replicas.
    db.query_row(
        "SELECT b.id, b.node_id, b.path FROM backends b
         WHERE b.status = 'online'
         AND b.id NOT IN (
             SELECT cr.backend_id FROM chunk_replicas cr
             JOIN file_chunks fc ON fc.id = cr.chunk_id
             WHERE fc.file_id = ?2
         )
         ORDER BY
             CASE WHEN b.node_id NOT IN (
                 SELECT DISTINCT cr2.node_id FROM chunk_replicas cr2
                 JOIN file_chunks fc2 ON fc2.id = cr2.chunk_id
                 WHERE fc2.file_id = ?2
             ) THEN 0 ELSE 1 END,
             b.free_bytes DESC
         LIMIT 1",
        rusqlite::params![volume_id, file_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).ok()
}
