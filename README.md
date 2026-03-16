<div align="center">

# CoreVM

**A pure-software x86 virtual machine engine built entirely in Rust and NASM assembly**

Full PC hardware emulation with KVM acceleration — run real operating systems
without depending on QEMU or any external emulator.

<br>

![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white)
![NASM](https://img.shields.io/badge/NASM-Assembly-0066B8?style=flat-square)
![x86_64](https://img.shields.io/badge/Arch-x86__64-4B7BEC?style=flat-square)
![License: MIT](https://img.shields.io/badge/License-MIT-2ecc71?style=flat-square)
![no_std](https://img.shields.io/badge/no__std-compatible-e67e22?style=flat-square)

<br>

[Features](#features) · [Architecture](#architecture) · [Quick Start](#quick-start) · [Building](#building) · [API Reference](#api-reference) · [Integration](#anyos-integration)

</div>

<br>

> CoreVM is the virtual machine engine behind [anyOS](https://github.com/nicosommelier/anyos). It runs as a standalone Linux/Windows application (with KVM or Hyper-V acceleration) and also compiles as a `no_std` library for embedded use inside the anyOS kernel — where hardware virtualization is provided by the anyOS VMd, which leverages VT-x and AMD-V directly, without relying on KVM or WHP.

---

## Features

### CPU Emulation

- **Full x86 ISA** — 16-bit real mode, 32-bit protected mode, 64-bit long mode
- **Paging support** — 2-level (32-bit), PAE (3-level), and 4-level (long mode) page table walks with NX, WP, U/S enforcement
- **JIT compiler** — two-phase acceleration: decode cache for hot basic blocks + native x86-64 code compilation
- **Multi-backend architecture** — KVM (Linux), Hyper-V/WHP (Windows), anyOS VMd hardware-accelerated (anyOS)

### PC Hardware Emulation

CoreVM emulates a complete IBM PC-compatible system with the following devices:

| Device | Description |
|--------|-------------|
| **Intel E1000** | Gigabit Ethernet NIC (82540EM) — MMIO, DMA, MSI, PXE boot ROM |
| **AHCI** | SATA controller (ICH9) — DMA, NCQ, multiple ports |
| **IDE/ATA** | Legacy ATA disk controller — PIO and DMA modes |
| **AC'97** | Audio codec (ICH, 8086:2415) — PCM playback |
| **UHCI** | USB 1.1 host controller — HID keyboard/mouse, mass storage |
| **VMware SVGA II** | GPU with 2D acceleration, hardware cursor |
| **VGA/Bochs VBE** | Standard VGA + VESA BIOS Extensions framebuffer |
| **Dual 8259A PIC** | Programmable Interrupt Controller (master + slave) |
| **8254 PIT** | Programmable Interval Timer (3 channels) |
| **HPET** | High Precision Event Timer |
| **Local APIC** | Advanced Programmable Interrupt Controller |
| **I/O APIC** | I/O interrupt routing (82093AA) |
| **PS/2** | Keyboard and mouse controller |
| **CMOS/RTC** | Real-time clock with NVRAM |
| **16550 UART** | Serial port (COM1–COM4) |
| **PCI Bus** | PCI configuration space with PCIe MMCFG support |
| **ACPI** | Power management (DSDT, FADT, MADT, MCFG tables) |
| **fw_cfg** | QEMU firmware configuration interface |
| **APM** | Advanced Power Management (shutdown/reboot) |

### Networking

- **User-mode NAT (SLIRP)** — built-in NAT + DHCP + DNS for guest VMs without root or TAP devices
  - Virtual 10.0.2.0/24 network with gateway, DHCP server, and DNS relay
  - TCP connection tracking with flow control, retransmit, and window scaling
  - UDP forwarding with automatic flow expiration
  - ICMP echo reply
- **E1000 NIC emulation** — full descriptor ring DMA, interrupt coalescing, MSI support

### BIOS

- **Custom 64 KB NASM BIOS** with complete interrupt services:
  - INT 10h — Video (text mode, cursor, scrolling, teletype output)
  - INT 13h — Disk (CHS/LBA read/write, El Torito CD boot)
  - INT 15h — System (E820 memory map, extended memory size, APM)
  - INT 16h — Keyboard (read key, check buffer, shift flags)
  - INT 19h — Bootstrap loader
  - INT 1Ah — Time of day (RTC, PCI BIOS)
- **SeaBIOS support** — can also boot with standard SeaBIOS firmware
- **PCI BIOS** — BAR enumeration, interrupt routing
- **POST self-test** — memory check, device initialization

### Storage

- **Disk image formats** — raw flat images, ISO 9660 (CD-ROM)
- **Disk cache** — LRU block cache for reduced I/O
- **Boot methods** — HDD (MBR), CD-ROM (El Torito), PXE network boot

### VM Manager GUI

- **Native desktop application** built with egui/eframe
- Create, configure, and run VMs with live VGA display
- Disk image creation and management
- Snapshot support
- Cross-platform: Linux (native) and Windows (MSVC)

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    VM Manager (egui)                      │
│              or vmctl (CLI)  or anyOS vmd                 │
├──────────────────────────────────────────────────────────┤
│                    C FFI Layer (58 exports)               │
│              corevm_create / corevm_run / ...             │
├──────────────────────────────────────────────────────────┤
│                      libcorevm                           │
│  ┌─────────────┐  ┌──────────┐  ┌─────────────────────┐ │
│  │   Backend    │  │  Devices │  │      Memory         │ │
│  │  ┌───────┐   │  │  E1000   │  │  Flat (guest RAM)   │ │
│  │  │  KVM  │   │  │  AHCI   │  │  MMIO dispatch      │ │
│  │  │  WHP  │   │  │  IDE    │  │  Segment (real mode) │ │
│  │  │ anyOS │   │  │  AC97   │  │  PCI hole mapping    │ │
│  │  └───────┘   │  │  SVGA   │  └─────────────────────┘ │
│  └─────────────┘  │  PIC/PIT │                           │
│                    │  APIC    │  ┌─────────────────────┐ │
│                    │  PS/2    │  │    SLIRP NAT        │ │
│                    │  UART    │  │  TCP/UDP/DHCP/DNS   │ │
│                    │  HPET    │  └─────────────────────┘ │
│                    │  USB     │                           │
│                    └──────────┘                           │
├──────────────────────────────────────────────────────────┤
│                     BIOS (NASM)                          │
│           INT 10h/13h/15h/16h/19h/1Ah                    │
└──────────────────────────────────────────────────────────┘
```

### Backend Architecture

CoreVM supports three execution backends, selected at compile time via Cargo features:

| Backend | Feature | Platform | Description |
|---------|---------|----------|-------------|
| **KVM** | `linux` | Linux | Hardware-accelerated via `/dev/kvm`. Guest code runs natively on the CPU; device emulation and I/O handled in userspace. |
| **WHP** | `windows` | Windows | Hardware-accelerated via Windows Hypervisor Platform (Hyper-V). |
| **anyOS** | `anyos` | anyOS | Hardware-accelerated via anyOS VMd (VT-x / AMD-V). Guest code runs natively on the CPU; no software interpreter. `no_std` compatible. |

### `no_std` Design

libcorevm is designed as a `no_std` library at its core. The `std` feature (enabled by `linux` and `windows`) adds:
- File I/O for disk images and BIOS loading
- Network sockets for SLIRP NAT backend
- Thread spawning for background TCP connects
- `eprintln!` debug logging

Without `std`, libcorevm compiles for bare-metal targets (like the anyOS kernel) with only `alloc` for heap allocations.

---

## Quick Start

### Build the VM Manager (Linux)

```bash
# Clone the repository
git clone https://github.com/nicosommelier/corevm.git
cd corevm

# Build the GUI application
./tools/build_linux.sh

# Or build and run immediately
./tools/build_linux.sh --run
```

### Build vmctl (CLI)

```bash
cd apps/vmctl
cargo +stable build --release
```

### Run a VM

```bash
# Boot an ISO with 512 MB RAM
./apps/vmctl/target/x86_64-unknown-linux-gnu/release/corevm-vmctl \
    run -r 512 -i debian-netinst.iso -b seabios -g

# Boot a disk image
./apps/vmctl/target/x86_64-unknown-linux-gnu/release/corevm-vmctl \
    run -r 1024 -d disk.img -b seabios -g
```

### Prerequisites

**Linux:**
```bash
# KVM support required
sudo apt install qemu-system-x86  # for SeaBIOS firmware
# Rust stable toolchain
rustup install stable
```

**Windows (MSVC):**
```bash
# Build from WSL or native Windows
./tools/build_win64.sh    # from WSL
# or
tools\build_win64.bat     # from cmd.exe
```

---

## Building

### Project Structure

```
corevm/
├── Cargo.toml              Workspace root
├── libs/
│   └── libcorevm/          Core VM engine (no_std library)
│       ├── src/
│       │   ├── lib.rs          Library root
│       │   ├── vm.rs           VM state and execution
│       │   ├── ffi.rs          C FFI exports (58 functions)
│       │   ├── setup.rs        VM setup and configuration
│       │   ├── instruction.rs  x86 instruction decoder
│       │   ├── interrupts.rs   Interrupt handling
│       │   ├── io.rs           I/O port dispatch
│       │   ├── registers.rs    CPU register file
│       │   ├── flags.rs        EFLAGS management
│       │   ├── error.rs        Error types
│       │   ├── net.rs          Network backend trait
│       │   ├── backend/        Execution backends (KVM, WHP, anyOS)
│       │   ├── devices/        Device emulation (25 devices)
│       │   └── memory/         Memory subsystem (flat, MMIO, segments)
│       └── bios/               Custom BIOS (23 NASM assembly files)
├── apps/
│   ├── vmctl/              CLI tool for running VMs
│   └── vmmanager/          GUI application (egui/eframe)
├── tests/
│   └── hosttests/          Integration and smoke tests
└── tools/                  Build and test scripts
```

### Cargo Features

| Feature | Description |
|---------|-------------|
| `linux` | Enable KVM backend + SLIRP networking + file I/O |
| `windows` | Enable WHP backend + file I/O |
| `anyos` | Enable anyOS VMd hardware-accelerated backend (default, `no_std`) |
| `std` | Enable standard library (auto-enabled by `linux`/`windows`) |
| `host_test` | Enable host-side test utilities |

### Build Commands

```bash
# Build libcorevm for Linux (KVM)
cd libs/libcorevm
cargo build --release --no-default-features --features linux

# Build the full workspace (vmctl + vmmanager + tests)
cargo build --release

# Run integration tests
cd tests/hosttests
cargo test --release
```

---

## API Reference

libcorevm exposes 58 C ABI functions via its FFI layer, designed for dynamic loading (`dlopen`/`dlsym`). Key APIs:

### VM Lifecycle

| Function | Description |
|----------|-------------|
| `corevm_create(ram_mb) → handle` | Create a new VM with the specified RAM size |
| `corevm_destroy(handle)` | Destroy a VM and free all resources |
| `corevm_run(handle) → exit_reason` | Run the VM until an exit occurs |
| `corevm_reset(handle)` | Reset the VM (soft reboot) |

### Configuration

| Function | Description |
|----------|-------------|
| `corevm_load_bios(handle, path)` | Load BIOS firmware from file |
| `corevm_setup_e1000(handle, mac)` | Set up E1000 NIC with MAC address |
| `corevm_setup_ahci(handle)` | Set up AHCI SATA controller |
| `corevm_setup_ide(handle)` | Set up legacy IDE controller |
| `corevm_setup_ac97(handle)` | Set up AC'97 audio |
| `corevm_setup_uhci(handle)` | Set up USB UHCI controller |
| `corevm_setup_svga(handle)` | Set up VMware SVGA II GPU |
| `corevm_setup_net(handle, mode)` | Set up networking (0=none, 1=SLIRP) |
| `corevm_attach_disk(handle, path, idx)` | Attach a disk image |
| `corevm_attach_cdrom(handle, path)` | Attach an ISO image |

### I/O and Interrupts

| Function | Description |
|----------|-------------|
| `corevm_inject_key(handle, scancode)` | Send a PS/2 scancode to the guest |
| `corevm_inject_mouse(handle, dx, dy, buttons)` | Send mouse movement/click |
| `corevm_net_poll(handle)` | Poll network backend (TX/RX) |
| `corevm_poll_irqs(handle)` | Poll and deliver pending interrupts |
| `corevm_get_framebuffer(handle) → ptr` | Get pointer to VGA framebuffer |

### Guest Memory

| Function | Description |
|----------|-------------|
| `corevm_get_ram_ptr(handle) → ptr` | Get pointer to guest RAM |
| `corevm_get_ram_size(handle) → size` | Get guest RAM size in bytes |

---

## Tested Guest Operating Systems

CoreVM has been tested with the following guest operating systems:

| OS | Boot | Status |
|----|------|--------|
| **Debian** (netinst) | CD-ROM | Boots installer, network via SLIRP |
| **TinyCore Linux** | CD-ROM | Fully boots to desktop |
| **Memtest86+** | CD-ROM | Runs memory test |
| **FreeDOS** | HDD | Boots to command prompt |
| **Windows XP** | CD-ROM | Boots installer |
| **anyOS** | HDD | Full desktop with all features |

---

## anyOS Integration

CoreVM originated as part of the [anyOS](https://github.com/nicosommelier/anyos) operating system project. When used inside anyOS:

1. **This repository is added as a git submodule** at `anyos/corevm/`
2. `libcorevm` compiles with `--features anyos` (the default) as a `no_std` static library
3. The anyOS kernel loads it as `libcorevm.so` via the ELF dynamic linker
4. `libcorevm_client` (in the anyOS repo) provides a safe Rust wrapper around the C FFI
5. `bin/vmd` (VM daemon) and `apps/vmmanager` (GUI) use `libcorevm_client` to interact with VMs

### Adding as Submodule

```bash
cd anyos
git submodule add https://github.com/nicosommelier/corevm.git corevm
git submodule update --init
```

The `libheap` and `libsyscall` path dependencies in `libs/libcorevm/Cargo.toml` resolve correctly when this repo sits at `anyos/corevm/` — they point to `anyos/libs/libheap/` and `anyos/libs/libsyscall/` via relative paths.

---

## License

This project is licensed under the MIT License — see [LICENSE](LICENSE) for details.

## Contact

**Christian Moeller** — [c.moeller.ffo@gmail.com](mailto:c.moeller.ffo@gmail.com) · [brianmayclone@googlemail.com](mailto:brianmayclone@googlemail.com)

---

<div align="center">
<sub>Part of the <a href="https://github.com/nicosommelier/anyos">anyOS</a> project — a 64-bit operating system built from scratch in Rust.</sub>
</div>
