//! Benchmark service — all database operations for benchmark results.

use rusqlite::Connection;

pub struct BenchmarkService;

impl BenchmarkService {
    pub fn store_result(db: &Connection, from_node: &str, to_node: &str,
                        bandwidth: f64, latency: f64, jitter: f64, loss: f64, test_size: i64) {
        db.execute(
            "INSERT INTO benchmark_results (from_node_id, to_node_id, bandwidth_mbps, latency_us, jitter_us, packet_loss_pct, test_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![from_node, to_node, bandwidth, latency, jitter, loss, test_size],
        ).ok();
    }

    pub fn cleanup_old(db: &Connection) {
        db.execute("DELETE FROM benchmark_results WHERE measured_at < datetime('now', '-24 hours')", []).ok();
    }

    pub fn get_recent_summary(db: &Connection) -> Option<(f64, f64, Option<String>, String)> {
        db.query_row(
            "SELECT AVG(bandwidth_mbps), AVG(latency_us), MAX(measured_at)
             FROM benchmark_results WHERE measured_at > datetime('now', '-10 minutes')",
            [], |row| Ok((
                row.get::<_, Option<f64>>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            )),
        ).ok().and_then(|(bw, lat, ts)| {
            let bw = bw?;
            let ts = ts?;
            let worst: Option<String> = db.query_row(
                "SELECT to_node_id FROM benchmark_results WHERE measured_at > datetime('now', '-10 minutes')
                 ORDER BY latency_us DESC LIMIT 1",
                [], |row| row.get(0),
            ).ok();
            Some((bw, lat.unwrap_or(0.0), worst, ts))
        })
    }
}
