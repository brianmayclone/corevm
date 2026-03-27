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

pub async fn run_all() -> Vec<ScenarioResult> {
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

    results
}

pub async fn run_single(name: &str) -> Option<ScenarioResult> {
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
    if !(s1.is_leader ^ s2.is_leader) { return Err("expected exactly one leader".into()); }
    Ok(())
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
