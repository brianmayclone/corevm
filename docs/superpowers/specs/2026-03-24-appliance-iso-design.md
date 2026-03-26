# CoreVM Appliance ISO — Design Spec

## Overview

CoreVM as a turnkey appliance: a custom Debian 12 (Bookworm) based ISO for 64-bit Intel systems that boots into a text-based installer wizard, then permanently runs a VMware ESXi-style DCUI (Direct Console User Interface). No desktop, no shell login — just the DCUI.

## Goals

- Single ISO image that installs CoreVM as a ready-to-use hypervisor appliance
- VMware ESXi-like experience: text console installer → permanent DCUI
- User chooses role at install time: **Standalone Server** (vmm-server) or **Cluster Controller** (vmm-cluster)
- Minimal Debian base — only what's needed to run CoreVM
- Hybrid UEFI + Legacy BIOS boot support
- Offline-capable update mechanism via update packages

## Architecture

### Components

```
ISO
├── Bootloader (GRUB EFI + isolinux BIOS — hybrid)
├── Squashfs image of minimal Debian 12 root-FS (via debootstrap)
├── Kernel + initramfs (with live-boot support for squashfs mount)
├── vmm-appliance binary (Rust/ratatui — installer + DCUI)
├── vmm-server binary + assets (BIOS files, Web-UI dist)
└── vmm-cluster binary + assets
```

### Boot Flow

1. ISO boot → GRUB (UEFI) or isolinux (BIOS) → ISO kernel + ISO initramfs
2. initramfs uses `live-boot` to mount squashfs as root-FS into RAM (tmpfs overlay for writability)
3. Live environment starts → `vmm-appliance --mode installer` runs automatically via systemd
4. Installer: disk selection → Debian install to disk → CoreVM setup wizard
5. Reboot from disk → GRUB on disk (hidden menu, `GRUB_TIMEOUT_STYLE=hidden`, `GRUB_TIMEOUT=2`, press Shift/Esc to access)
6. Installed system boots → `vmm-appliance --mode dcui` runs as systemd service on tty1

## Installer Wizard

Single Rust binary (`vmm-appliance --mode installer`), linear screen flow:

### Screen 1 — Welcome
- CoreVM logo/banner
- Role selection: `Standalone Server` or `Cluster Controller`
- Language selection (German/English) — applies to installer UI and DCUI, sets system locale accordingly

### Screen 2 — Disk Selection
- List detected disks (name, size, model)
- Select target disk
- Warning: "All data will be erased"
- Automatic partition layout (not user-configurable):
  - `/boot/efi` — 256 MB (FAT32, UEFI)
  - `/boot` — 512 MB (ext4)
  - `swap` — min(RAM size, 8 GB)
  - `/` — 8 GB (ext4, system)
  - `/var/lib/vmm` — remainder (ext4, VM data — isolated so full disks don't brick the OS)

### Screen 3 — Network
- List detected network interfaces
- Select management interface
- DHCP or static (IP, subnet, gateway, DNS)
- Set hostname

### Screen 4 — Timezone & NTP
- Select timezone (continent → city)
- Enable NTP (yes/no)
- NTP server (default: `pool.ntp.org`)

### Screen 5 — Users
- Set root password (with confirmation)
- Create standard user: username + password

### Screen 6 — Service Ports
- VMM-Server port (default: 8443)
- VMM-Cluster port (default: 9443) — only if cluster role selected
- Editable with validation

### Screen 7 — Certificates
- Generate self-signed (default) — CN = hostname/IP
- Or: import custom certificates (path to cert + key file)

### Screen 8 — Summary
- All settings at a glance
- "Start installation" / "Go back to change"

### Screen 9 — Progress
- Partitioning, formatting, Debian extraction, GRUB setup, CoreVM installation, service configuration
- Progress bar + current action text

### Screen 10 — Complete
- "Installation complete. System will reboot."
- Display Web-UI URL: `https://<IP>:<Port>`

### Installer System Actions

The installer performs these system operations:

1. Partition target disk (`parted`)
2. Format partitions (`mkfs.ext4`, `mkfs.vfat`, `mkswap`)
3. Mount partitions
4. Extract Debian root-FS (pre-built from debootstrap, included in ISO)
5. Copy CoreVM binaries + assets into installed system
6. Configure `/etc/fstab`, `/etc/hostname`, `/etc/systemd/network/*.network`, `/etc/resolv.conf`
7. Configure NTP (`/etc/chrony/chrony.conf`)
8. Create users (`useradd`, `chpasswd`)
9. Generate or install TLS certificates (`openssl`)
10. Write CoreVM config files (`/etc/vmm/vmm-server.toml` or `/etc/vmm/vmm-cluster.toml`)
11. Install GRUB (`grub-install` + `grub-mkconfig`)
12. Configure systemd: DCUI service on tty1, vmm-server/vmm-cluster service
13. `update-initramfs` in chroot
14. Unmount, reboot

## DCUI (Direct Console User Interface)

Same binary: `vmm-appliance --mode dcui`. Runs as a systemd service on tty1 (not a login shell — direct service execution).

### Layout

**Upper area — Status bar (always visible):**
```
╔══════════════════════════════════════════════════════════════╗
║  CoreVM Appliance v1.0.0          Role: Standalone Server   ║
║  Hostname: vmhost01               Uptime: 3d 14h 22m       ║
║  IP: 192.168.1.50                 CPU: 12% | RAM: 8.2/32GB ║
║  vmm-server: ● running            Port: 8443               ║
║  https://192.168.1.50:8443                                  ║
╚══════════════════════════════════════════════════════════════╝
```

**Lower area — Menu:**

| Key | Function | Description |
|-----|----------|-------------|
| F1 | Network | Interface, IP/DHCP, gateway, DNS, hostname. Applies live via `networkctl`, persists to `/etc/systemd/network/` |
| F2 | Passwords | Select root or standard user, set new password with confirmation |
| F3 | Ports | Show/edit service ports, auto-restart affected service |
| F4 | Certificates | Regenerate self-signed or import custom cert+key |
| F5 | Services | Status of vmm-server/vmm-cluster, start/stop/restart/enable/disable |
| F6 | Time & NTP | Change timezone, NTP on/off, edit NTP server, manual time set, force sync |
| F7 | Logs | Live tail of vmm-server/cluster logs, scrollable |
| F8 | Update | Specify path to update package (USB/local), validate, apply with progress |
| F9 | Shell | Warning ("unsupported, at own risk"), then bash. `exit` returns to DCUI |
| F10 | Reboot/Shutdown | Submenu with confirmation |
| F11 | Diagnostics | System info dump, hardware detection, network connectivity test (ping gateway, DNS), disk health |
| F12 | Factory Reset | Double confirmation — resets `/etc/vmm/*.toml` to defaults, deletes vmm.db/vmm-cluster.db, preserves SSH host keys and VM disk images. Does NOT re-run installer; restarts services with fresh config. User is prompted whether to also delete VM disk images. |

### Status Bar Refresh

The status bar refreshes every 5 seconds, reading:
- `sysinfo` crate for CPU/RAM/uptime
- systemd service status via `systemctl is-active`
- Network info from system interfaces

## ISO Build Process

### Build Script: `tools/build-iso.sh`

The ISO contains two separate filesystem images:
- **Squashfs (live)**: Read-only root-FS for the ISO live environment (installer)
- **Debootstrap tarball**: Minimal Debian root-FS that gets extracted onto the target disk during installation

**Step 1 — Build the installable root-FS tarball:**
- `debootstrap` minimal Debian 12 into temp directory
- Packages: `linux-image-amd64`, `grub-pc`, `grub-efi-amd64-bin`, `systemd`, `systemd-networkd`, `openssh-server`, `openssl`, `chrony`, `parted`, `e2fsprogs`, `dosfstools`, `iproute2`, `sudo`, `ca-certificates`, `util-linux`, `pciutils`, `nftables`
- Copy CoreVM binaries into the root-FS (vmm-appliance, vmm-server, vmm-cluster, Web-UI dist, BIOS files)
- Configure systemd services (DCUI on tty1, vmm-server/vmm-cluster)
- Build initramfs (`update-initramfs` in chroot)
- Pack as `rootfs.tar.gz`

**Step 2 — Build the live environment:**
- Second `debootstrap` with minimal packages + `live-boot` + `live-config`
- Copy `rootfs.tar.gz` and vmm-appliance binary into live root
- Configure live environment to auto-start `vmm-appliance --mode installer`
- Create squashfs image from live root

**Step 3 — Assemble ISO:**
- `xorriso` with:
  - El Torito boot (isolinux for BIOS)
  - EFI System Partition (GRUB EFI)
  - Hybrid MBR for dd-to-USB support
  - Squashfs image + kernel + initramfs in ISO layout

### Build Dependencies (host)

- `debootstrap`, `xorriso`, `isolinux`, `grub-efi-amd64-bin`, `mtools`, `squashfs-tools`, `live-boot`
- Rust toolchain (for building vmm-appliance)
- Node.js/npm (for building vmm-ui)

## Project Structure

New crate in the workspace:

```
apps/vmm-appliance/
├── Cargo.toml
└── src/
    ├── main.rs              # CLI: --mode installer | --mode dcui
    ├── installer/
    │   ├── mod.rs
    │   ├── welcome.rs       # Screen 1: welcome + role selection
    │   ├── disk.rs          # Screen 2: disk selection + partitioning
    │   ├── network.rs       # Screen 3: network config
    │   ├── timezone.rs      # Screen 4: timezone + NTP
    │   ├── users.rs         # Screen 5: user creation
    │   ├── ports.rs         # Screen 6: service ports
    │   ├── certs.rs         # Screen 7: certificates
    │   ├── summary.rs       # Screen 8: summary
    │   └── progress.rs      # Screen 9+10: installation + completion
    ├── dcui/
    │   ├── mod.rs
    │   ├── status.rs        # Status bar (upper area)
    │   ├── network.rs       # F1: network config
    │   ├── passwords.rs     # F2: password management
    │   ├── ports.rs         # F3: port config
    │   ├── certs.rs         # F4: certificate management
    │   ├── services.rs      # F5: service management
    │   ├── time.rs          # F6: time & NTP
    │   ├── logs.rs          # F7: log viewer
    │   ├── update.rs        # F8: update mechanism
    │   ├── shell.rs         # F9: shell access
    │   └── reset.rs         # F12: factory reset
    └── common/
        ├── mod.rs
        ├── widgets.rs       # Shared TUI widgets (input fields, lists, dialogs)
        ├── config.rs        # Shared config types (reads/writes /etc/vmm/*.toml, reuses vmm-core types)
        └── system.rs        # System interaction (disk, network, services, certs, firewall)
```

### Dependencies (vmm-appliance Cargo.toml)

- `ratatui` + `crossterm` — TUI rendering
- `sysinfo` — CPU/RAM/uptime for status bar
- `nix` — Linux syscalls (mount, reboot, chroot)
- `serde` + `toml` — Config file read/write

## Update Mechanism

- Update packages are self-extracting archives (same format as existing installers)
- Contains new binaries + optional migration scripts
- Each update package carries a version number and a minimum-compatible-version field to prevent incompatible upgrades/downgrades
- Applied via DCUI (F8): select file path → validate version compatibility → validate checksum → stop services → replace binaries → run migrations → restart services
- Rollback: previous binaries are backed up to `/opt/vmm-backup/` before update

## Systemd Integration

### Services

```ini
# /etc/systemd/system/vmm-dcui.service
[Unit]
Description=CoreVM DCUI
After=multi-user.target

[Service]
Type=simple
ExecStart=/opt/vmm/vmm-appliance --mode dcui
StandardInput=tty
StandardOutput=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
Restart=always

[Install]
WantedBy=multi-user.target
```

- `vmm-server.service` / `vmm-cluster.service` — as existing installer headers already define
- DCUI runs on tty1, SSH gives normal shell access on other ttys

## Network Stack

The appliance uses **systemd-networkd** for network management (not ifupdown or NetworkManager):
- Configuration files in `/etc/systemd/network/*.network`
- DCUI reads/writes these files and applies via `networkctl reload`
- DNS via `systemd-resolved` with fallback to static `/etc/resolv.conf`

## Firewall

The appliance uses **nftables** with a minimal default ruleset:
- Allow: SSH (22), VMM-Server port (default 8443), VMM-Cluster port (default 9443), ICMP
- Deny: everything else inbound
- Allow: all outbound
- DCUI port changes (F3) automatically update nftables rules

## Security Considerations

- DCUI runs as systemd service on tty1 — no login shell, no getty
- SSH enabled: root login via SSH is disabled; only the standard user can SSH in (with `sudo` access)
- Self-signed TLS generated at install time
- Passwords hashed with Argon2 (existing vmm-server mechanism)
- Factory reset preserves SSH host keys to avoid MITM warnings

## Out of Scope (for now)

- Online update repository / auto-update
- Clustering of multiple appliances (vmm-cluster handles this at app level)
- Custom partition layouts
- RAID configuration
- IPv6 configuration (can be added later)
- Serial console support (ttyS0 for IPMI/BMC — can be added later)
