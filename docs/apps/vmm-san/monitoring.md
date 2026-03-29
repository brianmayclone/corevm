# CoreSAN Monitoring & Benchmarking

## Health Monitoring

### Node Health Check

```bash
# Quick health probe (for load balancers / monitoring scripts)
curl -k https://node:7443/api/health
# 200 OK = running

# Full status with all details
curl -k https://node:7443/api/status
```

### Key Metrics to Monitor

| Metric | Source | Alert Threshold |
|--------|--------|----------------|
| Quorum status | `/api/status` → `quorum_status` | != "active" |
| Peer status | `/api/peers` → `status` | any "offline" |
| Volume status | `/api/status` → `volumes[].status` | != "online" |
| Degraded files | `/api/status` → `volumes[].degraded_files` | > 0 |
| Stale chunks | `/api/status` → `volumes[].stale_chunks` | > 0 (sustained) |
| Backend status | `/api/volumes/{id}/backends` → `status` | != "online" |
| Disk capacity | `/api/volumes/{id}/backends` → `free_bytes` | < 10% |
| Packet loss | `/api/benchmark/matrix` → `packet_loss_pct` | > 0% |
| Bandwidth | `/api/benchmark/matrix` → `bandwidth_mbps` | below expected |
| Disk SMART health | `/api/disks` → `smart_summary` | warning or critical |
| Disk temperature | `/api/disks/{name}/smart` → `temperature` | > 55C |
| Reallocated sectors | `/api/disks/{name}/smart` → `reallocated_sectors` | > 0 |

### Monitoring Script Example

```bash
#!/bin/bash
# coresan-health-check.sh
NODE="https://localhost:7443"

STATUS=$(curl -sk "$NODE/api/status")
QUORUM=$(echo "$STATUS" | jq -r '.quorum_status')
LEADER=$(echo "$STATUS" | jq -r '.is_leader')
PEERS=$(echo "$STATUS" | jq -r '.peer_count')

echo "Quorum: $QUORUM | Leader: $LEADER | Peers: $PEERS"

# Check for degraded volumes
DEGRADED=$(echo "$STATUS" | jq '[.volumes[] | select(.status != "online")] | length')
if [ "$DEGRADED" -gt 0 ]; then
  echo "WARNING: $DEGRADED volume(s) not online"
fi

# Check for stale chunks
STALE=$(echo "$STATUS" | jq '[.volumes[].stale_chunks] | add')
if [ "$STALE" -gt 0 ]; then
  echo "WARNING: $STALE stale chunk(s) pending sync"
fi

# Check for degraded files
DEG_FILES=$(echo "$STATUS" | jq '[.volumes[].degraded_files] | add')
if [ "$DEG_FILES" -gt 0 ]; then
  echo "WARNING: $DEG_FILES degraded file(s) — under-replicated"
fi

# Check disk SMART health
DISKS=$(curl -sk "$NODE/api/disks")
UNHEALTHY=$(echo "$DISKS" | jq '[.[] | select(.smart_summary.severity != "ok" and .smart_summary.severity != null)] | length')
if [ "$UNHEALTHY" -gt 0 ]; then
  echo "WARNING: $UNHEALTHY disk(s) with SMART warnings"
fi
```

## Network Benchmarking

### Automatic Benchmarks

The benchmark engine runs every 300 seconds (configurable) and tests:
1. **Latency**: 10 ping round-trips to each peer, calculates average and jitter (standard deviation)
2. **Throughput**: Sends a configurable payload (default 64 MB) via echo endpoint, measures transfer speed
3. **Packet Loss**: Compares received bytes vs. sent bytes

Results are stored in the `benchmark_results` table and automatically cleaned up after 24 hours.

### Manual Benchmark

```bash
# Trigger immediate benchmark
curl -k -X POST https://node:7443/api/benchmark/run

# View N×N matrix
curl -k https://node:7443/api/benchmark/matrix
```

### Reading the Benchmark Matrix

```json
{
  "node_ids": ["node-1", "node-2", "node-3"],
  "entries": [
    {
      "from_node_id": "node-1",
      "to_node_id": "node-2",
      "bandwidth_mbps": 1205.3,
      "latency_us": 342.1,
      "jitter_us": 28.5,
      "packet_loss_pct": 0.0,
      "test_size_bytes": 67108864,
      "measured_at": "2025-01-15T10:30:00Z"
    }
  ]
}
```

### Performance Expectations

| Network | Expected Bandwidth | Expected Latency |
|---------|-------------------|-----------------|
| 1 Gbps | 100-120 MB/s | 200-500 μs |
| 10 Gbps | 800-1200 MB/s | 100-300 μs |
| 25 Gbps | 2000-3000 MB/s | 50-200 μs |
| Same host (loopback) | 3000+ MB/s | < 100 μs |

**Factors affecting performance:**
- MTU (jumbo frames reduce overhead: MTU 9000 recommended)
- CPU load (encryption, checksum computation)
- Switch configuration (flow control, QoS)
- Disk I/O (for throughput tests that hit disk)
- Debug vs. release builds (debug is 10-50x slower)

### Benchmark Alerts

| Metric | Warning | Critical |
|--------|---------|----------|
| Bandwidth | < 50% of expected | < 10% of expected |
| Latency | > 2× baseline | > 10× baseline |
| Jitter | > latency average | > 5× latency average |
| Packet Loss | > 0% | > 1% |

## Integrity Verification

### How It Works

The integrity engine (default: every 3600 seconds) performs a full scan of all local chunk replicas:

1. Iterates over all `chunk_replicas` in `synced` state on local backends
2. Reads each chunk file from disk
3. Computes SHA256 hash
4. Compares with stored hash in `file_chunks` table
5. Records result in `integrity_log` table

### Integrity Log

Each check produces a log entry:

| Field | Description |
|-------|-------------|
| `file_id` | Reference to file_map entry |
| `backend_id` | Which backend was checked |
| `expected_sha256` | Hash stored in database |
| `actual_sha256` | Hash computed from disk read |
| `passed` | 1 = match, 0 = corruption detected |
| `checked_at` | Timestamp |

### What Happens on Failure

When a checksum mismatch is detected:

1. The chunk replica is marked as `error` in `chunk_replicas`
2. A warning is logged: `Integrity check FAILED for chunk X on backend Y`
3. The repair engine (leader-only, every 60 seconds) detects the under-replicated chunk
4. Repair pulls a healthy copy from another node
5. New replica is written and marked `synced`
6. Protection status is recalculated

### Manual Integrity Check

There is no explicit "run integrity check now" endpoint. To trigger a check:
- Restart the service (integrity check runs on first interval)
- Or reduce `integrity.interval_secs` temporarily

## S.M.A.R.T. Disk Health Monitoring

### SMART Monitor Engine (300-second interval)

The `smart_monitor` engine runs every 5 minutes and collects S.M.A.R.T. data from all disks:

**Data collection:**
- Reads via `smartctl -a -j` (JSON output mode)
- Supports SATA/SAS disks (attribute IDs 5, 9, 177, 194, 197, 198) and NVMe (health log)
- Virtual disks (virtio) return `supported=false`
- Results stored in the `smart_data` table

**API access:**
- `SmartSummary` is embedded in the `/api/disks` response for each disk
- Full `SmartDetail` available via `GET /api/disks/{device_name}/smart`

**Warning conditions:**

| Condition | Severity |
|-----------|----------|
| Health assessment FAILED | critical |
| Reallocated sectors > 0 | warning |
| Pending sectors > 0 | warning |
| Temperature > 55C | warning |
| NVMe media errors > 0 | warning |

**Severity levels:** `ok`, `warning`, `critical`

**Event reporting:** When warning or critical conditions are detected, the `smart_monitor` reports events to the cluster via the `event_reporter` engine (fire-and-forget HTTP POST to `/api/events/ingest`). Events use the `disk` category.

## Disk Health Monitoring

### Backend Refresh (30-second interval)

The backend refresh engine checks each local backend:
1. Verifies mount path exists
2. Tests writability (creates/deletes a test file)
3. Reads capacity via `statvfs` (total_bytes, free_bytes)
4. Updates database with latest stats

### Disk Monitor (5-second poll)

Scans `/sys/block/` to detect disk changes:

**Hot-remove detected:**
1. Backend marked `offline` immediately
2. Claimed disk status set to `error`
3. All chunk replicas on that backend marked `error`
4. FUSE mount unmounted if applicable
5. Repair engine begins restoring copies

**Hot-add detected:**
- New disk appears in `GET /api/disks` as `available`
- Must be manually claimed to join the storage pool

## Logging

### Log Levels

| Level | What's Logged |
|-------|--------------|
| `error` | Node fenced, database failures, unrecoverable I/O errors |
| `warn` | Peer offline, witness unreachable, integrity failures, backend degraded |
| `info` | Node joined/left, volume created/deleted, disk claimed, quorum changes, leader changes |
| `debug` | File writes, peer fetches, replication events, lease operations |
| `trace` | Heartbeat details, leader calculation, every DB query |

### Important Log Messages

| Message | Meaning | Action |
|---------|---------|--------|
| `Node FENCED: no quorum, witness denied` | Lost quorum, writes blocked | Check peers, network, witness |
| `Node recovered from fenced state` | Quorum restored | Normal operation resumes |
| `Peer {id} went OFFLINE` | Heartbeat failure | Check peer node, network |
| `Integrity check FAILED` | Data corruption detected | Repair engine will handle |
| `Backend {id} is offline` | Disk unreachable | Check disk, mount, hardware |
| `Write lease conflict` | Another node holds file lock | Normal with concurrent access |
| `Push replication failed` | Could not send data to peer | Stale replicator will retry |
| `Node SANITIZING` | Startup integrity check in progress | Wait for completion |
| `Sanitize complete` | Startup integrity check finished | Normal operation begins |
| `SMART warning for disk` | Disk health issue detected | Check disk, plan replacement |
| `SMART health FAILED` | Critical disk health failure | Replace disk urgently |

### Configuring Logging

```toml
[logging]
level = "info"
file = "/var/log/vmm-san.log"
```

Or via environment:
```bash
RUST_LOG=debug vmm-san --config /etc/vmm/vmm-san.toml
```

Fine-grained control:
```bash
RUST_LOG="vmm_san=debug,vmm_san::engine::peer_monitor=trace" vmm-san --config ...
```

## Cluster-Level Monitoring

When integrated with vmm-cluster, additional monitoring is available:

### Health Endpoint

```bash
curl -k https://cluster:9443/api/san/health
```

Returns aggregated health from the SAN health engine, which polls all SAN hosts periodically.

### Event Log

All mutating SAN operations are logged in the cluster event system. View via:
- vmm-ui event viewer
- `GET /api/events?target_type=san`

### Dashboard Metrics

The vmm-ui StorageCoresan page shows:
- Total cluster capacity and usage
- Per-volume health and capacity
- Per-node backend status
- Peer connectivity matrix
- Network benchmark results
- Disk discovery and status across all nodes
