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
│   ├── storage.rs          Cluster-wide storage
│   ├── datastores.rs       Shared datastore management
│   ├── network.rs          Network interface aggregation
│   ├── migration.rs        VM migration endpoints
│   ├── tasks.rs            Long-running operation tracking
│   ├── events.rs           Event log
│   ├── alarms.rs           Alert system
│   ├── drs.rs              DRS status and control
│   ├── activity.rs         Activity log
│   └── ...
│
├── services/               Business logic
│   ├── host.rs             Host registration, status tracking
│   ├── cluster.rs          Cluster config management
│   ├── vm.rs               Cluster-wide VM operations
│   ├── datastore.rs        Datastore management
│   ├── migration.rs        VM migration orchestration
│   ├── task.rs             Task tracking
│   ├── drs_service.rs      DRS scheduling logic
│   ├── alarm.rs            Alarm evaluation
│   └── ...
│
├── engine/                 Background orchestration engines
│   ├── heartbeat.rs        Node health monitoring (10s interval)
│   ├── drs.rs              Distributed Resource Scheduler (5m interval)
│   ├── ha.rs               High Availability engine
│   ├── maintenance.rs      Host maintenance mode (VM drain)
│   └── scheduler.rs        VM placement scheduler
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

#### Maintenance Engine (`engine/maintenance.rs`)

- **Trigger:** Host enters maintenance mode
- **Action:** Migrates all VMs off the host before completing the drain

#### Scheduler (`engine/scheduler.rs`)

- **Purpose:** VM placement decisions for new VM creation
- **Algorithm:** Selects the node with the most available resources

### VM Migration Flow

1. Client requests `POST /api/vms/{id}/migrate` with target host
2. Migration service validates source and target nodes
3. Source node stops the VM gracefully
4. VM configuration is transferred to the target node
5. Target node provisions and starts the VM
6. Migration task is updated with success/failure status

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
