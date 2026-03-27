# CoreVM Installation Guide

Self-extracting Linux installers for **vmm-server** and **vmm-cluster**.

## Supported Platforms

| Platform | Init System | Support |
|----------|------------|---------|
| Ubuntu / Debian / Fedora / RHEL / Arch | systemd | Full (systemd service unit) |
| Alpine Linux | OpenRC | Full (OpenRC init script) |
| Void Linux | runit | Full (runit service directory) |
| Older distros (Debian 7, CentOS 6) | SysVinit | Full (LSB init script) |
| WSL2 with systemd | systemd | Full (systemd + manual fallback) |
| WSL2 without systemd | — | Full (manual start/stop script + autostart via wsl.conf) |
| Any other Linux | — | Fallback (manual start/stop script) |

The installer automatically detects the init system and installs the appropriate service configuration.

## Building the Installers

Prerequisites: Rust toolchain, Node.js + npm.

```bash
# Build both installers
./tools/build-installers.sh

# Build only vmm-server installer
./tools/build-installers.sh --server

# Build only vmm-cluster installer
./tools/build-installers.sh --cluster
```

Output in `dist/`:
- `vmm-server-installer.sh` — standalone VM management server
- `vmm-cluster-installer.sh` — cluster orchestration server (includes Web UI)

## Installing

Copy the installer to the target machine and run with root privileges:

```bash
sudo ./vmm-server-installer.sh
# or
sudo ./vmm-cluster-installer.sh
```

### Installer Options

| Flag | Description |
|------|-------------|
| `--enable-cli-access` | Enable CLI/API access for remote management via vmmctl |
| `--enable-tls` | Generate a self-signed TLS certificate for HTTPS |
| `--uninstall` | Remove the installation |

If no flags are provided, the installer prompts interactively whether to enable CLI access.

```bash
# Install with CLI access and TLS enabled
sudo ./vmm-server-installer.sh --enable-cli-access --enable-tls
```

The installer will:
1. Detect the platform (native Linux / WSL2) and init system
2. Extract and install vmm-server and vmmctl to `/opt/vmm-server/`
3. Install BIOS assets (vmm-server only) and the Web UI
4. Generate a TLS certificate (if `--enable-tls`)
5. Create data directories and a config file with a random JWT secret
6. Install the appropriate service configuration for the detected init system

### Remote Management with vmmctl

After installation with `--enable-cli-access`, connect from a remote machine:

```bash
vmmctl config set-server https://<SERVER_IP>:8443 --insecure
vmmctl login    # admin / admin
vmmctl vm list
```

See the [vmmctl User Guide](apps/vmmctl/user-guide.md) for full CLI documentation.

### Starting the Service

The installer prints the correct commands at the end. Here is a summary:

**systemd** (Ubuntu, Debian, Fedora, Arch, ...):
```bash
sudo systemctl enable --now vmm-server    # or vmm-cluster
sudo systemctl status vmm-server
sudo journalctl -u vmm-server -f
```

**OpenRC** (Alpine):
```bash
sudo rc-update add vmm-server default
sudo rc-service vmm-server start
```

**SysVinit**:
```bash
sudo /etc/init.d/vmm-server start
```

**runit** (Void Linux):
```bash
sudo sv start vmm-server
```

**Fallback** (no init system detected):
```bash
sudo /opt/vmm-server/run.sh start
sudo /opt/vmm-server/run.sh status
sudo /opt/vmm-server/run.sh stop
```

## Uninstalling

```bash
sudo ./vmm-server-installer.sh --uninstall
# or
sudo ./vmm-cluster-installer.sh --uninstall
```

This removes the binary and service files but preserves config (`/etc/vmm/`) and data (`/var/lib/vmm/` or `/var/lib/vmm-cluster/`).

## File Locations

| What | vmm-server | vmm-cluster |
|------|-----------|-------------|
| Binary | `/opt/vmm-server/vmm-server` | `/opt/vmm-cluster/vmm-cluster` |
| CLI tool | `/opt/vmm-server/vmmctl` (+ `/usr/local/bin/vmmctl`) | — |
| Web UI | `/opt/vmm-server/ui/` | `/opt/vmm-cluster/ui/` |
| BIOS assets | `/opt/vmm-server/assets/bios/` | — |
| Config | `/etc/vmm/vmm-server.toml` | `/etc/vmm/vmm-cluster.toml` |
| Data | `/var/lib/vmm/` | `/var/lib/vmm-cluster/` |
| Logs | `/var/log/vmm-server.log` | `/var/log/vmm-cluster.log` |

## Default Ports

| Service | Port |
|---------|------|
| vmm-server | 8443 |
| vmm-cluster | 9443 |

Default login: **admin / admin** (change after first login).

---

## WSL2 Setup

Both vmm-server and vmm-cluster run on WSL2. The installer detects WSL2 automatically and adapts accordingly.

### WSL2 with systemd (recommended)

Modern WSL2 distributions (Ubuntu 22.04+) support systemd natively. To enable it, add to `/etc/wsl.conf` inside WSL:

```ini
[boot]
systemd=true
```

Then restart WSL from PowerShell:
```powershell
wsl --shutdown
```

After that, the installer will detect systemd and install a regular systemd service unit. It also installs a `run.sh` fallback script for convenience.

### WSL2 without systemd

If systemd is not available, the installer creates a manual start/stop script:

```bash
sudo /opt/vmm-server/run.sh start
sudo /opt/vmm-server/run.sh status
sudo /opt/vmm-server/run.sh stop
```

#### Autostart on WSL launch

Add to `/etc/wsl.conf`:

```ini
[boot]
command = /opt/vmm-server/run.sh start
```

This runs the start command every time the WSL instance boots (requires WSL 0.67.6+).

For vmm-cluster, use `/opt/vmm-cluster/run.sh start` instead. To autostart both:

```ini
[boot]
command = /opt/vmm-server/run.sh start && /opt/vmm-cluster/run.sh start
```

### Accessing the Web UI from Windows

#### Mirrored Networking (recommended, WSL 2.0+)

With mirrored networking, WSL2 shares the host's network stack. Access the UI directly:
- vmm-server: `http://localhost:8443`
- vmm-cluster: `http://localhost:9443`

To enable, create or edit `%USERPROFILE%\.wslconfig` on Windows:

```ini
[wsl2]
networkingMode=mirrored
```

Then restart WSL:
```powershell
wsl --shutdown
```

#### Classic NAT Networking

With the default NAT networking, WSL2 has its own IP address. Find it with:

```bash
hostname -I
```

Then access from Windows using that IP:
- vmm-server: `http://<WSL_IP>:8443`
- vmm-cluster: `http://<WSL_IP>:9443`

Note: The WSL2 IP address may change after restarts. Mirrored networking avoids this issue.

#### Port Forwarding (alternative for NAT mode)

If you need a stable `localhost` address with NAT networking, set up port forwarding from PowerShell (as Administrator):

```powershell
# Get the WSL IP
$wslIp = (wsl hostname -I).Trim().Split(" ")[0]

# Forward ports
netsh interface portproxy add v4tov4 listenport=8443 listenaddress=0.0.0.0 connectport=8443 connectaddress=$wslIp
netsh interface portproxy add v4tov4 listenport=9443 listenaddress=0.0.0.0 connectport=9443 connectaddress=$wslIp
```

To remove the forwarding:
```powershell
netsh interface portproxy delete v4tov4 listenport=8443 listenaddress=0.0.0.0
netsh interface portproxy delete v4tov4 listenport=9443 listenaddress=0.0.0.0
```

### Limitations on WSL2

- **No KVM**: WSL2 does not support nested virtualization by default. VM execution uses software emulation (slower). To enable nested virtualization (Windows 11 + Intel/AMD with VT-x/AMD-V):
  ```ini
  # %USERPROFILE%\.wslconfig
  [wsl2]
  nestedVirtualization=true
  ```
- **Storage**: Use the Linux filesystem (`/var/lib/vmm/`) for VM data — not `/mnt/c/`. The Windows filesystem (9P mount) is significantly slower.
- **Memory**: WSL2 limits memory by default. Adjust in `.wslconfig` if needed:
  ```ini
  [wsl2]
  memory=8GB
  ```
