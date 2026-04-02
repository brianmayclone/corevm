# SAN I/O Testbed Design

**Date:** 2026-04-02
**Goal:** Find the data corruption bug that occurs during OS installation via vmm-server (Unix socket path through CoreSAN disk_server) and establish performance baselines.

## Problem Statement

When installing operating systems through the vmm-server (which uses CoreSAN's Unix socket disk interface), the resulting disks are corrupt after installation. The suspected root cause: with 4MB chunks, partial writes in certain random I/O patterns may not correctly preserve existing chunk data — e.g., zero-filling instead of read-modify-write when chunks are < 4MB or when cache state is inconsistent.

## Approach

- Add new I/O test scenarios to the existing san-testbed project (alongside existing quorum/fencing/partition tests)
- Test via **Unix Domain Sockets** using `SanDiskConnection` — the exact production code path (disk_server cache → flush → write_chunk_data → disk)
- Start a real single-node vmm-san process with temp directories
- Maintain a local `Vec<u8>` ground-truth image to verify every byte after I/O operations

## Architecture

```
san-testbed binary
  │
  ├── Existing scenarios (quorum, fencing, partition, replication) — unchanged
  │
  └── New I/O scenarios:
        │
        ├── Starts 1x vmm-san process (single node, temp dirs)
        ├── Connects via SanDiskConnection to Unix socket
        │
        └── For each test:
              ├── Maintains Vec<u8> ground-truth image
              ├── Executes I/O operations over socket
              ├── After flush: reads everything back, compares with ground truth
              └── Measures timing for performance reporting
```

## New Files

### `src/io_harness.rs`

SAN client wrapper and verification engine:

```rust
struct IoHarness {
    conn: SanDiskConnection,   // UDS connection to vmm-san
    ground_truth: Vec<u8>,     // Reference image (same size as disk file)
    file_size: u64,            // Current file size
    rng: StdRng,               // Seeded RNG for reproducibility
    stats: IoStats,            // Accumulated performance stats
}
```

**Responsibilities:**
- `write(offset, data)` — sends CMD_WRITE over socket, updates ground truth in parallel
- `read(offset, size)` — sends CMD_READ over socket
- `flush()` — sends CMD_FLUSH
- `verify_all() -> Result<(), VerifyError>` — flushes, reads entire file in blocks, compares byte-by-byte against ground truth
- `verify_range(offset, size)` — partial verification for targeted checks
- Performance tracking (bytes written/read, operation counts, elapsed time)

**Error reporting on mismatch:**
- Exact byte offset of first mismatch
- Expected vs actual values (hex dump of surrounding ±32 bytes)
- Chunk index and local offset within chunk (for direct debugging)
- RNG seed used (for reproducibility)

### `src/io_tests.rs`

Five test scenarios, each as an async function returning `ScenarioResult`:

#### 1. `io-sequential` — Sequential Write + Verify

- Create 32MB file
- Write sequentially in 64KB blocks with deterministic pattern (`(chunk_idx ^ byte_offset) as u8`)
- Flush
- Read entire file back, compare byte-for-byte with ground truth
- **Purpose:** Baseline sanity check, confirms sequential path works

#### 2. `io-random-write` — Random Write + Verify

- Create 16MB file, pre-fill with pattern (0xAA)
- Execute 1000 random writes:
  - Random offset: `0..file_size - max_write_size`
  - Random size: `512B..256KB`
  - Random data: from seeded RNG
- Each write updates ground truth
- Flush, read entire file, verify against ground truth
- **Purpose:** Stress-test random I/O patterns — most likely to trigger the corruption bug
- **Seed:** Configurable via `--seed N`, defaults to fixed seed for determinism

#### 3. `io-partial-chunk` — Chunk Boundary Stress Test

Targeted writes that exercise chunk boundary handling:

| Write | Offset | Size | Tests |
|-------|--------|------|-------|
| 1 | 0 | 1B | First byte of first chunk |
| 2 | 2MB | 4KB | Middle of first chunk |
| 3 | 4MB - 4KB | 8KB | Crosses 4MB chunk boundary |
| 4 | 4MB - 1 | 1B | Last byte of first chunk |
| 5 | 4MB | 1B | First byte of second chunk |
| 6 | 0 | 4MB | Exact full first chunk |
| 7 | 4MB | 4MB | Exact full second chunk |
| 8 | 2MB | 4MB | Spans two chunks equally |
| 9 | 4MB - 512 | 1024 | Small write crossing boundary |
| 10 | 0 | 12MB | Spans three full chunks |

After each write: flush + full verification.
After all writes: one final full verification.

- **Purpose:** Targeted regression test for chunk boundary bugs

#### 4. `io-overwrite` — Overwrite Integrity Test

- Create 8MB file, fill entirely with pattern A (`0xAA` repeating)
- Flush + verify
- Overwrite specific regions with pattern B (`0xBB` repeating):
  - Bytes 0..4KB
  - Bytes 1MB..1MB+64KB
  - Bytes 4MB-2KB..4MB+2KB (crosses chunk boundary)
  - Bytes 7MB..8MB
- Flush + verify: pattern B at overwritten ranges, pattern A everywhere else
- Overwrite some of the B-regions with pattern C (`0xCC`)
- Flush + verify again
- **Purpose:** Confirms that writes only affect the targeted byte ranges and don't corrupt surrounding data

#### 5. `io-benchmark` — Performance Measurement

No data validation, pure performance:

- **Sequential write:** 64MB in 64KB blocks, measure MB/s
- **Sequential read:** 64MB in 64KB blocks, measure MB/s
- **Random 4KB write:** 10000 random 4KB writes, measure IOPS
- **Random 4KB read:** 10000 random 4KB reads, measure IOPS
- **Mixed random 4KB (70% read / 30% write):** 10000 ops, measure IOPS

Output as formatted table:

```
┌──────────────────────────────┬──────────────┬──────────────┐
│ Test                         │ Throughput   │ IOPS         │
├──────────────────────────────┼──────────────┼──────────────┤
│ Sequential Write 64KB        │ 234.5 MB/s   │ —            │
│ Sequential Read 64KB         │ 456.7 MB/s   │ —            │
│ Random Write 4KB             │ —            │ 12345        │
│ Random Read 4KB              │ —            │ 23456        │
│ Mixed Random 4KB (70r/30w)   │ —            │ 18900        │
└──────────────────────────────┴──────────────┴──────────────┘
```

- **Purpose:** Performance baseline; must not regress after bug fixes

## Integration with Existing Testbed

- New scenarios registered alongside existing ones in `scenarios.rs`
- Invocation: `san-testbed --scenario io-sequential`, `io-random-write`, `io-partial-chunk`, `io-overwrite`, `io-benchmark`
- `--scenario io-all` runs all 5 I/O tests
- `--scenario all` continues to run all scenarios (existing + new)
- Exit code: 0 = all pass, 1 = any failure (unchanged)

## Node Setup for I/O Tests

Single-node setup (reusing existing `NodeHandle` infrastructure):
- 1 vmm-san process
- 1 volume with 2 local backends (mirror mode for consistency with production)
- Chunk size: 4MB (default, matching production)
- No witness needed (single node)
- Temp directories for all data (cleaned up on exit)

## CLI Extensions

```
san-testbed --scenario io-random-write --seed 42    # Reproducible random test
san-testbed --scenario io-benchmark                  # Performance only
san-testbed --scenario io-all                        # All I/O tests
```

## Dependencies

No new crate dependencies needed. The existing testbed already has everything:
- `tokio` for async
- `tempfile` for temp dirs
- `tracing` for logging

For the SAN client connection, we connect directly via `tokio::net::UnixStream` and implement the SAN1/SANR binary protocol inline (it's simple: 32-byte request header + data, 16-byte response header + data). This avoids pulling in `libcorevm` as a dependency.

## Success Criteria

1. `io-sequential`, `io-partial-chunk`, `io-overwrite` pass — basic correctness confirmed
2. `io-random-write` either passes (bug not in this path) or fails with clear mismatch report pointing to the corrupted chunk/offset
3. `io-benchmark` produces numbers that serve as baseline for future comparison
