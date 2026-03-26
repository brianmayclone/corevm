//! Benchmark API — results, manual triggers, peer echo/ping.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Serialize)]
pub struct BenchmarkResult {
    pub from_node_id: String,
    pub to_node_id: String,
    pub bandwidth_mbps: f64,
    pub latency_us: f64,
    pub jitter_us: f64,
    pub packet_loss_pct: f64,
    pub test_size_bytes: u64,
    pub measured_at: String,
}

#[derive(Serialize)]
pub struct BenchmarkMatrix {
    pub node_ids: Vec<String>,
    pub entries: Vec<BenchmarkResult>,
}

/// GET /api/benchmark/results — latest benchmark results.
pub async fn results(
    State(state): State<Arc<CoreSanState>>,
) -> Json<Vec<BenchmarkResult>> {
    let db = state.db.lock().unwrap();

    let mut stmt = db.prepare(
        "SELECT from_node_id, to_node_id, bandwidth_mbps, latency_us,
                jitter_us, packet_loss_pct, test_size_bytes, measured_at
         FROM benchmark_results
         WHERE measured_at > datetime('now', '-1 hour')
         ORDER BY measured_at DESC"
    ).unwrap();

    let results = stmt.query_map([], |row| {
        Ok(BenchmarkResult {
            from_node_id: row.get(0)?,
            to_node_id: row.get(1)?,
            bandwidth_mbps: row.get(2)?,
            latency_us: row.get(3)?,
            jitter_us: row.get(4)?,
            packet_loss_pct: row.get(5)?,
            test_size_bytes: row.get(6)?,
            measured_at: row.get(7)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Json(results)
}

/// POST /api/benchmark/run — trigger a manual benchmark run.
pub async fn run(
    State(state): State<Arc<CoreSanState>>,
) -> Json<serde_json::Value> {
    let peer_count = state.peers.len();
    tracing::info!("Manual benchmark triggered against {} peers", peer_count);

    // Spawn the benchmark in the background
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        crate::engine::benchmark::run_benchmarks(&state_clone).await;
    });

    Json(serde_json::json!({
        "triggered": true,
        "peer_count": peer_count
    }))
}

/// GET /api/benchmark/matrix — N×N peer performance matrix.
pub async fn matrix(
    State(state): State<Arc<CoreSanState>>,
) -> Json<BenchmarkMatrix> {
    let db = state.db.lock().unwrap();

    // Get all node IDs (self + peers)
    let mut node_ids = vec![state.node_id.clone()];
    for peer in state.peers.iter() {
        node_ids.push(peer.node_id.clone());
    }

    // Get latest benchmark for each pair
    let mut stmt = db.prepare(
        "SELECT from_node_id, to_node_id, bandwidth_mbps, latency_us,
                jitter_us, packet_loss_pct, test_size_bytes, measured_at
         FROM benchmark_results br
         WHERE measured_at = (
             SELECT MAX(measured_at) FROM benchmark_results
             WHERE from_node_id = br.from_node_id AND to_node_id = br.to_node_id
         )"
    ).unwrap();

    let entries = stmt.query_map([], |row| {
        Ok(BenchmarkResult {
            from_node_id: row.get(0)?,
            to_node_id: row.get(1)?,
            bandwidth_mbps: row.get(2)?,
            latency_us: row.get(3)?,
            jitter_us: row.get(4)?,
            packet_loss_pct: row.get(5)?,
            test_size_bytes: row.get(6)?,
            measured_at: row.get(7)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Json(BenchmarkMatrix { node_ids, entries })
}

/// GET /api/benchmark/ping — minimal response for latency measurement.
pub async fn ping() -> StatusCode {
    StatusCode::OK
}

/// POST /api/benchmark/echo — receive data and echo it back (for throughput measurement).
pub async fn echo(body: axum::body::Bytes) -> axum::body::Bytes {
    body
}
