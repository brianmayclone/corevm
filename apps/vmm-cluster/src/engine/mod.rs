//! Cluster engine — background tasks that make the cluster smart.
//!
//! The engine is the brain of vmm-cluster: heartbeat monitoring,
//! HA (automatic VM restart), DRS (resource scheduling), and maintenance evacuation.

pub mod heartbeat;
pub mod ha;
pub mod drs;
pub mod maintenance;
pub mod scheduler;
pub mod notifier;
pub mod sdn;
pub mod reconciler;
pub mod discovery;
