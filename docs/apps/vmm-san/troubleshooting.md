# CoreSAN Troubleshooting

## Diagnostic Commands

### Quick Health Check

```bash
# Is the node running?
curl -sk https://localhost:7443/api/health

# Full status
curl -sk https://localhost:7443/api/status | jq .

# Peer connectivity
curl -sk https://localhost:7443/api/peers | jq .

# Volume health
curl -sk https://localhost:7443/api/volumes | jq .

# Backend health
curl -sk https://localhost:7443/api/volumes/<vol-id>/backends | jq .

# Disk status
curl -sk https://localhost:7443/api/disks | jq .

# Network performance
curl -sk https://localhost:7443/api/benchmark/matrix | jq .

# Disk SMART health
curl -sk https://localhost:7443/api/disks/sdb/smart | jq .

# Volume chunk map
curl -sk https://localhost:7443/api/volumes/<vol-id>/chunk-map | jq .
```

### Log Analysis

```bash
# Follow logs in real-time
journalctl -u vmm-san -f

# Filter for errors and warnings
journalctl -u vmm-san --since "1 hour ago" | grep -E "ERROR|WARN"

# Check for fencing events
journalctl -u vmm-san | grep -i "fenced"

# Check for peer failures
journalctl -u vmm-san | grep -i "offline"

# Check for integrity failures
journalctl -u vmm-san | grep -i "integrity.*FAIL"
```

---

## Common Problems

### Node is Fenced (Writes Rejected with 503)

**Symptoms:**
- `quorum_status: "fenced"` in `/api/status`
- Write requests return HTTP 503
- Log: `Node FENCED: no quorum, witness denied`

**Causes and Solutions:**

| Cause | Solution |
|-------|----------|
| Peers unreachable | Check network connectivity, firewall rules, peer nodes |
| Wrong peer addresses | Verify `address` field in `/api/peers` points to correct IP/port |
| Bind address mismatch | If `bind = "127.0.0.1"`, peers on other hosts can't reach this node |
| Witness unreachable | Check `[cluster] witness_url` and vmm-cluster availability |
| Witness denies | Node is in minority partition — this is correct behavior |
| All peers crashed | Restart peer nodes to restore majority |

**Diagnosis:**
```bash
# Check which peers are offline
curl -sk https://localhost:7443/api/peers | jq '.[] | select(.status == "offline")'

# Test connectivity to a peer
curl -sk https://peer-ip:7443/api/health

# Check witness
curl -sk https://cluster-ip:9443/api/san/witness/$(curl -sk https://localhost:7443/api/status | jq -r '.node_id')
```

**Recovery:** Once majority is restored (peers come back online), quorum recovers immediately and writes resume.

### Peer Shows as Offline

**Symptoms:**
- Peer status: `offline`
- Log: `Peer {id} went OFFLINE (3 missed heartbeats)`

**Causes:**

| Cause | Check |
|-------|-------|
| Peer node is down | SSH to peer, check service status |
| Network partition | Ping peer, check switch/firewall |
| Wrong address | `curl -sk https://peer-addr:7443/api/health` |
| Bind address issue | Peer bound to `127.0.0.1` but registered with external IP |
| Firewall | Port 7443 blocked between nodes |
| DNS resolution | Use IPs instead of hostnames |

**Address mismatch fix:**

If a node is bound to `127.0.0.1` but peers try to reach it via `172.x.x.x`:

```toml
# Option 1: Bind to all interfaces
[server]
bind = "0.0.0.0"

# Option 2: Bind to the specific SAN network IP
[server]
bind = "10.10.10.1"
```

When `bind` is a specific IP (not `0.0.0.0`), it's used as the advertised address in heartbeats, ensuring peers use the correct address.

### Node Stuck in Sanitizing State

**Symptoms:**
- `quorum_status: "sanitizing"` in `/api/status`
- Write requests return HTTP 503
- Node just started

**Cause:** The sanitize engine is verifying all local chunk replicas on startup. This is normal and expected.

**Resolution:**
- Wait for the sanitize engine to complete. Duration depends on the amount of local data.
- Check logs for progress: `journalctl -u vmm-san | grep -i "sanitiz"`
- If corrupt chunks are found, the sanitize engine will attempt to repair from peers automatically.

### File Read Returns 404

**Symptoms:**
- `GET /api/volumes/{id}/files/{path}` returns 404
- File was recently written on another node

**Causes:**

| Cause | Solution |
|-------|----------|
| Metadata sync not yet propagated | Wait for metadata_sync cycle (10s), retry |
| Push replication still in progress | Wait 1-2 seconds, retry |
| Push replication failed | Check peer connectivity, logs for push errors |
| No cross-registered backends | Verify backends table has entries for remote nodes |
| Peer unreachable for transparent fetch | Check peer status, network |

**How transparent peer-fetch works:**
1. Check has_local_chunks() + file_exists()
2. If missing chunks: fetch_chunks_from_peer() from known replicas
3. Broadcast to all online peers if no known replica
4. If all fail → 404

**Diagnosis:**
```bash
# Check if file exists on any node
for PORT in 7443 7444 7445; do
  echo "Node :$PORT:"
  curl -sk "https://localhost:$PORT/api/volumes/<vol>/files/<path>" -o /dev/null -w "%{http_code}\n"
done

# Check file list on writing node
curl -sk https://writing-node:7443/api/volumes/<vol>/files | jq '.[] | select(.rel_path == "path")'
```

### Disk Claim Fails

**Symptoms:**
- `POST /api/disks/claim` returns error
- Disk shows as `available` but can't be claimed

**Common causes:**

| Error | Solution |
|-------|----------|
| `Permission denied` | vmm-san needs root/sudo for `mkfs`, `mount`, `blkid` |
| `Disk has data` | Add `"confirm_format": true` to request |
| `Disk is OS disk` | OS disks are protected, cannot be claimed |
| `Disk is in use` | Unmount the disk first, or use a different disk |
| Partition/format error | Check `dmesg` for disk hardware errors |

**Manual disk inspection:**
```bash
lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINTS
blkid /dev/sdb
```

### Slow Replication / High Stale Count

**Symptoms:**
- `stale_chunks` count stays high
- Replication takes minutes instead of seconds

**Causes:**

| Cause | Solution |
|-------|----------|
| Slow network | Check benchmark results, upgrade to 10G+ |
| High MTU mismatch | Ensure all nodes use same MTU |
| CPU bottleneck | SHA256 computation on large files |
| Disk I/O bottleneck | Slow disks (HDD vs SSD) |
| Debug build | Release builds are 10-50x faster |
| Large file backlog | Wait for replicator to catch up |

**Check replication progress:**
```bash
# Overall stale count
curl -sk https://localhost:7443/api/status | jq '.volumes[].stale_chunks'

# Per-file sync status
curl -sk https://localhost:7443/api/volumes/<vol>/files | jq '.[] | select(.synced_count < .replica_count)'
```

### Write Lease Conflict (409)

**Symptoms:**
- Write returns HTTP 409 Conflict
- Log: `Write lease denied for {path}: owned by {other_node}`

**Cause:** Another node holds the write lease for this file.

**Resolution:**
- Wait for the lease to expire (30 seconds max)
- If the owning node has crashed, the peer monitor will release all its leases within 15 seconds (3 missed heartbeats)
- Retry the write after the lease is released

### Backend Offline

**Symptoms:**
- Backend shows `status: "offline"`
- Chunks on this backend are unavailable

**Causes:**

| Cause | Check |
|-------|-------|
| Disk physically removed | `lsblk`, `dmesg` |
| Mount lost | `mount | grep san-disks` |
| Filesystem corruption | `dmesg`, `fsck` (unmount first!) |
| Permission issue | Check mount permissions |

**Recovery:**
```bash
# If disk is still present but unmounted, re-mount
mount /dev/sdb1 /vmm/san-disks/<uuid>

# If disk has errors, reset it
curl -sk -X POST https://localhost:7443/api/disks/reset \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdb"}'
```

### SMART Warnings or Disk Health Issues

**Symptoms:**
- Disk shows "Warning" or "FAILED" health in the UI or TUI
- Log: `SMART warning for disk /dev/sdX`
- Events reported to cluster with `disk` category

**Diagnosis:**
```bash
# Check SMART detail for a specific disk
curl -sk https://localhost:7443/api/disks/sdb/smart | jq .

# Check all disks
curl -sk https://localhost:7443/api/disks | jq '.[] | {name, status, smart_summary}'
```

**Warning conditions and actions:**

| Condition | Action |
|-----------|--------|
| Health FAILED | Replace disk immediately. Release via API to drain data first. |
| Reallocated sectors > 0 | Monitor trend. Plan replacement if count increases. |
| Pending sectors > 0 | Disk may be failing. Plan replacement. |
| Temperature > 55C | Check cooling. Reduce workload or improve airflow. |
| NVMe media errors > 0 | Monitor trend. Plan replacement if count increases. |

**Note:** Virtual disks (virtio) report `supported=false` for SMART. This is expected and not a warning condition.

### Database Corruption

**Symptoms:**
- Service fails to start
- Log: `database disk image is malformed` or similar SQLite errors

**Recovery:**
CoreSAN mirrors its database to disk backends every 60 seconds. On startup, if the primary database is missing or corrupt:

```bash
# Check if backup exists on a claimed disk
ls /vmm/san-disks/*/coresan-backup.db

# Manual restore
cp /vmm/san-disks/<disk-uuid>/coresan-backup.db /var/lib/vmm-san/coresan.db
systemctl restart vmm-san
```

If no backup exists:
```bash
# Delete corrupt database (all metadata lost, data files still on disk)
rm /var/lib/vmm-san/coresan.db*
systemctl restart vmm-san
# Node starts fresh — re-claim disks, re-register peers
```

### Cluster Proxy Returns 502 Bad Gateway

**Symptoms:**
- `GET /api/san/status` through vmm-cluster returns 502
- Direct access to SAN node works fine

**Causes:**

| Cause | Solution |
|-------|----------|
| SAN node address wrong in cluster DB | Check `san_address` in hosts table |
| TLS certificate issue | SanClient accepts invalid certs — unlikely |
| SAN node not reporting in heartbeat | Check vmm-server agent is running |
| Timeout (30s) | SAN node overloaded or network issue |

**Diagnosis:**
```bash
# Check which SAN hosts the cluster knows about
curl -sk -H "Authorization: Bearer $TOKEN" https://cluster:9443/api/hosts | jq '.[] | select(.san_enabled) | {hostname, san_address, san_node_id}'
```

---

## Recovery Procedures

### Single Node Failure

1. Other nodes detect failure within 15 seconds (3 missed heartbeats)
2. Failed node's backends marked offline
3. Write leases released
4. Repair engine (leader) begins restoring FTT
5. When node recovers: automatic rejoin, stale replicas synced

**No manual intervention required** if FTT >= 1.

### Network Partition (Split Brain Prevention)

1. Nodes on each side of partition detect unreachable peers
2. Each side independently computes quorum
3. Majority side: continues as `Degraded`
4. Minority side: becomes `Fenced` (writes blocked)
5. With witness: only one side gets approval to continue
6. When partition heals: fenced side recovers, stale replicas synced

### Complete Cluster Restart

```bash
# Start nodes in any order
systemctl start vmm-san  # on each node

# Nodes will:
# 1. Load peers from database
# 2. Begin heartbeating
# 3. Establish quorum within 10-15 seconds
# 4. Elect leader
# 5. Resume normal operations
```

### Adding a New Node to Existing Cluster

**With vmm-cluster (automatic):**
1. Install and start vmm-san on new node
2. Ensure vmm-server agent is running (for heartbeat reporting)
3. Cluster auto-discovers and registers peers within 20 seconds

**Without vmm-cluster (manual):**
1. Start vmm-san on new node
2. Register existing peers on new node (POST /api/peers/join)
3. Register new node on each existing peer (POST /api/peers/join)
4. Claim disks on new node
5. Existing volumes will automatically rebalance

### Removing a Node from Cluster

1. Release all disks on the node (graceful drain)
2. Wait for drain to complete (all chunks moved)
3. Remove peer from other nodes: `DELETE /api/peers/{node_id}`
4. Stop vmm-san on the removed node

---

## Performance Tuning

### Network

| Setting | Impact | Recommendation |
|---------|--------|----------------|
| MTU | Higher = less overhead | 9000 (jumbo frames) |
| Dedicated NIC | No contention with VM traffic | Separate 10G+ NIC |
| TCP tuning | Buffer sizes | Default usually fine |

### Storage

| Setting | Impact | Recommendation |
|---------|--------|----------------|
| Disk type | IOPS and throughput | NVMe > SSD > HDD |
| Filesystem | Overhead and features | ext4 (default) |
| RAID policy | Capacity vs. redundancy | `stripe` for capacity, `mirror` for redundancy |
| Chunk size | Metadata overhead vs. granularity | 64 MB (default) for most workloads |

### Engine Intervals

| Engine | Default | When to Change |
|--------|---------|---------------|
| Benchmark | 300s | Increase if CPU constrained |
| Integrity | 3600s | Decrease for critical data, increase for large datasets |
| Repair | 60s | Decrease for faster recovery (more CPU) |
| Backend Refresh | 30s | Rarely needs changing |

### Build

**Always use release builds in production:**
```bash
cargo build -p vmm-san --release
```

Debug builds are 10-50x slower, especially for SHA256 computation and data transfer.
