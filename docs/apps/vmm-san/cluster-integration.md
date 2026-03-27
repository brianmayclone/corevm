# CoreSAN Cluster Integration

CoreSAN can operate standalone (direct API access) or integrated with vmm-cluster, which acts as a proxy, witness, and auto-discovery engine.

## Architecture

```
┌────────────┐       ┌───────────────────────────────────┐
│  vmm-ui    │──────►│         vmm-cluster                │
│ (Browser)  │ Bearer│                                     │
│            │ Token │  /api/san/* ──► SanClient ──► Node  │
└────────────┘       │                                     │
                     │  Heartbeat ──► Auto Peer Register   │
                     │                                     │
                     │  /api/san/witness/{id} ◄── Node     │
                     └───────────────────────────────────┘
```

**In cluster mode**, the UI only talks to vmm-cluster. All CoreSAN operations are proxied through `/api/san/*` endpoints.

**In standalone mode**, the UI talks directly to a local vmm-san instance on port 7443.

## Proxy Endpoints

vmm-cluster exposes CoreSAN operations under the `/api/san/` prefix. The proxy handles routing, multi-host fan-out, result aggregation, and event logging.

### Routing Patterns

| Pattern | Description | Example |
|---------|-------------|---------|
| **Any host** | Forward to any available SAN host (data is synced) | Volumes, Peers |
| **Fan-out** | Send to ALL SAN hosts, merge results | Status, Disks, Backends |
| **Targeted** | Route to specific host based on `host_id` in request | Disk claim, Backend add |

### Complete Endpoint Map

| Cluster Endpoint | Method | Routing | Event Logged |
|-----------------|--------|---------|--------------|
| `/api/san/status` | GET | Fan-out (parallel) | No |
| `/api/san/health` | GET | Memory snapshot | No |
| `/api/san/volumes` | GET | Any host | No |
| `/api/san/volumes` | POST | Any host | Yes: "Volume created" |
| `/api/san/volumes/{id}` | GET | Any host | No |
| `/api/san/volumes/{id}` | PUT | Any host | Yes: "Volume updated" |
| `/api/san/volumes/{id}` | DELETE | Any host | Yes: "Volume deleted" |
| `/api/san/volumes/{id}/backends` | GET | Fan-out | No |
| `/api/san/volumes/{id}/backends` | POST | Targeted (host_id) | Yes: "Backend added" |
| `/api/san/volumes/{vid}/backends/{bid}` | DELETE | Targeted (host_id) | Yes: "Backend removed" |
| `/api/san/peers` | GET | Any host | No |
| `/api/san/disks` | GET | Fan-out | No |
| `/api/san/disks/claim` | POST | Targeted (host_id) | Yes: "Disk claimed" |
| `/api/san/disks/release` | POST | Targeted (host_id) | Yes: "Disk released" |
| `/api/san/disks/reset` | POST | Targeted (host_id) | Yes: "Disk reset" |
| `/api/san/volumes/{id}/browse` | GET | Any host | No |
| `/api/san/volumes/{id}/browse/{*path}` | GET | Any host | No |
| `/api/san/volumes/{id}/mkdir` | POST | Any host | No |
| `/api/san/volumes/{id}/files/{*path}` | PUT | Any host | No |
| `/api/san/volumes/{id}/files/{*path}` | DELETE | Any host | No |
| `/api/san/benchmark` | GET | Any host | No |
| `/api/san/benchmark/run` | POST | Any host | Yes: "Benchmark triggered" |
| `/api/san/witness/{node_id}` | GET | Local (no proxy) | No |

### Fan-Out Aggregation

For fan-out endpoints (status, disks, backends), the cluster:
1. Queries all SAN-enabled hosts from its database
2. Sends HTTP requests to all hosts in parallel (`futures::future::join_all`)
3. Tags each response with `_host_id` and `_host_name`
4. Merges all results into a single array
5. Includes error entries for unreachable hosts (instead of failing entirely)

Example: `GET /api/san/disks` returns disks from ALL nodes with host identification:

```json
[
  {
    "name": "sdb",
    "path": "/dev/sdb",
    "size_bytes": 1000000000,
    "status": "available",
    "_host_id": "host-abc",
    "_host_name": "san-node-1"
  },
  {
    "name": "sdb",
    "path": "/dev/sdb",
    "size_bytes": 500000000,
    "status": "claimed",
    "_host_id": "host-xyz",
    "_host_name": "san-node-2"
  }
]
```

### Targeted Routing

Disk and backend operations target a specific host. The request body must include `host_id` to identify which SAN node should handle the operation:

```json
{
  "device_path": "/dev/sdb",
  "host_id": "host-abc"
}
```

The cluster resolves `host_id` to the SAN address and forwards the request.

## SanClient

The `SanClient` struct (`apps/vmm-cluster/src/san_client.rs`) handles HTTP communication with individual vmm-san instances:

- **Timeouts:** 30s request, 5s connect
- **TLS:** Accepts invalid certificates (internal network)
- **Methods:** Pass-through for all SAN API endpoints
- **Host resolution:** `get_san_hosts(db)` queries all online hosts with `san_enabled=1`

## Auto Peer Registration

The cluster's heartbeat engine (`apps/vmm-cluster/src/engine/heartbeat.rs`) automatically discovers and registers SAN peers:

### How It Works

1. Each managed host sends heartbeats to vmm-cluster every 10 seconds
2. Heartbeat includes SAN status: `san.running`, `san.node_id`, `san.address`, `san.peers`
3. When a host reports `san.running == true`:
   - The cluster stores `san_enabled=1`, `san_node_id`, `san_address` in the hosts table
   - If this is a new SAN host OR it has 0 peers:
     - For each existing SAN host: call `POST /api/peers/join` on new host with existing host info
     - For each existing SAN host: call `POST /api/peers/join` on existing host with new host info
     - Log event: "CoreSAN peer auto-registered: {hostname}"
4. When a host stops reporting `san.running`:
   - Set `san_enabled=0`, clear SAN fields

### Benefits

- **Zero manual configuration**: Add a new node, start vmm-san, peers are auto-discovered
- **Bidirectional**: Both sides of the peer relationship are registered
- **Idempotent**: Re-registration is safe (uses INSERT OR REPLACE)
- **Event-logged**: All registrations appear in the cluster event log

## Witness System

The witness provides quorum tie-breaking for SAN nodes that have lost majority.

### Endpoint

```
GET /api/san/witness/{node_id}
```

**No authentication required** — SAN nodes call this directly. The endpoint is intentionally open because SAN nodes may not have cluster bearer tokens.

### Decision Logic

1. Get all known SAN hosts from the cluster database
2. Check if the requesting node is one of them
3. Count total SAN hosts and determine majority threshold
4. **If majority can be determined**: Grant quorum to the partition with majority
5. **If exactly half (2-node split)**: Tie-break by lowest `host_id` in the cluster database
6. **If requesting node is not reachable from cluster**: Deny

### Response

```json
{
  "allowed": true,
  "reason": "majority partition (2 of 3 nodes reachable)"
}
```

Or:

```json
{
  "allowed": false,
  "reason": "minority partition"
}
```

### Configuration on SAN Side

Each SAN node must be configured with the cluster's witness URL:

```toml
[cluster]
witness_url = "https://cluster-ip:9443"
```

The peer monitor calls the witness only when majority quorum is not met locally. If the witness is unreachable, the node defaults to `Fenced`.

## Event Logging

All mutating SAN operations logged by the cluster use the standard event system:

| Event | Severity | Target Type |
|-------|----------|-------------|
| Volume created | info | san |
| Volume updated | info | san |
| Volume deleted | warning | san |
| Backend added | info | san |
| Backend removed | warning | san |
| Disk claimed | info | san |
| Disk released | warning | san |
| Disk reset | warning | san |
| Benchmark triggered | info | san |
| Peer auto-registered | info | san |

Events are visible in the vmm-cluster event log and the UI's event viewer.

## Database Integration

The cluster stores SAN-related fields in the `hosts` table:

| Column | Type | Description |
|--------|------|-------------|
| `san_enabled` | INTEGER | 1 if SAN is running on this host |
| `san_node_id` | TEXT | CoreSAN node UUID |
| `san_address` | TEXT | SAN API address (e.g., `https://10.0.0.1:7443`) |
| `san_volumes` | INTEGER | Number of volumes |
| `san_peers` | INTEGER | Number of registered peers |

These fields are updated on every heartbeat when the host reports SAN status.

## UI Integration

The StorageCoresan page (`apps/vmm-ui/src/pages/StorageCoresan.tsx`) switches between cluster and standalone mode:

**Cluster mode** (when the app is connected to vmm-cluster):
- API calls go to `/api/san/*` with bearer token authentication
- Disks, backends, and status are aggregated across all hosts
- Disk claim/release operations include `host_id` for routing
- No manual peer management needed

**Standalone mode** (direct local connection):
- API calls go directly to `https://localhost:7443/api/*`
- Only local node's disks and backends are visible
- Manual peer registration may be required

The mode is determined automatically based on whether the UI is connected to a cluster instance.
