//! API router assembly — all REST endpoints for CoreSAN.

use axum::{Router, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::CoreSanState;

pub mod volumes;
pub mod backends;
pub mod peers;
pub mod files;
pub mod status;
pub mod benchmark;
pub mod disks;

pub fn router() -> Router<Arc<CoreSanState>> {
    Router::new()
        // ── Status & Health ───────────────────────────────
        .route("/api/status", get(status::status))
        .route("/api/health", get(status::health))
        .route("/api/dashboard", get(status::dashboard))

        // ── Physical Disks ────────────────────────────────
        .route("/api/disks", get(disks::list))
        .route("/api/disks/claim", post(disks::claim))
        .route("/api/disks/release", post(disks::release))
        .route("/api/disks/reset", post(disks::reset))

        // ── Volumes (CRUD + resilience policy) ────────────
        .route("/api/volumes", get(volumes::list).post(volumes::create))
        .route("/api/volumes/{id}", get(volumes::get).put(volumes::update).delete(volumes::delete))

        // ── Backends (mountpoints per volume) ─────────────
        .route("/api/volumes/{id}/backends", get(backends::list).post(backends::add))
        .route("/api/volumes/{volume_id}/backends/{backend_id}", delete(backends::remove))

        // ── Peers ─────────────────────────────────────────
        .route("/api/peers", get(peers::list))
        .route("/api/peers/join", post(peers::join))
        .route("/api/peers/{node_id}", delete(peers::remove))
        .route("/api/peers/heartbeat", post(peers::heartbeat))

        // ── File Operations ───────────────────────────────
        .route("/api/volumes/{id}/files", get(files::list))
        .route("/api/volumes/{id}/files/{*path}", get(files::read).put(files::write).delete(files::delete))

        // ── Benchmark ─────────────────────────────────────
        .route("/api/benchmark/results", get(benchmark::results))
        .route("/api/benchmark/run", post(benchmark::run))
        .route("/api/benchmark/matrix", get(benchmark::matrix))
        .route("/api/benchmark/ping", get(benchmark::ping))
        .route("/api/benchmark/echo", post(benchmark::echo))
}
