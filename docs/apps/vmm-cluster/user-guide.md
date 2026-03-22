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

VMs created through the cluster are automatically placed on the optimal node based on available resources. The scheduler selects the node with the most available CPU and memory.

In cluster mode, VMs can be assigned to SDN networks for automatic DHCP/DNS provisioning.

### VM Migration

Move a VM from one node to another using direct host-to-host transfer:

```bash
curl -X POST http://cluster:9443/api/vms/{id}/migrate \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"target_host_id":"<host-id>"}'
```

**How migration works:**
1. Cluster generates a one-time migration token (UUID, 5-minute expiry)
2. Source host stops the VM and streams disk data directly to target host
3. Target host receives disks, provisions VM config, and starts the VM
4. If both hosts share a datastore, disk copy is skipped (shared storage mode)

Migration progress is tracked as a task — monitor via `GET /api/tasks`.

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

### DRS Exclusions

Exclude specific VMs or resource groups from DRS rebalancing:

```bash
# Exclude a VM
curl -X POST http://cluster:9443/api/cluster/drs-exclusions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"type":"vm","target_id":"<vm-id>"}'

# Exclude a resource group
curl -X POST http://cluster:9443/api/cluster/drs-exclusions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"type":"resource_group","target_id":"<group-id>"}'
```

## High Availability (HA)

When a node goes offline, the HA engine automatically restarts its VMs on healthy nodes.

- Detects node failure via heartbeat timeout
- Selects target nodes based on available capacity
- Restarts VMs with their saved configuration

### State Reconciler

When a node reconnects after being offline, the reconciler prevents split-brain:

- If a VM was moved to another host by HA while the node was down, the reconciler **stops** the stale copy on the reconnected node
- Orphaned VMs (running on the node but missing from cluster DB) are stopped
- This prevents the same VM from running on two hosts simultaneously

## SDN (Software-Defined Networking)

Cluster-wide virtual networks with integrated DHCP, DNS, and PXE boot services.

### Creating a Network

```bash
curl -X POST http://cluster:9443/api/networks \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "production",
    "cidr": "10.10.0.0/24",
    "gateway": "10.10.0.1",
    "vlan_id": 100,
    "dhcp_enabled": true,
    "dhcp_range_start": "10.10.0.100",
    "dhcp_range_end": "10.10.0.200",
    "dns_domain": "prod.local",
    "dns_upstream": ["8.8.8.8", "8.8.4.4"]
  }'
```

### DHCP Management

- Automatic IP assignment from the configured range
- Static DHCP reservations (MAC-to-IP mapping with permanent lease):

```bash
# View network details including leases
curl http://cluster:9443/api/networks/{id} \
  -H "Authorization: Bearer <token>"
```

### DNS Management

- Automatic DNS A-record registration when VMs start (vm-name.domain → IP)
- Manual DNS A records and CNAME records
- Upstream DNS server forwarding

### PXE Boot

Per-network PXE configuration for network boot:
- Boot file path, TFTP root directory, next-server address
- DHCP options 66/67 automatically configured

### Configuration Generation

The SDN engine generates `dnsmasq` configuration files for each network at `/etc/vmm/dnsmasq-net-{network_id}.conf`, applied automatically to all cluster hosts.

### Input Validation

All network configuration is validated:
- CIDR format (e.g., `10.0.0.0/24`)
- IP addresses within subnet range
- VLAN IDs (1–4094)
- DHCP range (start < end, no overlap with gateway)
- MAC address format

## Storage Wizard

Guided setup for cluster-wide shared filesystems. Accessible via **Storage** → **Setup Wizard** in the UI.

### Supported Filesystems

| Filesystem | Description |
|-----------|-------------|
| **NFS** | Network File System — simple shared storage |
| **GlusterFS** | Distributed replicated filesystem |
| **CephFS** | Distributed storage with high availability |

### Setup Steps

1. **Choose filesystem type** and target cluster
2. **Check packages** — detects if required packages are installed on each host
3. **Install packages** — installs missing packages (apt/dnf/yum with optional sudo)
4. **Configure & mount** — creates volumes, mounts on all hosts, registers datastore

```bash
# Step 1: Check package status
curl -X POST http://cluster:9443/api/storage/wizard/check \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"filesystem":"nfs","host_ids":["host-1","host-2"]}'

# Step 2: Install missing packages
curl -X POST http://cluster:9443/api/storage/wizard/install \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"filesystem":"nfs","host_ids":["host-1"],"sudo_passwords":{"host-1":"pass"}}'

# Step 3: Setup filesystem
curl -X POST http://cluster:9443/api/storage/wizard/setup \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"filesystem":"nfs","nfs_server":"192.168.1.1","nfs_path":"/exports/vms"}'
```

## Notifications

Configure alert channels and rules for cluster events.

### Channels

| Type | Description |
|------|-------------|
| **Email** | SMTP with optional TLS, PLAIN auth |
| **Webhook** | HTTP POST with JSON payload, optional HMAC signature |
| **Log** | Writes to stdout / logging system |

### Rules

Rules connect events to channels:
- Filter by **category** (vm, host, datastore, migration, etc.)
- Minimum **severity** threshold (info, warning, error)
- Per-rule **cooldown** to prevent notification spam

```bash
# Create a webhook channel
curl -X POST http://cluster:9443/api/notifications/channels \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"type":"webhook","name":"Slack","webhook_url":"https://hooks.slack.com/..."}'

# Test it
curl -X POST http://cluster:9443/api/notifications/channels/{id}/test \
  -H "Authorization: Bearer <token>"

# Create a rule
curl -X POST http://cluster:9443/api/notifications/rules \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"channel_id":"<id>","category":"host","min_severity":"warning"}'
```

## LDAP / Active Directory

External authentication via LDAP or Active Directory.

### Configuration

```bash
curl -X PUT http://cluster:9443/api/cluster/ldap \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "server_url": "ldaps://ad.example.com:636",
    "bind_dn": "CN=svc-corevm,OU=Service,DC=example,DC=com",
    "bind_password": "secret",
    "base_dn": "DC=example,DC=com",
    "user_search_dn": "OU=Users,DC=example,DC=com",
    "user_filter": "(sAMAccountName={username})",
    "group_search_dn": "OU=Groups,DC=example,DC=com",
    "tls_enabled": true
  }'
```

### Group-to-Role Mapping

Map AD/LDAP groups to CoreVM roles (admin, operator, viewer). When a user authenticates via LDAP, their group membership determines their cluster role.

### Test Connection

```bash
curl -X POST http://cluster:9443/api/cluster/ldap/test \
  -H "Authorization: Bearer <token>"
```

## Managed Mode Enforcement

When a vmm-server node is managed by the cluster, its regular API is blocked:
- Direct VM creation, deletion, and management are rejected
- The error response includes the cluster URL for redirection
- Only agent endpoints (`/agent/*`), WebSocket (`/ws/*`), and system info remain accessible
- This prevents out-of-band changes that could desync cluster state

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
