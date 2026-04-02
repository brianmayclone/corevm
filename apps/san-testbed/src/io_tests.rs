//! I/O validation and performance test scenarios for CoreSAN disk server.

use crate::io_harness::IoHarness;
use crate::context::TestContext;
use crate::scenarios::ScenarioResult;
use rand::prelude::*;

const CHUNK_SIZE: u64 = 4 * 1024 * 1024; // 4 MB
const VOLUME_ID: &str = "testbed-vol";

fn socket_path() -> String {
    let dir = std::env::var("VMM_SAN_SOCK_DIR")
        .unwrap_or_else(|_| "/run/vmm-san".to_string());
    format!("{}/{}.sock", dir, VOLUME_ID)
}

/// Create the file in the SAN via HTTP API so that file_map entry exists,
/// then open the UDS connection for direct I/O.
async fn open_harness(ctx: &TestContext, rel_path: &str, file_size: u64) -> Result<IoHarness, String> {
    // Create initial file via HTTP (writes a single zero byte to ensure file_map entry)
    let initial = vec![0u8; 1];
    let status = ctx.write_file(1, VOLUME_ID, rel_path, &initial).await?;
    if status >= 400 {
        return Err(format!("Failed to pre-create file {} via HTTP: status {}", rel_path, status));
    }
    tracing::debug!("Pre-created file '{}' via HTTP API", rel_path);

    IoHarness::open(&socket_path(), rel_path, file_size).await
}

/// Helper: create single-node context (reuses existing infra).
async fn single_node_context() -> Result<TestContext, String> {
    // Remove stale socket from previous runs or production instances
    std::fs::remove_file(&socket_path()).ok();
    tracing::info!("Cleaned stale socket (if any) at {}", &socket_path());

    let ctx = TestContext::new(1).await?;

    // Wait for node health via HTTP first
    tracing::info!("Waiting for node 1 health check on {}...", ctx.nodes[0].address());
    ctx.wait_node_healthy(1).await?;
    tracing::info!("Node 1 healthy, waiting for UDS socket at {}...", &socket_path());

    // Then wait for the UDS socket to appear (disk_server starts after HTTP)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if std::path::Path::new(&socket_path()).exists() {
            tracing::info!("Socket found, connecting...");
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            break;
        }
        if std::time::Instant::now() > deadline {
            // Dump node log for debugging
            let log = ctx.read_log(1);
            let last_lines: Vec<&str> = log.lines().rev().take(30).collect();
            for line in last_lines.iter().rev() {
                tracing::error!("vmm-san log: {}", line);
            }
            // Also list what's in /run/vmm-san/
            let entries: Vec<String> = std::fs::read_dir("/run/vmm-san")
                .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
                .unwrap_or_default();
            tracing::error!("Contents of /run/vmm-san/: {:?}", entries);
            return Err(format!("Socket {} did not appear within 30s", &socket_path()));
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
    Ok(ctx)
}

/// Helper: run an I/O scenario with timing.
async fn run_io_scenario<F, Fut>(name: &str, f: F) -> ScenarioResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    tracing::info!("━━━ I/O Scenario: {} ━━━", name);
    let start = std::time::Instant::now();
    let result = f().await;
    ScenarioResult {
        name: name.to_string(),
        passed: result.is_ok(),
        message: result.err().unwrap_or_else(|| "OK".into()),
        duration: start.elapsed(),
    }
}

// ── Scenario 1: Sequential Write + Verify ────────────────────

pub async fn scenario_io_sequential() -> ScenarioResult {
    run_io_scenario("io-sequential", || async {
        let mut ctx = single_node_context().await?;

        let file_size: u64 = 32 * 1024 * 1024; // 32 MB
        let block_size: usize = 64 * 1024; // 64 KB
        let mut harness = open_harness(&ctx, "io-seq-test.img", file_size).await?;

        // Write sequential blocks with deterministic pattern
        let mut offset: u64 = 0;
        while offset < file_size {
            let chunk_idx = (offset / CHUNK_SIZE) as u8;
            let block: Vec<u8> = (0..block_size)
                .map(|i| chunk_idx ^ ((offset as usize + i) & 0xFF) as u8)
                .collect();
            harness.write(offset, &block).await?;
            offset += block_size as u64;
        }

        tracing::info!("Sequential write complete ({} MB), verifying...", file_size / 1_048_576);
        harness.verify_all().await?;

        tracing::info!(
            "io-sequential PASS: write {:.1} MB/s, read {:.1} MB/s",
            harness.stats.write_mbps(), harness.stats.read_mbps()
        );

        harness.close().await?;
        ctx.shutdown();
        Ok(())
    }).await
}

// ── Scenario 2: Random Write + Verify ────────────────────────

pub async fn scenario_io_random_write(seed: u64) -> ScenarioResult {
    run_io_scenario("io-random-write", || async move {
        let mut ctx = single_node_context().await?;

        let file_size: u64 = 16 * 1024 * 1024; // 16 MB
        let mut harness = open_harness(&ctx, "io-rand-test.img", file_size).await?;

        // Pre-fill with 0xAA
        let fill_block = vec![0xAAu8; 64 * 1024];
        let mut offset: u64 = 0;
        while offset < file_size {
            harness.write(offset, &fill_block).await?;
            offset += fill_block.len() as u64;
        }
        harness.flush().await?;
        tracing::info!("Pre-fill complete, starting random writes (seed={})", seed);

        // Random writes
        let mut rng = StdRng::seed_from_u64(seed);
        let num_ops = 1000;
        for i in 0..num_ops {
            let max_write_size: usize = 256 * 1024; // 256 KB
            let min_write_size: usize = 512;
            let write_size = rng.gen_range(min_write_size..=max_write_size);
            let max_offset = file_size as usize - write_size;
            let write_offset = rng.gen_range(0..=max_offset) as u64;

            let data: Vec<u8> = (0..write_size).map(|_| rng.gen()).collect();
            harness.write(write_offset, &data).await?;

            if (i + 1) % 200 == 0 {
                tracing::debug!("Random write progress: {}/{}", i + 1, num_ops);
            }
        }

        tracing::info!("Random writes complete ({}), verifying...", num_ops);
        harness.verify_all().await?;

        tracing::info!(
            "io-random-write PASS (seed={}): write {:.1} MB/s, read {:.1} MB/s",
            seed, harness.stats.write_mbps(), harness.stats.read_mbps()
        );

        harness.close().await?;
        ctx.shutdown();
        Ok(())
    }).await
}

// ── Scenario 3: Partial Chunk Boundary Stress ────────────────

pub async fn scenario_io_partial_chunk() -> ScenarioResult {
    run_io_scenario("io-partial-chunk", || async {
        let mut ctx = single_node_context().await?;

        let file_size: u64 = 12 * 1024 * 1024; // 12 MB (3 chunks)
        let mut harness = open_harness(&ctx, "io-partial-test.img", file_size).await?;

        // Pre-fill entire file with 0x55 so we can detect corruption
        let fill = vec![0x55u8; 64 * 1024];
        let mut off: u64 = 0;
        while off < file_size {
            harness.write(off, &fill).await?;
            off += fill.len() as u64;
        }
        harness.flush().await?;
        tracing::info!("Pre-fill complete, running chunk boundary tests...");

        // Test 1: 1 byte at offset 0
        tracing::debug!("Partial test 1: 1B at offset 0");
        harness.write(0, &[0xA1]).await?;
        harness.verify_all().await?;

        // Test 2: 4KB in the middle of first chunk (offset 2MB)
        tracing::debug!("Partial test 2: 4KB at offset 2MB");
        let data_2 = vec![0xA2u8; 4096];
        harness.write(2 * 1024 * 1024, &data_2).await?;
        harness.verify_all().await?;

        // Test 3: 8KB crossing 4MB chunk boundary
        tracing::debug!("Partial test 3: 8KB crossing 4MB boundary");
        let data_3 = vec![0xA3u8; 8192];
        harness.write(CHUNK_SIZE - 4096, &data_3).await?;
        harness.verify_all().await?;

        // Test 4: 1 byte at last byte of first chunk
        tracing::debug!("Partial test 4: 1B at last byte of chunk 0");
        harness.write(CHUNK_SIZE - 1, &[0xA4]).await?;
        harness.verify_all().await?;

        // Test 5: 1 byte at first byte of second chunk
        tracing::debug!("Partial test 5: 1B at first byte of chunk 1");
        harness.write(CHUNK_SIZE, &[0xA5]).await?;
        harness.verify_all().await?;

        // Test 6: Exact full first chunk (4MB)
        tracing::debug!("Partial test 6: exact full chunk 0");
        let data_6: Vec<u8> = (0..CHUNK_SIZE as usize).map(|i| (0xA6u8).wrapping_add((i & 0xFF) as u8)).collect();
        harness.write(0, &data_6).await?;
        harness.verify_all().await?;

        // Test 7: Exact full second chunk
        tracing::debug!("Partial test 7: exact full chunk 1");
        let data_7: Vec<u8> = (0..CHUNK_SIZE as usize).map(|i| (0xA7u8).wrapping_add((i & 0xFF) as u8)).collect();
        harness.write(CHUNK_SIZE, &data_7).await?;
        harness.verify_all().await?;

        // Test 8: 4MB spanning two chunks equally (offset 2MB)
        tracing::debug!("Partial test 8: 4MB at offset 2MB (spans chunk 0+1)");
        let data_8: Vec<u8> = (0..CHUNK_SIZE as usize).map(|i| (0xA8u8).wrapping_add((i & 0xFF) as u8)).collect();
        harness.write(2 * 1024 * 1024, &data_8).await?;
        harness.verify_all().await?;

        // Test 9: Small write crossing boundary (1024B at 4MB-512)
        tracing::debug!("Partial test 9: 1024B crossing 4MB boundary");
        let data_9 = vec![0xA9u8; 1024];
        harness.write(CHUNK_SIZE - 512, &data_9).await?;
        harness.verify_all().await?;

        // Test 10: 12MB write spanning all three chunks
        tracing::debug!("Partial test 10: 12MB spanning all chunks");
        let data_10: Vec<u8> = (0..file_size as usize).map(|i| (0xAAu8).wrapping_add((i & 0xFF) as u8)).collect();
        harness.write(0, &data_10).await?;
        harness.verify_all().await?;

        tracing::info!("io-partial-chunk PASS: all 10 boundary tests verified");

        harness.close().await?;
        ctx.shutdown();
        Ok(())
    }).await
}

// ── Scenario 4: Overwrite Integrity ──────────────────────────

pub async fn scenario_io_overwrite() -> ScenarioResult {
    run_io_scenario("io-overwrite", || async {
        let mut ctx = single_node_context().await?;

        let file_size: u64 = 8 * 1024 * 1024; // 8 MB
        let mut harness = open_harness(&ctx, "io-overwrite-test.img", file_size).await?;

        // Fill with pattern A (0xAA)
        let fill_a = vec![0xAAu8; 64 * 1024];
        let mut off: u64 = 0;
        while off < file_size {
            harness.write(off, &fill_a).await?;
            off += fill_a.len() as u64;
        }
        harness.verify_all().await?;
        tracing::info!("Pattern A fill verified");

        // Overwrite regions with pattern B (0xBB)
        let regions_b: Vec<(u64, usize)> = vec![
            (0, 4096),                           // 0..4KB
            (1024 * 1024, 65536),                // 1MB..1MB+64KB
            (CHUNK_SIZE - 2048, 4096),           // crosses chunk boundary
            (7 * 1024 * 1024, 1024 * 1024),     // 7MB..8MB
        ];
        for (offset, size) in &regions_b {
            let data_b = vec![0xBBu8; *size];
            harness.write(*offset, &data_b).await?;
        }
        harness.verify_all().await?;
        tracing::info!("Pattern B overwrites verified");

        // Overwrite some B-regions with pattern C (0xCC)
        let data_c1 = vec![0xCCu8; 2048];
        harness.write(0, &data_c1).await?; // first 2KB of the 4KB B-region
        let data_c2 = vec![0xCCu8; 32768];
        harness.write(1024 * 1024, &data_c2).await?; // first 32KB of the 64KB B-region
        harness.verify_all().await?;
        tracing::info!("Pattern C overwrites verified");

        tracing::info!("io-overwrite PASS");

        harness.close().await?;
        ctx.shutdown();
        Ok(())
    }).await
}

// ── Scenario 5: Performance Benchmark ────────────────────────

pub async fn scenario_io_benchmark() -> ScenarioResult {
    run_io_scenario("io-benchmark", || async {
        let mut ctx = single_node_context().await?;

        let file_size: u64 = 64 * 1024 * 1024; // 64 MB
        let mut harness = open_harness(&ctx, "io-bench.img", file_size).await?;

        // ── Sequential write (64KB blocks) ──
        harness.reset_stats();
        let block = vec![0xBEu8; 65536];
        let mut off: u64 = 0;
        while off < file_size {
            harness.write(off, &block).await?;
            off += block.len() as u64;
        }
        harness.flush().await?;
        let seq_write_mbps = harness.stats.write_mbps();

        // ── Sequential read (64KB blocks) ──
        harness.reset_stats();
        off = 0;
        while off < file_size {
            let sz = 65536u64.min(file_size - off);
            harness.read(off, sz).await?;
            off += sz;
        }
        let seq_read_mbps = harness.stats.read_mbps();

        // ── Random 4KB write ──
        harness.reset_stats();
        let mut rng = StdRng::seed_from_u64(99999);
        let num_ops = 10_000;
        for _ in 0..num_ops {
            let roff = rng.gen_range(0..file_size - 4096);
            let data: Vec<u8> = (0..4096).map(|_| rng.gen()).collect();
            harness.write(roff, &data).await?;
        }
        harness.flush().await?;
        let rand_write_iops = harness.stats.write_iops();

        // ── Random 4KB read ──
        harness.reset_stats();
        for _ in 0..num_ops {
            let roff = rng.gen_range(0..file_size - 4096);
            harness.read(roff, 4096).await?;
        }
        let rand_read_iops = harness.stats.read_iops();

        // ── Mixed random 4KB (70% read / 30% write) ──
        harness.reset_stats();
        let mix_start = std::time::Instant::now();
        let mut mix_ops = 0u64;
        for _ in 0..num_ops {
            let roff = rng.gen_range(0..file_size - 4096);
            if rng.gen_ratio(7, 10) {
                harness.read(roff, 4096).await?;
            } else {
                let data: Vec<u8> = (0..4096).map(|_| rng.gen()).collect();
                harness.write(roff, &data).await?;
            }
            mix_ops += 1;
        }
        harness.flush().await?;
        let mix_duration = mix_start.elapsed();
        let mix_iops = mix_ops as f64 / mix_duration.as_secs_f64();

        // Print results table
        println!();
        println!("┌──────────────────────────────────┬──────────────┬──────────────┐");
        println!("│ Test                             │ Throughput   │ IOPS         │");
        println!("├──────────────────────────────────┼──────────────┼──────────────┤");
        println!("│ Sequential Write 64KB            │ {:>8.1} MB/s │ —            │", seq_write_mbps);
        println!("│ Sequential Read 64KB             │ {:>8.1} MB/s │ —            │", seq_read_mbps);
        println!("│ Random Write 4KB                 │ —            │ {:>10.0}  │", rand_write_iops);
        println!("│ Random Read 4KB                  │ —            │ {:>10.0}  │", rand_read_iops);
        println!("│ Mixed Random 4KB (70r/30w)       │ —            │ {:>10.0}  │", mix_iops);
        println!("└──────────────────────────────────┴──────────────┴──────────────┘");

        tracing::info!(
            "io-benchmark: seq_wr={:.1}MB/s seq_rd={:.1}MB/s rand_wr={:.0}iops rand_rd={:.0}iops mix={:.0}iops",
            seq_write_mbps, seq_read_mbps, rand_write_iops, rand_read_iops, mix_iops
        );

        harness.close().await?;
        ctx.shutdown();
        Ok(())
    }).await
}

/// Run all I/O test scenarios.
pub async fn run_all_io(seed: u64) -> Vec<ScenarioResult> {
    vec![
        scenario_io_sequential().await,
        scenario_io_random_write(seed).await,
        scenario_io_partial_chunk().await,
        scenario_io_overwrite().await,
        scenario_io_benchmark().await,
    ]
}
