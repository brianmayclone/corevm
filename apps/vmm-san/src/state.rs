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
/// We use a pool of read connections to prevent reader serialization:
/// - `read_pool`: N read connections — callers never block each other
/// - `write`: single write connection (SQLite only allows one writer)
///
/// Callers use `db.read()` or `db.write()`.
pub struct DbPool {
    read_pool: Vec<Mutex<Connection>>,
    read_next: std::sync::atomic::AtomicUsize,
    write: Mutex<Connection>,
}

const READ_POOL_SIZE: usize = 8;

impl DbPool {
    /// Create a new pool with multiple read connections and one write connection.
    pub fn new(db_path: &std::path::Path) -> Result<Self, String> {
        let write = Connection::open(db_path)
            .map_err(|e| format!("Cannot open write connection: {}", e))?;
        write.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("Write PRAGMA failed: {}", e))?;
        write.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Write busy_timeout: {}", e))?;

        let mut read_pool = Vec::with_capacity(READ_POOL_SIZE);
        for i in 0..READ_POOL_SIZE {
            let read = Connection::open(db_path)
                .map_err(|e| format!("Cannot open read connection {}: {}", i, e))?;
            read.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")
                .map_err(|e| format!("Read PRAGMA {} failed: {}", i, e))?;
            read.busy_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| format!("Read busy_timeout {}: {}", i, e))?;
            read_pool.push(Mutex::new(read));
        }

        Ok(Self {
            read_pool,
            read_next: std::sync::atomic::AtomicUsize::new(0),
            write: Mutex::new(write),
        })
    }

    /// Acquire a read connection (SELECT only).
    /// Round-robins across the pool so concurrent readers don't serialize.
    pub fn read(&self) -> std::sync::MutexGuard<'_, Connection> {
        let idx = self.read_next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.read_pool.len();
        self.read_pool[idx].lock().unwrap()
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
