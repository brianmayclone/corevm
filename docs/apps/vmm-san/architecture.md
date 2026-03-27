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
| **Push Replicator** | Event-driven | Distribute writes to peers immediately |
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
- File metadata (`file_map`) and replica tracking (`file_replicas`, `chunk_replicas`)
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
1. Check quorum (reject if Fenced)
2. Check volume sync_mode
3. Select best local backend (most free space)
4. Acquire write lease (30s exclusive lock)
5. Write to temp file → fsync → atomic rename
6. Compute SHA256
7. Update file_map (version++, ownership_epoch/tick)
8. Mark local replica as 'synced'
9. Mark all other nodes' replicas as 'stale'
10. Append to write_log
11. Send WriteEvent to push_replicator channel
  │
  ▼  (async, non-blocking)
Push Replicator:
  - Find peers with backends for this volume
  - PUT file data to each peer concurrently
  - Peer stores file and marks replica 'synced'
```

### Read Path

```
Client GET /api/volumes/{id}/files/path
  │
  ▼
1. Try local replica (fast path)
   → Found? Return file data immediately.
  │
  ▼  (no local replica)
2. Check if request is from a peer (X-CoreSAN-Secret header)
   → Peer request? Return 404 (prevent recursion)
  │
  ▼  (client request, no local data)
3. Query local DB for known remote replica
   → Found? Pull from that specific peer.
  │
  ▼  (no known replica in local DB)
4. Broadcast to all online peers
   → Ask each peer until one returns the file.
  │
  ▼  (nobody has it)
5. Return 404 Not Found
```

### Chunk-Level Storage

Files are divided into fixed-size chunks (default 64 MB). Each chunk is placed on backends according to the volume's local RAID policy:

| RAID Mode | Description | Chunks per Write |
|-----------|-------------|-----------------|
| `stripe` | Round-robin across backends | 1 copy |
| `mirror` | Copy to all local backends | N copies |
| `stripe_mirror` | Copy to 2 backends | 2 copies |

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
