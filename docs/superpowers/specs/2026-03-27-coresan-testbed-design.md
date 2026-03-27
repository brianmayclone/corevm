# CoreSAN Test Framework & Testbed

**Date:** 2026-03-27
**Status:** Approved
**Scope:** `apps/san-testbed` (new crate), `apps/vmm-san/src/engine/peer_monitor.rs` (unit tests)

## Problem

CoreSAN has no tests. The quorum/fencing/leader-election logic cannot be verified without deploying multiple VMs. There is no way for a developer or CI to confirm that quorum transitions, fencing, replication, and leader failover work correctly.

## Goals

1. Unit-test quorum calculation and leader election logic without network or database
2. Run a local multi-node CoreSAN cluster with a single command (`cargo run -p san-testbed -- --nodes 3`)
3. Simulate node failures and network partitions interactively
4. Run automated test scenarios that report PASS/FAIL (usable by CI and AI agents)
5. Include a mock witness for 2-node quorum tie-breaking tests
6. Use temp directories as fake disks (no real block devices needed)

## Non-Goals

- Testing FUSE mounts (requires root / fuser setup)
- Testing real block device partitioning / formatting
- Performance benchmarking
- UI testing

## Constraints

- Must work on a standard dev machine without root privileges
- No Docker dependency — pure Cargo
- Each scenario must be self-contained (no shared state between runs)
- Testbed must clean up temp files on exit

---

## Design

### Part 1: Unit Tests — Quorum & Leader Logic

**Location:** `apps/vmm-san/src/engine/peer_monitor.rs`, `#[cfg(test)]` module

**Problem:** `compute_quorum()` and `compute_is_leader()` currently take `&CoreSanState` (which requires DB, DashMap, channels, etc.). This makes them untestable without heavy setup.

**Solution:** Extract the core decision logic into pure functions:

```rust
/// Pure quorum calculation — no state, no IO.
pub fn calculate_quorum_status(
    total_nodes: usize,
    reachable_nodes: usize,
    witness_allowed: Option<bool>,  // None = no witness configured or unreachable
) -> QuorumStatus {
    if total_nodes <= 1 {
        return QuorumStatus::Solo;
    }
    let majority = (total_nodes / 2) + 1;
    if reachable_nodes >= majority {
        return if reachable_nodes == total_nodes {
            QuorumStatus::Active
        } else {
            QuorumStatus::Degraded
        };
    }
    // No majority — check witness
    if witness_allowed == Some(true) {
        return QuorumStatus::Degraded;
    }
    QuorumStatus::Fenced
}

/// Pure leader calculation — no state, no IO.
pub fn calculate_is_leader(
    our_node_id: &str,
    online_peer_ids: &[&str],
    quorum: QuorumStatus,
) -> bool {
    if quorum == QuorumStatus::Fenced {
        return false;
    }
    online_peer_ids.iter().all(|peer_id| *peer_id >= our_node_id)
}
```

The existing `compute_quorum()` and `compute_is_leader()` call these pure functions internally.

**Test cases (14 tests):**

| Test | Inputs | Expected |
|------|--------|----------|
| `solo_node` | total=1, reachable=1 | Solo |
| `two_nodes_all_online` | total=2, reachable=2 | Active |
| `two_nodes_one_offline_no_witness` | total=2, reachable=1, witness=None | Fenced |
| `two_nodes_one_offline_witness_allows` | total=2, reachable=1, witness=Some(true) | Degraded |
| `two_nodes_one_offline_witness_denies` | total=2, reachable=1, witness=Some(false) | Fenced |
| `three_nodes_all_online` | total=3, reachable=3 | Active |
| `three_nodes_one_offline` | total=3, reachable=2 | Degraded |
| `three_nodes_two_offline` | total=3, reachable=1 | Fenced |
| `five_nodes_two_offline` | total=5, reachable=3 | Degraded |
| `five_nodes_three_offline` | total=5, reachable=2 | Fenced |
| `ten_nodes_four_offline` | total=10, reachable=6 | Degraded |
| `leader_lowest_id` | our="aaa", peers=["bbb","ccc"], Active | true |
| `leader_not_lowest` | our="ccc", peers=["aaa","bbb"], Active | false |
| `leader_fenced_never` | our="aaa", peers=["bbb"], Fenced | false |

### Part 2: San-Testbed Binary

**Location:** `apps/san-testbed/` — new Cargo crate

**Dependencies:** `tokio`, `reqwest`, `serde_json`, `axum` (for witness mock), `tempfile`, `rusqlite`, `rustyline` (for CLI)

#### 2.1 Startup

`cargo run -p san-testbed -- --nodes 3`

1. Create temp directory: `/tmp/san-testbed-<random>/`
2. Per node (N = 1..nodes):
   - Create `node-N/data/` (data_dir, will hold vmm-san.db)
   - Create `node-N/disk-0/` and `node-N/disk-1/` (fake disks)
   - Create `node-N/fuse/` (fuse_root, unused but required by config)
   - Generate `node-N/vmm-san.toml`:
     ```toml
     [server]
     bind = "127.0.0.1"
     port = <7442 + N>   # 7443, 7444, 7445, ...

     [data]
     data_dir = "/tmp/san-testbed-xxx/node-N/data"
     fuse_root = "/tmp/san-testbed-xxx/node-N/fuse"

     [peer]
     port = <7542 + N>
     secret = "testbed-secret"

     [cluster]
     witness_url = "http://127.0.0.1:9443"

     [benchmark]
     enabled = false

     [integrity]
     enabled = false
     ```
3. Pre-initialize each node's SQLite database:
   - Run vmm-san's `db::init()` schema (import as library, or replicate the schema SQL)
   - Insert `node_settings` with a deterministic `node_id` (e.g., `node-1`, `node-2`, ...)
   - Insert `peers` rows for all other nodes (address = `http://127.0.0.1:<port>`)
   - Insert `claimed_disks` rows pointing to the temp directories
   - Insert `backends` rows (status = "online") linked to the claimed disks
   - Create one default volume `testbed-vol` with backends on all nodes
4. Start witness mock on port 9443
5. Start N vmm-san processes as child processes: `vmm-san --config <path>`
6. Wait for all nodes to be healthy (poll `/api/status` with 500ms retries, 30s timeout)
7. Enter interactive CLI or run scenario

#### 2.2 DB Pre-Initialization (Fake Disks)

Instead of calling `claim_disk` API (which tries to format real block devices), the testbed writes directly to each node's SQLite:

```sql
-- Fake claimed disk
INSERT INTO claimed_disks (id, device_path, mount_path, fs_type, size_bytes, status, backend_id)
VALUES ('disk-0', '/fake/dev/sda', '/tmp/.../node-1/disk-0', 'ext4', 107374182400, 'mounted', 'backend-node1-0');

-- Backend pointing to temp directory
INSERT INTO backends (id, node_id, path, total_bytes, free_bytes, status, claimed_disk_id)
VALUES ('backend-node1-0', 'node-1', '/tmp/.../node-1/disk-0', 107374182400, 107374182400, 'online', 'disk-0');

-- Default test volume
INSERT INTO volumes (id, name, ftt, status)
VALUES ('testbed-vol', 'testbed-vol', 1, 'online');
```

This is done per-node before the vmm-san process starts.

#### 2.3 Witness Mock

A minimal axum HTTP server on port 9443 with one endpoint:

`GET /api/san/witness/{node_id}` — returns `{"allowed": true}` or `{"allowed": false}`

**Modes** (controlled via CLI commands):
- `allow-all` (default): always returns `{"allowed": true}`
- `deny-all`: always returns `{"allowed": false}`
- `smart`: simulates real witness logic — tracks which nodes are "reachable" (based on testbed's partition state), applies majority + lowest-id tiebreaker
- `off`: stops responding (simulates witness unreachable — connection refused)

The witness mock holds its state in a `Arc<RwLock<WitnessState>>` shared with the CLI command handler.

#### 2.4 Interactive CLI

Uses `rustyline` for readline with history. Commands:

| Command | Action |
|---------|--------|
| `status` | Poll `/api/status` on all nodes, display table |
| `kill <n>` | Send SIGTERM to node N's child process |
| `start <n>` | Restart node N's child process with same config |
| `partition <a,b> vs <c,d>` | Simulate network partition by updating peer addresses in DB |
| `heal` | Restore all peer addresses to correct values |
| `write <n> <vol> <path> <content>` | PUT file to node N's API |
| `read <n> <vol> <path>` | GET file from node N's API |
| `volumes` | List volumes from any node |
| `witness allow-all` | Set witness to allow mode |
| `witness deny-all` | Set witness to deny mode |
| `witness smart` | Set witness to smart mode |
| `witness off` | Stop witness server |
| `wait <secs>` | Sleep (useful in scripts) |
| `exit` / `quit` | Shutdown all nodes, cleanup temp dir |

**Network partition simulation:**

When `partition 1,2 vs 3` is executed:
1. On Node 3's DB: `UPDATE peers SET address = 'http://127.0.0.1:1' WHERE node_id IN ('node-1', 'node-2')`
2. On Node 1's DB: `UPDATE peers SET address = 'http://127.0.0.1:1' WHERE node_id = 'node-3'`
3. On Node 2's DB: `UPDATE peers SET address = 'http://127.0.0.1:1' WHERE node_id = 'node-3'`

Heartbeats to `127.0.0.1:1` will fail (connection refused), causing the peer monitor to mark those peers offline after 3 missed heartbeats (15s).

When `heal` is executed: all peer addresses are restored to their correct `http://127.0.0.1:<port>` values.

**Limitation:** This approach requires that vmm-san re-reads peer addresses from DB on each heartbeat cycle. Currently `heartbeat_all_peers` reads from the in-memory `DashMap`, not the DB. Two options:
- **Option A (recommended):** The testbed also updates the in-memory DashMap by calling `POST /api/peers/join` with the invalid/valid address. This triggers the peer to update its in-memory map.
- **Option B:** Add a `POST /api/peers/update-address` internal endpoint. More code but cleaner.

We use **Option A** — the testbed calls each node's `/api/peers/join` to update the peer address in both DB and memory.

#### 2.5 Automated Scenarios

`cargo run -p san-testbed -- --scenario <name|all>`

Each scenario is a function with this signature:

```rust
async fn scenario_quorum_degraded(ctx: &TestContext) -> ScenarioResult {
    // Setup: 3 nodes running
    ctx.wait_all_healthy().await?;

    // Action: kill node 3
    ctx.kill_node(3).await?;

    // Wait for quorum to update (2 heartbeat cycles + hysteresis)
    ctx.wait_secs(15).await;

    // Assert
    let s1 = ctx.get_status(1).await?;
    let s2 = ctx.get_status(2).await?;
    assert_eq!(s1.quorum_status, "degraded");
    assert_eq!(s2.quorum_status, "degraded");

    // One of them should be leader
    assert!(s1.is_leader || s2.is_leader);
    assert!(!(s1.is_leader && s2.is_leader)); // not both

    Ok(())
}
```

**Scenarios (10):**

| # | Name | Nodes | Steps | Assertions |
|---|------|-------|-------|------------|
| 1 | `quorum-degraded` | 3 | Kill node 3, wait 15s | Nodes 1+2 = Degraded |
| 2 | `quorum-fenced` | 3 | Kill nodes 2+3, wait 15s | Node 1 = Fenced |
| 3 | `quorum-recovery` | 3 | Fence node 1 (kill 2+3), start node 2, wait 15s | Node 1+2 = Degraded |
| 4 | `fenced-write-denied` | 3 | Fence node 1, try write | HTTP 503 |
| 5 | `fenced-read-allowed` | 3 | Write file, fence node, try read | HTTP 200 + correct data |
| 6 | `leader-failover` | 3 | Identify leader, kill it, wait 15s | New leader elected |
| 7 | `partition-majority` | 3 | Partition {1,2} vs {3}, wait 15s | Nodes 1+2 Degraded, Node 3 Fenced |
| 8 | `partition-witness-2node` | 2 | Partition {1} vs {2}, witness smart mode, wait 15s | Lowest node_id = Degraded, other = Fenced |
| 9 | `replication-basic` | 3 | Write on node 1, wait 10s, read on node 2 | Same content |
| 10 | `repair-leader-only` | 3 | Check logs of non-leader for "skipping repair" | Log message found |

Each scenario:
1. Starts fresh (new temp dir, new processes)
2. Runs setup + actions + assertions
3. Tears down everything
4. Reports PASS/FAIL with timing

**Output format:**
```
[PASS] quorum-degraded (12.3s)
[PASS] quorum-fenced (14.1s)
[FAIL] fenced-write-denied: expected status 503, got 200
[PASS] leader-failover (16.2s)
...

Results: 8/10 passed, 2 failed
```

**Exit code:** 0 if all pass, 1 if any fail (for CI integration).

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `apps/san-testbed/Cargo.toml` | Crate manifest |
| `apps/san-testbed/src/main.rs` | CLI arg parsing, startup orchestration |
| `apps/san-testbed/src/cluster.rs` | Node lifecycle management (start/stop/status) |
| `apps/san-testbed/src/db_init.rs` | SQLite pre-initialization (fake disks, backends, volumes, peers) |
| `apps/san-testbed/src/witness.rs` | Mock witness HTTP server |
| `apps/san-testbed/src/cli.rs` | Interactive CLI (rustyline-based command loop) |
| `apps/san-testbed/src/partition.rs` | Network partition simulation via peer address manipulation |
| `apps/san-testbed/src/scenarios.rs` | Automated test scenario definitions |
| `apps/san-testbed/src/context.rs` | `TestContext` — shared state and helper methods for scenarios |

### Modified Files

| File | Change |
|------|--------|
| `apps/vmm-san/src/engine/peer_monitor.rs` | Extract pure `calculate_quorum_status()` and `calculate_is_leader()`, add `#[cfg(test)]` module with 14 unit tests |
| `Cargo.toml` (workspace) | Add `apps/san-testbed` to workspace members |

---

## TestContext API

The `TestContext` struct provides a clean API for both interactive CLI and automated scenarios:

```rust
pub struct TestContext {
    nodes: Vec<NodeHandle>,       // child processes + config
    witness: WitnessHandle,       // mock witness server
    temp_dir: PathBuf,            // root temp directory
    original_peers: HashMap<(usize, String), String>,  // for heal
}

impl TestContext {
    pub async fn wait_all_healthy(&self) -> Result<()>;
    pub async fn get_status(&self, node: usize) -> Result<NodeStatus>;
    pub async fn kill_node(&self, node: usize) -> Result<()>;
    pub async fn start_node(&self, node: usize) -> Result<()>;
    pub async fn partition(&self, group_a: &[usize], group_b: &[usize]) -> Result<()>;
    pub async fn heal(&self) -> Result<()>;
    pub async fn write_file(&self, node: usize, vol: &str, path: &str, content: &[u8]) -> Result<u16>;
    pub async fn read_file(&self, node: usize, vol: &str, path: &str) -> Result<(u16, Vec<u8>)>;
    pub async fn wait_secs(&self, secs: u64);
    pub fn set_witness_mode(&self, mode: WitnessMode);
    pub async fn shutdown(self) -> Result<()>;
}
```

---

## Cleanup

On `exit`, `quit`, SIGINT (Ctrl+C), or scenario completion:
1. Send SIGTERM to all child processes
2. Wait up to 5s for graceful shutdown
3. Send SIGKILL if still alive
4. Remove temp directory recursively

The testbed registers a `ctrlc` handler to ensure cleanup even on Ctrl+C.
