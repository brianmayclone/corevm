# SAN I/O Testbed Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add I/O validation and performance test scenarios to the san-testbed that exercise the Unix socket disk_server path, to find the data corruption bug during OS installations.

**Architecture:** Single-node vmm-san process started via existing testbed infrastructure. A new `IoHarness` connects over UDS using the SAN1/SANR binary protocol. Each test maintains a local `Vec<u8>` ground-truth image and verifies byte-for-byte correctness after flush cycles. Performance benchmarks measure throughput/IOPS.

**Tech Stack:** Rust, tokio (async UDS), existing san-testbed infrastructure (NodeHandle, TestContext, db_init)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `apps/san-testbed/Cargo.toml` | Add `rand` dependency |
| `apps/san-testbed/src/io_harness.rs` | SAN UDS client, ground-truth tracking, verification, performance stats |
| `apps/san-testbed/src/io_tests.rs` | Five test scenario functions |
| `apps/san-testbed/src/main.rs` | Register new modules, add `--seed` CLI arg |
| `apps/san-testbed/src/scenarios.rs` | Register new I/O scenarios in `run_all` and `run_single` |

---

### Task 1: Add `rand` dependency and register new modules

**Files:**
- Modify: `apps/san-testbed/Cargo.toml`
- Modify: `apps/san-testbed/src/main.rs`
- Create: `apps/san-testbed/src/io_harness.rs` (stub)
- Create: `apps/san-testbed/src/io_tests.rs` (stub)

- [ ] **Step 1: Add rand dependency to Cargo.toml**

Add after the `libc` line in `apps/san-testbed/Cargo.toml`:

```toml
rand = "0.8"
```

- [ ] **Step 2: Create io_harness.rs stub**

Create `apps/san-testbed/src/io_harness.rs`:

```rust
//! SAN disk I/O harness — UDS client with ground-truth verification.
```

- [ ] **Step 3: Create io_tests.rs stub**

Create `apps/san-testbed/src/io_tests.rs`:

```rust
//! I/O validation and performance test scenarios for CoreSAN disk server.

use crate::io_harness::IoHarness;
use crate::context::TestContext;
```

- [ ] **Step 4: Register modules and add --seed arg in main.rs**

In `apps/san-testbed/src/main.rs`, add module declarations after the existing ones:

```rust
mod io_harness;
mod io_tests;
```

Add `--seed` argument parsing after the `num_nodes` parsing (around line 48):

```rust
    let io_seed: u64 = args.iter()
        .position(|a| a == "--seed")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(12345);
```

Update the help text — add after the `"  Other:     repair-leader-only, all"` line:

```rust
        println!("  I/O:       io-sequential, io-random-write, io-partial-chunk,");
        println!("             io-overwrite, io-benchmark, io-all");
        println!();
        println!("Options:");
        println!("  --nodes N            Number of nodes (default: 3)");
        println!("  --scenario <name>    Run automated scenario");
        println!("  --seed N             RNG seed for io-random-write (default: 12345)");
```

Pass `io_seed` to scenario runners — update the `scenario_name == "all"` branch:

```rust
        if scenario_name == "all" {
            let results = scenarios::run_all(io_seed).await;
```

And the single-scenario branch:

```rust
            match scenarios::run_single(&scenario_name, io_seed).await {
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p san-testbed 2>&1 | tail -5`

Expected: Build succeeds (io_tests may have unused import warnings, that's fine)

- [ ] **Step 6: Commit**

```bash
git add apps/san-testbed/Cargo.toml apps/san-testbed/src/main.rs \
  apps/san-testbed/src/io_harness.rs apps/san-testbed/src/io_tests.rs
git commit -m "feat(san-testbed): scaffold I/O test modules and add rand dependency"
```

---

### Task 2: Implement IoHarness — UDS client with ground-truth verification

**Files:**
- Modify: `apps/san-testbed/src/io_harness.rs`

This is the core component. It connects to the vmm-san UDS, implements the SAN1/SANR binary protocol, tracks a ground-truth buffer, and provides verification.

- [ ] **Step 1: Write the full IoHarness implementation**

Replace `apps/san-testbed/src/io_harness.rs` with:

```rust
//! SAN disk I/O harness — UDS client with ground-truth verification.

use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const REQUEST_MAGIC: u32 = 0x53414E31; // "SAN1"
const RESPONSE_MAGIC: u32 = 0x53414E52; // "SANR"

const CMD_OPEN: u32 = 0;
const CMD_READ: u32 = 1;
const CMD_WRITE: u32 = 2;
const CMD_FLUSH: u32 = 3;
const CMD_CLOSE: u32 = 4;
const CMD_GETSIZE: u32 = 5;

const CHUNK_SIZE: u64 = 4 * 1024 * 1024; // 4 MB

/// Build a 32-byte SAN request header.
fn request_header(cmd: u32, file_id: u64, offset: u64, size: u32) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[0..4].copy_from_slice(&REQUEST_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&cmd.to_le_bytes());
    buf[8..16].copy_from_slice(&file_id.to_le_bytes());
    buf[16..24].copy_from_slice(&offset.to_le_bytes());
    buf[24..28].copy_from_slice(&size.to_le_bytes());
    // flags = 0
    buf
}

/// Parse a 16-byte SAN response header. Returns (status, data_size).
fn parse_response(buf: &[u8; 16]) -> (u32, u32) {
    let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    assert_eq!(magic, RESPONSE_MAGIC, "bad response magic");
    let status = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let size = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    (status, size)
}

/// Detailed mismatch information for verification failures.
pub struct VerifyError {
    pub offset: u64,
    pub expected: u8,
    pub actual: u8,
    pub chunk_index: u32,
    pub local_offset: u64,
    pub context_expected: Vec<u8>,
    pub context_actual: Vec<u8>,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DATA MISMATCH at byte offset {} (chunk {} + local_offset {}): expected 0x{:02X}, got 0x{:02X}\n\
             Expected (±32 bytes): {:02X?}\n\
             Actual   (±32 bytes): {:02X?}",
            self.offset, self.chunk_index, self.local_offset,
            self.expected, self.actual,
            self.context_expected, self.context_actual,
        )
    }
}

/// Performance statistics for a test run.
pub struct IoStats {
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub write_ops: u64,
    pub read_ops: u64,
    pub flush_ops: u64,
    pub write_duration: std::time::Duration,
    pub read_duration: std::time::Duration,
}

impl IoStats {
    fn new() -> Self {
        Self {
            bytes_written: 0,
            bytes_read: 0,
            write_ops: 0,
            read_ops: 0,
            flush_ops: 0,
            write_duration: std::time::Duration::ZERO,
            read_duration: std::time::Duration::ZERO,
        }
    }

    pub fn write_mbps(&self) -> f64 {
        let secs = self.write_duration.as_secs_f64();
        if secs > 0.0 { self.bytes_written as f64 / 1_048_576.0 / secs } else { 0.0 }
    }

    pub fn read_mbps(&self) -> f64 {
        let secs = self.read_duration.as_secs_f64();
        if secs > 0.0 { self.bytes_read as f64 / 1_048_576.0 / secs } else { 0.0 }
    }

    pub fn write_iops(&self) -> f64 {
        let secs = self.write_duration.as_secs_f64();
        if secs > 0.0 { self.write_ops as f64 / secs } else { 0.0 }
    }

    pub fn read_iops(&self) -> f64 {
        let secs = self.read_duration.as_secs_f64();
        if secs > 0.0 { self.read_ops as f64 / secs } else { 0.0 }
    }
}

/// I/O test harness: SAN UDS client + ground-truth buffer + verification.
pub struct IoHarness {
    stream: UnixStream,
    file_id: i64,
    pub ground_truth: Vec<u8>,
    pub stats: IoStats,
}

impl IoHarness {
    /// Connect to the vmm-san UDS and open a file.
    pub async fn open(socket_path: &str, rel_path: &str, file_size: u64) -> Result<Self, String> {
        let stream = UnixStream::connect(socket_path).await
            .map_err(|e| format!("UDS connect {}: {}", socket_path, e))?;

        let mut harness = Self {
            stream,
            file_id: 0,
            ground_truth: vec![0u8; file_size as usize],
            stats: IoStats::new(),
        };

        // Send CMD_OPEN with rel_path as payload
        let path_bytes = rel_path.as_bytes();
        let hdr = request_header(CMD_OPEN, 0, 0, path_bytes.len() as u32);
        harness.stream.write_all(&hdr).await.map_err(|e| format!("open write hdr: {}", e))?;
        harness.stream.write_all(path_bytes).await.map_err(|e| format!("open write path: {}", e))?;

        // Read response
        let mut resp_buf = [0u8; 16];
        harness.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("open read resp: {}", e))?;
        let (status, data_size) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_OPEN failed: status={}", status));
        }

        // Read file_id from response data
        if data_size >= 8 {
            let mut id_buf = [0u8; 8];
            harness.stream.read_exact(&mut id_buf).await.map_err(|e| format!("open read id: {}", e))?;
            harness.file_id = i64::from_le_bytes(id_buf);
            // Drain any extra response bytes
            if data_size > 8 {
                let mut discard = vec![0u8; (data_size - 8) as usize];
                harness.stream.read_exact(&mut discard).await.ok();
            }
        }

        tracing::debug!("IoHarness: opened '{}' file_id={}", rel_path, harness.file_id);
        Ok(harness)
    }

    /// Write data at offset. Updates ground truth.
    pub async fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), String> {
        let start = Instant::now();

        let hdr = request_header(CMD_WRITE, self.file_id as u64, offset, data.len() as u32);
        self.stream.write_all(&hdr).await.map_err(|e| format!("write hdr: {}", e))?;
        self.stream.write_all(data).await.map_err(|e| format!("write data: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("write resp: {}", e))?;
        let (status, _) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_WRITE failed: status={}", status));
        }

        let elapsed = start.elapsed();
        self.stats.bytes_written += data.len() as u64;
        self.stats.write_ops += 1;
        self.stats.write_duration += elapsed;

        // Update ground truth
        let end = offset as usize + data.len();
        if end > self.ground_truth.len() {
            self.ground_truth.resize(end, 0);
        }
        self.ground_truth[offset as usize..end].copy_from_slice(data);

        Ok(())
    }

    /// Read data at offset.
    pub async fn read(&mut self, offset: u64, size: u64) -> Result<Vec<u8>, String> {
        let start = Instant::now();

        let hdr = request_header(CMD_READ, self.file_id as u64, offset, size as u32);
        self.stream.write_all(&hdr).await.map_err(|e| format!("read hdr: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("read resp: {}", e))?;
        let (status, data_size) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_READ failed: status={}", status));
        }

        let mut data = vec![0u8; data_size as usize];
        if data_size > 0 {
            self.stream.read_exact(&mut data).await.map_err(|e| format!("read data: {}", e))?;
        }

        let elapsed = start.elapsed();
        self.stats.bytes_read += data.len() as u64;
        self.stats.read_ops += 1;
        self.stats.read_duration += elapsed;

        Ok(data)
    }

    /// Flush all dirty cached chunks to disk.
    pub async fn flush(&mut self) -> Result<(), String> {
        let hdr = request_header(CMD_FLUSH, self.file_id as u64, 0, 0);
        self.stream.write_all(&hdr).await.map_err(|e| format!("flush hdr: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("flush resp: {}", e))?;
        let (status, _) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_FLUSH failed: status={}", status));
        }
        self.stats.flush_ops += 1;
        Ok(())
    }

    /// Close the file handle.
    pub async fn close(mut self) -> Result<(), String> {
        let hdr = request_header(CMD_CLOSE, self.file_id as u64, 0, 0);
        self.stream.write_all(&hdr).await.map_err(|e| format!("close hdr: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("close resp: {}", e))?;
        Ok(())
    }

    /// Flush, then read the entire file and compare against ground truth.
    /// Returns Ok(()) if all bytes match, or Err with detailed mismatch info.
    pub async fn verify_all(&mut self) -> Result<(), String> {
        self.flush().await?;

        let read_block: u64 = 256 * 1024; // 256 KB read blocks
        let file_size = self.ground_truth.len() as u64;
        let mut offset: u64 = 0;

        while offset < file_size {
            let remaining = file_size - offset;
            let block = remaining.min(read_block);
            let actual = self.read(offset, block).await?;
            let expected = &self.ground_truth[offset as usize..(offset + block) as usize];

            if actual.len() != expected.len() {
                return Err(format!(
                    "Size mismatch at offset {}: expected {} bytes, got {}",
                    offset, expected.len(), actual.len()
                ));
            }

            // Find first mismatch
            for i in 0..actual.len() {
                if actual[i] != expected[i] {
                    let abs_offset = offset + i as u64;
                    let chunk_index = (abs_offset / CHUNK_SIZE) as u32;
                    let local_offset = abs_offset % CHUNK_SIZE;

                    // Extract context (±32 bytes)
                    let ctx_start = if i >= 32 { i - 32 } else { 0 };
                    let ctx_end = (i + 32).min(actual.len());

                    let err = VerifyError {
                        offset: abs_offset,
                        expected: expected[i],
                        actual: actual[i],
                        chunk_index,
                        local_offset,
                        context_expected: expected[ctx_start..ctx_end].to_vec(),
                        context_actual: actual[ctx_start..ctx_end].to_vec(),
                    };
                    return Err(err.to_string());
                }
            }

            offset += block;
        }

        Ok(())
    }

    /// Reset stats for a fresh measurement.
    pub fn reset_stats(&mut self) {
        self.stats = IoStats::new();
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p san-testbed 2>&1 | tail -5`

Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add apps/san-testbed/src/io_harness.rs
git commit -m "feat(san-testbed): implement IoHarness with UDS client and ground-truth verification"
```

---

### Task 3: Implement I/O test scenarios

**Files:**
- Modify: `apps/san-testbed/src/io_tests.rs`

- [ ] **Step 1: Write all five scenario implementations**

Replace `apps/san-testbed/src/io_tests.rs` with:

```rust
//! I/O validation and performance test scenarios for CoreSAN disk server.

use crate::io_harness::IoHarness;
use crate::context::TestContext;
use crate::scenarios::ScenarioResult;
use rand::prelude::*;

const CHUNK_SIZE: u64 = 4 * 1024 * 1024; // 4 MB
const SOCKET_PATH: &str = "/run/vmm-san/testbed-vol.sock";

/// Helper: create single-node context (reuses existing infra).
async fn single_node_context() -> Result<TestContext, String> {
    let ctx = TestContext::new(1).await?;
    // Wait for node + disk server to be ready
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    Ok(ctx)
}

/// Helper: run an I/O scenario with fresh single-node context and timing.
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
        let mut harness = IoHarness::open(SOCKET_PATH, "io-seq-test.img", file_size).await?;

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
        let mut harness = IoHarness::open(SOCKET_PATH, "io-rand-test.img", file_size).await?;

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
        let mut harness = IoHarness::open(SOCKET_PATH, "io-partial-test.img", file_size).await?;

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

        // Test 9: Small write crossing boundary (512B at 4MB-512)
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
        let mut harness = IoHarness::open(SOCKET_PATH, "io-overwrite-test.img", file_size).await?;

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
        let mut harness = IoHarness::open(SOCKET_PATH, "io-bench.img", file_size).await?;

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
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p san-testbed 2>&1 | tail -5`

Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add apps/san-testbed/src/io_tests.rs
git commit -m "feat(san-testbed): implement 5 I/O test scenarios (sequential, random, partial-chunk, overwrite, benchmark)"
```

---

### Task 4: Wire scenarios into the scenario runner

**Files:**
- Modify: `apps/san-testbed/src/scenarios.rs`

- [ ] **Step 1: Update run_all to accept seed and include I/O tests**

In `apps/san-testbed/src/scenarios.rs`, change the `run_all` function signature and add I/O scenarios at the end:

```rust
pub async fn run_all(seed: u64) -> Vec<ScenarioResult> {
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

    // I/O validation and performance scenarios
    results.extend(crate::io_tests::run_all_io(seed).await);

    results
}
```

- [ ] **Step 2: Update run_single to accept seed and route I/O scenarios**

Change the function signature and add the new match arms:

```rust
pub async fn run_single(name: &str, seed: u64) -> Option<ScenarioResult> {
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
        // I/O scenarios
        "io-sequential" => Some(crate::io_tests::scenario_io_sequential().await),
        "io-random-write" => Some(crate::io_tests::scenario_io_random_write(seed).await),
        "io-partial-chunk" => Some(crate::io_tests::scenario_io_partial_chunk().await),
        "io-overwrite" => Some(crate::io_tests::scenario_io_overwrite().await),
        "io-benchmark" => Some(crate::io_tests::scenario_io_benchmark().await),
        "io-all" => {
            let results = crate::io_tests::run_all_io(seed).await;
            let any_failed = results.iter().any(|r| !r.passed);
            let messages: Vec<String> = results.iter()
                .map(|r| format!("{}: {}", r.name, if r.passed { "OK" } else { &r.message }))
                .collect();
            Some(ScenarioResult {
                name: "io-all".to_string(),
                passed: !any_failed,
                message: messages.join("; "),
                duration: results.iter().map(|r| r.duration).sum(),
            })
        }
        _ => None,
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p san-testbed 2>&1 | tail -5`

Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add apps/san-testbed/src/scenarios.rs
git commit -m "feat(san-testbed): wire I/O test scenarios into scenario runner"
```

---

### Task 5: Build, run a quick smoke test, and verify end-to-end

**Files:** None (verification only)

- [ ] **Step 1: Build in release mode for performance**

Run: `cargo build -p san-testbed --release 2>&1 | tail -10`

Expected: Build succeeds

- [ ] **Step 2: Build vmm-san (required dependency)**

Run: `cargo build -p vmm-san 2>&1 | tail -10`

Expected: Build succeeds

- [ ] **Step 3: Run the simplest I/O scenario as smoke test**

Run: `sudo /tmp/corevm-target/debug/san-testbed --scenario io-sequential 2>&1`

(Needs sudo because the UDS socket is at `/run/vmm-san/`.)

Expected: Either PASS (sequential I/O works) or FAIL with a specific mismatch error (which would confirm the bug).

- [ ] **Step 4: If smoke test passes, run the critical random test**

Run: `sudo /tmp/corevm-target/debug/san-testbed --scenario io-random-write --seed 12345 2>&1`

Expected: This is the test most likely to expose the corruption bug. If it fails, the output will show exactly which byte offset, chunk index, and local offset are affected.

- [ ] **Step 5: Commit the final build verification**

No code changes needed. If any compilation issues were found and fixed in previous steps, they should already be committed.

```bash
git add -A
git commit -m "chore(san-testbed): verify I/O test scenarios build and run"
```

(Skip this commit if there are no changes.)
