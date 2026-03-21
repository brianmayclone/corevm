//! REST API route assembly.

use axum::{Router, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::AppState;

pub mod auth;
pub mod system;
pub mod users;
pub mod vms;
pub mod storage;
pub mod network;
pub mod settings;
pub mod resource_groups;
pub mod guard;

/// Build the complete API router (regular API + agent API).
/// The managed-mode guard middleware is applied in main.rs after with_state().
pub fn router() -> Router<Arc<AppState>> {
    let agent_routes = crate::agent::router();

    Router::new()
        .merge(agent_routes)
        // System
        .route("/api/system/info", get(system::info))
        .route("/api/system/stats", get(system::stats))
        .route("/api/system/activity", get(system::activity))
        // Auth
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))
        // Users (admin)
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{id}", put(users::update).delete(users::delete))
        .route("/api/users/{id}/password", put(users::change_password))
        // VMs
        .route("/api/vms", get(vms::list).post(vms::create))
        .route("/api/vms/{id}", get(vms::get).put(vms::update).delete(vms::delete))
        .route("/api/vms/{id}/start", post(vms::start))
        .route("/api/vms/{id}/stop", post(vms::stop))
        .route("/api/vms/{id}/force-stop", post(vms::force_stop))
        .route("/api/vms/{id}/screenshot", get(vms::screenshot))
        // Storage pools
        .route("/api/storage/pools", get(storage::list_pools).post(storage::create_pool))
        .route("/api/storage/pools/{id}", delete(storage::delete_pool))
        .route("/api/storage/pools/{id}/browse", get(storage::browse_pool))
        .route("/api/storage/stats", get(storage::storage_stats))
        .route("/api/storage/vm-disk", post(storage::create_vm_disk))
        // Disk images
        .route("/api/storage/images", get(storage::list_images).post(storage::create_image))
        .route("/api/storage/images/{id}", delete(storage::delete_image))
        .route("/api/storage/images/{id}/resize", post(storage::resize_image))
        // ISOs
        .route("/api/storage/isos", get(storage::list_isos))
        .route("/api/storage/isos/upload", post(storage::upload_iso))
        .route("/api/storage/isos/{id}", delete(storage::delete_iso))
        // Resource Groups
        .route("/api/resource-groups", get(resource_groups::list).post(resource_groups::create))
        .route("/api/resource-groups/permissions-list", get(resource_groups::permissions_list))
        .route("/api/resource-groups/{id}", get(resource_groups::get).put(resource_groups::update).delete(resource_groups::delete))
        .route("/api/resource-groups/{id}/permissions", post(resource_groups::set_permissions).delete(resource_groups::remove_permissions))
        .route("/api/resource-groups/{id}/assign-vm", post(resource_groups::assign_vm))
        // Network
        .route("/api/network/interfaces", get(network::list_interfaces))
        .route("/api/network/stats", get(network::network_stats))
        // Settings
        .route("/api/settings/server", get(settings::get_server))
        .route("/api/settings/time", get(settings::get_time))
        .route("/api/settings/time/timezone", put(settings::set_timezone))
        .route("/api/settings/security", get(settings::get_security))
        .route("/api/settings/groups", get(settings::list_groups).post(settings::create_group))
        .route("/api/settings/groups/{id}", delete(settings::delete_group))
        // WebSocket console + terminal — always allowed
        .route("/ws/console/{vm_id}", get(crate::ws::console::handler))
        .route("/ws/terminal", get(crate::ws::terminal::ws_terminal))
}
