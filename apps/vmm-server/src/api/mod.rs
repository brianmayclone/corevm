//! REST API route assembly.

use axum::{Router, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::AppState;

pub mod auth;
pub mod system;
pub mod users;
pub mod vms;
pub mod storage;

/// Build the complete API router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // System (health check — no auth)
        .route("/api/system/info", get(system::info))
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
        // Disk images
        .route("/api/storage/images", get(storage::list_images).post(storage::create_image))
        .route("/api/storage/images/{id}", delete(storage::delete_image))
        .route("/api/storage/images/{id}/resize", post(storage::resize_image))
        // ISOs
        .route("/api/storage/isos", get(storage::list_isos))
        .route("/api/storage/isos/upload", post(storage::upload_iso))
        .route("/api/storage/isos/{id}", delete(storage::delete_iso))
        // WebSocket console
        .route("/ws/console/{vm_id}", get(crate::ws::console::handler))
}
