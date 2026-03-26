//! CoreSAN application state — shared across all handlers and engine tasks.

use std::sync::Mutex;
use dashmap::DashMap;
use rusqlite::Connection;
use crate::config::CoreSanConfig;
use crate::engine::push_replicator::WriteSender;

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

/// Central CoreSAN state — shared across all request handlers and engine tasks.
pub struct CoreSanState {
    /// In-memory peer connections indexed by node_id.
    pub peers: DashMap<String, PeerConnection>,
    /// SQLite database (metadata store).
    pub db: Mutex<Connection>,
    /// Immutable configuration.
    pub config: CoreSanConfig,
    /// This node's unique ID.
    pub node_id: String,
    /// This node's hostname.
    pub hostname: String,
    /// Server start time for uptime tracking.
    pub started_at: std::time::Instant,
    /// Channel to push write events for immediate replication to peers.
    pub write_tx: WriteSender,
}
