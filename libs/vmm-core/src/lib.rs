//! vmm-core: Shared data models for corevm.
//!
//! Contains VM configuration types with serde support, shared between
//! the vmmanager desktop app and the vmm-server web backend.
//! Does NOT contain lifecycle logic — that lives in libcorevm.

pub mod cluster;
pub mod config;
pub mod san_disk;
pub mod san_iscsi;
pub mod san_mgmt;
pub mod san_object;
pub mod snapshot;
