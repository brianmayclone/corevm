# CoreSAN Architecture

## Overview

CoreSAN is a fully decentralized, peer-to-peer distributed storage system. There is no central metadata server or coordinator. Each node independently maintains its own SQLite database, communicates with peers via HTTP, and makes local decisions about quorum and leadership.

```
┌──────────────────────────────────────────────────────────────────┐
│                        vmm-cluster                               │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │
│  │ Proxy Layer  │  │ Witness API  │  │ Auto Peer Registration │  │
│  │ /api/san/*   │  │ /api/san/    │  │ (heartbeat engine)     │  │
│  │              │  │  witness/    │  │                        │  │
│  └──────┬───────┘  └──────┬───────┘  └────────────┬───────────┘  │
│         │                 │                       │              │
└─────────┼─────────────────┼───────────────────────┼──────────────┘
          │                 │                       │
    ┌─────▼─────────────────▼───────────────────────▼──────┐
    │                   SAN Network                         │
    │                                                       │
    │  ┌──────────┐    ┌──────────┐    ┌──────────┐        │
    │  │  Node 1  │◄──►│  Node 2  │◄──►│  Node 3  │        │
    │  │ :7443    │    │ :7443    │    │ :7443    │        │
    │  │ (Leader) │    │          │    │          │        │
    │  └──┬───────┘    └──┬───────┘    └──┬───────┘        │
    │     │               │               │                │
    │  ┌──▼──┐         ┌──▼──┐         ┌──▼──┐             │
    │  │sda  │         │sda  │         │sda  │  Backends   │
    │  │sdb  │         │sdb  │         │sdb  │  (Disks)    │
    │  │sdc  │         │     │         │sdc  │             │
    │  └─────┘         └─────┘         └─────┘             │
    └───────────────────────────────────────────────────────┘
```

## Components

### 1. API Server (Port 7443)

Axum-based HTTP server providing the management and data API. All client-facing operations (volume CRUD, file read/write, disk management, peer management, benchmarks) are exposed here.

The API server also serves peer-to-peer communication — peers call each other's API endpoints for heartbeats, file push/pull, volume sync, and benchmark probes.

### 2. Background Engines

CoreSAN runs multiple asynchronous background tasks, each on its own interval:

| Engine | Interval | Purpose |
|--------|----------|---------|
| **Peer Monitor** | 5s | Heartbeat all peers, compute quorum, elect leader |
| **Push Replicator** | Event-driven | Distribute writes to peers immediately (chunk-level) |
| **Stale Replicator** | 5s | Pull/push stale replicas to restore consistency |
| **Repair Engine** | 60s | Restore FTT on under-replicated chunks (leader-only) |
| **Integrity Checker** | 3600s | SHA256-verify all local chunk replicas |
| **Backend Refresh** | 30s | Update disk capacity stats (statvfs) |
| **Rebalancer** | 30s | Evacuate chunks from draining/failed backends |
| **Disk Monitor** | 5s | Detect hot-add/hot-remove of disks |
| **Discovery Beacon** | 10s | UDP broadcast for auto-discovery |
| **Benchmark Engine** | 300s | All-to-all network performance testing |
| **DB Mirror** | 60s | Backup SQLite to disk backends |
| **FUSE Mount** | Startup | Mount FUSE filesystems for volumes |
| **Write Log Cleanup** | 300s | Purge write_log entries older than 1 hour |
| **Metadata Sync** | 10s | Leader pushes file_map + file_chunks to all peers; non-leaders sync files they own. Only syncs changes since last watermark. |
| **Sanitize** | Startup only | Verifies all local chunk replicas (SHA256), repairs from peers if corrupt/missing. Cleans orphaned .tmp files. Node stays in Sanitizing state until done. |
| **SMART Monitor** | 300s | Reads S.M.A.R.T. data from all disks via `smartctl -a -j`. Stores in smart_data table. Logs warnings for failing disks. Reports critical events to cluster. |
| **Event Reporter** | Fire-and-forget | HTTP POST to cluster's `/api/events/ingest`. Used by smart_monitor and peer_monitor for proactive error reporting. |

### 3. State Management

All shared state is held in `CoreSanState` (wrapped in `Arc`):

| Field | Type | Purpose |
|-------|------|---------|
| `peers` | `DashMap<String, PeerConnection>` | In-memory peer map (lock-free concurrent) |
| `db` | `Mutex<Connection>` | SQLite database |
| `config` | `CoreSanConfig` | Immutable configuration |
| `node_id` | `String` | This node's unique ID (UUID, persisted in DB) |
| `hostname` | `String` | This node's hostname |
| `write_tx` | `WriteSender` | Channel to push_replicator for write events |
| `quorum_status` | `RwLock<QuorumStatus>` | Current quorum state |
| `is_leader` | `AtomicBool` | Whether this node is the elected leader |

### 4. Database (SQLite with WAL)

Each node maintains its own SQLite database in WAL mode. The database stores:
- Volume definitions (synced across nodes via peer communication)
- Backend registrations (local + cross-registered remote backends)
- File metadata (`file_map`) and replica tracking (`chunk_replicas` is authoritative; `file_replicas` is legacy)
- S.M.A.R.T. disk health data (`smart_data`)
- Peer records, benchmark results, integrity logs, write logs, claimed disks

There is **no distributed database** — each node's DB is independent. Consistency is maintained through:
- Volume sync on creation/deletion (pushed to all peers)
- Push replication on writes (immediate async distribution)
- Stale replica detection and repair (periodic background tasks)

## Data Flow

### Write Path

```
Client PUT /api/volumes/{id}/files/path
  │
  ▼
1. Check quorum (reject if Fenced or Sanitizing)
2. Check volume sync_mode
3. Acquire write lease (30s exclusive lock)
4. Split file data into chunks (chunk_size_bytes, default 64 MB)
5. For each chunk:
   a. place_chunk() selects backend(s) per local RAID policy
   b. write_chunk_data() writes to <backend>/.coresan/<volume_id>/<file_id>/chunk_<index:06>
   c. Compute SHA256 per chunk
   d. Record in file_chunks table
   e. Record in chunk_replicas table (local, state='synced')
6. Update file_map (version++, ownership_epoch/tick)
7. Mark all other nodes' chunk replicas as 'stale'
8. Append to write_log
9. Send WriteEvent (with file_id) to push_replicator channel
  │
  ▼  (async, non-blocking)
Push Replicator:
  - Find peers with backends for this volume
  - Send individual chunks to each peer via PUT /api/chunks/{vol}/{file}/{index}
  - After successful push, record chunk_replicas entry for remote node in LOCAL DB
  - Peer stores chunk and marks replica 'synced'
```

### Read Path

```
Client GET /api/volumes/{id}/files/path
  │
  ▼
1. Check has_local_chunks() + file_exists() (fast path)
   → All chunks local? Reassemble via read_chunk_data() and return.
  │
  ▼  (missing chunks)
2. Check if request is from a peer (X-CoreSAN-Secret header)
   → Peer request? Return 404 (prevent recursion)
  │
  ▼  (client request, missing chunks)
3. fetch_chunks_from_peer() — pull individual missing chunks
   → Query local DB for known remote chunk replicas
   → Pull via GET /api/chunks/{vol}/{file}/{index}
  │
  ▼  (no known replica in local DB)
4. Broadcast to all online peers
   → Ask each peer until chunks are found.
  │
  ▼  (nobody has it)
5. Return 404 Not Found
```

### Chunk-Based Storage Architecture

All file data is stored as fixed-size chunks. The old file-level storage (writing whole files to `<backend>/<rel_path>`) has been replaced entirely. Both the API and FUSE paths use `write_chunk_data()` and `read_chunk_data()`.

**Chunk storage path:** `<backend>/.coresan/<volume_id>/<file_id>/chunk_<index:06>`

Each chunk is placed on backends according to the volume's local RAID policy via `place_chunk()`:

| RAID Mode | Description | Chunks per Write |
|-----------|-------------|-----------------|
| `stripe` | Round-robin across backends | 1 copy |
| `mirror` | Copy to all local backends | N copies |
| `stripe_mirror` | Copy to 2 backends | 2 copies |

The chunk size is configurable per volume via `chunk_size_bytes` (default 64 MB).

Cross-node replication is governed by FTT:
- **FTT 0**: Data exists on 1 node only (no protection)
- **FTT 1**: Data replicated to 2 nodes (survives 1 node failure)
- **FTT 2**: Data replicated to 3 nodes (survives 2 node failures)

## Quorum & Leader Election

### Quorum States

| State | Condition | Read | Write |
|-------|-----------|------|-------|
| **Active** | All peers reachable | Yes | Yes |
| **Degraded** | Majority reachable, or witness grants permission | Yes | Yes |
| **Fenced** | No majority and no witness approval | Yes | **No** |
| **Solo** | No peers configured | Yes | Yes |
| **Sanitizing** | Startup integrity check in progress | Yes | **No** |

### Quorum Calculation

```
majority = (total_nodes / 2) + 1

if reachable >= majority:
    if reachable == total_nodes: Active
    else: Degraded
elif witness_allowed == true:
    Degraded
else:
    Fenced
```

### Witness Tie-Breaking

For 2-node clusters, a network partition means neither node has a majority. The witness (hosted by vmm-cluster) resolves this:

1. Partitioned node calls `GET /api/san/witness/{node_id}` on vmm-cluster
2. Cluster checks which SAN nodes are still reachable
3. If one is reachable and the other is not: grant quorum to reachable node
4. If both reachable: tie-break by lowest host_id
5. If neither reachable: deny both

### Leader Election

The leader is the node with the **lowest node_id** among all online peers (including itself), provided the node is not fenced. The leader is responsible for:
- Running repair operations (restore FTT on under-replicated chunks)
- Protection status updates

Leader election is deterministic and requires no voting — every node independently reaches the same conclusion based on the same peer status information.

## Security

### Peer Authentication

All peer-to-peer communication is authenticated via a shared secret sent in the `X-CoreSAN-Secret` HTTP header. If no secret is configured, authentication is disabled (single-node mode).

The secret can be:
- Explicitly configured in `vmm-san.toml` under `[peer] secret`
- Auto-generated (UUID) on first startup if left empty

### Recursion Prevention

When a node fetches a file from a peer (transparent peer-fetch), the request includes the `X-CoreSAN-Secret` header. The receiving node detects this and only performs a local lookup — it never broadcasts to other peers. This prevents infinite fetch loops.

### Cluster Integration Auth

When accessed through vmm-cluster, the proxy layer handles bearer token authentication. The cluster-to-SAN communication uses the SAN client without additional auth (trusted internal network).

## Service Layer

CoreSAN uses a structured service layer for all database and business logic operations.

### Database Helpers (`db/helpers.rs`)

- `DbResult<T>` / `DbError` — unified error types for all DB operations
- `DbContext` trait (`.ctx()`) — adds context to error chains
- `db_transaction()` — safe transaction wrapper with automatic rollback on error
- `db_exec()` — execute with error context
- `log_err!` macro — replaces all `.ok()` calls on DB operations. Errors are logged, never silently swallowed.

### Service Modules

| Service | Responsibility |
|---------|---------------|
| **ChunkService** | 20+ methods for all `chunk_replicas` / `file_chunks` operations. Key methods: `receive_chunk()`, `track_remote_replica()`, `move_replica()`, `find_under_replicated()`, `find_stale_replicas()` |
| **FileService** | `sync_metadata()`, `delete()` (transactional with cascade), `acquire_lease()` |
| **PeerService** | Peer CRUD, heartbeat handling, lease release on peer failure |
| **VolumeService** | Volume CRUD, sync, status management |
| **DiskService** | Disk discovery, claim, release, reset, SMART data access |
| **BackendService** | Backend CRUD, capacity refresh, drain management |
| **BenchmarkService** | Benchmark execution, result storage, matrix computation |

All services use the `log_err!` macro to ensure no database errors are silently swallowed.
