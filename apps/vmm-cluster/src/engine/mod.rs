//! Cluster engine — background tasks that make the cluster smart.
//!
//! The engine is the brain of vmm-cluster: heartbeat monitoring,
//! HA (automatic VM restart), DRS (resource scheduling), and maintenance evacuation.

pub mod heartbeat;

// TODO: Phase 4 — pub mod ha;
// TODO: Phase 5 — pub mod drs;
// TODO: Phase 5 — pub mod maintenance;
// TODO: Phase 5 — pub mod scheduler;
// TODO: Phase 5 — pub mod reconciler;
