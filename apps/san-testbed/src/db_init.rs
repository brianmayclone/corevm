//! SQLite pre-initialization for testbed nodes.

use rusqlite::Connection;
use std::path::Path;

/// The CoreSAN schema — replicated from vmm-san/src/db/mod.rs.
/// Must be kept in sync manually (testbed only, not production).
const SCHEMA: &str = include_str!("schema.sql");

/// Initialize a node's SQLite database with schema, node_settings, peers, backends, and a test volume.
pub fn init_node_db(
    db_path: &Path,
    node_id: &str,
    node_index: usize,
    total_nodes: usize,
    base_port: u16,
    disk_paths: &[String],
) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Cannot open DB {}: {}", db_path.display(), e))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("PRAGMA: {}", e))?;

    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("Schema: {}", e))?;

    // node_settings table (created in vmm-san main.rs, not in schema)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS node_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);"
    ).map_err(|e| format!("node_settings table: {}", e))?;

    conn.execute(
        "INSERT OR REPLACE INTO node_settings (key, value) VALUES ('node_id', ?1)",
        rusqlite::params![node_id],
    ).map_err(|e| format!("node_id: {}", e))?;

    // Insert peers (all other nodes)
    for i in 1..=total_nodes {
        if i == node_index { continue; }
        let peer_id = format!("node-{}", i);
        let peer_port = base_port + i as u16;
        let peer_addr = format!("http://127.0.0.1:{}", peer_port);
        let hostname = format!("testbed-node-{}", i);
        conn.execute(
            "INSERT OR REPLACE INTO peers (node_id, address, peer_port, hostname, status)
             VALUES (?1, ?2, ?3, ?4, 'connecting')",
            rusqlite::params![&peer_id, &peer_addr, peer_port + 100, &hostname],
        ).map_err(|e| format!("peer insert: {}", e))?;
    }

    // Insert test volume
    conn.execute(
        "INSERT OR IGNORE INTO volumes (id, name, ftt, status)
         VALUES ('testbed-vol', 'testbed-vol', 1, 'online')",
        [],
    ).map_err(|e| format!("volume: {}", e))?;

    // Insert claimed disks and backends
    for (idx, disk_path) in disk_paths.iter().enumerate() {
        let disk_id = format!("disk-{}-{}", node_index, idx);
        let backend_id = format!("backend-{}-{}", node_id, idx);

        conn.execute(
            "INSERT OR REPLACE INTO claimed_disks (id, device_path, mount_path, fs_type, size_bytes, status, backend_id)
             VALUES (?1, ?2, ?3, 'ext4', 107374182400, 'mounted', ?4)",
            rusqlite::params![&disk_id, &format!("/fake/dev/sd{}", (b'a' + idx as u8) as char), disk_path, &backend_id],
        ).map_err(|e| format!("claimed_disk: {}", e))?;

        conn.execute(
            "INSERT OR REPLACE INTO backends (id, node_id, path, total_bytes, free_bytes, status, claimed_disk_id)
             VALUES (?1, ?2, ?3, 107374182400, 107374182400, 'online', ?4)",
            rusqlite::params![&backend_id, node_id, disk_path, &disk_id],
        ).map_err(|e| format!("backend: {}", e))?;
    }

    Ok(())
}
