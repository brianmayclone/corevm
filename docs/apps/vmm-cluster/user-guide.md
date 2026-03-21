# vmm-cluster — User Guide

vmm-cluster is the cluster orchestration authority for CoreVM. It manages multiple vmm-server nodes, providing centralized VM management, automatic load balancing, high availability, and live migration.

Think of it as analogous to VMware vCenter — a single control plane for your entire virtualization infrastructure.

## Architecture

```
                    ┌──────────────────┐
                    │   vmm-cluster     │
                    │ (central authority)│
                    └────────┬─────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
        ┌──────────┐  ┌──────────┐  ┌──────────┐
        │vmm-server│  │vmm-server│  │vmm-server│
        │ (agent)  │  │ (agent)  │  │ (agent)  │
        │  Node 1  │  │  Node 2  │  │  Node 3  │
        └──────────┘  └──────────┘  └──────────┘
```

- **vmm-cluster** is the central authority — owns all state (VMs, storage, users)
- **vmm-server** nodes run as agents — register with the cluster and accept commands
- **vmm-ui** connects to vmm-cluster and automatically adapts to cluster mode

## Installation

### Prerequisites

- Multiple machines or VMs, each running vmm-server
- All nodes must be network-reachable from the cluster server
- KVM support on each node

### Building

```bash
# Build vmm-cluster
cargo build --release -p vmm-cluster

# Build vmm-server (for each node)
cargo build --release -p vmm-server
```

### Configuration

**vmm-cluster** is configured via `vmm-cluster.toml`:

```toml
[server]
bind = "0.0.0.0"
port = 9443

[auth]
jwt_secret = "cluster-secret-change-me"
session_timeout_hours = 24

[storage]
default_pool = "/var/lib/vmm-cluster/images"
iso_pool = "/var/lib/vmm-cluster/isos"

[logging]
level = "info"
```

### Starting the Cluster

```bash
# Start the cluster authority
./target/release/vmm-cluster

# On each node, start vmm-server normally
./target/release/vmm-server
```

## Adding Nodes

### Via Web UI

1. Navigate to **Hosts** in the sidebar
2. Click **Add Host**
3. Enter the node's address (e.g., `http://192.168.1.10:8443`)
4. The cluster will register with the node via the Agent API

### Via API

```bash
curl -X POST http://cluster:9443/api/hosts \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"address":"http://192.168.1.10:8443","name":"node-1"}'
```

## Node Management

### Node States

| State | Description |
|-------|-------------|
| **Online** | Node is healthy and accepting workloads |
| **Offline** | Node is unreachable (heartbeat timeout) |
| **Maintenance** | Node is in maintenance mode — VMs migrated away |

### Heartbeat Monitoring

The cluster polls each node's `/agent/status` endpoint every 10 seconds. If a node fails to respond, it is marked offline and HA procedures may trigger.

### Maintenance Mode

Put a node into maintenance mode to safely perform updates:

```bash
curl -X POST http://cluster:9443/api/hosts/{id}/maintenance \
  -H "Authorization: Bearer <token>"
```

This triggers VM live migration to other healthy nodes before the node is drained.

## VM Management

### Creating VMs

VMs created through the cluster are automatically placed on the optimal node based on available resources.

### VM Migration

Move a running VM from one node to another:

```bash
curl -X POST http://cluster:9443/api/vms/{id}/migrate \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"target_host_id":"<host-id>"}'
```

## Distributed Resource Scheduler (DRS)

DRS automatically balances VM workloads across nodes.

- **Interval:** Runs every 5 minutes
- **Algorithm:** Evaluates CPU and memory utilization across all nodes
- **Actions:** Recommends or automatically migrates VMs to balance load

### DRS Modes

| Mode | Behavior |
|------|----------|
| **Manual** | DRS provides recommendations only |
| **Automatic** | DRS automatically migrates VMs |

## High Availability (HA)

When a node goes offline, the HA engine automatically restarts its VMs on healthy nodes.

- Detects node failure via heartbeat timeout
- Selects target nodes based on available capacity
- Restarts VMs with their saved configuration

## Datastores

Cluster-wide shared storage visible to all nodes.

```bash
# List datastores
curl http://cluster:9443/api/storage/datastores \
  -H "Authorization: Bearer <token>"
```

## Events & Alarms

### Events

All cluster operations are logged as events:

```bash
curl http://cluster:9443/api/events \
  -H "Authorization: Bearer <token>"
```

### Alarms

The alarm system monitors thresholds and triggers notifications:

```bash
curl http://cluster:9443/api/alarms \
  -H "Authorization: Bearer <token>"
```

## Tasks

Long-running operations (migration, provisioning) are tracked as tasks:

```bash
curl http://cluster:9443/api/tasks \
  -H "Authorization: Bearer <token>"
```

## Default Credentials

- **Username:** `admin`
- **Password:** `admin`

Change immediately after first login.
