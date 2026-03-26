//! Peer service — all database operations for peers.

use rusqlite::Connection;

pub struct PeerService;

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: String,
    pub address: String,
    pub peer_port: u16,
    pub hostname: String,
    pub status: String,
    pub last_heartbeat: Option<String>,
}

impl PeerService {
    pub fn upsert(db: &Connection, node_id: &str, address: &str, peer_port: u16, hostname: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT OR REPLACE INTO peers (node_id, address, peer_port, hostname, status, last_heartbeat, joined_at)
             VALUES (?1, ?2, ?3, ?4, 'online', ?5, ?5)",
            rusqlite::params![node_id, address, peer_port, hostname, &now],
        ).ok();
    }

    pub fn list(db: &Connection) -> Vec<PeerInfo> {
        let mut stmt = db.prepare(
            "SELECT node_id, address, peer_port, hostname, status, last_heartbeat FROM peers ORDER BY hostname"
        ).unwrap();
        stmt.query_map([], |row| Ok(PeerInfo {
            node_id: row.get(0)?, address: row.get(1)?, peer_port: row.get(2)?,
            hostname: row.get(3)?, status: row.get(4)?, last_heartbeat: row.get(5)?,
        })).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn delete(db: &Connection, node_id: &str) {
        db.execute("DELETE FROM peers WHERE node_id = ?1", rusqlite::params![node_id]).ok();
    }

    pub fn set_status(db: &Connection, node_id: &str, status: &str) {
        db.execute("UPDATE peers SET status = ?1 WHERE node_id = ?2", rusqlite::params![status, node_id]).ok();
    }

    pub fn update_heartbeat(db: &Connection, node_id: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "UPDATE peers SET status = 'online', last_heartbeat = ?1 WHERE node_id = ?2",
            rusqlite::params![&now, node_id],
        ).ok();
    }

    pub fn mark_backends_offline(db: &Connection, node_id: &str) {
        db.execute("UPDATE backends SET status = 'offline' WHERE node_id = ?1", rusqlite::params![node_id]).ok();
    }
}
