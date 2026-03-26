//! Cluster application state — central authority state.
//!
//! Unlike vmm-server which manages local VMs, the cluster state
//! holds the authoritative view of ALL VMs, hosts, and datastores.

use std::sync::{Arc, Mutex, RwLock};
use dashmap::DashMap;
use rusqlite::Connection;
use crate::config::ClusterConfig;
use crate::engine::discovery::DiscoveryStore;

/// Live node connection info, kept in memory alongside the DB record.
#[derive(Debug, Clone)]
pub struct NodeConnection {
    pub node_id: String,
    pub hostname: String,
    pub address: String,
    pub agent_token: String,
    pub status: NodeStatus,
    pub missed_heartbeats: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Connecting,
    Online,
    Offline,
    Maintenance,
    Error,
}

/// Central cluster state — shared across all request handlers and engine tasks.
pub struct ClusterState {
    /// In-memory node connections indexed by node_id.
    pub nodes: DashMap<String, NodeConnection>,
    /// SQLite database (the authoritative store for everything).
    pub db: Mutex<Connection>,
    /// Immutable cluster configuration.
    pub config: ClusterConfig,
    /// JWT signing secret.
    pub jwt_secret: String,
    /// Server start time for uptime tracking.
    pub started_at: std::time::Instant,
    /// UDP discovery — auto-discovered nodes on the network.
    pub discovery: Arc<DiscoveryStore>,
    /// Latest SAN health snapshot (updated by san_health engine).
    pub san_health: RwLock<serde_json::Value>,
}
