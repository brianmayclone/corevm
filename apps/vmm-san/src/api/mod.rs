//! API router assembly — all REST endpoints for CoreSAN.

use axum::{Router, extract::DefaultBodyLimit, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::CoreSanState;

pub mod volumes;
pub mod backends;
pub mod peers;
pub mod files;
pub mod chunks;
pub mod status;
pub mod benchmark;
pub mod disks;
pub mod s3;

pub fn router() -> Router<Arc<CoreSanState>> {
    Router::new()
        // ── Status & Health ───────────────────────────────
        .route("/api/status", get(status::status))
        .route("/api/health", get(status::health))
        .route("/api/dashboard", get(status::dashboard))
        .route("/api/network/config", get(status::get_network_config).put(status::update_network_config))
        .route("/api/network/interfaces", get(status::list_interfaces))

        // ── Physical Disks ────────────────────────────────
        .route("/api/disks", get(disks::list))
        .route("/api/disks/claim", post(disks::claim))
        .route("/api/disks/release", post(disks::release))
        .route("/api/disks/reset", post(disks::reset))
        .route("/api/disks/{device_name}/smart", get(disks::smart_detail))

        // ── Volumes (CRUD + resilience policy) ────────────
        .route("/api/volumes", get(volumes::list).post(volumes::create))
        .route("/api/volumes/sync", post(volumes::sync))
        .route("/api/volumes/{id}", get(volumes::get).put(volumes::update).delete(volumes::delete))
        .route("/api/volumes/{id}/health", get(volumes::health))
        .route("/api/volumes/{id}/repair", post(volumes::trigger_repair))
        .route("/api/volumes/{id}/remove-host", post(volumes::remove_host_from_volume))

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
        .route("/api/volumes/{id}/mkdir", post(files::mkdir))
        .route("/api/volumes/{id}/browse/{*path}", get(files::browse))
        .route("/api/volumes/{id}/browse", get(files::browse_root))
        .route("/api/volumes/{id}/chunk-map", get(files::chunk_map))
        .route("/api/volumes/{id}/allocate-disk", post(files::allocate_disk))

        // ── Chunk Operations (peer-to-peer replication) ───
        .route("/api/chunks/{volume_id}/{file_id}/{chunk_index}",
            get(chunks::read_chunk).put(chunks::write_chunk))
        .route("/api/file-meta/sync", post(chunks::sync_file_meta))

        // ── S3 Credential Management ─────────────────────
        .route("/api/s3/credentials", get(s3::list).post(s3::create))
        .route("/api/s3/credentials/{id}", delete(s3::delete))

        // ── Benchmark ─────────────────────────────────────
        .route("/api/benchmark/results", get(benchmark::results))
        .route("/api/benchmark/run", post(benchmark::run))
        .route("/api/benchmark/matrix", get(benchmark::matrix))
        .route("/api/benchmark/ping", get(benchmark::ping))
        .route("/api/benchmark/echo", post(benchmark::echo))
        // Allow uploads up to 10 GB (ISOs, disk images)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024 * 1024))
}
