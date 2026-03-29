# CoreSAN Replication & Fault Tolerance

## Overview

CoreSAN uses a multi-layered replication strategy:

1. **Local RAID** — how chunks are distributed across disks within a single node
2. **Cross-Node Replication (FTT)** — how data is copied between nodes
3. **Write Leases** — how concurrent writes are serialized
4. **Push Replication** — how writes are distributed immediately
5. **Repair** — how the system self-heals after failures

## Local RAID Policies

Within a single node, chunks are placed across local backends (disks) according to the volume's `local_raid` policy:

### Stripe (RAID-0)

```
Backend 0:  [Chunk 0] [Chunk 3] [Chunk 6]
Backend 1:  [Chunk 1] [Chunk 4] [Chunk 7]
Backend 2:  [Chunk 2] [Chunk 5] [Chunk 8]
```

- Chunks assigned by `chunk_index % num_backends`
- **Capacity:** 100% of raw (all disks contribute)
- **Performance:** Reads/writes distributed across all disks
- **Risk:** Loss of any disk loses its chunks (cross-node FTT protects against this)

### Mirror (RAID-1)

```
Backend 0:  [Chunk 0] [Chunk 1] [Chunk 2] [Chunk 3]
Backend 1:  [Chunk 0] [Chunk 1] [Chunk 2] [Chunk 3]
Backend 2:  [Chunk 0] [Chunk 1] [Chunk 2] [Chunk 3]
```

- Every chunk copied to all local backends
- **Capacity:** 1/N of raw (N = number of disks)
- **Performance:** Reads can use any disk, writes go to all
- **Risk:** Can lose all disks except one without local data loss

### Stripe-Mirror (RAID-10)

```
Backend 0:  [Chunk 0] [Chunk 2] [Chunk 4]
Backend 1:  [Chunk 0] [Chunk 2] [Chunk 4]
Backend 2:  [Chunk 1] [Chunk 3] [Chunk 5]
Backend 3:  [Chunk 1] [Chunk 3] [Chunk 5]
```

- Chunks striped across pairs of backends, mirrored within each pair
- **Capacity:** 50% of raw
- **Performance:** Balanced — stripe for throughput, mirror for redundancy
- **Risk:** Can lose one disk from each mirror pair

## Cross-Node Replication (FTT)

FTT (Failures To Tolerate) determines how many complete node failures a volume can survive:

| FTT | Node Copies | Survives | Effective Capacity |
|-----|-------------|----------|-------------------|
| 0 | 1 | 0 failures | 100% |
| 1 | 2 | 1 failure | 50% |
| 2 | 3 | 2 failures | 33% |

### Protection Status

Each file's protection status is computed based on its chunk distribution:

- **`protected`** — Every chunk has at least FTT+1 copies on distinct nodes
- **`degraded`** — Some chunks have fewer copies than required
- **`unprotected`** — Initial state before replication completes

The protection status is recomputed by the repair engine (leader-only).

## Write Leases

Write leases prevent concurrent modifications to the same file by different nodes.

### Lease Properties

| Property | Value |
|----------|-------|
| Duration | 30 seconds |
| Scope | Per file (volume_id + rel_path) |
| Conflict resolution | First-come-first-served + lease expiration |

### Lease Lifecycle

```
State: No Lease
  │
  ▼  Node A acquires lease
State: Owner=A, Until=now+30s
  │
  ├─► Node A writes → lease renewed (until=now+30s)
  │
  ├─► Node B tries to write → DENIED (409 Conflict)
  │     └─► Waits for expiration, then retries
  │
  ├─► Node A releases lease → State: No Lease
  │
  └─► Lease expires (30s timeout) → State: No Lease
        └─► Any node can acquire
```

### Ownership Tracking

Each file tracks:
- `write_owner` — node_id of the current lease holder
- `write_lease_until` — expiration timestamp
- `ownership_epoch` — incremented when ownership changes (detects stale writes)
- `ownership_tick` — incremented within same epoch (write count)

### Lease Stealing

If a node goes offline while holding a lease:
1. After 30 seconds, the lease expires naturally
2. Alternatively, when peer_monitor detects the node offline (3 missed heartbeats), it calls `release_all_leases_for_node()` to clear all leases immediately

### Fencing and Leases

Fenced nodes cannot acquire or renew leases. The `acquire_lease` function checks quorum status and returns `Denied` if the node is fenced.

## Push Replication

When a file is written, push replication distributes individual chunks to peers immediately.

### Write Event Pipeline

```
Write Handler
  │ Creates WriteEvent {
  │   volume_id, file_id,
  │   writer_node_id
  │ }
  ▼
mpsc channel (unbounded)
  │
  ▼
Push Replicator (background task)
  │ 1. Skip if node is fenced or sanitizing
  │ 2. Query backends table for peer node_ids
  │ 3. For each peer with backends for this volume:
  │    └─ For each chunk of the file:
  │       → PUT /api/chunks/{volume_id}/{file_id}/{chunk_index}
  │       → On success: record chunk_replicas entry for remote node in LOCAL DB
  │         (with empty backend_id — remote backend is unknown to sender)
  ▼
Peer Node receives chunk
  │ 1. Stores chunk to local backend via write_chunk_data()
  │ 2. Updates chunk_replicas
  │ 3. Marks local chunk replica as 'synced'
```

### Metadata Sync

The `metadata_sync` engine (10-second interval) ensures all nodes know about all files:

- **Leader**: Pushes `file_map` + `file_chunks` records to all peers via `POST /api/file-meta/sync`
- **Non-leaders**: Sync metadata for files they own
- **Watermark**: Only syncs changes since last successful sync, minimizing traffic
- **Solves**: "Host B can't see files written by Host A" — metadata is propagated regardless of chunk replication status

### Cross-Registration

For push replication to work, each node must know about backends on other nodes. This is achieved through:
- **Auto-registration**: When a volume is created, backends on each node are registered
- **Cross-registration**: The testbed's `cross_register_backends()` inserts peer backends into each node's DB
- **Heartbeat sync**: Peers exchange backend information during heartbeats

## Stale Replica Syncing

The replication engine (5-second interval) handles replicas marked as `stale`:

### How Replicas Become Stale

When Node A writes a file:
1. Node A marks its own replica as `synced` with the new version
2. Node A marks all other nodes' replicas (in its local DB) as `stale`
3. Push replicator sends the new version to peers
4. Peers update their replicas to `synced`

If push replication fails (peer temporarily unreachable), the stale replica persists until the background replicator catches up.

### Stale Replica Resolution

```
Every 5 seconds:
  1. Query chunk_replicas WHERE state = 'stale'
  2. For each stale chunk replica:
     a. If replica is LOCAL:
        → Pull latest chunk from a peer with 'synced' copy
          via GET /api/chunks/{vol}/{file}/{index}
        → Write to local backend
        → Mark as 'synced'
     b. If replica is REMOTE:
        → Push local 'synced' chunk to the peer
          via PUT /api/chunks/{vol}/{file}/{index}
        → Peer marks as 'synced'
```

Stale-replica sync now works on the `chunk_replicas` table (not `file_replicas`), pulling and pushing individual chunks via the `/api/chunks/` endpoints.

## Repair Engine

The repair engine (leader-only, 60-second default interval) handles under-replicated data:

### What Triggers Repair

- Node goes offline → its chunk replicas become unavailable
- Disk fails → chunk replicas on that backend marked `error`
- FTT increased → more copies needed
- New node joins → redistribution opportunity

### Repair Process

```
Leader runs every 60 seconds:
  1. Query under-replicated files:
     WHERE synced_count < (FTT + 1)
     ORDER BY deficit DESC, size ASC (prioritize most under-replicated, smallest first)

  2. For each under-replicated chunk (rate-limited to 30 chunks/cycle):
     a. Find source node with 'synced' copy
     b. Verify source SHA256 before pushing
     c. Try local target: pull from peer, store locally
     d. Else find remote peer without chunk: push from local
     e. Verify received SHA256 after pulling
     f. Update chunk_replicas, mark new copy as 'synced'

  3. Update protection_status for all files:
     → 'protected' if all chunks have FTT+1 node-distinct copies
     → 'degraded' otherwise
```

### Repair Priority

Files are repaired in order of:
1. **Highest deficit first** — files missing the most copies
2. **Smallest files first** — faster to repair, more files protected sooner

## Replica State Machine

```
                    ┌─────────┐
                    │ syncing │ (initial state)
                    └────┬────┘
                         │ data received + verified
                         ▼
                    ┌─────────┐
              ┌─────│ synced  │◄──── repair/resync
              │     └────┬────┘
              │          │ newer version written elsewhere
              │          ▼
              │     ┌─────────┐
              │     │  stale  │
              │     └────┬────┘
              │          │ pull/push new version
              │          ▼
              │     ┌─────────┐
              └─────│ synced  │
                    └────┬────┘
                         │ checksum mismatch / disk failure
                         ▼
                    ┌─────────┐
                    │  error  │
                    └────┬────┘
                         │ repair engine restores copy
                         ▼
                    ┌─────────┐
                    │ synced  │
                    └─────────┘
```

## Integrity Verification

The integrity engine (hourly by default) verifies data hasn't been corrupted:

1. For each local chunk replica in `synced` state:
   - Read file from disk
   - Compute SHA256
   - Compare against stored hash in `file_chunks` table
2. On mismatch:
   - Mark replica as `error`
   - Log to `integrity_log` table
   - Repair engine will restore from healthy copy on another node
3. On success:
   - Log to `integrity_log` with `passed=1`

## Sanitize Engine (Startup)

The sanitize engine runs once at startup before the node accepts writes:

1. Node enters `QuorumStatus::Sanitizing` state (writes rejected)
2. Iterates all local `chunk_replicas`
3. For each chunk: reads from disk, computes SHA256, compares with stored hash
4. **Corrupt/missing chunks**: Attempts repair from peers via `/api/chunks/` endpoints
5. **Orphaned .tmp files**: Cleaned up automatically
6. Once complete, node transitions to normal quorum state and begins accepting writes

The sanitize engine ensures data integrity after unclean shutdowns, power failures, or disk errors.

## Rebalancer

The rebalancer (30-second interval) handles data movement for backend changes:

### Triggers

- Backend marked as `draining` (being removed)
- Backend marked as `offline` (disk failure)
- Backend marked as `degraded`

### Process

```
Every 30 seconds:
  Phase 1 — Repair missing local mirror copies:
    1. Find chunks that should have local mirror copies but don't
    2. Copy chunk to missing mirror backend
    3. Verify SHA256 after copy
    4. All operations use transactions for atomicity

  Phase 2 — Evacuate draining/failed backends:
    1. Find chunk_replicas on draining/offline/degraded backends
    2. For each chunk:
       a. Copy to a healthy target backend on the same node
       b. Verify SHA256 after copy
       c. Update chunk_replicas (new backend_id) within transaction
       d. Delete old replica entry
       e. Remove old chunk file from disk
    3. When draining backend has 0 remaining chunks:
       → Mark backend as 'offline' (fully drained)
```

## Transparent Peer-Fetch

When a client reads a file that doesn't exist locally, CoreSAN transparently fetches it from a peer:

### Fetch Strategy

1. **Local replica** — fastest, check local backends first
2. **Known remote** — query local DB for a known replica on another node
3. **Broadcast** — ask all online peers if local DB has no knowledge

### Recursion Prevention

Peer-to-peer fetch requests include the `X-CoreSAN-Secret` header. When a node receives a read request with this header, it only checks locally and returns 404 if not found — it never forwards to other peers. This prevents infinite fetch loops.

```
Client → Node A (no local copy)
  → Node A asks Node B (with X-CoreSAN-Secret)
    → Node B checks locally only
    → Returns data (or 404)
```

## Write Ordering and Consistency

### Within a Single File

- Write leases ensure only one node writes at a time
- `version` is incremented atomically on each write
- `ownership_epoch` detects ownership changes
- `write_log` maintains ordered history

### Across Files

- No cross-file transactions
- Each file is independently versioned and leased
- Writes to different files can proceed concurrently on different nodes

### Eventual Consistency

With `sync_mode: async`, CoreSAN is eventually consistent:
- Writes return immediately after local persistence
- Peer copies arrive milliseconds to seconds later (depending on network)
- A read immediately after a write on a different node may see stale data
- After replication completes, all nodes see the same version

### Read-After-Write Consistency

- **Same node**: Guaranteed (write goes to local disk first)
- **Different node**: Not guaranteed with async replication. Use `sync_mode: quorum` (when implemented) for strong consistency.
