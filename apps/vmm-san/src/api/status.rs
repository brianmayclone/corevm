//! Status and health endpoints for CoreSAN.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Serialize)]
pub struct StatusResponse {
    pub running: bool,
    pub node_id: String,
    pub hostname: String,
    pub uptime_secs: u64,
    pub volumes: Vec<VolumeStatusSummary>,
    pub peer_count: u32,
    pub available_disks: u32,
    pub claimed_disks: u32,
    pub benchmark_summary: Option<BenchmarkSummary>,
}

#[derive(Serialize)]
pub struct VolumeStatusSummary {
    pub volume_id: String,
    pub volume_name: String,
    pub resilience_mode: String,
    pub replica_count: u32,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub status: String,
    pub backend_count: u32,
    pub files_synced: u64,
    pub files_syncing: u64,
}

#[derive(Serialize)]
pub struct BenchmarkSummary {
    pub avg_bandwidth_mbps: f64,
    pub avg_latency_us: f64,
    pub worst_peer: Option<String>,
    pub measured_at: String,
}

#[derive(Serialize)]
pub struct DashboardResponse {
    pub status: StatusResponse,
    pub total_capacity_bytes: u64,
    pub used_capacity_bytes: u64,
    pub total_files: u64,
    pub replication_pending: u64,
    pub integrity_errors: u64,
}

/// GET /api/status — full node status (used by vmm-server heartbeat).
pub async fn status(
    State(state): State<Arc<CoreSanState>>,
) -> Json<StatusResponse> {
    let db = state.db.lock().unwrap();
    let volumes = query_volume_summaries(&db);
    let peer_count = state.peers.len() as u32;
    let benchmark_summary = query_benchmark_summary(&db);

    // Count disks
    let disks = crate::storage::disk::discover_disks(&db);
    let available_disks = disks.iter().filter(|d| matches!(d.status,
        crate::storage::disk::DiskStatus::Available | crate::storage::disk::DiskStatus::HasData { .. }
    )).count() as u32;
    let claimed_disks = disks.iter().filter(|d| matches!(d.status,
        crate::storage::disk::DiskStatus::Claimed { .. }
    )).count() as u32;

    Json(StatusResponse {
        running: true,
        node_id: state.node_id.clone(),
        hostname: state.hostname.clone(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        volumes,
        peer_count,
        available_disks,
        claimed_disks,
        benchmark_summary,
    })
}

/// GET /api/health — minimal health check.
pub async fn health() -> StatusCode {
    StatusCode::OK
}

/// GET /api/dashboard — aggregated dashboard data.
pub async fn dashboard(
    State(state): State<Arc<CoreSanState>>,
) -> Json<DashboardResponse> {
    let db = state.db.lock().unwrap();
    let volumes = query_volume_summaries(&db);
    let benchmark_summary = query_benchmark_summary(&db);

    let total_capacity_bytes: u64 = volumes.iter().map(|v| v.total_bytes).sum();
    let free_bytes: u64 = volumes.iter().map(|v| v.free_bytes).sum();
    let used_capacity_bytes = total_capacity_bytes.saturating_sub(free_bytes);

    let total_files: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_map", [], |row| row.get(0),
    ).unwrap_or(0);

    let replication_pending: u64 = db.query_row(
        "SELECT COUNT(*) FROM file_replicas WHERE state != 'synced'", [], |row| row.get(0),
    ).unwrap_or(0);

    let integrity_errors: u64 = db.query_row(
        "SELECT COUNT(*) FROM integrity_log WHERE passed = 0", [], |row| row.get(0),
    ).unwrap_or(0);

    let disks = crate::storage::disk::discover_disks(&db);
    let status = StatusResponse {
        running: true,
        node_id: state.node_id.clone(),
        hostname: state.hostname.clone(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        volumes,
        peer_count: state.peers.len() as u32,
        available_disks: disks.iter().filter(|d| matches!(d.status,
            crate::storage::disk::DiskStatus::Available | crate::storage::disk::DiskStatus::HasData { .. }
        )).count() as u32,
        claimed_disks: disks.iter().filter(|d| matches!(d.status,
            crate::storage::disk::DiskStatus::Claimed { .. }
        )).count() as u32,
        benchmark_summary,
    };

    Json(DashboardResponse {
        status,
        total_capacity_bytes,
        used_capacity_bytes,
        total_files,
        replication_pending,
        integrity_errors,
    })
}

fn query_volume_summaries(db: &rusqlite::Connection) -> Vec<VolumeStatusSummary> {
    let mut stmt = db.prepare(
        "SELECT v.id, v.name, v.resilience_mode, v.replica_count, v.status,
                COALESCE(SUM(b.total_bytes), 0) AS total_bytes,
                COALESCE(SUM(b.free_bytes), 0) AS free_bytes,
                COUNT(b.id) AS backend_count
         FROM volumes v
         LEFT JOIN backends b ON b.volume_id = v.id AND b.status = 'online'
         GROUP BY v.id"
    ).unwrap();

    let volumes: Vec<VolumeStatusSummary> = stmt.query_map([], |row| {
        let vol_id: String = row.get(0)?;
        Ok((vol_id, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
            row.get(5)?, row.get(6)?, row.get::<_, u32>(7)?))
    }).unwrap().filter_map(|r| r.ok()).map(|(vol_id, name, mode, replica, status, total, free, bcount)| {
        let (synced, syncing) = query_file_sync_counts(db, &vol_id);
        VolumeStatusSummary {
            volume_id: vol_id,
            volume_name: name,
            resilience_mode: mode,
            replica_count: replica,
            total_bytes: total,
            free_bytes: free,
            status,
            backend_count: bcount,
            files_synced: synced,
            files_syncing: syncing,
        }
    }).collect();

    volumes
}

fn query_file_sync_counts(db: &rusqlite::Connection, volume_id: &str) -> (u64, u64) {
    let synced: u64 = db.query_row(
        "SELECT COUNT(DISTINCT fm.id) FROM file_map fm
         JOIN file_replicas fr ON fr.file_id = fm.id
         WHERE fm.volume_id = ?1 AND fr.state = 'synced'",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    let syncing: u64 = db.query_row(
        "SELECT COUNT(DISTINCT fm.id) FROM file_map fm
         JOIN file_replicas fr ON fr.file_id = fm.id
         WHERE fm.volume_id = ?1 AND fr.state = 'syncing'",
        rusqlite::params![volume_id], |row| row.get(0),
    ).unwrap_or(0);

    (synced, syncing)
}

fn query_benchmark_summary(db: &rusqlite::Connection) -> Option<BenchmarkSummary> {
    let row = db.query_row(
        "SELECT AVG(bandwidth_mbps), AVG(latency_us), MAX(measured_at)
         FROM benchmark_results
         WHERE measured_at > datetime('now', '-10 minutes')",
        [], |row| {
            Ok((
                row.get::<_, Option<f64>>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    ).ok()?;

    let (avg_bw, avg_lat, measured_at) = row;
    let avg_bw = avg_bw?;
    let measured_at = measured_at?;

    // Find worst peer (highest latency)
    let worst_peer: Option<String> = db.query_row(
        "SELECT to_node_id FROM benchmark_results
         WHERE measured_at > datetime('now', '-10 minutes')
         ORDER BY latency_us DESC LIMIT 1",
        [], |row| row.get(0),
    ).ok();

    Some(BenchmarkSummary {
        avg_bandwidth_mbps: avg_bw,
        avg_latency_us: avg_lat.unwrap_or(0.0),
        worst_peer,
        measured_at,
    })
}
