# libcorevm — Emulated Hardware Devices

CoreVM emulates a complete IBM PC-compatible system with 25+ hardware devices. This document provides a reference for each device.

## Device Overview

| Category | Device | Description |
|----------|--------|-------------|
| **Network** | Intel E1000 | Gigabit Ethernet NIC (82540EM) |
| **Network** | SLIRP NAT | User-mode NAT with DHCP/DNS |
| **Network** | VirtIO Net | Paravirtualized network adapter |
| **Storage** | AHCI | SATA controller (ICH9) |
| **Storage** | IDE/ATA | Legacy ATA disk controller |
| **Storage** | USB | USB controller |
| **Graphics** | VMware SVGA II | GPU with 2D acceleration |
| **Graphics** | VGA/Bochs VBE | Standard VGA + VESA framebuffer |
| **Graphics** | VirtIO GPU | Paravirtualized GPU |
| **Audio** | AC'97 | Audio codec (ICH, 8086:2415) |
| **Input** | PS/2 | Keyboard and mouse controller |
| **Input** | VirtIO Input | Paravirtualized input device |
| **Timer** | 8254 PIT | Programmable Interval Timer |
| **Timer** | HPET | High Precision Event Timer |
| **Timer** | Local APIC | APIC timer |
| **Interrupt** | Dual 8259A PIC | Programmable Interrupt Controller |
| **Interrupt** | I/O APIC | Interrupt routing (82093AA) |
| **System** | CMOS/RTC | Real-time clock with NVRAM |
| **System** | 16550 UART | Serial ports (COM1–COM4) |
| **System** | PCI Bus | PCI config space + PCIe MMCFG |
| **System** | Q35 MCH | Q35 chipset memory controller |
| **System** | ACPI | Power management tables |
| **System** | APM | Advanced Power Management |
| **Firmware** | fw_cfg | QEMU firmware configuration interface |

---

## Networking

### Intel E1000 (82540EM)

Full Gigabit Ethernet NIC emulation compatible with the Intel 82540EM chipset.

- **PCI ID:** 8086:100E
- **Features:** MMIO registers, DMA ring buffers, MSI interrupt support, PXE boot ROM
- **Registers:** Transmit/receive descriptor rings, control, status, interrupt cause/mask
- **Guest drivers:** Linux `e1000`, Windows built-in, FreeBSD `em`

### SLIRP NAT

User-mode networking stack — no root or TAP device required.

- **Network:** 10.0.2.0/24 virtual subnet
- **Gateway:** 10.0.2.2
- **DHCP:** Automatic IP assignment (10.0.2.15)
- **DNS:** Relay to host DNS
- **Protocols:** TCP (with flow control, retransmit, window scaling), UDP, ICMP echo

### VirtIO Net

Paravirtualized network adapter for high-performance guest networking.

- Requires VirtIO drivers in the guest OS
- Lower overhead than E1000 emulation

---

## Storage

### AHCI (ICH9 SATA)

Advanced Host Controller Interface — modern SATA controller emulation.

- **PCI ID:** 8086:2922
- **Features:** DMA, Native Command Queuing (NCQ), multiple ports
- **Disk formats:** Raw flat images
- **Guest drivers:** Standard AHCI drivers (Linux, Windows Vista+)

### IDE/ATA

Legacy ATA disk controller for maximum compatibility.

- **Modes:** PIO (programmed I/O), DMA
- **Ports:** Primary (0x1F0) and secondary (0x170)
- **Guest drivers:** Universal — all x86 operating systems

### Disk Cache

LRU block cache layer for reduced I/O on disk operations.

- **Modes:** WriteBack, WriteThrough, None
- Configurable per disk

---

## Graphics

### VMware SVGA II

GPU emulation with 2D acceleration and hardware cursor support.

- **PCI ID:** 15AD:0405
- **Features:** SVGA framebuffer, hardware cursor, 2D acceleration commands
- **Resolutions:** Configurable, supports modern resolutions
- **Guest drivers:** VMware SVGA drivers (Linux, Windows)

### VGA / Bochs VBE

Standard VGA with VESA BIOS Extensions for high-resolution framebuffer modes.

- **Features:** Text mode (80x25), VGA graphics modes, VBE linear framebuffer
- **VRAM:** Configurable size
- **Guest drivers:** Universal VGA support + VBE-aware drivers

### VirtIO GPU

Paravirtualized GPU for efficient framebuffer access.

- Requires VirtIO drivers in the guest OS

---

## Audio

### AC'97 (ICH)

Audio codec emulation based on the Intel ICH AC'97 specification.

- **PCI ID:** 8086:2415
- **Features:** PCM playback via DMA buffer descriptors
- **Guest drivers:** Linux `snd_intel8x0`, Windows built-in AC'97

---

## Input

### PS/2 Controller

Standard i8042 keyboard and mouse controller.

- **Keyboard:** Scancode Set 2, typematic rate, LED control
- **Mouse:** Standard PS/2 mouse protocol with scroll support
- **Ports:** 0x60 (data), 0x64 (status/command)
- **IRQ:** 1 (keyboard), 12 (mouse)

### VirtIO Input

Paravirtualized input device for keyboard and mouse.

- Requires VirtIO drivers in the guest OS

---

## Timers

### 8254 PIT

Programmable Interval Timer with 3 channels.

- **Channel 0:** System timer (IRQ 0), ~1.193 MHz base frequency
- **Channel 1:** DRAM refresh (legacy)
- **Channel 2:** PC speaker
- **Modes:** One-shot, rate generator, square wave, etc.
- **Port:** 0x40–0x43

### HPET

High Precision Event Timer for nanosecond-resolution timing.

- **MMIO base:** 0xFED00000
- **Features:** 64-bit main counter, multiple comparators, periodic/one-shot modes

### Local APIC Timer

Per-CPU timer integrated into the Local APIC.

- **Modes:** One-shot, periodic, TSC-deadline
- **Divisor:** Configurable

---

## Interrupts

### Dual 8259A PIC

Master + slave Programmable Interrupt Controller for legacy interrupt routing.

- **Master:** IRQ 0–7 (port 0x20–0x21)
- **Slave:** IRQ 8–15 (port 0xA0–0xA1)
- **Features:** ICW/OCW programming, cascade mode, edge/level triggering

### I/O APIC (82093AA)

Advanced interrupt routing for multi-processor systems.

- **MMIO base:** 0xFEC00000
- **Features:** 24 redirection entries, interrupt remapping, level/edge triggering
- Used together with Local APIC for modern interrupt delivery

### Local APIC

Advanced Programmable Interrupt Controller — per-CPU interrupt handling.

- **MMIO base:** 0xFEE00000
- **Features:** IPI (inter-processor interrupts), timer, spurious interrupt handling, EOI

---

## System Devices

### CMOS/RTC

Real-time clock with 128 bytes of battery-backed NVRAM.

- **Ports:** 0x70 (address), 0x71 (data)
- **Features:** Time/date, alarm, periodic interrupt (IRQ 8), boot config storage

### 16550 UART

Serial port controller — 4 ports (COM1–COM4).

- **COM1:** 0x3F8 (IRQ 4)
- **COM2:** 0x2F8 (IRQ 3)
- **COM3:** 0x3E8 (IRQ 4)
- **COM4:** 0x2E8 (IRQ 3)
- **Features:** 16-byte FIFO, configurable baud rate, modem control

### PCI Bus

Full PCI configuration space with PCIe MMCFG support.

- **Config I/O:** 0xCF8 (address), 0xCFC (data)
- **MMCFG:** Extended PCI Express configuration space
- **Features:** BAR enumeration, MSI support, interrupt routing

### Q35 MCH

Q35 chipset memory controller hub emulation.

- Memory controller for guest RAM mapping
- PCI host bridge
- PCI hole for MMIO (below 4 GB)

### ACPI

Advanced Configuration and Power Interface tables.

- **Tables:** DSDT, FADT, MADT, MCFG
- **Features:** Power state management, sleep states, SCI interrupt

### APM

Advanced Power Management for shutdown and reboot signaling.

- Handles APM shutdown (`poweroff`) and reboot requests from the guest

### fw_cfg

QEMU-compatible firmware configuration interface.

- Passes configuration data (RAM size, kernel parameters, etc.) to firmware/BIOS
- Used by SeaBIOS for hardware discovery
