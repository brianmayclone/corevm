//! CoreSAN application state — shared across all handlers and engine tasks.

use std::sync::Mutex;
use dashmap::DashMap;
use rusqlite::Connection;
use serde::Serialize;
use crate::config::CoreSanConfig;
use crate::engine::push_replicator::WriteSender;

/// Read/write database pool for SQLite WAL mode.
///
/// SQLite in WAL mode allows concurrent readers alongside a single writer.
/// We use two separate connections to exploit this:
/// - `read`: for SELECT-only queries (never blocks writes)
/// - `write`: for INSERT/UPDATE/DELETE (never blocks reads)
///
/// Callers use `db.read()` or `db.write()` instead of `db.lock().unwrap()`.
pub struct DbPool {
    read: Mutex<Connection>,
    write: Mutex<Connection>,
}

impl DbPool {
    /// Create a new pool with separate read and write connections to the same database.
    pub fn new(db_path: &std::path::Path) -> Result<Self, String> {
        let write = Connection::open(db_path)
            .map_err(|e| format!("Cannot open write connection: {}", e))?;
        write.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("Write PRAGMA failed: {}", e))?;
        // Busy timeout: wait up to 5 seconds if the write lock is held
        write.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Write busy_timeout: {}", e))?;

        let read = Connection::open(db_path)
            .map_err(|e| format!("Cannot open read connection: {}", e))?;
        read.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")
            .map_err(|e| format!("Read PRAGMA failed: {}", e))?;
        read.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Read busy_timeout: {}", e))?;

        Ok(Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
        })
    }

    /// Acquire the read connection (SELECT only).
    /// Multiple concurrent readers are possible in WAL mode, but this Mutex
    /// serializes access to this particular Connection object.
    pub fn read(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.read.lock().unwrap()
    }

    /// Acquire the write connection (INSERT/UPDATE/DELETE).
    pub fn write(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.write.lock().unwrap()
    }

    /// Legacy: acquire write connection. Migration helper — same as write().
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, std::sync::PoisonError<std::sync::MutexGuard<'_, Connection>>> {
        self.write.lock()
    }
}

/// Live peer connection info, kept in memory.
#[derive(Debug, Clone)]
pub struct PeerConnection {
    pub node_id: String,
    pub address: String,
    pub peer_port: u16,
    pub hostname: String,
    pub status: PeerStatus,
    pub missed_heartbeats: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeerStatus {
    Connecting,
    Online,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum QuorumStatus {
    /// Node is starting up, performing integrity check — not yet available
    Sanitizing,
    /// All peers reachable, full read/write
    Active,
    /// Quorum met but some peers unreachable, full read/write
    Degraded,
    /// No quorum, no witness — new leases denied, effectively read-only
    Fenced,
    /// No peers configured — no quorum required, full read/write
    Solo,
}

/// Central CoreSAN state — shared across all request handlers and engine tasks.
pub struct CoreSanState {
    /// In-memory peer connections indexed by node_id.
    pub peers: DashMap<String, PeerConnection>,
    /// SQLite database pool — use db.read() for SELECTs, db.write() for mutations.
    pub db: DbPool,
    /// Immutable configuration.
    pub config: CoreSanConfig,
    /// Path to the TOML config file (for persisting runtime updates).
    pub config_path: Option<std::path::PathBuf>,
    /// This node's unique ID.
    pub node_id: String,
    /// This node's hostname.
    pub hostname: String,
    /// Server start time for uptime tracking.
    pub started_at: std::time::Instant,
    /// Channel to push write events for immediate replication to peers.
    pub write_tx: WriteSender,
    /// Current quorum status — checked on every write.
    pub quorum_status: std::sync::RwLock<QuorumStatus>,
    /// Whether this node is the elected leader (lowest node_id among active nodes).
    pub is_leader: std::sync::atomic::AtomicBool,
}
