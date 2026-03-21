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
pub mod storage;
pub mod events;
pub mod tasks;
pub mod drs;
pub mod migration;
pub mod alarms;
pub mod activity;

pub fn router() -> Router<Arc<ClusterState>> {
    Router::new()
        // ── Auth ────────────────────────────────────────
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))

        // ── System ──────────────────────────────────────
        .route("/api/system/info", get(system::info))
        .route("/api/system/stats", get(system::stats))
        .route("/api/system/activity", get(activity::activity))

        // ── Users (admin only) ──────────────────────────
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{id}", put(users::update).delete(users::delete))
        .route("/api/users/{id}/password", put(users::change_password))

        // ── Clusters ────────────────────────────────────
        .route("/api/clusters", get(clusters::list).post(clusters::create))
        .route("/api/clusters/{id}", get(clusters::get).put(clusters::update).delete(clusters::delete))

        // ── Hosts ───────────────────────────────────────
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
        .route("/api/vms/{id}/migrate", post(migration::migrate))

        // ── Storage (cluster-wide datastores + compat endpoints) ────
        .route("/api/storage/datastores", get(storage::list_datastores).post(storage::create_datastore))
        .route("/api/storage/datastores/{id}", get(storage::get_datastore).delete(storage::delete_datastore))
        .route("/api/storage/pools", get(activity::list_storage_pools))
        .route("/api/storage/pools/{id}/browse", get(activity::browse_storage_pool))
        .route("/api/storage/stats", get(activity::storage_stats))
        .route("/api/storage/images", get(activity::list_images))
        .route("/api/storage/isos", get(activity::list_isos))

        // ── Resource Groups (compat) ────────────────────
        .route("/api/resource-groups", get(activity::list_resource_groups))

        // ── Network (compat stubs) ──────────────────────
        .route("/api/network/interfaces", get(activity::network_interfaces))
        .route("/api/network/stats", get(activity::network_stats))

        // ── Events ──────────────────────────────────────
        .route("/api/events", get(events::list))

        // ── Tasks ───────────────────────────────────────
        .route("/api/tasks", get(tasks::list))

        // ── DRS ─────────────────────────────────────────
        .route("/api/drs/recommendations", get(drs::list))
        .route("/api/drs/{id}/apply", post(drs::apply))
        .route("/api/drs/{id}/dismiss", post(drs::dismiss))

        // ── Alarms ──────────────────────────────────────
        .route("/api/alarms", get(alarms::list))
        .route("/api/alarms/{id}/acknowledge", post(alarms::acknowledge))

        // ── WebSocket ───────────────────────────────────
        .route("/ws/console/{vm_id}", get(crate::ws::console_bridge::handler))
}
