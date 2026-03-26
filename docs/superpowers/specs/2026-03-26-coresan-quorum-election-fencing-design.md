# CoreSAN Quorum, Leader Election & Fencing

**Date:** 2026-03-26
**Status:** Approved
**Scope:** `apps/vmm-san` (primary), `apps/vmm-cluster` (witness endpoint)

## Problem

CoreSAN has no distributed consensus. Each node operates autonomously with time-based write leases (30s). During a network partition, both sides mark each other offline, steal leases, and write to the same files independently. There is no split-brain detection, no quorum, no fencing, and no leader election.

## Goals

1. Every node knows at all times whether it may write (quorum-based)
2. Isolated nodes stop accepting writes (soft fence)
3. 2-node clusters are supported via vmm-cluster as witness
4. Up to 10 nodes supported
5. Lightweight — no Raft/Paxos overhead
6. Ownership ticks prepared for future quorum-write sync mode

## Non-Goals

- Quorum-writes (synchronous replication to majority before ACK) — deferred, will be per-volume policy later
- VM-pause signaling to vmm-server — deferred
- Hard fence (immediate I/O kill) — too disruptive for VM workloads

## Constraints

- 2-node clusters must work (no natural majority possible)
- vmm-cluster is the witness for even-node and 2-node tie-breaking
- Performance impact on write path must be minimal (quorum check is local, not networked)
- FUSE reads must always work, even when fenced

---

## Design

### 1. Quorum Status

Each node computes its own quorum status every heartbeat cycle (5s). The status is stored in shared application state and checked on every write.

**Enum:**

```
QuorumStatus:
  Active    — Majority reachable. Full read/write.
  Degraded  — Quorum met but some peers unreachable. Full read/write, warning logged.
  Fenced    — No quorum, no witness. New leases denied. Read-only after existing leases expire (max 30s).
  Solo      — No peers configured. No quorum required. Full read/write.
```

**Stored in:** `CoreSanState` as `RwLock<QuorumStatus>` (read on every write, written every 5s by peer monitor).

### 2. Quorum Calculation

Runs inside `peer_monitor.rs` after each heartbeat cycle completes.

**Inputs:**
- `total_nodes` = 1 (self) + number of configured peers
- `reachable` = 1 (self) + number of peers with status `Online`
- `majority` = floor(total_nodes / 2) + 1

**Logic:**

```
if total_nodes == 1:
    return Solo

if reachable >= majority AND reachable == total_nodes:
    return Active

if reachable >= majority AND reachable < total_nodes:
    return Degraded

// No majority — ask witness
if witness_allows(node_id):
    if reachable > 1: return Degraded
    else: return Degraded  // 2-node: other side is down but witness says OK

return Fenced
```

**Hysteresis:** Status transitions to `Fenced` only after 2 consecutive cycles without quorum (10s). Prevents flapping on transient network hiccups. Transition away from `Fenced` (recovery) is immediate on first cycle with quorum.

### 3. Witness Endpoint

**Location:** vmm-cluster, `GET /api/san/witness/{node_id}`

**Behavior:** vmm-cluster knows the status of all SAN hosts through its own heartbeat engine. When a node asks "may I write?":

1. Query all SAN-enabled hosts and their last heartbeat status
2. Partition nodes into "reachable from cluster" and "unreachable from cluster"
3. If the requesting node is in the reachable set AND has more (or equal) reachable peers than the other partition: `{"allowed": true}`
4. Otherwise: `{"allowed": false}`

**Edge case — total partition (cluster sees no SAN nodes):** Returns `{"allowed": false}` for all. Both nodes fenced. Safe.

**Edge case — witness unreachable:** Node treats this as "no witness confirmation" → Fenced (if no quorum by majority alone).

**Timeout:** 3 seconds. Must not block the heartbeat cycle.

### 4. Soft Fence Mechanism

When quorum status transitions to `Fenced`:

| Component | Behavior |
|-----------|----------|
| `acquire_lease()` | Returns `Denied` immediately with reason `"node is fenced"` |
| Existing leases | Run out naturally (max 30s). No active kill. |
| FUSE writes | `EACCES` (lease acquisition fails) |
| FUSE reads | Continue working (local data) |
| API writes | `503 Service Unavailable` with `"node is fenced (no quorum)"` |
| API reads | Continue working |
| Push replication | Paused (peers unreachable anyway) |
| Pull replication | Paused |
| Repair engine | Paused — no repair decisions in fenced state |

When quorum status recovers to `Active` or `Degraded`:

- Leases allowed again
- Replication and repair resume
- Stale replicas caught up automatically by existing pull-replication
- Event logged: `"Node recovered from fenced state"`

**Key property:** Since fenced nodes do not write, there are no diverged files after recovery. No conflict resolution needed.

### 5. Leader Election (Bully Algorithm)

**Purpose:** Coordinate repair decisions and membership changes. Not involved in write path.

**Algorithm:**
- Every node knows the full peer list
- Leader = node with the lowest `node_id` (lexicographic) among all `Active` or `Degraded` nodes
- No election messages needed — deterministic from peer status
- Leader status included in heartbeat: `"is_leader": true/false`
- If leader goes offline, next-lowest node_id automatically becomes leader

**Leader responsibilities:**
- **Repair coordination:** Only the leader runs the repair engine. Other nodes skip repair cycles.
- **Membership changes:** Peer join/leave requests are forwarded to leader for coordination (prevents conflicting membership views).

**Non-leader behavior:**
- Repair engine: skip cycle (log `"not leader, skipping repair"`)
- All other engines: unchanged

**Fenced nodes:** Cannot be leader (not Active/Degraded).

### 6. Ownership Ticks

New fields added to prepare for future quorum-write sync mode. The fencing system makes these unnecessary for now (no diverged writes possible), but the data structure is cheap and enables future sync-mode policy per volume.

**New columns in `file_map`:**

| Column | Type | Default | Description |
|--------|------|---------|-------------|
| `ownership_epoch` | INTEGER | 0 | Incremented when write_owner changes to a different node. Stays same on renewal. |
| `ownership_tick` | INTEGER | 0 | Incremented on every write. Monotonically increasing per file. |

**New columns in `write_log`:**

| Column | Type | Description |
|--------|------|-------------|
| `ownership_epoch` | INTEGER | Epoch at time of write |
| `ownership_tick` | INTEGER | Tick at time of write |

**Behavior:**
- `acquire_lease()`: If `write_owner` changes to a different node → `ownership_epoch += 1`
- `atomic_write()`: On every write → `ownership_tick += 1`. Both values written to `write_log`.

**Future conflict resolution (when quorum-write sync mode is added):**
- Same `ownership_epoch` → no conflict, normal sync
- Different `ownership_epoch` + different `ownership_tick` → higher tick wins (last-writer-wins)
- Same tick → lower `node_id` wins (deterministic tiebreaker)

### 7. Volume Sync-Mode Policy (Data Model Only)

New column in `volumes` table, not enforced yet:

| Column | Type | Default | Description |
|--------|------|---------|-------------|
| `sync_mode` | TEXT | `'async'` | `'async'` = write-local + push (current behavior). `'quorum'` = reserved for future use. |

The write path checks this field but only `async` is implemented. `quorum` returns an error: `"sync_mode 'quorum' not yet implemented"`.

---

## State Changes Summary

### `CoreSanState` (state.rs)

New fields:
```rust
pub quorum_status: RwLock<QuorumStatus>,
pub is_leader: AtomicBool,
```

### Database Migrations (db/mod.rs)

```sql
ALTER TABLE file_map ADD COLUMN ownership_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE file_map ADD COLUMN ownership_tick INTEGER NOT NULL DEFAULT 0;
ALTER TABLE write_log ADD COLUMN ownership_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE write_log ADD COLUMN ownership_tick INTEGER NOT NULL DEFAULT 0;
ALTER TABLE volumes ADD COLUMN sync_mode TEXT NOT NULL DEFAULT 'async';
```

---

## Files Modified

| File | Change |
|------|--------|
| `vmm-san/src/state.rs` | Add `quorum_status: RwLock<QuorumStatus>`, `is_leader: AtomicBool`, `QuorumStatus` enum |
| `vmm-san/src/db/mod.rs` | Add migrations for new columns |
| `vmm-san/src/engine/peer_monitor.rs` | Add quorum calculation after heartbeat cycle, leader determination, witness call |
| `vmm-san/src/engine/write_lease.rs` | Check `quorum_status` in `acquire_lease()`, increment `ownership_epoch`/`ownership_tick` in `atomic_write()` |
| `vmm-san/src/engine/fuse_mount.rs` | No change needed (already calls `acquire_lease()` which will deny when fenced) |
| `vmm-san/src/engine/repair.rs` | Skip cycle if not leader |
| `vmm-san/src/engine/replication.rs` | Skip cycle if fenced |
| `vmm-san/src/engine/push_replicator.rs` | Skip push if fenced |
| `vmm-san/src/api/files.rs` | Return 503 if fenced (before attempting write) |
| `vmm-san/src/api/status.rs` | Include `quorum_status` and `is_leader` in status response |
| `vmm-san/src/peer/client.rs` | Add `witness_check()` method, add `is_leader` to heartbeat |
| `vmm-san/src/main.rs` | Initialize new state fields |
| `vmm-cluster/src/api/san.rs` | Add `witness()` endpoint handler |
| `vmm-cluster/src/api/mod.rs` | Register `/api/san/witness/{node_id}` route |

## Files NOT Modified

| File | Reason |
|------|--------|
| `vmm-san/src/engine/fuse_mount.rs` | Already calls `acquire_lease()` — fencing is enforced there |
| `vmm-san/src/storage/chunk.rs` | Chunk write is below lease layer — no quorum check needed |
| `vmm-san/src/api/peers.rs` | Peer join/leave unchanged for now (leader coordination is Phase 2 optimization) |

---

## Event Logging

All state transitions are logged via the existing event/tracing system:

| Event | Severity | Message |
|-------|----------|---------|
| Quorum → Fenced | `error` | `"Node fenced: no quorum (reachable {}/{}), witness denied"` |
| Fenced → Active | `info` | `"Node recovered from fenced state"` |
| Active → Degraded | `warning` | `"Quorum degraded: {n} of {total} peers unreachable"` |
| Degraded → Active | `info` | `"All peers reachable, quorum fully healthy"` |
| Leader change | `info` | `"Node became leader"` / `"Node is no longer leader"` |
| Witness consulted | `debug` | `"Witness check: allowed={}"` |

---

## Testing Strategy

1. **Unit tests:** Quorum calculation logic with various node counts (1, 2, 3, 5, 10) and reachable counts
2. **Integration:** Simulate partition by making heartbeats fail → verify node transitions to Fenced → verify writes are denied → restore heartbeats → verify recovery
3. **2-node witness:** Start 2 SAN nodes + cluster → partition → verify witness breaks tie → verify one node fenced, one active
4. **Leader election:** Kill leader node → verify next node becomes leader → verify repair only runs on leader
5. **FUSE:** Verify reads continue during fenced state, writes return EACCES
