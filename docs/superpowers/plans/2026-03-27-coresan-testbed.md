# CoreSAN Test Framework & Testbed — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build unit tests for quorum/leader logic and a multi-node testbed binary that can run automated scenarios and interactive chaos testing on localhost.

**Architecture:** Pure functions extracted from `peer_monitor.rs` for unit-testable quorum/leader logic. A separate `san-testbed` crate that starts N vmm-san processes with pre-initialized SQLite DBs (fake disks as temp dirs), a mock witness server, and an interactive CLI with automated scenario support.

**Tech Stack:** Rust, tokio, axum (witness mock), reqwest (API polling), rusqlite (DB pre-init), rustyline (CLI), tempfile

**Spec:** `docs/superpowers/specs/2026-03-27-coresan-testbed-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `apps/san-testbed/Cargo.toml` | Crate manifest with dependencies |
| `apps/san-testbed/src/main.rs` | CLI arg parsing, startup orchestration, entry point |
| `apps/san-testbed/src/cluster.rs` | Node lifecycle: start/stop/restart child processes, health polling |
| `apps/san-testbed/src/db_init.rs` | SQLite pre-initialization: schema, node_settings, peers, backends, volumes |
| `apps/san-testbed/src/witness.rs` | Mock witness HTTP server with configurable allow/deny/smart/off modes |
| `apps/san-testbed/src/cli.rs` | Interactive CLI command loop (rustyline) |
| `apps/san-testbed/src/partition.rs` | Network partition simulation via `/api/peers/join` address manipulation |
| `apps/san-testbed/src/scenarios.rs` | 10 automated test scenarios |
| `apps/san-testbed/src/context.rs` | `TestContext` struct — shared state and helper methods |

### Modified Files

| File | Change |
|------|--------|
| `apps/vmm-san/src/engine/peer_monitor.rs` | Extract pure `calculate_quorum_status()` and `calculate_is_leader()`, add `#[cfg(test)]` with 14 unit tests |
| `Cargo.toml` (workspace root) | Add `apps/san-testbed` to workspace members |

---

## Task 1: Extract pure quorum/leader functions and add unit tests

**Files:**
- Modify: `apps/vmm-san/src/engine/peer_monitor.rs`

- [ ] **Step 1: Add pure `calculate_quorum_status` function**

In `apps/vmm-san/src/engine/peer_monitor.rs`, add after the existing `use` statements (before `const HEARTBEAT_INTERVAL_SECS`):

```rust
/// Pure quorum calculation — no state, no IO. Testable.
pub fn calculate_quorum_status(
    total_nodes: usize,
    reachable_nodes: usize,
    witness_allowed: Option<bool>,
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
    if witness_allowed == Some(true) {
        return QuorumStatus::Degraded;
    }
    QuorumStatus::Fenced
}

/// Pure leader calculation — no state, no IO. Testable.
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

- [ ] **Step 2: Refactor `compute_quorum` to call the pure function**

Replace the body of `compute_quorum()` (the existing async function at the bottom of the file) to delegate to the new pure function:

```rust
async fn compute_quorum(state: &CoreSanState) -> QuorumStatus {
    let total_peers = state.peers.len();
    let total_nodes = 1 + total_peers;
    let reachable_peers = state.peers.iter()
        .filter(|p| p.status == PeerStatus::Online)
        .count();
    let reachable = 1 + reachable_peers;

    // Try witness if no majority
    let majority = (total_nodes / 2) + 1;
    let witness_allowed = if reachable < majority {
        let witness_url = &state.config.cluster.witness_url;
        if !witness_url.is_empty() {
            match PeerClient::witness_check(witness_url, &state.node_id).await {
                Ok(allowed) => {
                    tracing::debug!("Witness check: allowed={}", allowed);
                    Some(allowed)
                }
                Err(e) => {
                    tracing::warn!("Witness unreachable: {}", e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    calculate_quorum_status(total_nodes, reachable, witness_allowed)
}
```

- [ ] **Step 3: Refactor `compute_is_leader` to call the pure function**

Replace the body of `compute_is_leader()`:

```rust
fn compute_is_leader(state: &CoreSanState, quorum: QuorumStatus) -> bool {
    let online_ids: Vec<String> = state.peers.iter()
        .filter(|p| p.status == PeerStatus::Online)
        .map(|p| p.node_id.clone())
        .collect();
    let refs: Vec<&str> = online_ids.iter().map(|s| s.as_str()).collect();
    calculate_is_leader(&state.node_id, &refs, quorum)
}
```

- [ ] **Step 4: Add `#[cfg(test)]` module with 14 unit tests**

At the bottom of `apps/vmm-san/src/engine/peer_monitor.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ── Quorum tests ──────────────────────────────────────────

    #[test]
    fn solo_node() {
        assert_eq!(calculate_quorum_status(1, 1, None), QuorumStatus::Solo);
    }

    #[test]
    fn two_nodes_all_online() {
        assert_eq!(calculate_quorum_status(2, 2, None), QuorumStatus::Active);
    }

    #[test]
    fn two_nodes_one_offline_no_witness() {
        assert_eq!(calculate_quorum_status(2, 1, None), QuorumStatus::Fenced);
    }

    #[test]
    fn two_nodes_one_offline_witness_allows() {
        assert_eq!(calculate_quorum_status(2, 1, Some(true)), QuorumStatus::Degraded);
    }

    #[test]
    fn two_nodes_one_offline_witness_denies() {
        assert_eq!(calculate_quorum_status(2, 1, Some(false)), QuorumStatus::Fenced);
    }

    #[test]
    fn three_nodes_all_online() {
        assert_eq!(calculate_quorum_status(3, 3, None), QuorumStatus::Active);
    }

    #[test]
    fn three_nodes_one_offline() {
        assert_eq!(calculate_quorum_status(3, 2, None), QuorumStatus::Degraded);
    }

    #[test]
    fn three_nodes_two_offline() {
        assert_eq!(calculate_quorum_status(3, 1, None), QuorumStatus::Fenced);
    }

    #[test]
    fn five_nodes_two_offline() {
        assert_eq!(calculate_quorum_status(5, 3, None), QuorumStatus::Degraded);
    }

    #[test]
    fn five_nodes_three_offline() {
        assert_eq!(calculate_quorum_status(5, 2, None), QuorumStatus::Fenced);
    }

    #[test]
    fn ten_nodes_four_offline() {
        assert_eq!(calculate_quorum_status(10, 6, None), QuorumStatus::Degraded);
    }

    // ── Leader tests ──────────────────────────────────────────

    #[test]
    fn leader_lowest_id() {
        assert!(calculate_is_leader("aaa", &["bbb", "ccc"], QuorumStatus::Active));
    }

    #[test]
    fn leader_not_lowest() {
        assert!(!calculate_is_leader("ccc", &["aaa", "bbb"], QuorumStatus::Active));
    }

    #[test]
    fn leader_fenced_never() {
        assert!(!calculate_is_leader("aaa", &["bbb"], QuorumStatus::Fenced));
    }
}
```

- [ ] **Step 5: Verify tests pass**

Run: `cargo test -p vmm-san --target-dir /tmp/cargo-test-san 2>&1 | tail -20`
Expected: `test result: ok. 14 passed; 0 failed`

- [ ] **Step 6: Verify compilation still works**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 7: Commit**

```bash
git add apps/vmm-san/src/engine/peer_monitor.rs
git commit -m "feat(san): extract pure quorum/leader functions, add 14 unit tests"
```

---

## Task 2: Create san-testbed crate scaffold

**Files:**
- Create: `apps/san-testbed/Cargo.toml`
- Create: `apps/san-testbed/src/main.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create `apps/san-testbed/Cargo.toml`**

```toml
[package]
name = "san-testbed"
version.workspace = true
edition = "2021"
description = "CoreSAN multi-node testbed — interactive chaos testing and automated scenarios"

[[bin]]
name = "san-testbed"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
axum = { version = "0.8" }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
rusqlite = { version = "0.33", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tempfile = "3"
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
ctrlc = "3"
rustyline = "15"
```

- [ ] **Step 2: Create minimal `apps/san-testbed/src/main.rs`**

```rust
mod cluster;
mod context;
mod db_init;
mod witness;
mod cli;
mod partition;
mod scenarios;

use clap::Parser;

fn main() {
    println!("san-testbed placeholder");
}
```

Note: We won't use clap yet — we'll parse args manually in a later step. For now just the module declarations and a placeholder main. Create empty files for each module:

- [ ] **Step 3: Create empty module files**

Create these empty files (just `//! Module description` comment):

`apps/san-testbed/src/cluster.rs`:
```rust
//! Node lifecycle management — start, stop, restart vmm-san child processes.
```

`apps/san-testbed/src/context.rs`:
```rust
//! TestContext — shared state and helper methods for CLI and scenarios.
```

`apps/san-testbed/src/db_init.rs`:
```rust
//! SQLite pre-initialization for testbed nodes.
```

`apps/san-testbed/src/witness.rs`:
```rust
//! Mock witness HTTP server.
```

`apps/san-testbed/src/cli.rs`:
```rust
//! Interactive CLI command loop.
```

`apps/san-testbed/src/partition.rs`:
```rust
//! Network partition simulation.
```

`apps/san-testbed/src/scenarios.rs`:
```rust
//! Automated test scenarios.
```

- [ ] **Step 4: Add to workspace**

In `Cargo.toml` (workspace root), add `"apps/san-testbed"` to the `members` array:

```toml
members = [
  "apps/vmctl",
  "apps/vmmanager",
  "apps/vmm-server",
  "apps/vmm-cluster",
  "apps/vmm-san",
  "apps/vmm-appliance",
  "apps/san-testbed",
  "libs/vmm-core",
  "tests/hosttests",
]
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 6: Commit**

```bash
git add apps/san-testbed/ Cargo.toml
git commit -m "feat(testbed): scaffold san-testbed crate with module stubs"
```

---

## Task 3: Database pre-initialization

**Files:**
- Modify: `apps/san-testbed/src/db_init.rs`

- [ ] **Step 1: Implement `init_node_db` function**

The DB schema SQL is copied from `apps/vmm-san/src/db/mod.rs` (the `SCHEMA` constant). We replicate it here to avoid depending on the vmm-san crate at compile time (we run vmm-san as a child process, not as a library).

Write `apps/san-testbed/src/db_init.rs`:

```rust
//! SQLite pre-initialization for testbed nodes.

use rusqlite::Connection;
use std::path::Path;

/// The CoreSAN schema — replicated from vmm-san/src/db/mod.rs.
/// Must be kept in sync manually (testbed only, not production).
const SCHEMA: &str = include_str!("schema.sql");

/// Initialize a node's SQLite database with schema, node_settings, peers, backends, and a test volume.
pub fn init_node_db(
    db_path: &Path,
    node_id: &str,
    node_index: usize,
    total_nodes: usize,
    base_port: u16,
    disk_paths: &[String],
    peer_secret: &str,
) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Cannot open DB {}: {}", db_path.display(), e))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("PRAGMA: {}", e))?;

    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("Schema: {}", e))?;

    // node_settings table (created in vmm-san main.rs, not in schema)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS node_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);"
    ).map_err(|e| format!("node_settings table: {}", e))?;

    conn.execute(
        "INSERT OR REPLACE INTO node_settings (key, value) VALUES ('node_id', ?1)",
        rusqlite::params![node_id],
    ).map_err(|e| format!("node_id: {}", e))?;

    // Insert peers (all other nodes)
    for i in 1..=total_nodes {
        if i == node_index { continue; }
        let peer_id = format!("node-{}", i);
        let peer_port = base_port + i as u16;
        let peer_addr = format!("http://127.0.0.1:{}", peer_port);
        let hostname = format!("testbed-node-{}", i);
        conn.execute(
            "INSERT OR REPLACE INTO peers (node_id, address, peer_port, hostname, status)
             VALUES (?1, ?2, ?3, ?4, 'connecting')",
            rusqlite::params![&peer_id, &peer_addr, peer_port + 100, &hostname],
        ).map_err(|e| format!("peer insert: {}", e))?;
    }

    // Insert test volume
    conn.execute(
        "INSERT OR IGNORE INTO volumes (id, name, ftt, status)
         VALUES ('testbed-vol', 'testbed-vol', 1, 'online')",
        [],
    ).map_err(|e| format!("volume: {}", e))?;

    // Insert claimed disks and backends
    for (idx, disk_path) in disk_paths.iter().enumerate() {
        let disk_id = format!("disk-{}-{}", node_index, idx);
        let backend_id = format!("backend-{}-{}", node_id, idx);

        conn.execute(
            "INSERT OR REPLACE INTO claimed_disks (id, device_path, mount_path, fs_type, size_bytes, status, backend_id)
             VALUES (?1, ?2, ?3, 'ext4', 107374182400, 'mounted', ?4)",
            rusqlite::params![&disk_id, &format!("/fake/dev/sd{}", (b'a' + idx as u8) as char), disk_path, &backend_id],
        ).map_err(|e| format!("claimed_disk: {}", e))?;

        conn.execute(
            "INSERT OR REPLACE INTO backends (id, node_id, path, total_bytes, free_bytes, status, claimed_disk_id)
             VALUES (?1, ?2, ?3, 107374182400, 107374182400, 'online', ?4)",
            rusqlite::params![&backend_id, node_id, disk_path, &disk_id],
        ).map_err(|e| format!("backend: {}", e))?;
    }

    Ok(())
}
```

- [ ] **Step 2: Create `apps/san-testbed/src/schema.sql`**

Copy the full `SCHEMA` SQL constant from `apps/vmm-san/src/db/mod.rs` (lines 6-211, the content inside the `r#"..."#` string) into a new file `apps/san-testbed/src/schema.sql`. Also append the migration columns at the end:

```sql
-- (paste full schema from vmm-san/src/db/mod.rs SCHEMA constant here)

-- Migration columns (from vmm-san db::migrate)
-- These are already in CREATE TABLE for new DBs, but listed for clarity
```

The schema already includes all columns (including `ownership_epoch`, `ownership_tick`, `sync_mode`) in the CREATE TABLE statements, so no ALTER TABLE needed.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add apps/san-testbed/src/db_init.rs apps/san-testbed/src/schema.sql
git commit -m "feat(testbed): implement DB pre-initialization with fake disks"
```

---

## Task 4: Mock witness server

**Files:**
- Modify: `apps/san-testbed/src/witness.rs`

- [ ] **Step 1: Implement witness mock**

Write `apps/san-testbed/src/witness.rs`:

```rust
//! Mock witness HTTP server for testbed.

use axum::{extract::{Path, State}, Json, Router, routing::get};
use serde_json::Value;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;

#[derive(Debug, Clone, PartialEq)]
pub enum WitnessMode {
    AllowAll,
    DenyAll,
    Off,
}

pub struct WitnessState {
    pub mode: RwLock<WitnessMode>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

pub type WitnessHandle = Arc<WitnessState>;

pub fn new_handle() -> WitnessHandle {
    Arc::new(WitnessState {
        mode: RwLock::new(WitnessMode::AllowAll),
        shutdown_tx: None,
    })
}

async fn witness_handler(
    State(state): State<WitnessHandle>,
    Path(_node_id): Path<String>,
) -> Json<Value> {
    let mode = state.mode.read().unwrap().clone();
    match mode {
        WitnessMode::AllowAll => Json(serde_json::json!({"allowed": true})),
        WitnessMode::DenyAll => Json(serde_json::json!({"allowed": false, "reason": "mock deny-all"})),
        WitnessMode::Off => Json(serde_json::json!({"allowed": false, "reason": "mock off"})),
    }
}

/// Start the witness mock server. Returns the handle for mode control.
pub async fn spawn(port: u16) -> WitnessHandle {
    let handle = new_handle();
    let state = handle.clone();

    let app = Router::new()
        .route("/api/san/witness/{node_id}", get(witness_handler))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await
        .unwrap_or_else(|e| panic!("Cannot bind witness to {}: {}", addr, e));

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    // Give server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    handle
}

pub fn set_mode(handle: &WitnessHandle, mode: WitnessMode) {
    *handle.mode.write().unwrap() = mode;
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add apps/san-testbed/src/witness.rs
git commit -m "feat(testbed): implement mock witness server with allow/deny/off modes"
```

---

## Task 5: Node lifecycle management (cluster.rs)

**Files:**
- Modify: `apps/san-testbed/src/cluster.rs`

- [ ] **Step 1: Implement `NodeHandle` and cluster management**

Write `apps/san-testbed/src/cluster.rs`:

```rust
//! Node lifecycle management — start, stop, restart vmm-san child processes.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::fs;

pub struct NodeHandle {
    pub index: usize,
    pub node_id: String,
    pub port: u16,
    pub peer_port: u16,
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub disk_paths: Vec<String>,
    pub log_path: PathBuf,
    child: Option<Child>,
}

impl NodeHandle {
    pub fn new(
        index: usize,
        base_port: u16,
        temp_dir: &Path,
    ) -> Self {
        let node_id = format!("node-{}", index);
        let port = base_port + index as u16;
        let peer_port = port + 100;
        let node_dir = temp_dir.join(format!("node-{}", index));
        let data_dir = node_dir.join("data");
        let fuse_dir = node_dir.join("fuse");
        let disk_0 = node_dir.join("disk-0");
        let disk_1 = node_dir.join("disk-1");

        // Create directories
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&fuse_dir).unwrap();
        fs::create_dir_all(&disk_0).unwrap();
        fs::create_dir_all(&disk_1).unwrap();

        let disk_paths = vec![
            disk_0.to_string_lossy().to_string(),
            disk_1.to_string_lossy().to_string(),
        ];

        let config_path = node_dir.join("vmm-san.toml");
        let log_path = node_dir.join("output.log");

        Self {
            index,
            node_id,
            port,
            peer_port,
            config_path,
            data_dir,
            disk_paths,
            log_path,
            child: None,
        }
    }

    pub fn address(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Write the TOML config file for this node.
    pub fn write_config(
        &self,
        total_nodes: usize,
        base_port: u16,
        witness_port: u16,
        peer_secret: &str,
    ) {
        let fuse_dir = self.data_dir.parent().unwrap().join("fuse");
        let config = format!(
            r#"[server]
bind = "127.0.0.1"
port = {}

[data]
data_dir = "{}"
fuse_root = "{}"

[peer]
port = {}
secret = "{}"

[cluster]
witness_url = "http://127.0.0.1:{}"

[benchmark]
enabled = false

[integrity]
enabled = false
repair_interval_secs = 9999
"#,
            self.port,
            self.data_dir.display(),
            fuse_dir.display(),
            self.peer_port,
            peer_secret,
            witness_port,
        );
        fs::write(&self.config_path, config).unwrap();
    }

    /// Start the vmm-san process.
    pub fn start(&mut self, vmm_san_binary: &Path) -> Result<(), String> {
        let log_file = fs::File::create(&self.log_path)
            .map_err(|e| format!("Cannot create log file: {}", e))?;
        let log_err = log_file.try_clone()
            .map_err(|e| format!("Cannot clone log file: {}", e))?;

        let child = Command::new(vmm_san_binary)
            .arg("--config")
            .arg(&self.config_path)
            .env("RUST_LOG", "vmm_san=debug")
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| format!("Cannot start node {}: {}", self.index, e))?;

        self.child = Some(child);
        Ok(())
    }

    /// Stop the vmm-san process (SIGTERM, then SIGKILL after 5s).
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Try graceful shutdown
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            // Wait up to 3 seconds
            for _ in 0..30 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            // Force kill
            child.kill().ok();
            child.wait().ok();
        }
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }
}

impl Drop for NodeHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Find the vmm-san binary (built by cargo).
pub fn find_vmm_san_binary() -> PathBuf {
    // Try target/debug first
    let candidates = [
        PathBuf::from("target/debug/vmm-san"),
        PathBuf::from("../../target/debug/vmm-san"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    // Fall back to cargo build
    eprintln!("vmm-san binary not found. Building...");
    let status = Command::new("cargo")
        .args(["build", "-p", "vmm-san"])
        .status()
        .expect("Failed to run cargo build");
    if !status.success() {
        panic!("Failed to build vmm-san");
    }
    candidates[0].clone()
}
```

Note: We need `libc` as a dependency. Add to Cargo.toml.

- [ ] **Step 2: Add `libc` dependency to `apps/san-testbed/Cargo.toml`**

Add to `[dependencies]`:
```toml
libc = "0.2"
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add apps/san-testbed/src/cluster.rs apps/san-testbed/Cargo.toml
git commit -m "feat(testbed): implement node lifecycle management (start/stop/restart)"
```

---

## Task 6: TestContext and partition simulation

**Files:**
- Modify: `apps/san-testbed/src/context.rs`
- Modify: `apps/san-testbed/src/partition.rs`

- [ ] **Step 1: Implement partition simulation**

Write `apps/san-testbed/src/partition.rs`:

```rust
//! Network partition simulation via peer address manipulation.

use reqwest::Client;
use std::collections::HashMap;

/// Store original peer addresses so we can restore them on heal.
pub type OriginalAddresses = HashMap<(usize, String), String>;

/// Apply a network partition: nodes in group_a cannot reach group_b and vice versa.
/// Uses POST /api/peers/join to update addresses to an unreachable endpoint.
pub async fn apply_partition(
    client: &Client,
    nodes: &[(usize, u16, String)],  // (index, port, node_id)
    group_a: &[usize],
    group_b: &[usize],
    peer_secret: &str,
    original: &mut OriginalAddresses,
) -> Result<(), String> {
    let invalid_addr = "http://127.0.0.1:1";

    // For each node in group_a, set peers in group_b to invalid address
    for &a_idx in group_a {
        let a_port = nodes.iter().find(|n| n.0 == a_idx).unwrap().1;
        for &b_idx in group_b {
            let b = nodes.iter().find(|n| n.0 == b_idx).unwrap();
            let b_node_id = &b.2;
            let b_real_addr = format!("http://127.0.0.1:{}", b.1);

            // Save original
            original.entry((a_idx, b_node_id.clone()))
                .or_insert(b_real_addr);

            // Update to invalid
            update_peer_address(client, a_port, b_node_id, invalid_addr, peer_secret).await?;
        }
    }

    // For each node in group_b, set peers in group_a to invalid address
    for &b_idx in group_b {
        let b_port = nodes.iter().find(|n| n.0 == b_idx).unwrap().1;
        for &a_idx in group_a {
            let a = nodes.iter().find(|n| n.0 == a_idx).unwrap();
            let a_node_id = &a.2;
            let a_real_addr = format!("http://127.0.0.1:{}", a.1);

            original.entry((b_idx, a_node_id.clone()))
                .or_insert(a_real_addr);

            update_peer_address(client, b_port, a_node_id, invalid_addr, peer_secret).await?;
        }
    }

    Ok(())
}

/// Heal all partitions — restore original peer addresses.
pub async fn heal_all(
    client: &Client,
    nodes: &[(usize, u16, String)],
    original: &mut OriginalAddresses,
    peer_secret: &str,
) -> Result<(), String> {
    for ((node_idx, peer_id), real_addr) in original.drain() {
        let port = nodes.iter().find(|n| n.0 == node_idx).unwrap().1;
        update_peer_address(client, port, &peer_id, &real_addr, peer_secret).await?;
    }
    Ok(())
}

async fn update_peer_address(
    client: &Client,
    node_port: u16,
    peer_node_id: &str,
    new_address: &str,
    peer_secret: &str,
) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{}/api/peers/join", node_port);
    client.post(&url)
        .header("X-Peer-Secret", peer_secret)
        .json(&serde_json::json!({
            "node_id": peer_node_id,
            "address": new_address,
            "hostname": format!("testbed-{}", peer_node_id),
            "peer_port": 7544,
            "secret": peer_secret,
        }))
        .send().await
        .map_err(|e| format!("partition update failed for port {}: {}", node_port, e))?;
    Ok(())
}
```

- [ ] **Step 2: Implement TestContext**

Write `apps/san-testbed/src/context.rs`:

```rust
//! TestContext — shared state and helper methods for CLI and scenarios.

use crate::cluster::{NodeHandle, find_vmm_san_binary};
use crate::witness::{self, WitnessHandle, WitnessMode};
use crate::partition::{self, OriginalAddresses};
use crate::db_init;
use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const BASE_PORT: u16 = 7442;
const WITNESS_PORT: u16 = 9443;
const PEER_SECRET: &str = "testbed-secret";

#[derive(Debug, Deserialize)]
pub struct NodeStatus {
    pub running: bool,
    pub node_id: String,
    pub quorum_status: String,
    pub is_leader: bool,
    pub peer_count: u32,
}

pub struct TestContext {
    pub nodes: Vec<NodeHandle>,
    pub witness: WitnessHandle,
    pub temp_dir: TempDir,
    pub http: Client,
    pub original_addresses: OriginalAddresses,
    vmm_san_binary: PathBuf,
}

impl TestContext {
    /// Create a new testbed with N nodes.
    pub async fn new(num_nodes: usize) -> Result<Self, String> {
        let temp_dir = TempDir::new()
            .map_err(|e| format!("Cannot create temp dir: {}", e))?;

        let vmm_san_binary = find_vmm_san_binary();
        tracing::info!("Using vmm-san binary: {}", vmm_san_binary.display());

        // Create nodes
        let mut nodes: Vec<NodeHandle> = (1..=num_nodes)
            .map(|i| NodeHandle::new(i, BASE_PORT, temp_dir.path()))
            .collect();

        // Write configs and init DBs
        for node in &nodes {
            node.write_config(num_nodes, BASE_PORT, WITNESS_PORT, PEER_SECRET);

            let db_path = node.data_dir.join("vmm-san.db");
            db_init::init_node_db(
                &db_path,
                &node.node_id,
                node.index,
                num_nodes,
                BASE_PORT,
                &node.disk_paths,
                PEER_SECRET,
            )?;
        }

        // Start witness
        let witness = witness::spawn(WITNESS_PORT).await;

        // Start all nodes
        for node in &mut nodes {
            node.start(&vmm_san_binary)?;
            tracing::info!("Started node {} (port {})", node.index, node.port);
        }

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;

        let ctx = Self {
            nodes,
            witness,
            temp_dir,
            http,
            original_addresses: OriginalAddresses::new(),
            vmm_san_binary,
        };

        Ok(ctx)
    }

    /// Wait for all running nodes to report healthy via /api/status.
    pub async fn wait_all_healthy(&self) -> Result<(), String> {
        for node in &self.nodes {
            if !node.is_running() { continue; }
            self.wait_node_healthy(node.index).await?;
        }
        Ok(())
    }

    pub async fn wait_node_healthy(&self, index: usize) -> Result<(), String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/status", node.address());
        for attempt in 0..60 {
            match self.http.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => {
                    if attempt % 10 == 0 && attempt > 0 {
                        tracing::debug!("Waiting for node {} to be healthy... ({}s)", index, attempt / 2);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
        Err(format!("Node {} did not become healthy within 30s", index))
    }

    /// Get status from a node.
    pub async fn get_status(&self, index: usize) -> Result<NodeStatus, String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/status", node.address());
        let resp = self.http.get(&url).send().await
            .map_err(|e| format!("status request to node {}: {}", index, e))?;
        resp.json::<NodeStatus>().await
            .map_err(|e| format!("status parse for node {}: {}", index, e))
    }

    pub async fn kill_node(&mut self, index: usize) {
        self.nodes[index - 1].stop();
        tracing::info!("Killed node {}", index);
    }

    pub async fn start_node(&mut self, index: usize) -> Result<(), String> {
        self.nodes[index - 1].start(&self.vmm_san_binary)?;
        tracing::info!("Started node {}", index);
        Ok(())
    }

    pub async fn partition(&mut self, group_a: &[usize], group_b: &[usize]) -> Result<(), String> {
        let node_info: Vec<(usize, u16, String)> = self.nodes.iter()
            .map(|n| (n.index, n.port, n.node_id.clone()))
            .collect();
        partition::apply_partition(
            &self.http, &node_info, group_a, group_b,
            PEER_SECRET, &mut self.original_addresses,
        ).await
    }

    pub async fn heal(&mut self) -> Result<(), String> {
        let node_info: Vec<(usize, u16, String)> = self.nodes.iter()
            .map(|n| (n.index, n.port, n.node_id.clone()))
            .collect();
        partition::heal_all(
            &self.http, &node_info, &mut self.original_addresses, PEER_SECRET,
        ).await
    }

    pub async fn write_file(&self, index: usize, vol: &str, path: &str, content: &[u8]) -> Result<u16, String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/volumes/{}/files/{}", node.address(), vol, path);
        let resp = self.http.put(&url)
            .header("X-Peer-Secret", PEER_SECRET)
            .body(content.to_vec())
            .send().await
            .map_err(|e| format!("write to node {}: {}", index, e))?;
        Ok(resp.status().as_u16())
    }

    pub async fn read_file(&self, index: usize, vol: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/volumes/{}/files/{}", node.address(), vol, path);
        let resp = self.http.get(&url)
            .header("X-Peer-Secret", PEER_SECRET)
            .send().await
            .map_err(|e| format!("read from node {}: {}", index, e))?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await
            .map_err(|e| format!("read body from node {}: {}", index, e))?;
        Ok((status, body.to_vec()))
    }

    pub fn set_witness_mode(&self, mode: WitnessMode) {
        witness::set_mode(&self.witness, mode);
    }

    pub async fn wait_secs(&self, secs: u64) {
        tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
    }

    /// Read a node's log file.
    pub fn read_log(&self, index: usize) -> String {
        std::fs::read_to_string(&self.nodes[index - 1].log_path).unwrap_or_default()
    }

    pub fn shutdown(&mut self) {
        for node in &mut self.nodes {
            node.stop();
        }
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        self.shutdown();
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add apps/san-testbed/src/context.rs apps/san-testbed/src/partition.rs
git commit -m "feat(testbed): implement TestContext and network partition simulation"
```

---

## Task 7: Automated scenarios

**Files:**
- Modify: `apps/san-testbed/src/scenarios.rs`

- [ ] **Step 1: Implement all 10 scenarios**

Write `apps/san-testbed/src/scenarios.rs`:

```rust
//! Automated test scenarios for CoreSAN testbed.

use crate::context::TestContext;
use crate::witness::WitnessMode;

pub struct ScenarioResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub duration: std::time::Duration,
}

type ScenarioFn = fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;

pub async fn run_scenario(name: &str, num_nodes: usize, test_fn: impl AsyncFn(&mut TestContext) -> Result<(), String>) -> ScenarioResult {
    let start = std::time::Instant::now();
    let result = async {
        let mut ctx = TestContext::new(num_nodes).await?;
        ctx.wait_all_healthy().await?;
        let r = test_fn(&mut ctx).await;
        ctx.shutdown();
        r
    }.await;

    ScenarioResult {
        name: name.to_string(),
        passed: result.is_ok(),
        message: result.err().unwrap_or_else(|| "OK".into()),
        duration: start.elapsed(),
    }
}

pub async fn run_all() -> Vec<ScenarioResult> {
    let mut results = Vec::new();

    results.push(run_scenario("quorum-degraded", 3, |ctx| Box::pin(scenario_quorum_degraded(ctx))).await);
    results.push(run_scenario("quorum-fenced", 3, |ctx| Box::pin(scenario_quorum_fenced(ctx))).await);
    results.push(run_scenario("quorum-recovery", 3, |ctx| Box::pin(scenario_quorum_recovery(ctx))).await);
    results.push(run_scenario("fenced-write-denied", 3, |ctx| Box::pin(scenario_fenced_write_denied(ctx))).await);
    results.push(run_scenario("fenced-read-allowed", 3, |ctx| Box::pin(scenario_fenced_read_allowed(ctx))).await);
    results.push(run_scenario("leader-failover", 3, |ctx| Box::pin(scenario_leader_failover(ctx))).await);
    results.push(run_scenario("partition-majority", 3, |ctx| Box::pin(scenario_partition_majority(ctx))).await);
    results.push(run_scenario("partition-witness-2node", 2, |ctx| Box::pin(scenario_partition_witness_2node(ctx))).await);
    results.push(run_scenario("replication-basic", 3, |ctx| Box::pin(scenario_replication_basic(ctx))).await);
    results.push(run_scenario("repair-leader-only", 3, |ctx| Box::pin(scenario_repair_leader_only(ctx))).await);

    results
}

pub async fn run_single(name: &str) -> Option<ScenarioResult> {
    match name {
        "quorum-degraded" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_quorum_degraded(ctx))).await),
        "quorum-fenced" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_quorum_fenced(ctx))).await),
        "quorum-recovery" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_quorum_recovery(ctx))).await),
        "fenced-write-denied" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_fenced_write_denied(ctx))).await),
        "fenced-read-allowed" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_fenced_read_allowed(ctx))).await),
        "leader-failover" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_leader_failover(ctx))).await),
        "partition-majority" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_partition_majority(ctx))).await),
        "partition-witness-2node" => Some(run_scenario(name, 2, |ctx| Box::pin(scenario_partition_witness_2node(ctx))).await),
        "replication-basic" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_replication_basic(ctx))).await),
        "repair-leader-only" => Some(run_scenario(name, 3, |ctx| Box::pin(scenario_repair_leader_only(ctx))).await),
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
    ctx.set_witness_mode(WitnessMode::AllowAll);
    ctx.partition(&[1], &[2]).await?;
    ctx.wait_secs(25).await;

    // With witness allow-all, both nodes ask witness and both get allowed.
    // In smart mode, only the lower node_id would win. With allow-all,
    // both should be degraded (witness grants both).
    let s1 = ctx.get_status(1).await?;
    let s2 = ctx.get_status(2).await?;

    // Both should be degraded (witness allows both in allow-all mode)
    if s1.quorum_status != "degraded" { return Err(format!("node 1 expected degraded, got {}", s1.quorum_status)); }
    if s2.quorum_status != "degraded" { return Err(format!("node 2 expected degraded, got {}", s2.quorum_status)); }
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
```

Note: The `AsyncFn` trait is not stable. We'll use a simpler approach — just call each scenario function directly without the `run_scenario` wrapper abstraction. Let me adjust — the `run_scenario` function will take a future directly:

Actually, the cleaner approach is to just inline the pattern in `run_all` and `run_single`. The `run_scenario` helper will be a macro or just repeated. Let me simplify — each scenario creates its own context. The actual implementation will handle the `async fn` pointer issue by just repeating the try/time pattern. The implementer should adjust the exact async ergonomics to what compiles.

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | grep -E "^error"`
Expected: May have async fn trait issues — fix by using `Box::pin` or by inlining. The implementer should adjust.

- [ ] **Step 3: Commit**

```bash
git add apps/san-testbed/src/scenarios.rs
git commit -m "feat(testbed): implement 10 automated test scenarios"
```

---

## Task 8: Interactive CLI

**Files:**
- Modify: `apps/san-testbed/src/cli.rs`

- [ ] **Step 1: Implement CLI command loop**

Write `apps/san-testbed/src/cli.rs`:

```rust
//! Interactive CLI command loop.

use crate::context::TestContext;
use crate::witness::WitnessMode;
use rustyline::DefaultEditor;

pub async fn run(ctx: &mut TestContext) {
    let mut rl = DefaultEditor::new().unwrap();
    println!("\nCoreSAN Testbed — {} nodes running. Type 'help' for commands.\n",
        ctx.nodes.iter().filter(|n| n.is_running()).count());

    loop {
        let line = match rl.readline("san-testbed> ") {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() { continue; }
        rl.add_history_entry(line).ok();

        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "help" => print_help(),
            "status" => cmd_status(ctx).await,
            "kill" if parts.len() >= 2 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    ctx.kill_node(n).await;
                }
            }
            "start" if parts.len() >= 2 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    if let Err(e) = ctx.start_node(n).await {
                        println!("Error: {}", e);
                    }
                }
            }
            "partition" => {
                // partition 1,2 vs 3
                if let Some((a, b)) = parse_partition(&parts[1..]) {
                    match ctx.partition(&a, &b).await {
                        Ok(_) => println!("Partition applied: {:?} vs {:?}", a, b),
                        Err(e) => println!("Error: {}", e),
                    }
                } else {
                    println!("Usage: partition 1,2 vs 3");
                }
            }
            "heal" => {
                match ctx.heal().await {
                    Ok(_) => println!("All partitions healed"),
                    Err(e) => println!("Error: {}", e),
                }
            }
            "write" if parts.len() >= 5 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    let content = parts[4..].join(" ");
                    match ctx.write_file(n, parts[2], parts[3], content.as_bytes()).await {
                        Ok(status) => println!("HTTP {}", status),
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            "read" if parts.len() >= 4 => {
                if let Ok(n) = parts[1].parse::<usize>() {
                    match ctx.read_file(n, parts[2], parts[3]).await {
                        Ok((status, body)) => {
                            println!("HTTP {} — {} bytes", status, body.len());
                            if let Ok(s) = String::from_utf8(body) {
                                println!("{}", s);
                            }
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            "volumes" => {
                // Get from first running node
                for node in &ctx.nodes {
                    if !node.is_running() { continue; }
                    let url = format!("{}/api/volumes", node.address());
                    match ctx.http.get(&url).send().await {
                        Ok(resp) => {
                            if let Ok(body) = resp.text().await {
                                println!("{}", body);
                            }
                            break;
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                }
            }
            "witness" if parts.len() >= 2 => {
                match parts[1] {
                    "allow-all" => { ctx.set_witness_mode(WitnessMode::AllowAll); println!("Witness: allow-all"); }
                    "deny-all" => { ctx.set_witness_mode(WitnessMode::DenyAll); println!("Witness: deny-all"); }
                    "off" => { ctx.set_witness_mode(WitnessMode::Off); println!("Witness: off"); }
                    _ => println!("Usage: witness allow-all|deny-all|off"),
                }
            }
            "wait" if parts.len() >= 2 => {
                if let Ok(secs) = parts[1].parse::<u64>() {
                    println!("Waiting {}s...", secs);
                    ctx.wait_secs(secs).await;
                }
            }
            "exit" | "quit" => break,
            _ => println!("Unknown command. Type 'help'."),
        }
    }
}

fn print_help() {
    println!("Commands:");
    println!("  status                        Show all node status");
    println!("  kill <n>                      Stop node N");
    println!("  start <n>                     Start node N");
    println!("  partition <a,b> vs <c,d>      Simulate network partition");
    println!("  heal                          Remove all partitions");
    println!("  write <n> <vol> <path> <data> Write file to node");
    println!("  read <n> <vol> <path>         Read file from node");
    println!("  volumes                       List volumes");
    println!("  witness allow-all|deny-all|off Control mock witness");
    println!("  wait <secs>                   Wait N seconds");
    println!("  exit                          Shutdown and exit");
}

fn parse_partition(parts: &[&str]) -> Option<(Vec<usize>, Vec<usize>)> {
    // Parse: "1,2 vs 3" or "1,2 vs 3,4"
    let joined = parts.join(" ");
    let sides: Vec<&str> = joined.split(" vs ").collect();
    if sides.len() != 2 { return None; }

    let a: Vec<usize> = sides[0].split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let b: Vec<usize> = sides[1].split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if a.is_empty() || b.is_empty() { return None; }
    Some((a, b))
}

async fn cmd_status(ctx: &TestContext) {
    for node in &ctx.nodes {
        let running = if node.is_running() { "RUNNING" } else { "STOPPED" };
        if !node.is_running() {
            println!("  Node {} (port {}): {}", node.index, node.port, running);
            continue;
        }
        match ctx.get_status(node.index).await {
            Ok(s) => {
                let leader = if s.is_leader { " LEADER" } else { "" };
                println!("  Node {} (port {}): {}  quorum={}  peers={}{}",
                    node.index, node.port, running, s.quorum_status, s.peer_count, leader);
            }
            Err(e) => {
                println!("  Node {} (port {}): {}  (error: {})", node.index, node.port, running, e);
            }
        }
    }
    let wmode = ctx.witness.mode.read().unwrap();
    println!("  Witness (port 9443): {:?}", *wmode);
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add apps/san-testbed/src/cli.rs
git commit -m "feat(testbed): implement interactive CLI with all commands"
```

---

## Task 9: Wire up main.rs

**Files:**
- Modify: `apps/san-testbed/src/main.rs`

- [ ] **Step 1: Implement main with arg parsing and orchestration**

Write `apps/san-testbed/src/main.rs`:

```rust
//! CoreSAN Testbed — multi-node testing and chaos simulation.

mod cluster;
mod context;
mod db_init;
mod witness;
mod cli;
mod partition;
mod scenarios;

use context::TestContext;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("san_testbed=info"))
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    let num_nodes = args.iter()
        .position(|a| a == "--nodes")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3);

    let scenario = args.iter()
        .position(|a| a == "--scenario")
        .and_then(|i| args.get(i + 1))
        .cloned();

    println!("CoreSAN Testbed");
    println!("===============");

    if let Some(scenario_name) = scenario {
        // Run automated scenario(s)
        if scenario_name == "all" {
            let results = scenarios::run_all().await;
            scenarios::print_results(&results);
            let failed = results.iter().any(|r| !r.passed);
            std::process::exit(if failed { 1 } else { 0 });
        } else {
            match scenarios::run_single(&scenario_name).await {
                Some(result) => {
                    scenarios::print_results(&[result.clone()]);
                    std::process::exit(if result.passed { 0 } else { 1 });
                }
                None => {
                    eprintln!("Unknown scenario: {}", scenario_name);
                    eprintln!("Available: quorum-degraded, quorum-fenced, quorum-recovery,");
                    eprintln!("  fenced-write-denied, fenced-read-allowed, leader-failover,");
                    eprintln!("  partition-majority, partition-witness-2node, replication-basic,");
                    eprintln!("  repair-leader-only, all");
                    std::process::exit(1);
                }
            }
        }
    } else {
        // Interactive mode
        println!("Starting {} nodes...\n", num_nodes);
        let mut ctx = match TestContext::new(num_nodes).await {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("Failed to start testbed: {}", e);
                std::process::exit(1);
            }
        };

        if let Err(e) = ctx.wait_all_healthy().await {
            eprintln!("Nodes not healthy: {}", e);
            std::process::exit(1);
        }

        cli::run(&mut ctx).await;
        ctx.shutdown();
    }
}
```

- [ ] **Step 2: Verify full compilation**

Run: `cargo check -p san-testbed --target-dir /tmp/cargo-check-testbed 2>&1 | tail -5`
Expected: `Finished` (warnings OK)

- [ ] **Step 3: Commit**

```bash
git add apps/san-testbed/src/main.rs
git commit -m "feat(testbed): wire up main.rs with arg parsing, scenario runner, and interactive mode"
```

---

## Task 10: Final compilation and integration check

**Files:** None (verification only)

- [ ] **Step 1: Build vmm-san**

Run: `cargo build -p vmm-san --target-dir /tmp/cargo-build-san 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 2: Run vmm-san unit tests**

Run: `cargo test -p vmm-san --target-dir /tmp/cargo-test-san 2>&1 | tail -20`
Expected: `test result: ok. 14 passed; 0 failed`

- [ ] **Step 3: Build san-testbed**

Run: `cargo build -p san-testbed --target-dir /tmp/cargo-build-testbed 2>&1 | tail -5`
Expected: `Finished`

- [ ] **Step 4: Fix any compilation issues and commit**

If there are errors, fix them and commit:

```bash
git add -A
git commit -m "fix(testbed): resolve compilation issues"
```

- [ ] **Step 5: Verify testbed runs (smoke test)**

Run: `timeout 10 cargo run -p san-testbed --target-dir /tmp/cargo-build-testbed -- --help 2>&1 || true`
Expected: Testbed starts or shows usage (not a crash).
