//! ServiceRegistry — IoC container for discovering services.
//!
//! Provides a single access point for all service instances.
//! API handlers use `registry.stats()`, `registry.drs()`, etc.
//! instead of directly importing service structs.
//!
//! The registry holds a reference to the shared ClusterState and
//! provides convenience methods that acquire the DB lock internally.

use std::sync::Arc;
use rusqlite::Connection;
use crate::state::ClusterState;
use crate::services::*;

/// Central service registry — constructed from ClusterState.
/// API handlers receive this via axum State and call services through it.
pub struct ServiceRegistry {
    state: Arc<ClusterState>,
}

impl ServiceRegistry {
    pub fn new(state: Arc<ClusterState>) -> Self {
        Self { state }
    }

    /// Acquire a database connection lock.
    /// All service calls go through this.
    pub fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.state.db.lock().map_err(|_| "Database lock error".to_string())
    }

    // ── Service accessors ───────────────────────────────────────────────
    // Each returns a zero-sized service struct. The real work happens
    // when you call methods on them, passing &Connection from self.db().

    pub fn auth(&self) -> auth::AuthService { auth::AuthService }
    pub fn users(&self) -> user::UserService { user::UserService }
    pub fn audit(&self) -> audit::AuditService { audit::AuditService }
    pub fn events(&self) -> event::EventService { event::EventService }
    pub fn clusters(&self) -> cluster::ClusterService { cluster::ClusterService }
    pub fn hosts(&self) -> host::HostService { host::HostService }
    pub fn vms(&self) -> vm::VmService { vm::VmService }
    pub fn datastores(&self) -> datastore::DatastoreService { datastore::DatastoreService }
    pub fn tasks(&self) -> task::TaskService { task::TaskService }
    pub fn alarms(&self) -> alarm::AlarmService { alarm::AlarmService }
    pub fn stats(&self) -> stats::StatsService { stats::StatsService }
    pub fn drs(&self) -> drs_service::DrsService { drs_service::DrsService }
    pub fn storage_compat(&self) -> storage_compat::StorageCompatService { storage_compat::StorageCompatService }
    pub fn resource_groups(&self) -> resource_group::ResourceGroupService { resource_group::ResourceGroupService }

    /// Access the underlying state (for node connections, etc.).
    pub fn state(&self) -> &Arc<ClusterState> { &self.state }
}
