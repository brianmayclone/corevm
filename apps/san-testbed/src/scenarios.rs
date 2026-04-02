//! Automated test scenarios for CoreSAN testbed.

use crate::context::TestContext;
use crate::witness::WitnessMode;

#[derive(Clone)]
pub struct ScenarioResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub duration: std::time::Duration,
}

/// Run a single scenario with fresh context, timing, and error capture.
macro_rules! run_scenario {
    ($name:expr, $num_nodes:expr, $body:expr) => {{
        tracing::info!("━━━ Scenario: {} ({} nodes) ━━━", $name, $num_nodes);
        let start = std::time::Instant::now();
        let result: Result<(), String> = async {
            let mut ctx = TestContext::new($num_nodes).await?;
            ctx.wait_all_healthy().await?;
            tracing::debug!("All nodes healthy, running scenario logic...");
            let r = $body(&mut ctx).await;
            match &r {
                Ok(_) => tracing::debug!("Scenario logic completed successfully"),
                Err(e) => tracing::warn!("Scenario logic failed: {}", e),
            }
            ctx.shutdown();
            r
        }.await;

        ScenarioResult {
            name: $name.to_string(),
            passed: result.is_ok(),
            message: result.err().unwrap_or_else(|| "OK".into()),
            duration: start.elapsed(),
        }
    }};
}

pub async fn run_all(_seed: u64) -> Vec<ScenarioResult> {
    let mut results = Vec::new();

    results.push(run_scenario!("quorum-degraded", 3, scenario_quorum_degraded));
    results.push(run_scenario!("quorum-fenced", 3, scenario_quorum_fenced));
    results.push(run_scenario!("quorum-recovery", 3, scenario_quorum_recovery));
    results.push(run_scenario!("fenced-write-denied", 3, scenario_fenced_write_denied));
    results.push(run_scenario!("fenced-read-allowed", 3, scenario_fenced_read_allowed));
    results.push(run_scenario!("leader-failover", 3, scenario_leader_failover));
    results.push(run_scenario!("partition-majority", 3, scenario_partition_majority));
    results.push(run_scenario!("partition-witness-2node", 2, scenario_partition_witness_2node));
    results.push(run_scenario!("replication-basic", 3, scenario_replication_basic));
    results.push(run_scenario!("repair-leader-only", 3, scenario_repair_leader_only));
    results.push(run_scenario!("transfer-small", 3, scenario_transfer_small));
    results.push(run_scenario!("transfer-large", 3, scenario_transfer_large));
    results.push(run_scenario!("transfer-throughput", 3, scenario_transfer_throughput));
    results.push(run_scenario!("cross-node-read", 3, scenario_cross_node_read));
    results.push(run_scenario!("replication-verify", 3, scenario_replication_verify));

    results
}

pub async fn run_single(name: &str, _seed: u64) -> Option<ScenarioResult> {
    match name {
        "quorum-degraded" => Some(run_scenario!(name, 3, scenario_quorum_degraded)),
        "quorum-fenced" => Some(run_scenario!(name, 3, scenario_quorum_fenced)),
        "quorum-recovery" => Some(run_scenario!(name, 3, scenario_quorum_recovery)),
        "fenced-write-denied" => Some(run_scenario!(name, 3, scenario_fenced_write_denied)),
        "fenced-read-allowed" => Some(run_scenario!(name, 3, scenario_fenced_read_allowed)),
        "leader-failover" => Some(run_scenario!(name, 3, scenario_leader_failover)),
        "partition-majority" => Some(run_scenario!(name, 3, scenario_partition_majority)),
        "partition-witness-2node" => Some(run_scenario!(name, 2, scenario_partition_witness_2node)),
        "replication-basic" => Some(run_scenario!(name, 3, scenario_replication_basic)),
        "repair-leader-only" => Some(run_scenario!(name, 3, scenario_repair_leader_only)),
        "transfer-small" => Some(run_scenario!(name, 3, scenario_transfer_small)),
        "transfer-large" => Some(run_scenario!(name, 3, scenario_transfer_large)),
        "transfer-throughput" => Some(run_scenario!(name, 3, scenario_transfer_throughput)),
        "cross-node-read" => Some(run_scenario!(name, 3, scenario_cross_node_read)),
        "replication-verify" => Some(run_scenario!(name, 3, scenario_replication_verify)),
        _ => None,
    }
}

// ── Scenario implementations ──────────────────────────────────

async fn scenario_quorum_degraded(ctx: &mut TestContext) -> Result<(), String> {
    ctx.kill_node(3).await;
    ctx.wait_secs(25).await;
    let s1 = ctx.get_status(1).await?;
    let s2 = ctx.get_status(2).await?;
    if s1.quorum_status != "degraded" { return Err(format!("node 1 expected degraded, got {}", s1.quorum_status)); }
    if s2.quorum_status != "degraded" { return Err(format!("node 2 expected degraded, got {}", s2.quorum_status)); }

    // Leader election may take a few more heartbeat cycles — poll with timeout
    for attempt in 0..6 {
        let s1 = ctx.get_status(1).await?;
        let s2 = ctx.get_status(2).await?;
        if s1.is_leader ^ s2.is_leader {
            tracing::debug!("Leader found after {} extra polls", attempt);
            return Ok(());
        }
        tracing::debug!("No unique leader yet (s1.leader={}, s2.leader={}), waiting 5s...", s1.is_leader, s2.is_leader);
        ctx.wait_secs(5).await;
    }
    let s1 = ctx.get_status(1).await?;
    let s2 = ctx.get_status(2).await?;

    // Dump node logs for debugging leader election
    for i in 1..=2 {
        let log = ctx.read_log(i);
        let leader_lines: Vec<&str> = log.lines()
            .filter(|l| l.contains("Leader calc") || l.contains("leader") || l.contains("is now the leader"))
            .collect();
        tracing::warn!("Node {} leader-related log lines ({} total):", i, leader_lines.len());
        for line in leader_lines.iter().rev().take(10).rev() {
            tracing::warn!("  {}", line);
        }
    }

    Err(format!("expected exactly one leader, got s1.leader={} s2.leader={}", s1.is_leader, s2.is_leader))
}

async fn scenario_quorum_fenced(ctx: &mut TestContext) -> Result<(), String> {
    ctx.set_witness_mode(WitnessMode::DenyAll);
    ctx.kill_node(2).await;
    ctx.kill_node(3).await;
    ctx.wait_secs(25).await;
    let s1 = ctx.get_status(1).await?;
    if s1.quorum_status != "fenced" { return Err(format!("node 1 expected fenced, got {}", s1.quorum_status)); }
    if s1.is_leader { return Err("fenced node should not be leader".into()); }
    Ok(())
}

async fn scenario_quorum_recovery(ctx: &mut TestContext) -> Result<(), String> {
    ctx.set_witness_mode(WitnessMode::DenyAll);
    ctx.kill_node(2).await;
    ctx.kill_node(3).await;
    ctx.wait_secs(25).await;
    let s1 = ctx.get_status(1).await?;
    if s1.quorum_status != "fenced" { return Err(format!("node 1 should be fenced first, got {}", s1.quorum_status)); }

    // Recover: start node 2
    ctx.start_node(2).await?;
    ctx.wait_node_healthy(2).await?;
    ctx.wait_secs(15).await;

    let s1 = ctx.get_status(1).await?;
    if s1.quorum_status == "fenced" { return Err("node 1 should have recovered from fenced".into()); }
    Ok(())
}

async fn scenario_fenced_write_denied(ctx: &mut TestContext) -> Result<(), String> {
    ctx.set_witness_mode(WitnessMode::DenyAll);
    ctx.kill_node(2).await;
    ctx.kill_node(3).await;
    ctx.wait_secs(25).await;

    let status = ctx.write_file(1, "testbed-vol", "test.txt", b"hello").await?;
    if status != 503 {
        return Err(format!("expected 503, got {}", status));
    }
    Ok(())
}

async fn scenario_fenced_read_allowed(ctx: &mut TestContext) -> Result<(), String> {
    // Write a file while healthy
    let status = ctx.write_file(1, "testbed-vol", "readtest.txt", b"hello world").await?;
    if status != 200 && status != 201 {
        return Err(format!("write failed with status {}", status));
    }

    // Fence node 1
    ctx.set_witness_mode(WitnessMode::DenyAll);
    ctx.kill_node(2).await;
    ctx.kill_node(3).await;
    ctx.wait_secs(25).await;

    // Read should still work
    let (read_status, body) = ctx.read_file(1, "testbed-vol", "readtest.txt").await?;
    if read_status != 200 { return Err(format!("read expected 200, got {}", read_status)); }
    if body != b"hello world" { return Err("read returned wrong content".into()); }
    Ok(())
}

async fn scenario_leader_failover(ctx: &mut TestContext) -> Result<(), String> {
    ctx.wait_secs(10).await;

    // Find current leader
    let mut leader_idx = 0;
    for i in 1..=3 {
        let s = ctx.get_status(i).await?;
        if s.is_leader { leader_idx = i; break; }
    }
    if leader_idx == 0 { return Err("no leader found".into()); }

    // Kill leader
    ctx.kill_node(leader_idx).await;
    ctx.wait_secs(25).await;

    // Check remaining nodes — one should be leader
    let mut new_leader = false;
    for i in 1..=3 {
        if i == leader_idx { continue; }
        if let Ok(s) = ctx.get_status(i).await {
            if s.is_leader { new_leader = true; }
        }
    }
    if !new_leader { return Err("no new leader elected after failover".into()); }
    Ok(())
}

async fn scenario_partition_majority(ctx: &mut TestContext) -> Result<(), String> {
    // Deny witness so isolated node 3 can't get witness-based quorum
    ctx.set_witness_mode(WitnessMode::DenyAll);
    ctx.partition(&[1, 2], &[3]).await?;
    ctx.wait_secs(25).await;

    let s1 = ctx.get_status(1).await?;
    let s2 = ctx.get_status(2).await?;
    let s3 = ctx.get_status(3).await?;

    if s1.quorum_status != "degraded" { return Err(format!("node 1 expected degraded, got {}", s1.quorum_status)); }
    if s2.quorum_status != "degraded" { return Err(format!("node 2 expected degraded, got {}", s2.quorum_status)); }
    if s3.quorum_status != "fenced" { return Err(format!("node 3 expected fenced, got {}", s3.quorum_status)); }
    Ok(())
}

async fn scenario_partition_witness_2node(ctx: &mut TestContext) -> Result<(), String> {
    // Smart mode: lowest node_id gets allowed, other denied
    ctx.set_witness_mode(WitnessMode::Smart);
    ctx.partition(&[1], &[2]).await?;
    ctx.wait_secs(25).await;

    let s1 = ctx.get_status(1).await?;
    let s2 = ctx.get_status(2).await?;

    // node-1 is lowest → witness allows → degraded
    if s1.quorum_status != "degraded" { return Err(format!("node 1 (lowest) expected degraded, got {}", s1.quorum_status)); }
    // node-2 is higher → witness denies → fenced
    if s2.quorum_status != "fenced" { return Err(format!("node 2 expected fenced, got {}", s2.quorum_status)); }
    Ok(())
}

async fn scenario_replication_basic(ctx: &mut TestContext) -> Result<(), String> {
    let status = ctx.write_file(1, "testbed-vol", "repltest.txt", b"replicate me").await?;
    if status != 200 && status != 201 {
        return Err(format!("write failed with status {}", status));
    }

    // Wait for push replication
    ctx.wait_secs(10).await;

    let (read_status, body) = ctx.read_file(2, "testbed-vol", "repltest.txt").await?;
    if read_status != 200 { return Err(format!("read from node 2 expected 200, got {}", read_status)); }
    if body != b"replicate me" { return Err(format!("wrong content: {:?}", String::from_utf8_lossy(&body))); }
    Ok(())
}

async fn scenario_repair_leader_only(ctx: &mut TestContext) -> Result<(), String> {
    ctx.wait_secs(15).await;

    // Find non-leader
    let mut non_leader_idx = 0;
    for i in 1..=3 {
        let s = ctx.get_status(i).await?;
        if !s.is_leader { non_leader_idx = i; break; }
    }
    if non_leader_idx == 0 { return Err("all nodes claim to be leader".into()); }

    let log = ctx.read_log(non_leader_idx);
    if !log.contains("skipping repair") && !log.contains("Not leader") {
        return Err(format!("non-leader node {} log doesn't contain repair skip message", non_leader_idx));
    }
    Ok(())
}

// ── File transfer & performance scenarios ────────────────────

/// Small file write + read with timing.
async fn scenario_transfer_small(ctx: &mut TestContext) -> Result<(), String> {
    let data = vec![0xABu8; 1024]; // 1 KB
    let (status, write_dur, write_bps) = ctx.write_file_timed(1, "testbed-vol", "small.bin", &data).await?;
    if status >= 400 { return Err(format!("write failed: HTTP {}", status)); }

    let (status, body, read_dur, read_bps) = ctx.read_file_timed(1, "testbed-vol", "small.bin").await?;
    if status != 200 { return Err(format!("read failed: HTTP {}", status)); }
    if body != data { return Err("read content mismatch".into()); }

    tracing::info!("Small file (1 KB): write {:.1}ms, read {:.1}ms",
        write_dur.as_secs_f64() * 1000.0, read_dur.as_secs_f64() * 1000.0);
    let _ = (write_bps, read_bps); // logged inside helpers
    Ok(())
}

/// Large file write + read with timing (512 KB — safe under Axum 2 MiB default).
async fn scenario_transfer_large(ctx: &mut TestContext) -> Result<(), String> {
    let size = 512 * 1024; // 512 KB
    // Deterministic pattern so we can verify integrity
    let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

    let (status, write_dur, _) = ctx.write_file_timed(1, "testbed-vol", "large.bin", &data).await?;
    if status >= 400 { return Err(format!("write failed: HTTP {}", status)); }

    let (status, body, read_dur, _) = ctx.read_file_timed(1, "testbed-vol", "large.bin").await?;
    if status != 200 { return Err(format!("read failed: HTTP {}", status)); }
    if body.len() != data.len() {
        return Err(format!("size mismatch: wrote {} bytes, read {} bytes", data.len(), body.len()));
    }
    if body != data { return Err("content mismatch (data corruption)".into()); }

    tracing::info!("Large file (512 KB): write {:.1}ms, read {:.1}ms",
        write_dur.as_secs_f64() * 1000.0, read_dur.as_secs_f64() * 1000.0);
    Ok(())
}

/// Throughput test: write multiple files sequentially and measure aggregate speed.
async fn scenario_transfer_throughput(ctx: &mut TestContext) -> Result<(), String> {
    let file_size = 64 * 1024; // 64 KB per file
    let num_files = 10;
    let data: Vec<u8> = (0..file_size).map(|i| (i % 199) as u8).collect();

    let start = std::time::Instant::now();
    for i in 0..num_files {
        let path = format!("throughput/file-{}.bin", i);
        let (status, _, _) = ctx.write_file_timed(1, "testbed-vol", &path, &data).await?;
        if status >= 400 { return Err(format!("write {} failed: HTTP {}", path, status)); }
    }
    let write_elapsed = start.elapsed();
    let total_bytes = file_size * num_files;
    let write_mbps = (total_bytes as f64 / 1_048_576.0) / write_elapsed.as_secs_f64();

    // Read them all back
    let start = std::time::Instant::now();
    for i in 0..num_files {
        let path = format!("throughput/file-{}.bin", i);
        let (status, body, _, _) = ctx.read_file_timed(1, "testbed-vol", &path).await?;
        if status != 200 { return Err(format!("read {} failed: HTTP {}", path, status)); }
        if body.len() != file_size { return Err(format!("size mismatch on {}", path)); }
    }
    let read_elapsed = start.elapsed();
    let read_mbps = (total_bytes as f64 / 1_048_576.0) / read_elapsed.as_secs_f64();

    tracing::info!(
        "Throughput ({} x {} KB = {} KB): write {:.1} MB/s ({:.0}ms), read {:.1} MB/s ({:.0}ms)",
        num_files, file_size / 1024, total_bytes / 1024,
        write_mbps, write_elapsed.as_secs_f64() * 1000.0,
        read_mbps, read_elapsed.as_secs_f64() * 1000.0,
    );

    // Sanity check: debug builds are slow, so use conservative thresholds
    if write_mbps < 0.1 {
        return Err(format!("write throughput too low: {:.2} MB/s", write_mbps));
    }
    if read_mbps < 0.5 {
        return Err(format!("read throughput too low: {:.2} MB/s", read_mbps));
    }
    Ok(())
}

/// Write on node 1, read from node 2 and node 3 after replication.
async fn scenario_cross_node_read(ctx: &mut TestContext) -> Result<(), String> {
    let data: Vec<u8> = (0..4096).map(|i| (i % 173) as u8).collect(); // 4 KB

    let (status, _, _) = ctx.write_file_timed(1, "testbed-vol", "cross-read.bin", &data).await?;
    if status >= 400 { return Err(format!("write failed: HTTP {}", status)); }

    // Wait for replication by polling reads on peer nodes
    let start = std::time::Instant::now();
    let body2 = ctx.wait_readable(2, "testbed-vol", "cross-read.bin", 30).await?;
    let repl_dur2 = start.elapsed();
    if body2 != data { return Err("node 2 content mismatch".into()); }

    let body3 = ctx.wait_readable(3, "testbed-vol", "cross-read.bin", 30).await?;
    let repl_dur3 = start.elapsed();
    if body3 != data { return Err("node 3 content mismatch".into()); }

    tracing::info!("Cross-node read: node 2 available after {:.1}s, node 3 after {:.1}s",
        repl_dur2.as_secs_f64(), repl_dur3.as_secs_f64());
    Ok(())
}

/// Write files on different nodes, verify cross-node reads after replication.
async fn scenario_replication_verify(ctx: &mut TestContext) -> Result<(), String> {
    // Write from node 1
    let data1 = b"from-node-1";
    let s = ctx.write_file(1, "testbed-vol", "repl-v/a.txt", data1).await?;
    if s >= 400 { return Err(format!("write a.txt failed: HTTP {}", s)); }

    // Write from node 2
    let data2 = b"from-node-2";
    let s = ctx.write_file(2, "testbed-vol", "repl-v/b.txt", data2).await?;
    if s >= 400 { return Err(format!("write b.txt failed: HTTP {}", s)); }

    // Wait for replication by polling reads on peer nodes
    let body = ctx.wait_readable(2, "testbed-vol", "repl-v/a.txt", 30).await?;
    if body != data1 { return Err("a.txt content mismatch on node 2".into()); }
    tracing::info!("a.txt (written on node 1) readable on node 2");

    let body = ctx.wait_readable(3, "testbed-vol", "repl-v/a.txt", 30).await?;
    if body != data1 { return Err("a.txt content mismatch on node 3".into()); }
    tracing::info!("a.txt (written on node 1) readable on node 3");

    let body = ctx.wait_readable(1, "testbed-vol", "repl-v/b.txt", 30).await?;
    if body != data2 { return Err("b.txt content mismatch on node 1".into()); }
    tracing::info!("b.txt (written on node 2) readable on node 1");

    let body = ctx.wait_readable(3, "testbed-vol", "repl-v/b.txt", 30).await?;
    if body != data2 { return Err("b.txt content mismatch on node 3".into()); }
    tracing::info!("b.txt (written on node 2) readable on node 3");

    Ok(())
}

pub fn print_results(results: &[ScenarioResult]) {
    println!();
    for r in results {
        let tag = if r.passed { "\x1b[32m[PASS]\x1b[0m" } else { "\x1b[31m[FAIL]\x1b[0m" };
        if r.passed {
            println!("{} {} ({:.1}s)", tag, r.name, r.duration.as_secs_f64());
        } else {
            println!("{} {}: {} ({:.1}s)", tag, r.name, r.message, r.duration.as_secs_f64());
        }
    }
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();
    println!("\nResults: {}/{} passed", passed, total);
}
