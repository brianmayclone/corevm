# CoreSAN Quorum, Leader Election & Fencing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add quorum-based health checks, soft fencing, witness support, leader election, and ownership ticks to CoreSAN so that isolated nodes stop accepting writes and split-brain is prevented.

**Architecture:** Each node computes its quorum status locally every heartbeat cycle (5s) based on reachable peers. Nodes without quorum ask vmm-cluster as witness (tie-breaker for 2-node and even-count clusters). Fenced nodes deny all new write leases. Leader is deterministic (lowest node_id among active nodes) and coordinates repair. Ownership ticks are added to file_map for future quorum-write sync mode.

**Tech Stack:** Rust, SQLite, axum, tokio, FUSE (fuser), reqwest, DashMap

**Spec:** `docs/superpowers/specs/2026-03-26-coresan-quorum-election-fencing-design.md`

---

## File Structure

### New Files
None — all changes go into existing files.

### Modified Files

| File | Responsibility |
|------|---------------|
| `apps/vmm-san/src/state.rs` | Add `QuorumStatus` enum, new fields to `CoreSanState` |
| `apps/vmm-san/src/config.rs` | Add `ClusterSection` with `witness_url` |
| `apps/vmm-san/src/db/mod.rs` | Add migrations for ownership_epoch, ownership_tick, sync_mode |
| `apps/vmm-san/src/engine/peer_monitor.rs` | Add quorum calculation, witness check, leader determination |
| `apps/vmm-san/src/engine/write_lease.rs` | Add quorum gate in `acquire_lease()`, ownership tick/epoch in `atomic_write()` |
| `apps/vmm-san/src/engine/repair.rs` | Skip cycle if not leader |
| `apps/vmm-san/src/engine/replication.rs` | Skip cycle if fenced |
| `apps/vmm-san/src/engine/push_replicator.rs` | Skip push if fenced |
| `apps/vmm-san/src/engine/fuse_mount.rs` | Pass quorum to `acquire_lease()` calls |
| `apps/vmm-san/src/api/files.rs` | Return 503 if fenced, guard sync_mode |
| `apps/vmm-san/src/services/file.rs` | Add quorum gate to duplicate `acquire_lease` wrapper |
| `apps/vmm-san/src/api/status.rs` | Include quorum_status and is_leader in response |
| `apps/vmm-san/src/peer/client.rs` | Add `witness_check()` method, add `is_leader` to heartbeat |
| `apps/vmm-san/src/main.rs` | Initialize new state fields |
| `apps/vmm-cluster/src/api/san.rs` | Add `witness()` endpoint handler |
| `apps/vmm-cluster/src/api/mod.rs` | Register witness route |

---

## Task 1: QuorumStatus enum and state fields

**Files:**
- Modify: `apps/vmm-san/src/state.rs`
- Modify: `apps/vmm-san/src/main.rs:137-145`

- [ ] **Step 1: Add QuorumStatus enum and new fields to state.rs**

In `apps/vmm-san/src/state.rs`, add after the `PeerStatus` enum (after line 25):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum QuorumStatus {
    /// All peers reachable, full read/write
    Active,
    /// Quorum met but some peers unreachable, full read/write
    Degraded,
    /// No quorum, no witness — new leases denied, effectively read-only
    Fenced,
    /// No peers configured — no quorum required, full read/write
    Solo,
}
```

Add `use serde::Serialize;` to the imports at the top.

Add to `CoreSanState` struct (after `write_tx`):

```rust
pub quorum_status: std::sync::RwLock<QuorumStatus>,
pub is_leader: std::sync::atomic::AtomicBool,
```

- [ ] **Step 2: Initialize new fields in main.rs**

In `apps/vmm-san/src/main.rs`, in the `CoreSanState` construction block (around line 137-145), add:

```rust
quorum_status: std::sync::RwLock::new(if peers.is_empty() {
    crate::state::QuorumStatus::Solo
} else {
    crate::state::QuorumStatus::Active  // optimistic until first heartbeat
}),
is_leader: std::sync::atomic::AtomicBool::new(false),
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors (warnings OK)

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/state.rs apps/vmm-san/src/main.rs
git commit -m "feat(san): add QuorumStatus enum and state fields for quorum/fencing"
```

---

## Task 2: Witness URL in config

**Files:**
- Modify: `apps/vmm-san/src/config.rs`

- [ ] **Step 1: Add ClusterSection to config**

In `apps/vmm-san/src/config.rs`, add a new section struct after `LoggingSection` (around line 100):

```rust
#[derive(Debug, Deserialize)]
pub struct ClusterSection {
    /// URL of vmm-cluster for witness tie-breaking (e.g. "https://10.0.0.1:9443").
    /// Empty = no witness, pure majority quorum only.
    #[serde(default)]
    pub witness_url: String,
}

impl Default for ClusterSection {
    fn default() -> Self {
        Self { witness_url: String::new() }
    }
}
```

Add the field to `CoreSanConfig` struct:

```rust
#[serde(default)]
pub cluster: ClusterSection,
```

Add to `CoreSanConfig::default()`:

```rust
cluster: Default::default(),
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-san/src/config.rs
git commit -m "feat(san): add cluster.witness_url config for quorum witness"
```

---

## Task 3: Database migrations for ownership ticks and sync_mode

**Files:**
- Modify: `apps/vmm-san/src/db/mod.rs:229-254` (migrate function)

- [ ] **Step 1: Add migrations**

In `apps/vmm-san/src/db/mod.rs`, in the `migrate()` function, add after the last existing ALTER TABLE statement:

```rust
// Ownership ticks for split-brain conflict resolution
let _ = db.execute("ALTER TABLE file_map ADD COLUMN ownership_epoch INTEGER NOT NULL DEFAULT 0", []);
let _ = db.execute("ALTER TABLE file_map ADD COLUMN ownership_tick INTEGER NOT NULL DEFAULT 0", []);
let _ = db.execute("ALTER TABLE write_log ADD COLUMN ownership_epoch INTEGER NOT NULL DEFAULT 0", []);
let _ = db.execute("ALTER TABLE write_log ADD COLUMN ownership_tick INTEGER NOT NULL DEFAULT 0", []);

// Volume sync mode policy
let _ = db.execute("ALTER TABLE volumes ADD COLUMN sync_mode TEXT NOT NULL DEFAULT 'async'", []);
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add apps/vmm-san/src/db/mod.rs
git commit -m "feat(san): add DB migrations for ownership ticks and sync_mode"
```

---

## Task 4: Witness client method

**Files:**
- Modify: `apps/vmm-san/src/peer/client.rs`

- [ ] **Step 1: Add witness_check method to PeerClient**

In `apps/vmm-san/src/peer/client.rs`, add a new method:

```rust
/// Ask vmm-cluster witness whether this node is allowed to write.
/// Returns Ok(true) if allowed, Ok(false) if denied, Err on timeout/unreachable.
pub async fn witness_check(witness_url: &str, node_id: &str) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(3))
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| format!("witness client build error: {}", e))?;

    let url = format!("{}/api/san/witness/{}", witness_url.trim_end_matches('/'), node_id);
    let resp = client.get(&url)
        .send().await
        .map_err(|e| format!("witness unreachable: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("witness returned {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await
        .map_err(|e| format!("witness response parse error: {}", e))?;

    Ok(body.get("allowed").and_then(|v| v.as_bool()).unwrap_or(false))
}
```

- [ ] **Step 2: Add is_leader field to heartbeat request**

In the `heartbeat()` method body, add `is_leader` to the JSON body being sent. Find the `serde_json::json!` call and add:

```rust
"is_leader": is_leader,
```

Update the `heartbeat()` method signature to accept `is_leader: bool` as an additional parameter.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: Errors in `peer_monitor.rs` where `heartbeat()` is called (missing `is_leader` arg) — that's OK, we fix it in Task 5.

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/peer/client.rs
git commit -m "feat(san): add witness_check() and is_leader to heartbeat"
```

---

## Task 5: Quorum calculation and leader election in peer_monitor

**Files:**
- Modify: `apps/vmm-san/src/engine/peer_monitor.rs`

- [ ] **Step 1: Add quorum calculation function**

In `apps/vmm-san/src/engine/peer_monitor.rs`, add after the `heartbeat_all_peers` function:

```rust
use crate::state::QuorumStatus;

/// Compute quorum status based on reachable peers and optional witness.
async fn compute_quorum(state: &CoreSanState) -> QuorumStatus {
    let total_peers = state.peers.len();
    let total_nodes = 1 + total_peers; // self + peers

    if total_peers == 0 {
        return QuorumStatus::Solo;
    }

    let reachable_peers = state.peers.iter()
        .filter(|p| p.status == crate::state::PeerStatus::Online)
        .count();
    let reachable = 1 + reachable_peers; // self + online peers
    let majority = (total_nodes / 2) + 1;

    if reachable >= majority {
        return if reachable == total_nodes {
            QuorumStatus::Active
        } else {
            QuorumStatus::Degraded
        };
    }

    // No majority — try witness
    let witness_url = &state.config.cluster.witness_url;
    if !witness_url.is_empty() {
        match crate::peer::client::PeerClient::witness_check(witness_url, &state.node_id).await {
            Ok(true) => {
                tracing::debug!("Witness granted quorum for this node");
                return QuorumStatus::Degraded;
            }
            Ok(false) => {
                tracing::warn!("Witness denied quorum for this node");
            }
            Err(e) => {
                tracing::warn!("Witness unreachable: {}", e);
            }
        }
    }

    QuorumStatus::Fenced
}
```

- [ ] **Step 2: Add leader determination function**

```rust
/// Determine if this node is the leader (lowest node_id among Active/Degraded nodes).
fn compute_is_leader(state: &CoreSanState, quorum: QuorumStatus) -> bool {
    if quorum == QuorumStatus::Fenced {
        return false;
    }

    // Check if any online peer has a lower node_id
    for peer in state.peers.iter() {
        if peer.status == crate::state::PeerStatus::Online && peer.node_id < state.node_id {
            return false;
        }
    }
    true
}
```

- [ ] **Step 3: Integrate into heartbeat loop**

Modify the `spawn()` function to add quorum/leader calculation after heartbeats. Add a `fenced_cycles` counter for hysteresis. Replace the loop body:

```rust
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        let client = PeerClient::new(&state.config.peer.secret);
        let mut fenced_cycles: u32 = 0;

        loop {
            tick.tick().await;
            heartbeat_all_peers(&state, &client).await;

            // Compute quorum
            let new_quorum = compute_quorum(&state).await;
            let old_quorum = *state.quorum_status.read().unwrap();

            // Hysteresis: require 2 consecutive fenced cycles before transitioning
            let effective_quorum = if new_quorum == QuorumStatus::Fenced {
                fenced_cycles += 1;
                if fenced_cycles >= 2 {
                    QuorumStatus::Fenced
                } else {
                    old_quorum // keep previous status for 1 more cycle
                }
            } else {
                fenced_cycles = 0;
                new_quorum
            };

            // Log state transitions
            if effective_quorum != old_quorum {
                match effective_quorum {
                    QuorumStatus::Fenced => {
                        tracing::error!("Node FENCED: no quorum, witness denied");
                    }
                    QuorumStatus::Degraded if old_quorum == QuorumStatus::Fenced => {
                        tracing::info!("Node recovered from fenced state");
                    }
                    QuorumStatus::Active if old_quorum == QuorumStatus::Fenced => {
                        tracing::info!("Node recovered from fenced state");
                    }
                    QuorumStatus::Active if old_quorum == QuorumStatus::Degraded => {
                        tracing::info!("All peers reachable, quorum fully healthy");
                    }
                    QuorumStatus::Degraded => {
                        let unreachable = state.peers.iter()
                            .filter(|p| p.status != crate::state::PeerStatus::Online).count();
                        tracing::warn!("Quorum degraded: {} peer(s) unreachable", unreachable);
                    }
                    _ => {}
                }
                *state.quorum_status.write().unwrap() = effective_quorum;
            }

            // Leader election
            let new_leader = compute_is_leader(&state, effective_quorum);
            let old_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);
            if new_leader != old_leader {
                state.is_leader.store(new_leader, std::sync::atomic::Ordering::Relaxed);
                if new_leader {
                    tracing::info!("This node is now the leader");
                } else {
                    tracing::info!("This node is no longer the leader");
                }
            }
        }
    });
}
```

- [ ] **Step 4: Update heartbeat call to pass is_leader**

In `heartbeat_all_peers`, at the top of the function, read the leader status:

```rust
let is_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);
```

Then update the `client.heartbeat()` call to pass `is_leader` as the last argument. The call currently looks like:

```rust
client.heartbeat(&peer.address, &state.node_id, &state.hostname, uptime, &our_address).await
```

Change to:

```rust
client.heartbeat(&peer.address, &state.node_id, &state.hostname, uptime, &our_address, is_leader).await
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-san/src/engine/peer_monitor.rs
git commit -m "feat(san): add quorum calculation, witness check, and leader election"
```

---

## Task 6: Quorum gate in write_lease and ownership ticks

**Files:**
- Modify: `apps/vmm-san/src/engine/write_lease.rs:42-101` (acquire_lease)
- Modify: `apps/vmm-san/src/engine/write_lease.rs:144-255` (atomic_write)

- [ ] **Step 1: Add quorum check to acquire_lease**

In `apps/vmm-san/src/engine/write_lease.rs`, change `acquire_lease` to also accept a reference to `CoreSanState` (or just the `QuorumStatus`). The simplest approach: add a `quorum: QuorumStatus` parameter.

Change the function signature:

```rust
pub fn acquire_lease(
    db: &Connection,
    volume_id: &str,
    rel_path: &str,
    node_id: &str,
    quorum: crate::state::QuorumStatus,
) -> LeaseResult {
```

Add at the top of the function body, before any other logic:

```rust
// Fenced nodes cannot acquire or renew leases
if quorum == crate::state::QuorumStatus::Fenced {
    return LeaseResult::Denied {
        owner_node_id: String::new(),
        until: "node is fenced (no quorum)".into(),
    };
}
```

- [ ] **Step 2: Add ownership epoch increment on owner change**

In the `acquire_lease` function, in the branch where `owner.is_empty()` (new acquisition) and in the branch where `lease_until < now_str` (lease stealing), add after updating write_owner:

```rust
// Increment ownership epoch when owner changes
db.execute(
    "UPDATE file_map SET ownership_epoch = ownership_epoch + 1
     WHERE volume_id = ?1 AND rel_path = ?2",
    rusqlite::params![volume_id, rel_path],
).ok();
```

Do NOT increment epoch in the `owner == node_id` (renewal) branch.

- [ ] **Step 3: Add ownership tick to the file_map UPSERT in atomic_write**

In `atomic_write()` (line 210-221), modify the existing `INSERT ... ON CONFLICT DO UPDATE` to include `ownership_tick`:

Change the UPSERT (line 210-221) to:

```rust
db.execute(
    "INSERT INTO file_map (volume_id, rel_path, size_bytes, sha256, version, write_owner, write_lease_until, created_at, updated_at, ownership_tick)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)
     ON CONFLICT(volume_id, rel_path) DO UPDATE SET
        size_bytes = excluded.size_bytes, sha256 = excluded.sha256,
        version = excluded.version, write_owner = excluded.write_owner,
        write_lease_until = excluded.write_lease_until,
        updated_at = excluded.updated_at,
        ownership_tick = ownership_tick + 1",
    rusqlite::params![volume_id, rel_path, size, &sha256, new_version, node_id,
                      &(chrono::Utc::now() + chrono::Duration::seconds(LEASE_DURATION_SECS)).to_rfc3339(),
                      &now],
).map_err(|e| format!("db file_map: {}", e))?;
```

Key change: `ownership_tick = ownership_tick + 1` in the ON CONFLICT clause (uses the existing value + 1, not the excluded value).

- [ ] **Step 4: Add epoch and tick to write_log INSERT**

Modify the write_log INSERT (line 248-252) to read current epoch/tick and include them:

```rust
// Get current epoch and tick for write_log
let (epoch, tick): (i64, i64) = db.query_row(
    "SELECT ownership_epoch, ownership_tick FROM file_map
     WHERE volume_id = ?1 AND rel_path = ?2",
    rusqlite::params![volume_id, rel_path],
    |row| Ok((row.get(0)?, row.get(1)?)),
).unwrap_or((0, 0));

db.execute(
    "INSERT INTO write_log (file_id, volume_id, rel_path, version, writer_node_id, size_bytes, sha256, ownership_epoch, ownership_tick)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    rusqlite::params![file_id, volume_id, rel_path, new_version, node_id, size, &sha256, epoch, tick],
).ok();
```

- [ ] **Step 5: Update all callers of acquire_lease to pass quorum**

Search for all calls to `acquire_lease()` and add the quorum parameter. There are calls in:
- `write_lease.rs` itself (`atomic_write` calls `acquire_lease` at line 155) — `atomic_write` needs a new `quorum: QuorumStatus` parameter added to its signature
- `engine/fuse_mount.rs` (FUSE write handler) — read quorum from `self.state.quorum_status`
- `services/file.rs:68` has an independent `acquire_lease` wrapper — add the same quorum gate at the top of that function

For `atomic_write`, add `quorum: crate::state::QuorumStatus` parameter and pass it through to `acquire_lease(..., quorum)`.

For `fuse_mount.rs`, read quorum from state before calling:

```rust
let quorum = *self.state.quorum_status.read().unwrap();
```

For `services/file.rs`, add quorum check at the top:

```rust
pub fn acquire_lease(db: &Connection, volume_id: &str, rel_path: &str, node_id: &str, quorum: crate::state::QuorumStatus) -> Result<i64, String> {
    if quorum == crate::state::QuorumStatus::Fenced {
        return Err("node is fenced (no quorum)".into());
    }
    // ... rest unchanged
```

Update any callers of `services::file::FileService::acquire_lease` similarly (grep to confirm — may be unused).

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 7: Commit**

```bash
git add apps/vmm-san/src/engine/write_lease.rs apps/vmm-san/src/engine/fuse_mount.rs apps/vmm-san/src/services/file.rs
git commit -m "feat(san): add quorum gate in acquire_lease and ownership tick/epoch tracking"
```

---

## Task 7: Fence API writes

**Files:**
- Modify: `apps/vmm-san/src/api/files.rs:80-132`

- [ ] **Step 1: Add quorum check at top of write handler**

In `apps/vmm-san/src/api/files.rs`, at the top of the `write()` function body, before selecting a backend, add:

```rust
// Check quorum — fenced nodes reject writes
let quorum = *state.quorum_status.read().unwrap();
if quorum == crate::state::QuorumStatus::Fenced {
    return Err((StatusCode::SERVICE_UNAVAILABLE,
        "node is fenced (no quorum) — writes are not allowed".into()));
}
```

- [ ] **Step 2: Add sync_mode guard (placeholder for future quorum-write)**

In `apps/vmm-san/src/api/files.rs`, after the quorum check, add:

```rust
// Check volume sync_mode — 'quorum' mode not yet implemented
{
    let db = state.db.lock().unwrap();
    let sync_mode: String = db.query_row(
        "SELECT sync_mode FROM volumes WHERE id = ?1",
        rusqlite::params![&volume_id],
        |row| row.get(0),
    ).unwrap_or_else(|_| "async".into());
    if sync_mode == "quorum" {
        return Err((StatusCode::NOT_IMPLEMENTED,
            "sync_mode 'quorum' is not yet implemented".into()));
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/api/files.rs
git commit -m "feat(san): return 503 on fenced writes, guard against quorum sync_mode"
```

---

## Task 8: Pause replication and repair when fenced / not leader

**Files:**
- Modify: `apps/vmm-san/src/engine/repair.rs:14-26`
- Modify: `apps/vmm-san/src/engine/replication.rs:12-22`
- Modify: `apps/vmm-san/src/engine/push_replicator.rs`

- [ ] **Step 1: Skip repair if not leader**

In `apps/vmm-san/src/engine/repair.rs`, in the `spawn()` function's loop body, add at the top of the loop before calling `run_chunk_repair`:

```rust
if !state.is_leader.load(std::sync::atomic::Ordering::Relaxed) {
    tracing::trace!("Not leader, skipping repair cycle");
    continue;
}
```

- [ ] **Step 2: Skip replication if fenced**

In `apps/vmm-san/src/engine/replication.rs`, in the `spawn()` function's loop body, add at the top before calling `process_stale_replicas`:

```rust
let quorum = *state.quorum_status.read().unwrap();
if quorum == crate::state::QuorumStatus::Fenced {
    tracing::trace!("Node fenced, skipping replication cycle");
    continue;
}
```

- [ ] **Step 3: Skip push replication if fenced**

In `apps/vmm-san/src/engine/push_replicator.rs`, in the main replication loop (where it processes `WriteEvent`s from the channel), add a quorum check before pushing to peers:

```rust
let quorum = *state.quorum_status.read().unwrap();
if quorum == crate::state::QuorumStatus::Fenced {
    tracing::trace!("Node fenced, dropping push event");
    continue;
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 5: Commit**

```bash
git add apps/vmm-san/src/engine/repair.rs apps/vmm-san/src/engine/replication.rs apps/vmm-san/src/engine/push_replicator.rs
git commit -m "feat(san): pause repair/replication when fenced or not leader"
```

---

## Task 9: Include quorum_status and is_leader in status API

**Files:**
- Modify: `apps/vmm-san/src/api/status.rs:11-22` (StatusResponse struct)
- Modify: `apps/vmm-san/src/api/status.rs:61-93` (status handler)

- [ ] **Step 1: Add fields to StatusResponse**

In `apps/vmm-san/src/api/status.rs`, add to the `StatusResponse` struct:

```rust
pub quorum_status: String,
pub is_leader: bool,
```

- [ ] **Step 2: Populate fields in status handler**

In the `status()` handler, add before the `Json(StatusResponse { ... })` return:

```rust
let quorum_status = format!("{:?}", *state.quorum_status.read().unwrap()).to_lowercase();
let is_leader = state.is_leader.load(std::sync::atomic::Ordering::Relaxed);
```

Add `quorum_status` and `is_leader` to the `StatusResponse` struct literal.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-san/src/api/status.rs
git commit -m "feat(san): expose quorum_status and is_leader in /api/status"
```

---

## Task 10: Witness endpoint in vmm-cluster

**Files:**
- Modify: `apps/vmm-cluster/src/api/san.rs`
- Modify: `apps/vmm-cluster/src/api/mod.rs`

- [ ] **Step 1: Add witness handler**

In `apps/vmm-cluster/src/api/san.rs`, add:

```rust
/// GET /api/san/witness/{node_id} — witness tie-breaker for SAN quorum.
///
/// The `node_id` parameter is the **cluster host ID** (hosts.id), since that's what
/// SAN nodes know about themselves in cluster context.
///
/// Logic: the cluster checks which SAN hosts it can reach via its own heartbeats.
/// The requesting node is "allowed" if it's in the majority of reachable hosts.
/// On tie: the partition containing the host with the lowest host ID wins.
pub async fn witness(
    State(state): State<Arc<ClusterState>>,
    Path(requesting_host_id): Path<String>,
) -> Json<Value> {
    // Get ALL known SAN hosts (including offline ones)
    let all_san_host_ids: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id FROM hosts WHERE san_enabled = 1 AND san_address != ''"
        ).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    if all_san_host_ids.is_empty() {
        return Json(serde_json::json!({"allowed": false, "reason": "no SAN hosts known"}));
    }

    // Get SAN hosts the cluster considers ONLINE (via its own heartbeat)
    let reachable_ids: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id FROM hosts WHERE san_enabled = 1 AND san_address != '' AND status = 'online'"
        ).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap().filter_map(|r| r.ok()).collect()
    };

    // Is the requesting node reachable from the cluster?
    if !reachable_ids.contains(&requesting_host_id) {
        return Json(serde_json::json!({
            "allowed": false,
            "reason": "requesting node not reachable from cluster"
        }));
    }

    let total = all_san_host_ids.len();
    let reachable = reachable_ids.len();
    let unreachable = total - reachable;

    if reachable > unreachable {
        // Requesting node is in the strictly larger partition
        return Json(serde_json::json!({"allowed": true}));
    }

    if reachable < unreachable {
        // Requesting node is in the smaller partition
        return Json(serde_json::json!({"allowed": false, "reason": "minority partition"}));
    }

    // Tie (e.g. 2-2 split) — the partition containing the lowest host_id wins
    let lowest_overall = all_san_host_ids.iter().min().cloned().unwrap_or_default();
    let allowed = reachable_ids.contains(&lowest_overall);

    Json(serde_json::json!({
        "allowed": allowed,
        "reason": if allowed { "tie broken by lowest host_id" } else { "tie lost — lowest host_id in other partition" }
    }))
}
```

- [ ] **Step 2: Register route**

In `apps/vmm-cluster/src/api/mod.rs`, add the route (no auth required — SAN nodes don't have cluster JWT):

```rust
.route("/api/san/witness/{node_id}", get(san::witness))
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p vmm-cluster --target-dir /tmp/cargo-check-cluster 2>&1 | grep -E "^error"`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-cluster/src/api/san.rs apps/vmm-cluster/src/api/mod.rs
git commit -m "feat(cluster): add /api/san/witness endpoint for SAN quorum tie-breaking"
```

---

## Task 11: Final compilation check and integration verification

**Files:** None (verification only)

- [ ] **Step 1: Full vmm-san compilation**

Run: `cargo check -p vmm-san --target-dir /tmp/cargo-check-san 2>&1 | tail -5`
Expected: `Finished` with only pre-existing warnings

- [ ] **Step 2: Full vmm-cluster compilation**

Run: `cargo check -p vmm-cluster --target-dir /tmp/cargo-check-cluster 2>&1 | tail -5`
Expected: `Finished` with only pre-existing warnings

- [ ] **Step 3: Commit any remaining fixes**

If compilation reveals issues, fix them and commit:

```bash
git add -A
git commit -m "fix(san): resolve compilation issues in quorum/fencing implementation"
```
