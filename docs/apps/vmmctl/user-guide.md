# vmmctl — User Guide

vmmctl is the remote management CLI for CoreVM. It communicates with vmm-server and vmm-cluster via REST API over HTTPS, enabling scriptable server administration — similar to `kubectl` for Kubernetes or `govc` for VMware.

## Installation

### Building from Source

```bash
cargo build --release -p vmmctl
```

The binary is output to `target/release/vmmctl`.

### Via Installer

When installing vmm-server with the self-extracting installer, vmmctl is included automatically:

```bash
sudo ./vmm-server-installer.sh --enable-cli-access
```

vmmctl is installed to `/opt/vmm-server/vmmctl` with a symlink at `/usr/local/bin/vmmctl`.

### Via ISO

When deploying from the CoreVM appliance ISO, vmmctl is pre-installed at `/usr/local/bin/vmmctl`. During the installer wizard, the "API Access" screen lets you enable or disable CLI access.

## Quick Start

```bash
# 1. Configure the server connection
vmmctl config set-server https://192.168.1.100:8443 --insecure

# 2. Log in
vmmctl login
# Username: admin
# Password: ****

# 3. List VMs
vmmctl vm list

# 4. Create and start a VM
vmmctl vm create --name webserver --ram 4096 --cpus 4 --iso debian.iso
vmmctl vm start webserver
```

## Configuration

### Server Contexts

vmmctl supports managing multiple servers via named contexts (like kubectl):

```bash
# Add a server (first server becomes default)
vmmctl config set-server https://10.0.0.5:8443 --insecure

# Add another server with a custom name
vmmctl config set-server https://cluster.prod:9443 --name production

# Switch between contexts
vmmctl config use-context production

# List all contexts (* = active)
vmmctl config list-contexts

# Show current context details
vmmctl config current
```

### Config File

The configuration is stored at `~/.vmmctl/config.toml`:

```toml
current_context = "default"

[[contexts]]
name = "default"
server = "https://192.168.1.100:8443"
insecure = true

[[contexts]]
name = "production"
server = "https://cluster.prod:9443"
insecure = false
```

### Token Storage

JWT tokens are stored per context in `~/.vmmctl/tokens/<context-name>` with `0600` permissions. You can also pass a token via the `VMMCTL_TOKEN` environment variable.

## Authentication

### Interactive Login

```bash
vmmctl login
# Username: admin
# Password: ****
# Logged in as admin (role: admin)
```

### Non-Interactive Login (for Scripts)

```bash
echo "mypassword" | vmmctl login --username admin --password-stdin
```

### Environment Variable

```bash
export VMMCTL_TOKEN=eyJhbGciOi...
vmmctl vm list
```

### Check Auth Status

```bash
vmmctl auth
# Context:              default
# Server:               https://192.168.1.100:8443
# Username:             admin
# Role:                 admin
# Token expires:        2026-03-26 14:30:00 UTC
# Status:               Active
```

## Command Reference

### Global Options

| Option | Description |
|--------|-------------|
| `-o, --output <format>` | Output format: `table` (default), `json`, `wide` |
| `--insecure` | Accept self-signed TLS certificates |
| `--no-header` | Suppress table headers |
| `-s, --server <url>` | Override server URL for this command |

### VM Management

```bash
# List all VMs
vmmctl vm list
vmmctl vm list -o json          # JSON output for scripting

# Show VM details
vmmctl vm info <id>

# Create a VM
vmmctl vm create \
  --name myvm \
  --ram 2048 \
  --cpus 2 \
  --disk /path/to/disk.img \
  --iso /path/to/installer.iso \
  --os ubuntu \
  --bios seabios \
  --net usermode

# Lifecycle
vmmctl vm start <id|name>
vmmctl vm stop <id|name>        # Graceful shutdown (ACPI)
vmmctl vm force-stop <id|name>  # Immediate termination
vmmctl vm delete <id|name>

# Screenshot
vmmctl vm screenshot <id> -o screen.png
```

#### VM Create Options

| Option | Default | Description |
|--------|---------|-------------|
| `--name` | (required) | VM display name |
| `--ram` | `2048` | RAM in MB |
| `--cpus` | `2` | CPU core count |
| `--disk` | | Disk image path |
| `--iso` | | ISO image path |
| `--os` | `other` | Guest OS type (ubuntu, debian, win10, etc.) |
| `--bios` | `seabios` | BIOS type: `seabios`, `uefi`, `corevm` |
| `--net` | `usermode` | Network mode: `usermode`, `bridge`, `disconnected` |

### System

```bash
vmmctl system info              # Server version, hostname, CPU, RAM, disk
vmmctl system stats             # Dashboard stats (VM count, resource usage)
vmmctl system activity          # Recent audit log entries
vmmctl system activity --limit 50
```

### Storage

```bash
# Storage pools
vmmctl storage pool list
vmmctl storage pool create --name pool1 --path /data/pool1
vmmctl storage pool delete <id>

# Disk images
vmmctl storage disk list
vmmctl storage disk create --name disk1.raw --size-gb 50 --pool-id 1
vmmctl storage disk resize <id> --size-gb 100
vmmctl storage disk delete <id>

# ISO images
vmmctl storage iso list
vmmctl storage iso upload ./ubuntu-22.04.iso
vmmctl storage iso delete <id>

# Aggregate stats
vmmctl storage stats
```

### Network

```bash
# Host interfaces
vmmctl network list

# Network stats
vmmctl network stats

# Bridge management
vmmctl network bridge list
vmmctl network bridge create --name br0
vmmctl network bridge delete br0
```

### Users (Admin Only)

```bash
# List users
vmmctl user list

# Create user
vmmctl user create --username operator1 --role operator
vmmctl user create --username viewer1 --role viewer --password "secret"

# Delete user
vmmctl user delete <id>

# Change password
vmmctl user password <id>
```

#### Roles

| Role | Permissions |
|------|-------------|
| `admin` | Full access: users, settings, VMs, storage, network |
| `operator` | VM lifecycle, storage, network (no user/settings management) |
| `viewer` | Read-only access |

### Cluster (vmm-cluster only)

```bash
# Host management
vmmctl cluster host list
vmmctl cluster host add https://10.0.0.10:8443 --name node01
vmmctl cluster host remove <id>
vmmctl cluster host maintenance <id> --enable

# DRS
vmmctl cluster drs status

# Live migration
vmmctl cluster migrate --vm <vm-id> --to <host-id>
```

### API Access Control

```bash
# Check CLI/API access status
vmmctl api-access status

# Enable/disable (admin only)
vmmctl api-access enable
vmmctl api-access disable
```

## Output Formats

All list commands support multiple output formats:

```bash
# Default: human-readable table
vmmctl vm list

# JSON (for jq, scripts, piping)
vmmctl vm list -o json

# Table without headers (for grep/awk)
vmmctl vm list --no-header
```

### Scripting Examples

```bash
# Get the ID of a specific VM
VM_ID=$(vmmctl vm list -o json | jq -r '.[] | select(.name=="webserver") | .id')

# Start all stopped VMs
vmmctl vm list -o json | jq -r '.[] | select(.state=="stopped") | .id' | while read id; do
  vmmctl vm start "$id"
done

# Monitor system stats
watch -n 5 vmmctl system stats

# Automated server setup script
#!/bin/bash
SERVER="https://10.0.0.5:8443"
vmmctl config set-server "$SERVER" --insecure
echo "admin" | vmmctl login --username admin --password-stdin
vmmctl user create --username operator --role operator --password "changeme"
vmmctl storage pool create --name production --path /data/vms
vmmctl vm create --name web01 --ram 4096 --cpus 4 --iso debian.iso
vmmctl vm start web01
```

## TLS / Security

### Self-Signed Certificates

When the server uses a self-signed certificate (default for ISO installations), use `--insecure` or configure it per context:

```bash
vmmctl config set-server https://10.0.0.5:8443 --insecure
```

### API Access Control

The server can restrict CLI/API access via the `[api]` section in `vmm-server.toml`:

```toml
[api]
cli_access_enabled = true      # Set to false to block CLI access
allowed_ips = []               # IP whitelist (empty = all allowed)
```

This can be configured:
- During ISO installation (API Access screen in the installer wizard)
- Via the DCUI console (F9 — API Access)
- Via the web UI (Settings > API Access)
- Via the CLI itself: `vmmctl api-access enable/disable`

## Troubleshooting

### Connection Failed

```
Error: Connection failed — is the server running?
```

- Verify the server is running: `systemctl status vmm-server`
- Check the URL: `vmmctl config current`
- Check firewall: port 8443 must be open

### Unauthorized

```
Error: Unauthorized — run 'vmmctl login' first
```

- Token may be expired. Run `vmmctl login` again
- Check auth status: `vmmctl auth`

### CLI Access Disabled

```
Error: Access denied: cli_access_disabled
```

- CLI/API access is disabled on the server
- Enable via web UI or edit `vmm-server.toml`: set `cli_access_enabled = true`
- On the appliance, use the DCUI console (F9) to enable it
