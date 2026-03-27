//! Agent module — enables vmm-server to be managed by vmm-cluster.
//!
//! When registered with a cluster, vmm-server becomes a passive agent:
//! - Regular API (/api/*) is blocked (returns managed_by_cluster error)
//! - Agent API (/agent/*) is active (authenticated via X-Agent-Token)
//! - The cluster is the authority — this node executes commands

pub mod auth;
pub mod handlers;
pub mod registration;

use axum::{Router, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::AppState;

/// Build the agent API router.
/// These routes are always mounted but only functional when in managed mode.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Registration
        .route("/agent/register", post(registration::register))
        .route("/agent/deregister", post(registration::deregister))
        // Status & monitoring (Cluster polls these)
        .route("/agent/status", get(handlers::status))
        .route("/agent/hardware", get(handlers::hardware))
        .route("/agent/vms", get(handlers::list_vms))
        .route("/agent/vms/{id}/screenshot", get(handlers::screenshot))
        .route("/agent/storage/pools", get(handlers::list_storage_pools))
        // VM lifecycle commands (Cluster sends these)
        .route("/agent/vms/provision", post(handlers::provision_vm))
        .route("/agent/vms/{id}/start", post(handlers::start_vm))
        .route("/agent/vms/{id}/stop", post(handlers::stop_vm))
        .route("/agent/vms/{id}/force-stop", post(handlers::force_stop_vm))
        .route("/agent/vms/{id}/destroy", post(handlers::destroy_vm))
        .route("/agent/vms/{id}/config", put(handlers::update_vm_config))
        // Storage commands (Cluster sends these)
        .route("/agent/storage/mount", post(handlers::mount_datastore))
        .route("/agent/storage/unmount", post(handlers::unmount_datastore))
        .route("/agent/storage/create-disk", post(handlers::create_disk))
        .route("/agent/storage/delete-disk", delete(handlers::delete_disk))
        // Direct host-to-host migration
        .route("/agent/migration/send", post(handlers::migration_send))
        .route("/agent/migration/receive", post(handlers::migration_receive))
        // Network / Bridge management (Cluster sends these)
        .route("/agent/network/bridge/setup", post(handlers::setup_bridge))
        .route("/agent/network/bridge/teardown", post(handlers::teardown_bridge))
        // Logs (cluster fetches service logs from hosts)
        .route("/agent/logs", get(handlers::logs))
        // Package management + command execution (for storage wizard)
        .route("/agent/packages/check", post(handlers::check_packages))
        .route("/agent/packages/install", post(handlers::install_packages))
        .route("/agent/exec", post(handlers::exec_command))
}
