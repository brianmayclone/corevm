# CoreSAN Installation & Setup

## Prerequisites

### Hardware Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | 2 cores | 4+ cores |
| RAM | 2 GB | 8+ GB |
| System Disk | 20 GB | 50 GB (OS + CoreSAN binary + DB) |
| Data Disks | 1 × any size | 2+ disks per node, NVMe/SSD preferred |
| Network | 1 Gbps | 10 Gbps dedicated SAN network, jumbo frames |
| Nodes | 1 (Solo mode) | 3 nodes (full quorum + witness) |

### Node Count Requirements by FTT

| FTT | Min Nodes | Survives | Effective Capacity |
|-----|-----------|----------|-------------------|
| 0 | 1 | 0 failures | 100% of raw |
| 1 | 2 (3 recommended) | 1 node failure | 50% of raw |
| 2 | 3 (5 recommended) | 2 node failures | 33% of raw |

With 2 nodes and FTT=1, a witness (vmm-cluster) is strongly recommended to avoid both nodes being fenced on network partition.

### Software Requirements

- Linux kernel 5.x+ (block device management, FUSE support)
- `lsblk`, `blkid`, `mkfs.ext4`, `mount`, `umount` commands
- `smartmontools` (`smartctl`) for S.M.A.R.T. disk health monitoring
- `libfuse3` (for FUSE filesystem mounts)
- Rust toolchain (for building from source)

### Network Requirements

- All nodes must be able to reach each other on port 7443 (API) and 7444 (peer)
- If using a dedicated SAN network: configure `[network]` section in config
- UDP broadcast capability for auto-discovery (optional)
- If using vmm-cluster witness: nodes must reach cluster on its HTTPS port

## Building from Source

```bash
# Clone the repository
git clone <repo-url> corevm
cd corevm

# Build CoreSAN
cargo build -p vmm-san --release

# Binary location
ls target/release/vmm-san
```

## Installation

### Single Binary Deployment

```bash
# Copy binary to target node
scp target/release/vmm-san root@node1:/usr/local/bin/

# Create config directory
ssh root@node1 "mkdir -p /etc/vmm"

# Create data directory
ssh root@node1 "mkdir -p /var/lib/vmm-san"

# Create FUSE mount directory
ssh root@node1 "mkdir -p /vmm/san"
```

### Configuration File

Create `/etc/vmm/vmm-san.toml` on each node:

```toml
[server]
bind = "0.0.0.0"
port = 7443

[data]
data_dir = "/var/lib/vmm-san"
fuse_root = "/vmm/san"

[peer]
port = 7444
secret = "my-shared-secret-change-me"   # Same on all nodes!

[network]
san_interface = ""      # Empty = all interfaces
san_ip = ""             # Empty = use existing IP
san_mtu = 9000          # Jumbo frames (optional)

[replication]
sync_mode = "async"

[benchmark]
enabled = true
interval_secs = 300
bandwidth_test_size_mb = 64

[integrity]
enabled = true
interval_secs = 3600
repair_interval_secs = 60

[logging]
level = "info"

[cluster]
witness_url = ""        # Set to vmm-cluster URL for 2-node witness
```

**Important:** The `[peer] secret` must be identical on all nodes in the cluster.

### Systemd Service (Optional)

Create `/etc/systemd/system/vmm-san.service`:

```ini
[Unit]
Description=CoreSAN Distributed Storage
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/vmm-san --config /etc/vmm/vmm-san.toml
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload
systemctl enable vmm-san
systemctl start vmm-san
```

## Initial Setup

### Step 1: Start Nodes

Start `vmm-san` on all nodes. On first startup, each node:
1. Creates the data directory if it doesn't exist
2. Initializes the SQLite database with the full schema
3. Generates a unique node ID (UUID) and persists it
4. Auto-generates a peer secret if none is configured
5. Runs the sanitize engine (verifies all local chunk replicas; node stays in `Sanitizing` state until complete)
6. Starts all background engines

```bash
# Node 1
vmm-san --config /etc/vmm/vmm-san.toml

# Node 2 (on another machine)
vmm-san --config /etc/vmm/vmm-san.toml
```

Verify each node is running:

```bash
curl -k https://localhost:7443/api/health
# {"status":"ok"}

curl -k https://localhost:7443/api/status
# Returns full node status with node_id, hostname, uptime, etc.
```

### Step 2: Register Peers

Nodes must be introduced to each other. This can happen in three ways:

**A) Manual peer registration:**

```bash
# On Node 1, register Node 2
curl -k -X POST https://node1:7443/api/peers/join \
  -H 'Content-Type: application/json' \
  -H 'X-CoreSAN-Secret: my-shared-secret-change-me' \
  -d '{
    "address": "https://node2:7443",
    "node_id": "<node2-id>",
    "hostname": "node2",
    "peer_port": 7444,
    "secret": "my-shared-secret-change-me"
  }'

# On Node 2, register Node 1 (bidirectional)
curl -k -X POST https://node2:7443/api/peers/join \
  -H 'Content-Type: application/json' \
  -H 'X-CoreSAN-Secret: my-shared-secret-change-me' \
  -d '{
    "address": "https://node1:7443",
    "node_id": "<node1-id>",
    "hostname": "node1",
    "peer_port": 7444,
    "secret": "my-shared-secret-change-me"
  }'
```

**B) Auto-discovery via UDP beacon:**

If nodes are on the same L2 network, the discovery engine sends UDP broadcasts every 10 seconds. Nodes automatically detect each other.

**C) Auto-registration via vmm-cluster (recommended):**

When CoreSAN nodes are managed by vmm-cluster, peer registration is fully automatic. The cluster's heartbeat engine detects new SAN-enabled hosts and registers all peers bidirectionally. No manual intervention required.

### Step 3: Claim Disks

Identify available disks on each node:

```bash
curl -k https://node1:7443/api/disks
# Returns list of discovered block devices with status
```

Claim empty disks:

```bash
curl -k -X POST https://node1:7443/api/disks/claim \
  -H 'Content-Type: application/json' \
  -d '{"device_path": "/dev/sdb"}'
```

This will:
1. Partition the disk (8 GiB root partition)
2. Format with ext4
3. Mount at `/vmm/san-disks/<uuid>`
4. Register as a backend

For disks with existing data, add `"confirm_format": true` to acknowledge data destruction.

### Step 4: Create a Volume

```bash
curl -k -X POST https://node1:7443/api/volumes \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "production-data",
    "ftt": 1,
    "local_raid": "stripe"
  }'
```

The volume is automatically synced to all peers. Backends on each node are auto-assigned based on available claimed disks.

### Step 5: Configure Witness (2-Node Clusters)

If running exactly 2 SAN nodes, configure the witness URL pointing to your vmm-cluster instance:

```toml
[cluster]
witness_url = "https://cluster-ip:9443"
```

Without a witness, a network partition between 2 nodes will fence **both** nodes (neither has majority).

### Step 6: Verify Cluster Health

```bash
# Check peer status
curl -k https://node1:7443/api/peers
# All peers should show status "online"

# Check quorum
curl -k https://node1:7443/api/status
# quorum_status should be "active"

# Check volume health
curl -k https://node1:7443/api/volumes
# Volumes should show status "online"

# Run a benchmark
curl -k -X POST https://node1:7443/api/benchmark/run
# Wait ~30s, then check results:
curl -k https://node1:7443/api/benchmark/matrix
```

## Upgrading

1. Build the new binary
2. Stop the service on each node (rolling upgrade)
3. Replace the binary
4. Start the service

CoreSAN handles database migrations automatically on startup. The schema is forward-compatible — new columns are added with defaults.

```bash
# Rolling upgrade (one node at a time)
systemctl stop vmm-san
cp vmm-san-new /usr/local/bin/vmm-san
systemctl start vmm-san
# Wait for node to rejoin cluster, verify health, then proceed to next node
```
