//! API router assembly — all REST endpoints for the cluster.
//!
//! The cluster exposes the SAME endpoint structure as vmm-server (so the UI
//! can connect to either), plus additional cluster-specific endpoints.

use axum::{Router, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::ClusterState;

pub mod auth;
pub mod system;
pub mod users;
pub mod clusters;
pub mod hosts;
pub mod vms;

pub fn router() -> Router<Arc<ClusterState>> {
    Router::new()
        // ── Auth ────────────────────────────────────────
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))

        // ── System ──────────────────────────────────────
        .route("/api/system/info", get(system::info))
        .route("/api/system/stats", get(system::stats))

        // ── Users (admin only) ──────────────────────────
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{id}", put(users::update).delete(users::delete))
        .route("/api/users/{id}/password", put(users::change_password))

        // ── Clusters (new — cluster management) ─────────
        .route("/api/clusters", get(clusters::list).post(clusters::create))
        .route("/api/clusters/{id}", get(clusters::get).put(clusters::update).delete(clusters::delete))

        // ── Hosts (new — host management) ───────────────
        .route("/api/hosts", get(hosts::list).post(hosts::register))
        .route("/api/hosts/{id}", get(hosts::get).delete(hosts::deregister))
        .route("/api/hosts/{id}/maintenance", post(hosts::enter_maintenance))
        .route("/api/hosts/{id}/activate", post(hosts::exit_maintenance))

        // ── VMs (cluster authority) ─────────────────────
        .route("/api/vms", get(vms::list).post(vms::create))
        .route("/api/vms/{id}", get(vms::get).delete(vms::delete))
        .route("/api/vms/{id}/start", post(vms::start))
        .route("/api/vms/{id}/stop", post(vms::stop))
        .route("/api/vms/{id}/force-stop", post(vms::force_stop))

        // TODO: Phase 3 — Storage/Datastore endpoints
        // TODO: Phase 5 — Migration, DRS, Tasks endpoints
        // TODO: Phase 6 — WebSocket console/terminal bridging
}
