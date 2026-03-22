# vmm-cluster — Developer Guide

This guide covers the internal architecture of vmm-cluster for contributors and developers extending the cluster orchestration system.

## Architecture Overview

```
                    ┌──────────────────────┐
                    │      Axum Router      │
                    │   (REST + WebSocket)   │
                    └──────────┬───────────┘
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
        ┌──────────┐    ┌──────────┐    ┌──────────┐
        │  API     │    │ Services │    │ Engines  │
        │ src/api/ │    │src/svc/  │    │src/engine│
        └──────────┘    └──────────┘    └──────────┘
                               │                │
                               ▼                ▼
                    ┌──────────────────────────────┐
                    │       ClusterState            │
                    │  DashMap<NodeId, NodeConn>    │
                    │  SQLite DB                    │
                    └──────────────┬───────────────┘
                                   │
                                   ▼
                    ┌──────────────────────────────┐
                    │      Node Client (reqwest)    │
                    │  → /agent/* on vmm-server     │
                    └──────────────────────────────┘
```

## Source Structure

```
apps/vmm-cluster/src/
├── main.rs                 Server bootstrap, node registration, engine startup
├── config.rs               Configuration (vmm-cluster.toml)
├── state.rs                ClusterState (nodes, DB, config)
│
├── api/                    REST API endpoints
│   ├── mod.rs              Router — maps all routes
│   ├── auth.rs             Authentication endpoints
│   ├── vms.rs              Cluster-wide VM management
│   ├── hosts.rs            Host/node management
│   ├── clusters.rs         Cluster configuration
│   ├── cluster_settings.rs LDAP, DRS exclusions, SMTP settings
│   ├── storage.rs          Cluster-wide storage
│   ├── storage_wizard.rs   Guided filesystem setup API
│   ├── datastores.rs       Shared datastore management
│   ├── network.rs          SDN virtual networks + DHCP/DNS/PXE
│   ├── notifications.rs    Notification channels and rules
│   ├── migration.rs        VM migration endpoints
│   ├── tasks.rs            Long-running operation tracking
│   ├── events.rs           Event log
│   ├── alarms.rs           Alert system
│   ├── drs.rs              DRS status and control
│   ├── activity.rs         Activity log
│   └── ...
│
├── services/               Business logic
│   ├── host.rs             Host registration, status tracking, VM import
│   ├── cluster.rs          Cluster config management
│   ├── vm.rs               Cluster-wide VM operations
│   ├── datastore.rs        Datastore management
│   ├── migration.rs        Direct host-to-host migration orchestration
│   ├── task.rs             Task tracking
│   ├── drs_service.rs      DRS scheduling logic
│   ├── drs_exclusion.rs    DRS exclusion rules (per-VM, per-group)
│   ├── alarm.rs            Alarm evaluation
│   ├── notification.rs     Notification channels, rules, dispatch
│   ├── network.rs          SDN network CRUD + DHCP/DNS/PXE
│   ├── storage_wizard.rs   NFS/GlusterFS/CephFS setup orchestration
│   ├── validation.rs       Input validation (IP, CIDR, MAC, VLAN, etc.)
│   ├── group.rs            User group management with role mapping
│   ├── ldap.rs             LDAP/AD configuration and connection testing
│   ├── auth.rs             Authentication (local + LDAP)
│   └── ...
│
├── engine/                 Background orchestration engines
│   ├── heartbeat.rs        Node health monitoring (10s interval)
│   ├── drs.rs              Distributed Resource Scheduler (5m interval)
│   ├── ha.rs               High Availability engine
│   ├── maintenance.rs      Host maintenance mode (VM drain)
│   ├── scheduler.rs        VM placement scheduler
│   ├── notifier.rs         Async notification dispatcher (email/webhook/log)
│   ├── reconciler.rs       State reconciliation on node reconnect
│   └── sdn.rs              dnsmasq config generation for SDN networks
│
├── node_client/            HTTP client for agent communication
│   └── mod.rs              reqwest-based client for /agent/* endpoints
│
├── ws/                     WebSocket handlers
│   └── ...
│
└── db/                     Database layer
    ├── mod.rs              SQLite pool, migrations
    └── schema.rs           Cluster-specific tables
```

## Key Concepts

### ClusterState

Central state shared across all handlers and engines:

```rust
struct ClusterState {
    nodes: DashMap<String, NodeConnection>,
    db: SqlitePool,
    config: ClusterConfig,
}
```

Each `NodeConnection` tracks:
- `node_id` — unique identifier
- `hostname` — human-readable name
- `address` — HTTP address of the vmm-server agent
- `token` — authentication token for agent API
- `status` — last known status (CPU, RAM, VMs, datastores)
- `heartbeat_count` — consecutive successful heartbeats

### Agent Protocol

Communication between cluster and nodes uses HTTP:

| Direction | Endpoint | Purpose |
|-----------|----------|---------|
| Cluster → Node | `POST /agent/register` | Register node with cluster |
| Cluster → Node | `GET /agent/status` | Poll node health/status |
| Cluster → Node | `POST /agent/vms/provision` | Create VM on node |
| Cluster → Node | `POST /agent/vms/start` | Start VM on node |
| Cluster → Node | `POST /agent/vms/stop` | Stop VM on node |
| Cluster → Node | `POST /agent/vms/force-stop` | Force stop VM on node |
| Cluster → Node | `POST /agent/vms/destroy` | Destroy VM on node |
| Cluster → Node | `POST /agent/storage/*` | Storage operations |
| Cluster → Node | `POST /agent/migration/send` | Send VM disks to target host |
| Cluster → Node | `POST /agent/migration/receive` | Receive VM disks from source host |
| Cluster → Node | `POST /agent/packages/check` | Check installed packages |
| Cluster → Node | `POST /agent/packages/install` | Install packages (apt/dnf/yum) |
| Cluster → Node | `POST /agent/exec` | Execute shell command with optional sudo |

Authentication uses `X-Agent-Token` header with a shared secret established during registration.

### Background Engines

Engines are async Tokio tasks spawned at startup:

#### Heartbeat Engine (`engine/heartbeat.rs`)

- **Interval:** 10 seconds
- **Action:** Polls `/agent/status` on every registered node
- **On success:** Updates node status in ClusterState
- **On failure:** Increments failure counter → marks node offline after threshold

#### DRS Engine (`engine/drs.rs`)

- **Interval:** 5 minutes
- **Action:** Evaluates resource utilization across all nodes
- **Algorithm:** Identifies overloaded/underloaded nodes, recommends migrations
- **Modes:** Manual (recommendations) or Automatic (executes migrations)

#### HA Engine (`engine/ha.rs`)

- **Trigger:** Node goes offline (heartbeat failure)
- **Action:** Reschedules failed node's VMs on healthy nodes
- **Selection:** Chooses target nodes based on available capacity

#### Reconciler Engine (`engine/reconciler.rs`)

- **Trigger:** Node transitions from offline → online
- **Action:** Syncs cluster DB with actual host state to prevent split-brain
- **Logic:**
  1. Queries agent for all VMs running on the reconnected host
  2. If a VM is assigned to a **different** host in the DB → **force-stop** on this host (HA already moved it)
  3. If a VM is orphaned (running but not in DB) → **force-stop**
  4. If a VM is orphaned but matches a lost record → **reclaim** to this host

#### Notifier Engine (`engine/notifier.rs`)

- **Type:** Async worker (mpsc channel consumer)
- **Action:** Dispatches queued notifications to configured channels
- **Email:** Raw SMTP (EHLO, AUTH PLAIN, MAIL FROM, DATA) with optional TLS
- **Webhook:** HTTP POST with JSON payload + optional HMAC-SHA256 signature
- **Log:** Writes to stdout/tracing

#### SDN Engine (`engine/sdn.rs`)

- **Purpose:** Generates dnsmasq configuration files for SDN networks
- **Output:** `/etc/vmm/dnsmasq-net-{network_id}.conf` on each host
- **Contents:** DHCP range, static reservations, DNS records, PXE options, upstream DNS

#### Maintenance Engine (`engine/maintenance.rs`)

- **Trigger:** Host enters maintenance mode
- **Action:** Migrates all VMs off the host before completing the drain

#### Scheduler (`engine/scheduler.rs`)

- **Purpose:** VM placement decisions for new VM creation
- **Algorithm:** Selects the node with the most available resources

### VM Migration Flow (Direct Host-to-Host)

1. Client requests `POST /api/vms/{id}/migrate` with target host
2. Migration service checks if source and target share a datastore
3. Cluster generates a one-time migration token (UUID, 5-minute expiry)
4. **Shared storage path:** Source stops VM, cluster tells target to provision + start (no disk copy)
5. **Local storage path:**
   - Cluster tells target to expect transfer: `POST /agent/migration/receive` with token
   - Cluster tells source to send: `POST /agent/migration/send` with target address + token
   - Source streams disk files directly to target via HTTP (bypasses cluster)
   - Target provisions VM config and starts
6. Migration task is updated with progress (bytes_sent/bytes_total) and final status

## Adding a New Engine

1. Create a new file in `src/engine/`
2. Implement the engine as an async function that loops on a `tokio::time::interval`
3. Spawn the engine in `main.rs` during startup
4. Access `ClusterState` for node information and database

## Adding a New API Endpoint

1. Add the handler in `src/api/`
2. Register the route in `src/api/mod.rs`
3. Add service logic in `src/services/`
4. Add database operations if needed
5. Log events via the event service

## Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` / `tokio` | HTTP framework and async runtime |
| `reqwest` | HTTP client for agent communication |
| `rusqlite` | SQLite database |
| `dashmap` | Concurrent node state map |
| `jsonwebtoken` / `argon2` | Auth |
| `tokio-tungstenite` | WebSocket client |
| `vmm-core` | Shared data models (cluster types) |
| `tracing` | Logging |
