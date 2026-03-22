# vmm-server — User Guide

vmm-server is the web backend for CoreVM. It provides a REST API and WebSocket server for managing virtual machines, storage, networking, and users.

## Installation

### Prerequisites

- **Linux** with KVM support (`/dev/kvm` accessible)
- **Rust stable toolchain** (`rustup install stable`)
- **Node.js 18+** (for building vmm-ui)

### Building

```bash
# Build vmm-server + vmm-ui together
./tools/build-vmm.sh

# Build only vmm-server
cargo build --release -p vmm-server

# Build and run immediately
./tools/build-vmm.sh --run
```

### Running

```bash
# Start the server
./target/release/vmm-server

# Or with custom config
./target/release/vmm-server --config /path/to/vmm-server.toml
```

The server starts on `http://localhost:8443` by default.

## Configuration

vmm-server is configured via `vmm-server.toml` in the project root.

```toml
[server]
bind = "0.0.0.0"           # Listen address
port = 8443                 # Listen port

[auth]
jwt_secret = "your-secret"  # JWT signing secret (change in production!)
session_timeout_hours = 24   # Session expiry

[storage]
default_pool = "/var/lib/vmm/images"  # Default disk image storage
iso_pool = "/var/lib/vmm/isos"        # ISO file storage

[vms]
config_dir = "/var/lib/vmm/vms"       # VM configuration directory

[logging]
level = "info"              # Log level: trace, debug, info, warn, error
```

## Default Credentials

- **Username:** `admin`
- **Password:** `admin`

Change the password immediately after first login.

## Features

### VM Management

- **Create VMs** with custom CPU, RAM, disk, network, and BIOS settings
- **Start/Stop/Force-Stop** VMs via API or Web UI
- **Live Console** — VGA framebuffer streaming over WebSocket with keyboard and mouse
- **Screenshots** — capture the current framebuffer as JPEG

### Storage Management

- **Storage Pools** — organize disk images and ISOs in named pools
- **Disk Images** — create, resize, and delete raw disk images
- **ISO Upload** — upload ISO files for VM installation

### Networking

- **Interface Overview** — list host network interfaces
- **Traffic Statistics** — monitor network I/O

### User & Access Management

- **JWT Authentication** — stateless token-based auth
- **Role-based Access** — admin and user roles
- **User Management** — create, update, delete users (admin only)
- **Resource Groups** — organize VMs into permission-scoped groups
- **Audit Logging** — all operations are logged with timestamps

### Settings

- **Server Configuration** — bind address, port, storage paths
- **Timezone** — configurable timezone for logs and UI
- **Security Policies** — session timeout, password requirements

### Database

vmm-server uses SQLite for persistent storage:
- VM configurations and state
- User accounts and groups
- Audit log entries
- Storage pool definitions

Auto-backup runs every 30 minutes, retaining the last 10 backups.

## REST API

See the [REST API section in the main README](../../README.md#rest-api) for the complete endpoint reference.

### Quick Reference

```bash
# Login
curl -X POST http://localhost:8443/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"admin"}'

# List VMs (with token)
curl http://localhost:8443/api/vms \
  -H "Authorization: Bearer <token>"

# Create a VM
curl -X POST http://localhost:8443/api/vms \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"name":"my-vm","ram_mb":512,"cpus":1,"bios":"seabios"}'
```

### WebSocket Console

Connect to `ws://localhost:8443/ws/console/{vm_id}` with a valid JWT token to get live VGA framebuffer frames and send keyboard/mouse input.

## Cluster Agent Mode

vmm-server can operate as a managed agent in a vmm-cluster deployment. When registered with a cluster, it enters **managed mode**:

### Managed Mode Enforcement

In managed mode, the regular REST API is blocked to prevent out-of-band changes:
- Direct VM creation, deletion, and management return `403 managed_by_cluster`
- The error response includes the cluster URL for redirection to the correct management interface
- Only these endpoints remain accessible:
  - `/agent/*` — agent API for cluster communication
  - `/ws/*` — WebSocket connections (console)
  - `/api/system/info` — system information (shows managed status)
  - `/api/auth/login` — allows UI to display managed-by-cluster message

### Agent Capabilities

The cluster communicates with managed nodes via the `/agent/*` endpoints:

- **Health monitoring** — heartbeat polling with CPU, RAM, VM, and datastore status
- **VM lifecycle** — provision, start, stop, force-stop, and destroy VMs remotely
- **Storage management** — mount/unmount datastores, create disks
- **Direct migration** — send/receive VM disk data directly between nodes
- **Package management** — check and install packages (for Storage Wizard)
- **Command execution** — execute shell commands with optional sudo (for filesystem setup)

See the [vmm-cluster User Guide](../vmm-cluster/user-guide.md) for details.
