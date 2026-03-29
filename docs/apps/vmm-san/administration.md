# CoreSAN Administration Guide

## Physical Disk Management

### Discovering Disks

```bash
curl -k https://node:7443/api/disks
```

Each disk is classified into one of these statuses:

| Status | Meaning | Can Claim? |
|--------|---------|-----------|
| `available` | Empty disk, no filesystem | Yes (safe) |
| `has_data` | Has filesystem/data | Yes (with `confirm_format`) |
| `os_disk` | System/boot disk (/, /boot, /boot/efi, swap) | **No** |
| `in_use` | Mounted by another process | **No** |
| `claimed` | Already claimed by CoreSAN | Already done |

### Claiming a Disk

```bash
# Claim an empty disk
curl -k -X POST https://node:7443/api/disks/claim \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdb"}'

# Claim a disk with existing data (DESTROYS ALL DATA)
curl -k -X POST https://node:7443/api/disks/claim \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdc", "confirm_format": true}'
```

**What happens when you claim a disk:**

1. The disk is partitioned (8 GiB primary partition)
2. Partition is formatted with ext4
3. Mounted at `/vmm/san-disks/<uuid>`
4. Registered as a backend in the database
5. Backend is marked `online` and available for volume storage

### Releasing a Disk

```bash
curl -k -X POST https://node:7443/api/disks/release \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdb"}'
```

Release marks the backend as `draining`. The rebalancer engine will:
1. Copy all chunks from this backend to other healthy backends
2. Once empty, unmount the disk
3. Mark the claimed disk as `released`

**Note:** Release is a graceful operation. Data is moved before the disk is freed. This can take time depending on data volume and network speed.

### Resetting a Disk

```bash
curl -k -X POST https://node:7443/api/disks/reset \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdb"}'
```

Reset is for disks in `error` state (e.g., after hot-remove/hot-add). It wipes the disk and re-claims it.

### Hot-Add / Hot-Remove

CoreSAN's disk monitor engine (5-second polling) detects:

- **Hot-remove**: If a claimed disk disappears from `/sys/block/`, the backend is immediately marked `offline`, all chunk replicas on it are marked `error`, and the repair engine begins restoring copies from other nodes.
- **Hot-add**: When a new disk appears, it shows up in `GET /api/disks` as `available`. It must be manually claimed.

## Volume Management

### Creating a Volume

```bash
curl -k -X POST https://node:7443/api/volumes \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "vm-storage",
    "ftt": 1,
    "local_raid": "stripe"
  }'
```

**Parameters:**

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | String | required | Unique volume name |
| `ftt` | u32 | `1` | Failures to tolerate (0, 1, 2) |
| `local_raid` | String | `stripe` | Local RAID policy |
| `chunk_size_bytes` | u64 | `67108864` | Chunk size (64 MB default) |

**Local RAID policies:**

| Policy | Description | Use Case |
|--------|-------------|----------|
| `stripe` | Round-robin chunks across local disks (RAID-0) | Maximum capacity, best performance |
| `mirror` | Copy to all local disks (RAID-1) | Maximum local redundancy |
| `stripe_mirror` | Stripe across pairs (RAID-10) | Balance of performance and redundancy |

**Volume creation process:**
1. Volume record created in local DB
2. Synced to all peers via `POST /api/volumes/sync`
3. Status set to `online`
4. Available backends on each node are auto-assigned

### FTT and Capacity Planning

| FTT | Required Nodes | Raw → Effective | Example: 3 nodes × 1 TB |
|-----|---------------|-----------------|--------------------------|
| 0 | 1+ | 100% | 3 TB effective |
| 1 | 2+ | 50% | 1.5 TB effective |
| 2 | 3+ | 33% | 1 TB effective |

FTT determines how many **complete node failures** a volume can survive without data loss. FTT=1 means every chunk exists on at least 2 different nodes.

### Capacity Calculation

Only backends with `claimed_disk_id != ''` count toward capacity. The `/vmm/san-data/` backend on the root filesystem is excluded from capacity calculations.

**Local RAID impact on usable capacity:**

| RAID Policy | Usable Capacity |
|-------------|----------------|
| `mirror` | Smallest claimed disk (all disks mirror each other) |
| `stripe` | Sum of all claimed disks |
| `stripe_mirror` | Sum of all claimed disks / 2 |

### Updating a Volume

```bash
# Change FTT (triggers re-replication)
curl -k -X PUT https://node:7443/api/volumes/<volume-id> \
  -H 'Content-Type: application/json' \
  -d '{"ftt": 2}'

# Change local RAID policy
curl -k -X PUT https://node:7443/api/volumes/<volume-id> \
  -H 'Content-Type: application/json' \
  -d '{"local_raid": "mirror"}'
```

Changing FTT upward triggers the repair engine to create additional replicas. Changing FTT downward does not immediately remove replicas.

### Deleting a Volume

```bash
curl -k -X DELETE https://node:7443/api/volumes/<volume-id>
```

The volume must be empty (no files). Deletion is synced to all peers.

### Volume Status

| Status | Meaning |
|--------|---------|
| `creating` | Volume is being initialized |
| `online` | Fully operational |
| `degraded` | Some backends or replicas unhealthy |
| `offline` | No backends available |

## Backend Management

Backends are the storage mountpoints within a volume. When a disk is claimed, a backend is created automatically.

### Listing Backends

```bash
curl -k https://node:7443/api/volumes/<volume-id>/backends
```

### Adding a Backend Manually

```bash
curl -k -X POST https://node:7443/api/volumes/<volume-id>/backends \
  -H 'Content-Type: application/json' \
  -d '{"path": "/vmm/san-disks/<uuid>"}'
```

### Removing a Backend

```bash
curl -k -X DELETE https://node:7443/api/volumes/<volume-id>/backends/<backend-id>
```

Removing a backend initiates draining — all chunks are moved to other backends before removal completes.

### Backend Status

| Status | Meaning |
|--------|---------|
| `online` | Healthy, serving reads/writes |
| `degraded` | Functional but with warnings (disk errors) |
| `offline` | Not accessible (disk removed, mount failed) |
| `draining` | Being evacuated prior to removal |

## Peer Management

### Listing Peers

```bash
curl -k https://node:7443/api/peers
```

Returns all known peers with their status, last heartbeat time, and hostname.

### Manually Adding a Peer

```bash
curl -k -X POST https://node:7443/api/peers/join \
  -H 'Content-Type: application/json' \
  -H 'X-CoreSAN-Secret: <secret>' \
  -d '{
    "address": "https://10.0.0.2:7443",
    "node_id": "<peer-node-id>",
    "hostname": "node2",
    "peer_port": 7444,
    "secret": "<secret>"
  }'
```

**Remember:** Peer registration is bidirectional. Both nodes must know about each other. Use vmm-cluster auto-registration to avoid manual work.

### Removing a Peer

```bash
curl -k -X DELETE https://node:7443/api/peers/<peer-node-id>
```

### Peer Status

| Status | Meaning |
|--------|---------|
| `connecting` | Initial state, not yet heartbeated |
| `online` | Reachable, heartbeats succeeding |
| `offline` | 3+ missed heartbeats (15+ seconds) |

When a peer transitions to `offline`:
- All backends for that peer's node_id are marked `offline`
- All write leases held by that node are released
- The repair engine begins restoring under-replicated chunks

## File Operations

### Writing a File

```bash
curl -k -X PUT https://node:7443/api/volumes/<vol-id>/files/path/to/file.txt \
  -d 'File content here'
```

The write endpoint:
1. Checks quorum (rejects with 503 if fenced)
2. Selects the local backend with the most free space
3. Acquires a write lease (30s exclusive lock)
4. Performs atomic write (temp file → fsync → rename)
5. Pushes to peers asynchronously

### Reading a File

```bash
curl -k https://node:7443/api/volumes/<vol-id>/files/path/to/file.txt
```

Reads are transparent — if the file is not on the local node, CoreSAN automatically fetches it from a peer that has it. The client does not need to know which node holds the data.

### Listing Files

```bash
curl -k https://node:7443/api/volumes/<vol-id>/files
```

Returns all files with metadata (path, size, SHA256, timestamps, replica/sync counts).

### Browsing Directories

```bash
# Browse root
curl -k https://node:7443/api/volumes/<vol-id>/browse

# Browse subdirectory
curl -k https://node:7443/api/volumes/<vol-id>/browse/path/to/dir
```

Returns directory entries (files and subdirectories) with names, sizes, and types.

### Creating a Directory

```bash
curl -k -X POST https://node:7443/api/volumes/<vol-id>/mkdir \
  -H 'Content-Type: application/json' \
  -d '{"path": "my/new/directory"}'
```

### Deleting a File

```bash
curl -k -X DELETE https://node:7443/api/volumes/<vol-id>/files/path/to/file.txt
```

Deletion removes:
1. Chunk files on all local backends
2. `chunk_replicas` entries
3. `file_chunks` entries
4. `integrity_log` entries
5. `write_log` entries
6. `file_map` entry
7. Propagates deletion to all online peers via `DELETE /api/volumes/{id}/files/{path}`

## Quorum Management

### Checking Quorum Status

```bash
curl -k https://node:7443/api/status | jq '.quorum_status'
# "active", "degraded", "fenced", or "solo"
```

### Understanding Quorum Transitions

```
Sanitizing → Solo ←→ Active ←→ Degraded → Fenced
                                            ↑
                              (witness denied or unreachable)
```

- **Sanitizing → (next state)**: On startup, node enters Sanitizing while verifying local chunk integrity. Writes are rejected. Once complete, transitions to the appropriate quorum state.

- **Solo → Active**: First peer joins and comes online
- **Active → Degraded**: One peer goes offline but majority maintained
- **Degraded → Fenced**: More peers fail, no majority, witness denies/unreachable
- **Fenced → Degraded/Active**: Peers recover, majority restored

**Hysteresis:** Fencing requires 2 consecutive failed quorum cycles (10 seconds total) before taking effect. Recovery is immediate.

### Fencing Behavior

When a node is fenced:
- **Writes are rejected** with HTTP 503 (Service Unavailable)
- **Reads still work** for locally stored data
- **Leader election stops** — no node claims leadership
- **Repair operations pause** — no chunk redistribution
- **Push replication pauses** — no outbound writes

## Leadership

### Checking Leader Status

```bash
curl -k https://node:7443/api/status | jq '.is_leader'
# true or false
```

### Leader Responsibilities

Only the leader runs:
- **Repair engine**: Restores FTT on under-replicated chunks by pulling data from healthy nodes and distributing copies
- **Protection status updates**: Recomputes `protection_status` for all files

Leader election is automatic and deterministic — the node with the lowest node_id among online, non-fenced nodes becomes leader.

## Network Benchmarking

### Running a Benchmark

```bash
# Trigger manual benchmark
curl -k -X POST https://node:7443/api/benchmark/run

# View results matrix
curl -k https://node:7443/api/benchmark/matrix
```

### Interpreting Results

| Metric | Good | Acceptable | Poor |
|--------|------|-----------|------|
| Bandwidth | > 1000 MB/s | > 100 MB/s | < 100 MB/s |
| Latency | < 500 μs | < 2000 μs | > 5000 μs |
| Jitter | < 100 μs | < 500 μs | > 1000 μs |
| Packet Loss | 0% | < 1% | > 1% |

Poor network performance directly impacts replication speed and cross-node read latency. Consider:
- Dedicated SAN network (separate from management traffic)
- Jumbo frames (MTU 9000)
- 10 Gbps+ links
- Switch QoS for SAN traffic

## FUSE Filesystem

Volumes are accessible as local filesystem mounts via FUSE:

```
/vmm/san/<volume-name>/
```

This allows VMs and applications to access CoreSAN volumes as regular directories without using the HTTP API.

### FUSE Implementation Details

- **File resolution**: Uses `has_local_chunks()` + `file_exists()` instead of flat-file scanning. No more `resolve_file()`.
- **readdir**: Reads from `file_map` DB only. No disk scanning, which prevents internal `.coresan/` directories from appearing in listings.
- **setattr**: Uses UPSERT for truncate/extend operations, fixing a race condition with `File::set_len`.
- **read**: Automatically fetches chunks from peers if not available locally via `fetch_chunks_from_peer`.
- **unlink**: Deletes chunk files and propagates deletion to all online peers.
- **mkdir**: Creates virtual directories (file_map marker entry), not physical directories on disk.

## Maintenance Operations

### Database Backup

CoreSAN automatically mirrors its SQLite database to claimed disk backends every 60 seconds (`db_mirror` engine). On startup, if the primary database is missing, it restores from the latest disk backup.

### Manual Backup

The database is a single SQLite file:

```bash
cp /var/lib/vmm-san/coresan.db /backup/coresan-$(date +%Y%m%d).db
```

### Checking Data Integrity

Integrity verification runs automatically based on `[integrity] interval_secs`. To view results:

```bash
# Check for recent integrity failures
curl -k https://node:7443/api/status | jq '.integrity'
```

Failed integrity checks are logged at `warn` level and the affected chunk replicas are marked `error` for re-repair.
