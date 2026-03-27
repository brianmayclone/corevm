# CoreSAN API Reference

Base URL: `https://<node>:7443`

All responses are JSON unless otherwise noted. File read responses return raw bytes.

Peer-to-peer requests include the `X-CoreSAN-Secret` header for authentication.

---

## Status & Health

### GET /api/health

Minimal health check.

**Response:** `200 OK`
```json
{"status": "ok"}
```

### GET /api/status

Full node status.

**Response:** `200 OK`
```json
{
  "node_id": "a1b2c3d4-...",
  "hostname": "san-node-1",
  "uptime_secs": 3600,
  "quorum_status": "active",
  "is_leader": true,
  "peer_count": 2,
  "volumes": [
    {
      "volume_id": "vol-1",
      "volume_name": "production",
      "ftt": 1,
      "local_raid": "stripe",
      "chunk_size_bytes": 67108864,
      "total_bytes": 1073741824,
      "free_bytes": 536870912,
      "status": "online",
      "backend_count": 3,
      "total_chunks": 150,
      "synced_chunks": 150,
      "stale_chunks": 0,
      "protected_files": 42,
      "degraded_files": 0
    }
  ],
  "benchmark_summary": {
    "avg_bandwidth_mbps": 1200.5,
    "avg_latency_us": 350.2,
    "worst_peer": "node-3",
    "measured_at": "2025-01-15T10:30:00Z"
  }
}
```

### GET /api/dashboard

Aggregated dashboard data.

**Response:** `200 OK`
```json
{
  "total_capacity_bytes": 3221225472,
  "used_bytes": 1073741824,
  "volume_count": 2,
  "backend_count": 6,
  "peer_count": 2,
  "online_peers": 2,
  "quorum_status": "active",
  "is_leader": true
}
```

### GET /api/network/config

Current SAN network configuration.

**Response:** `200 OK`
```json
{
  "san_interface": "eth1",
  "san_ip": "10.10.10.1",
  "san_netmask": "255.255.255.0",
  "san_gateway": "",
  "san_mtu": 9000
}
```

### GET /api/network/interfaces

List all network interfaces.

**Response:** `200 OK`
```json
[
  {
    "name": "eth0",
    "ip": "192.168.1.10",
    "mac": "00:11:22:33:44:55",
    "mtu": 1500,
    "state": "up"
  }
]
```

---

## Physical Disks

### GET /api/disks

List all discovered block devices.

**Response:** `200 OK`
```json
[
  {
    "name": "sda",
    "path": "/dev/sda",
    "size_bytes": 256060514304,
    "model": "Samsung SSD 970",
    "serial": "S123456789",
    "status": "os_disk",
    "fs_type": "ext4"
  },
  {
    "name": "sdb",
    "path": "/dev/sdb",
    "size_bytes": 1000204886016,
    "model": "WDC WD10EZEX",
    "serial": "WD-12345",
    "status": "available",
    "fs_type": null
  },
  {
    "name": "sdc",
    "path": "/dev/sdc",
    "size_bytes": 500107862016,
    "model": "Seagate ST500",
    "serial": "SEA-67890",
    "status": "claimed",
    "fs_type": "ext4"
  }
]
```

**Status values:** `available`, `has_data`, `os_disk`, `in_use`, `claimed`

### POST /api/disks/claim

Claim and format a disk for CoreSAN.

**Request:**
```json
{
  "device_path": "/dev/sdb",
  "confirm_format": false
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `device_path` | String | Yes | Block device path |
| `confirm_format` | bool | No | Required if disk has existing data |

**Response:** `200 OK`
```json
{
  "disk_id": "disk-abc123",
  "device_path": "/dev/sdb",
  "mount_path": "/vmm/san-disks/abc123",
  "backend_id": "backend-xyz"
}
```

**Errors:**
- `400` — Disk is OS disk or already claimed
- `400` — Disk has data and `confirm_format` is false
- `500` — Partition/format/mount failure

### POST /api/disks/release

Release a claimed disk (graceful, drains data first).

**Request:**
```json
{
  "device_path": "/dev/sdb"
}
```

**Response:** `200 OK`
```json
{"success": true}
```

### POST /api/disks/reset

Wipe and re-claim a disk in error state.

**Request:**
```json
{
  "device_path": "/dev/sdb"
}
```

**Response:** `200 OK`
```json
{"success": true}
```

---

## Volumes

### GET /api/volumes

List all volumes.

**Response:** `200 OK`
```json
[
  {
    "id": "vol-abc123",
    "name": "production",
    "ftt": 1,
    "local_raid": "stripe",
    "chunk_size_bytes": 67108864,
    "status": "online",
    "total_bytes": 2147483648,
    "free_bytes": 1073741824,
    "backend_count": 4,
    "created_at": "2025-01-15T10:00:00Z"
  }
]
```

### POST /api/volumes

Create a new volume.

**Request:**
```json
{
  "name": "my-volume",
  "ftt": 1,
  "local_raid": "stripe",
  "chunk_size_bytes": 67108864
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | String | required | Unique volume name |
| `ftt` | u32 | `1` | Failures to tolerate (0, 1, 2) |
| `local_raid` | String | `stripe` | `stripe`, `mirror`, or `stripe_mirror` |
| `chunk_size_bytes` | u64 | `67108864` | Chunk size in bytes |

**Response:** `201 Created`
```json
{
  "id": "vol-abc123",
  "name": "my-volume",
  "ftt": 1,
  "local_raid": "stripe",
  "chunk_size_bytes": 67108864,
  "status": "online",
  "total_bytes": 0,
  "free_bytes": 0,
  "backend_count": 0,
  "created_at": "2025-01-15T10:00:00Z"
}
```

### GET /api/volumes/{id}

Get a single volume.

**Response:** `200 OK` — same format as list entry.

### PUT /api/volumes/{id}

Update volume resilience policy.

**Request:**
```json
{
  "ftt": 2,
  "local_raid": "mirror"
}
```

Both fields are optional — only provided fields are updated.

**Response:** `200 OK` — updated volume.

### DELETE /api/volumes/{id}

Delete a volume. The volume must have no files.

**Response:** `200 OK`
```json
{"success": true}
```

**Errors:**
- `409` — Volume is not empty

### POST /api/volumes/sync

Receive a volume definition from a peer (internal, peer-to-peer).

**Request:**
```json
{
  "id": "vol-abc123",
  "name": "production",
  "ftt": 1,
  "local_raid": "stripe",
  "chunk_size_bytes": 67108864
}
```

---

## Backends

### GET /api/volumes/{id}/backends

List backends for a volume.

**Response:** `200 OK`
```json
[
  {
    "id": "backend-xyz",
    "node_id": "node-abc",
    "path": "/vmm/san-disks/abc123",
    "total_bytes": 1000000000,
    "free_bytes": 500000000,
    "status": "online",
    "last_check": "2025-01-15T10:30:00Z",
    "claimed_disk_id": "disk-abc123"
  }
]
```

### POST /api/volumes/{id}/backends

Add a backend to a volume.

**Request:**
```json
{
  "path": "/vmm/san-disks/abc123"
}
```

**Response:** `201 Created` — backend object.

### DELETE /api/volumes/{vid}/backends/{bid}

Remove a backend (initiates draining).

**Response:** `200 OK`
```json
{"success": true}
```

---

## Peers

### GET /api/peers

List all known peers.

**Response:** `200 OK`
```json
[
  {
    "node_id": "node-abc",
    "address": "https://10.0.0.2:7443",
    "peer_port": 7444,
    "hostname": "san-node-2",
    "status": "online",
    "last_heartbeat": "2025-01-15T10:30:05Z"
  }
]
```

### POST /api/peers/join

Register a new peer.

**Request:**
```json
{
  "address": "https://10.0.0.2:7443",
  "node_id": "node-abc",
  "hostname": "san-node-2",
  "peer_port": 7444,
  "secret": "shared-secret"
}
```

**Response:** `200 OK`
```json
{"success": true, "node_id": "node-abc"}
```

### DELETE /api/peers/{node_id}

Remove a peer.

**Response:** `200 OK`
```json
{"success": true}
```

### POST /api/peers/heartbeat

Peer heartbeat (internal, peer-to-peer).

**Request:**
```json
{
  "node_id": "node-abc",
  "hostname": "san-node-2",
  "uptime_secs": 3600,
  "address": "https://10.0.0.2:7443"
}
```

**Response:** `200 OK`
```json
{
  "node_id": "node-xyz",
  "hostname": "san-node-1",
  "quorum_status": "active",
  "is_leader": true,
  "peer_count": 2
}
```

---

## File Operations

### GET /api/volumes/{id}/files

List all files in a volume.

**Response:** `200 OK`
```json
[
  {
    "rel_path": "data/report.pdf",
    "size_bytes": 1048576,
    "sha256": "e3b0c44298fc1c149...",
    "created_at": "2025-01-15T10:00:00Z",
    "updated_at": "2025-01-15T10:30:00Z",
    "replica_count": 2,
    "synced_count": 2
  }
]
```

### GET /api/volumes/{id}/files/{*path}

Read a file. Returns raw bytes with appropriate content type.

**Response:** `200 OK` — file contents as `application/octet-stream`

**Transparent peer-fetch:** If the file is not stored locally, the node automatically fetches it from a peer. The client receives the data without knowing which node actually holds it.

**Errors:**
- `404` — File not found on any node in the cluster
- `500` — Read error

### PUT /api/volumes/{id}/files/{*path}

Write (create or overwrite) a file.

**Request body:** Raw file bytes

**Response:** `200 OK`
```json
{
  "rel_path": "data/report.pdf",
  "size_bytes": 1048576,
  "sha256": "e3b0c44298fc1c149...",
  "created_at": "2025-01-15T10:30:00Z",
  "updated_at": "2025-01-15T10:30:00Z",
  "replica_count": 1,
  "synced_count": 1
}
```

**Errors:**
- `503` — Node is fenced (no quorum), writes not allowed
- `501` — Volume uses `sync_mode: quorum` (not implemented)
- `404` — No local backend available
- `409` — Write lease conflict (another node is writing this file)

### DELETE /api/volumes/{id}/files/{*path}

Delete a file and all its replicas.

**Response:** `200 OK`
```json
{"success": true}
```

### POST /api/volumes/{id}/mkdir

Create a directory.

**Request:**
```json
{"path": "my/new/directory"}
```

**Response:** `200 OK`
```json
{"success": true, "path": "my/new/directory"}
```

### GET /api/volumes/{id}/browse

Browse root directory of a volume.

**Response:** `200 OK`
```json
[
  {"name": "data", "is_dir": true, "size_bytes": 0, "updated_at": ""},
  {"name": "readme.txt", "is_dir": false, "size_bytes": 1024, "updated_at": "2025-01-15T10:00:00Z"}
]
```

Entries are sorted: directories first, then alphabetically.

### GET /api/volumes/{id}/browse/{*path}

Browse a subdirectory.

**Response:** Same format as root browse.

---

## Benchmark

### GET /api/benchmark/results

Latest benchmark results (last 1 hour).

**Response:** `200 OK`
```json
[
  {
    "from_node_id": "node-1",
    "to_node_id": "node-2",
    "bandwidth_mbps": 1200.5,
    "latency_us": 350.2,
    "jitter_us": 45.1,
    "packet_loss_pct": 0.0,
    "test_size_bytes": 67108864,
    "measured_at": "2025-01-15T10:30:00Z"
  }
]
```

### POST /api/benchmark/run

Trigger a manual benchmark run.

**Response:** `200 OK`
```json
{"started": true}
```

### GET /api/benchmark/matrix

N×N performance matrix between all nodes.

**Response:** `200 OK`
```json
{
  "node_ids": ["node-1", "node-2", "node-3"],
  "entries": [
    {
      "from_node_id": "node-1",
      "to_node_id": "node-2",
      "bandwidth_mbps": 1200.5,
      "latency_us": 350.2,
      "jitter_us": 45.1,
      "packet_loss_pct": 0.0,
      "test_size_bytes": 67108864,
      "measured_at": "2025-01-15T10:30:00Z"
    }
  ]
}
```

### GET /api/benchmark/ping

Minimal latency probe (used internally by benchmark engine).

**Response:** `200 OK`
```json
{"pong": true}
```

### POST /api/benchmark/echo

Throughput test — echoes received data back (used internally).

**Request body:** Raw bytes

**Response:** `200 OK` — same bytes echoed back

---

## Error Response Format

All error responses follow this pattern:

```json
{
  "error": "Description of what went wrong"
}
```

Or for StatusCode-only errors, the HTTP status code indicates the error:

| Code | Meaning |
|------|---------|
| `400` | Bad request (invalid input) |
| `401` | Unauthorized (missing/wrong peer secret) |
| `404` | Resource not found |
| `409` | Conflict (write lease held by another node, volume not empty) |
| `500` | Internal server error (database error, I/O failure) |
| `501` | Not implemented (quorum sync_mode) |
| `503` | Service unavailable (node is fenced) |
