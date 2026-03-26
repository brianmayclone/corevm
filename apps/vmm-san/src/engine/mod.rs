//! Background engines — heartbeat, replication, repair, integrity, benchmark, FUSE.
//!
//! All engines run as independent tokio tasks. They are spawned at daemon startup
//! and operate autonomously — no dependency on vmm-cluster or vmm-server.
//! CoreSAN peers communicate directly with each other.

pub mod placement;
pub mod replication;
pub mod repair;
pub mod integrity;
pub mod peer_monitor;
pub mod benchmark;
pub mod fuse_mount;
pub mod backend_refresh;
pub mod write_lease;
pub mod push_replicator;
pub mod discovery;
pub mod rebalancer;
pub mod db_mirror;
