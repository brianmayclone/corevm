# vmmanager — User Guide

vmmanager is the native desktop GUI for CoreVM. It provides a graphical interface for creating, configuring, and running virtual machines with live VGA display.

## Installation

### Prerequisites

**Linux:**
- KVM support (`/dev/kvm` accessible)
- OpenGL support (for framebuffer rendering)

**Windows:**
- Hyper-V / Windows Hypervisor Platform enabled
- OpenGL support

### Building

```bash
cd apps/vmmanager
cargo build --release
```

The binary is output to `target/x86_64-unknown-linux-gnu/release/vmmanager` (Linux) or `target/release/vmmanager.exe` (Windows).

### Running

```bash
./target/release/vmmanager
```

On startup, vmmanager checks for hardware virtualization support (KVM on Linux, WHP on Windows) and displays an error dialog if not available.

## User Interface

### Main Window

```
┌─────────────────────────────────────────────────────────┐
│  Toolbar: [Start] [Stop] [Reset] [Snapshot] [Settings]  │
├────────────┬────────────────────────────────────────────┤
│            │                                             │
│  Sidebar   │         VGA Display Area                    │
│            │                                             │
│  - VM 1    │    (live framebuffer rendering)             │
│  - VM 2    │                                             │
│  - VM 3    │                                             │
│            │                                             │
├────────────┴────────────────────────────────────────────┤
│  Status Bar: CPU: 12%  RAM: 256/512 MB  Disk I/O: 2 MB/s│
└─────────────────────────────────────────────────────────┘
```

### Sidebar

Lists all configured VMs. Click a VM to select it and view its console in the display area.

### Toolbar

| Button | Action |
|--------|--------|
| **Start** | Boot the selected VM |
| **Stop** | Graceful ACPI shutdown |
| **Reset** | Soft reboot |
| **Snapshot** | Create a disk snapshot |
| **Settings** | Open VM configuration dialog |

### Display Area

Shows the live VGA output of the running VM:
- Rendered as an OpenGL texture
- Keyboard input is captured when the display is focused
- Mouse input is captured when clicking inside the display

### Status Bar

Real-time metrics for the running VM:
- **CPU** — utilization percentage
- **RAM** — used / total
- **Disk I/O** — read/write throughput

## Creating a VM

1. Click the **+** button or use the menu
2. In the creation dialog, configure:
   - **Name** — descriptive name
   - **RAM** — memory size in MB
   - **CPU** — number of vCPUs
   - **BIOS** — CoreVM BIOS or SeaBIOS
   - **Disk** — create a new disk image (specify size) or select existing
   - **ISO** — attach an ISO file for installation
   - **Network** — Disconnected or User-mode NAT
   - **Boot Order** — disk or CD first
3. Click **Create**

## Configuring a VM

1. Select the VM in the sidebar
2. Click **Settings** in the toolbar (or right-click → Settings)
3. Modify any setting
4. Click **Save**

Changes to CPU, RAM, and BIOS require the VM to be stopped.

## Running a VM

1. Select the VM
2. Click **Start**
3. The VGA display appears in the main area
4. Interact via keyboard and mouse
5. Click **Stop** for graceful shutdown or close the window

## Disk Snapshots

1. Stop the VM (recommended)
2. Click **Snapshot**
3. Enter a snapshot name
4. The current disk state is saved

Restore snapshots from the Settings dialog.

## Keyboard & Mouse

- **Keyboard:** Captured when the display area is focused. Press Ctrl+Alt to release.
- **Mouse:** Captured when clicking inside the display. The cursor is confined to the display area.
- On Linux, evdev input is used for precise keyboard and mouse capture.

## ISO Detection

vmmanager can detect ISO files in common locations:
- Current directory
- `~/Downloads/`
- Configured ISO paths

## Troubleshooting

### "KVM not available"

- Ensure your CPU supports VT-x (Intel) or AMD-V (AMD)
- Enable virtualization in BIOS/UEFI settings
- On Linux: `sudo modprobe kvm_intel` or `sudo modprobe kvm_amd`
- Ensure your user is in the `kvm` group: `sudo usermod -aG kvm $USER`

### "WHP not available" (Windows)

- Enable Hyper-V in Windows Features
- Enable Windows Hypervisor Platform in Windows Features
- Reboot after enabling

### Black screen after start

- Check BIOS setting — try switching between CoreVM BIOS and SeaBIOS
- Ensure a bootable disk or ISO is attached
- Check the boot order configuration
