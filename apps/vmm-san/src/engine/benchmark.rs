//! Benchmark engine — automatic periodic network performance testing between peers.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::peer::client::PeerClient;

/// Spawn the benchmark engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    if !state.config.benchmark.enabled {
        tracing::info!("Benchmark engine disabled");
        return;
    }

    let interval_secs = state.config.benchmark.interval_secs;
    tokio::spawn(async move {
        // Wait a bit before first benchmark to let peers connect
        tokio::time::sleep(Duration::from_secs(30)).await;

        let mut tick = interval(Duration::from_secs(interval_secs));
        loop {
            tick.tick().await;
            run_benchmarks(&state).await;
        }
    });
}

/// Run benchmarks against all online peers.
pub async fn run_benchmarks(state: &CoreSanState) {
    let client = PeerClient::new(&state.config.peer.secret);
    let test_size = (state.config.benchmark.bandwidth_test_size_mb as usize) * 1024 * 1024;

    let peer_list: Vec<(String, String)> = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .map(|p| (p.node_id.clone(), p.address.clone()))
        .collect();

    if peer_list.is_empty() {
        return;
    }

    tracing::info!("Running benchmarks against {} peers ({}× 8KB = {}MB)",
        peer_list.len(), test_size / 8192, state.config.benchmark.bandwidth_test_size_mb);

    for (peer_id, peer_address) in &peer_list {
        // --- Latency test (multiple pings) ---
        let mut latencies = Vec::new();
        for _ in 0..10 {
            match client.ping(peer_address).await {
                Ok(d) => latencies.push(d.as_micros() as f64),
                Err(_) => continue,
            }
        }

        let (avg_latency_us, jitter_us) = if latencies.is_empty() {
            (0.0, 0.0)
        } else {
            let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
            let variance = latencies.iter()
                .map(|l| (l - avg).powi(2))
                .sum::<f64>() / latencies.len() as f64;
            (avg, variance.sqrt())
        };

        // --- Throughput test (many small 8KB echo requests) ---
        // Using small payloads avoids HTTP timeout issues on virtual/slow links
        // while still measuring realistic throughput.
        const CHUNK_SIZE: usize = 8192;
        let iterations = test_size / CHUNK_SIZE;
        let chunk = vec![0xABu8; CHUNK_SIZE];

        let mut total_sent: u64 = 0;
        let mut total_received: u64 = 0;
        let mut failed: u64 = 0;
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            total_sent += CHUNK_SIZE as u64;
            match client.echo(peer_address, &chunk).await {
                Ok((_dur, received_bytes)) => {
                    total_received += received_bytes as u64;
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        let bandwidth_mbps = if elapsed > 0.0 {
            (total_received as f64 * 8.0) / (elapsed * 1_000_000.0)
        } else {
            0.0
        };
        let packet_loss_pct = if total_sent > 0 {
            ((1.0 - (total_received as f64 / total_sent as f64)) * 100.0).max(0.0)
        } else {
            0.0
        };

        if failed > 0 {
            tracing::warn!("Benchmark echo: {}/{} requests failed for {}",
                failed, iterations, peer_id);
        }

        // Store results
        let db = state.db.lock().unwrap();
        db.execute(
            "INSERT INTO benchmark_results (from_node_id, to_node_id, bandwidth_mbps,
                latency_us, jitter_us, packet_loss_pct, test_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &state.node_id, peer_id, bandwidth_mbps,
                avg_latency_us, jitter_us, packet_loss_pct,
                test_size as i64
            ],
        ).ok();

        tracing::info!("Benchmark -> {}: {:.0} Mbit/s, {:.0}μs latency, {:.1}μs jitter, {:.1}% loss",
            peer_id, bandwidth_mbps, avg_latency_us, jitter_us, packet_loss_pct);
    }

    // Cleanup old results (keep last 24 hours)
    let db = state.db.lock().unwrap();
    db.execute(
        "DELETE FROM benchmark_results WHERE measured_at < datetime('now', '-24 hours')",
        [],
    ).ok();
}
