# CoreSAN Configuration Reference

CoreSAN is configured via a TOML file, passed at startup with `--config <path>` (default: `/etc/vmm/vmm-san.toml`).

All sections and fields are optional — defaults are applied for any missing values.

## Complete Configuration File

```toml
# ── Server ────────────────────────────────────────────────────────
[server]
# IP address to bind the HTTP API server.
# "0.0.0.0" = all interfaces, "127.0.0.1" = localhost only.
bind = "0.0.0.0"

# TCP port for the management API and peer communication.
port = 7443

# ── Data Storage ──────────────────────────────────────────────────
[data]
# Directory for the SQLite database and internal state.
# Created automatically if it doesn't exist.
data_dir = "/var/lib/vmm-san"

# Root directory for FUSE filesystem mounts.
# Each volume gets a subdirectory: <fuse_root>/<volume-name>
fuse_root = "/vmm/san"

# ── Peer Communication ───────────────────────────────────────────
[peer]
# Port for peer-to-peer protocol (currently same as API port).
port = 7444

# Shared secret for peer authentication.
# Must be identical on all nodes in the cluster.
# If empty: auto-generated UUID on first startup (single-node mode).
secret = ""

# ── Network Configuration ────────────────────────────────────────
[network]
# Network interface dedicated to SAN traffic (e.g., "eth1", "ens192").
# Empty = use all interfaces.
san_interface = ""

# Static IP for the SAN interface.
# Empty = use existing IP / DHCP.
san_ip = ""

# Netmask for SAN network (e.g., "255.255.255.0" or "/24").
san_netmask = ""

# Gateway for SAN network.
# Empty = no gateway (direct L2 connectivity assumed).
san_gateway = ""

# MTU for SAN interface.
# 0 = system default (typically 1500).
# 9000 = jumbo frames (recommended for dedicated SAN network).
san_mtu = 0

# ── Replication ──────────────────────────────────────────────────
[replication]
# Replication mode for write operations.
# "async" = write returns immediately, replication happens in background.
# "quorum" = (not yet implemented) write waits for majority ack.
sync_mode = "async"

# ── Benchmark ────────────────────────────────────────────────────
[benchmark]
# Enable automatic network performance testing between peers.
enabled = true

# Interval between benchmark runs (seconds).
interval_secs = 300

# Payload size for bandwidth tests (megabytes).
bandwidth_test_size_mb = 64

# ── Integrity ────────────────────────────────────────────────────
[integrity]
# Enable periodic SHA256 integrity verification of stored data.
enabled = true

# Interval between integrity scans (seconds).
# Default: 3600 (1 hour).
interval_secs = 3600

# Interval for repair attempts of under-replicated data (seconds).
# Only the leader node runs repair operations.
repair_interval_secs = 60

# ── Logging ──────────────────────────────────────────────────────
[logging]
# Log level: "trace", "debug", "info", "warn", "error".
# Can also be set via RUST_LOG environment variable.
level = "info"

# Optional log file path. If unset, logs go to stderr.
# file = "/var/log/vmm-san.log"

# ── Cluster Integration ─────────────────────────────────────────
[cluster]
# URL of vmm-cluster for witness tie-breaking.
# Used when this node loses majority and needs external arbitration.
# Example: "https://10.0.0.1:9443"
# Empty = no witness (pure majority quorum only).
witness_url = ""
```

## Section Details

### [server]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `bind` | String | `0.0.0.0` | Bind address for HTTP server |
| `port` | u16 | `7443` | API port (also used for peer communication) |

**Note:** If `bind` is set to a specific IP (not `0.0.0.0`), that IP is used as this node's advertised address in heartbeats. This is important when nodes are on different networks or behind NAT.

### [data]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `data_dir` | Path | `/var/lib/vmm-san` | Database and state directory |
| `fuse_root` | Path | `/vmm/san` | FUSE mount root |

**Files created in data_dir:**
- `coresan.db` — SQLite database (WAL mode)
- `coresan.db-wal` — Write-ahead log
- `coresan.db-shm` — Shared memory file

### [peer]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | u16 | `7444` | Peer protocol port |
| `secret` | String | `""` (auto-gen) | Shared authentication secret |

**Security considerations:**
- An empty secret means **auto-generation** on first startup. This is fine for single-node but problematic for multi-node (each node generates a different secret).
- For multi-node clusters, always set the secret explicitly and use the same value everywhere.
- The secret is transmitted in the `X-CoreSAN-Secret` HTTP header.

### [network]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `san_interface` | String | `""` | Dedicated NIC for SAN traffic |
| `san_ip` | String | `""` | Static IP for SAN NIC |
| `san_netmask` | String | `""` | Netmask (CIDR or dotted) |
| `san_gateway` | String | `""` | Gateway IP |
| `san_mtu` | u32 | `0` | MTU (0 = system default) |

**Recommendation:** For production, use a dedicated 10 Gbps+ network with jumbo frames (MTU 9000). This isolates SAN traffic from management and VM traffic.

### [replication]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `sync_mode` | String | `async` | Replication timing |

**Modes:**
- `async` — Write returns immediately after local persistence. Peers receive data asynchronously via push replication. Lowest latency, but a node failure immediately after write may lose the latest write on that node.
- `quorum` — (Not yet implemented) Write waits for FTT+1 nodes to acknowledge before returning. Higher latency, stronger durability guarantee.

### [benchmark]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable/disable benchmarks |
| `interval_secs` | u64 | `300` | Seconds between runs |
| `bandwidth_test_size_mb` | u32 | `64` | Payload size in MB |

**Benchmark metrics collected:**
- Bandwidth (MB/s) — throughput between each pair of nodes
- Latency (μs) — average round-trip time (10 pings)
- Jitter (μs) — standard deviation of latency
- Packet Loss (%) — data loss during throughput test

### [integrity]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable integrity checks |
| `interval_secs` | u64 | `3600` | Seconds between scans |
| `repair_interval_secs` | u64 | `60` | Seconds between repair cycles |

**How it works:**
1. The integrity engine reads every local chunk replica from disk
2. Computes SHA256 and compares against stored hash
3. Mismatches or missing files are marked as `error`
4. The repair engine (leader-only) detects under-replicated chunks and restores copies from healthy nodes

### [logging]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `level` | String | `info` | Log verbosity |
| `file` | Option<Path> | None | Optional log file |

**Log levels (from most to least verbose):**
- `trace` — Everything including heartbeat details, leader calc, DB queries
- `debug` — Operational details (file writes, peer fetches, replication events)
- `info` — Key events (node joined, volume created, disk claimed, fenced/recovered)
- `warn` — Problems (witness unreachable, peer offline, integrity failure)
- `error` — Critical (node fenced, database error, backend failure)

**Environment override:** `RUST_LOG=debug vmm-san --config ...` overrides the config file setting.

### [cluster]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `witness_url` | String | `""` | vmm-cluster URL for witness |

**When to use:**
- **2-node clusters**: Always configure witness to prevent dual-fence on partition
- **3+ node clusters**: Optional — majority quorum works without witness
- **Single node**: Not needed (Solo mode)

## Environment Variables

| Variable | Effect |
|----------|--------|
| `RUST_LOG` | Override log level (e.g., `RUST_LOG=debug`) |
| `VMM_SAN_CONFIG` | Alternative to `--config` flag |

## CLI Arguments

```
vmm-san [OPTIONS]

Options:
  --config <PATH>    Path to configuration file [default: /etc/vmm/vmm-san.toml]
  -h, --help         Print help
  -V, --version      Print version
```

## Example Configurations

### Single Node (Development)

```toml
[server]
bind = "127.0.0.1"
port = 7443

[data]
data_dir = "/tmp/vmm-san"
fuse_root = "/tmp/vmm-san/mnt"

[benchmark]
enabled = false

[integrity]
enabled = false

[logging]
level = "debug"
```

### 2-Node Cluster with Witness

```toml
[server]
bind = "0.0.0.0"
port = 7443

[data]
data_dir = "/var/lib/vmm-san"
fuse_root = "/vmm/san"

[peer]
secret = "change-me-to-a-strong-secret"

[network]
san_interface = "eth1"
san_mtu = 9000

[cluster]
witness_url = "https://cluster.example.com:9443"
```

### 3-Node Production Cluster

```toml
[server]
bind = "0.0.0.0"
port = 7443

[data]
data_dir = "/var/lib/vmm-san"
fuse_root = "/vmm/san"

[peer]
secret = "production-secret-32-chars-min!"

[network]
san_interface = "ens192"
san_ip = "10.10.10.1"       # .1, .2, .3 on respective nodes
san_netmask = "255.255.255.0"
san_mtu = 9000

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
file = "/var/log/vmm-san.log"
```
