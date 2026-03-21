# libcorevm — Architecture & Design

libcorevm is the core x86 virtual machine engine of CoreVM, written entirely in Rust and NASM assembly. It provides full PC hardware emulation with hardware-accelerated execution via KVM (Linux), Hyper-V/WHP (Windows), and anyOS VMd (bare-metal).

## Design Principles

- **`no_std` at its core** — compiles for bare-metal targets with only `alloc` for heap allocations
- **Multi-backend** — same VM code runs on KVM, Hyper-V, and anyOS VMd
- **C FFI** — 58 exported functions allow dynamic loading from any language
- **Complete emulation** — 25+ hardware devices for full IBM PC compatibility

## Module Structure

```
libs/libcorevm/src/
├── lib.rs              Library entry point, feature gates, no_std panic handler
├── vm.rs               VM state machine (create, configure, run, destroy)
├── ffi.rs              C FFI layer — 58 exported functions
├── io.rs               Port I/O dispatch (IN/OUT instruction handling)
├── interrupts.rs       Interrupt controller interface
├── instruction.rs      x86 instruction decoding
├── registers.rs        CPU register definitions (GPR, segment, control)
├── flags.rs            CPU flags and operand sizes
├── error.rs            Error types
│
├── backend/            Execution backends
│   ├── mod.rs          Backend trait definition
│   ├── kvm.rs          KVM backend (Linux)
│   ├── whp.rs          WHP backend (Windows / Hyper-V)
│   └── anyos.rs        anyOS VMd backend (bare-metal)
│
├── devices/            25+ emulated hardware devices
│   ├── mod.rs          Device trait and registration
│   ├── e1000.rs        Intel E1000 Gigabit Ethernet NIC
│   ├── ahci.rs         AHCI SATA controller (ICH9)
│   ├── ide.rs          Legacy IDE/ATA controller
│   ├── svga.rs         VMware SVGA II GPU
│   ├── vga.rs          VGA/Bochs VBE framebuffer
│   ├── ac97.rs         AC'97 audio codec
│   ├── pic.rs          Dual 8259A PIC
│   ├── pit.rs          8254 Programmable Interval Timer
│   ├── hpet.rs         High Precision Event Timer
│   ├── apic.rs         Local APIC
│   ├── ioapic.rs       I/O APIC (82093AA)
│   ├── ps2.rs          PS/2 keyboard & mouse controller
│   ├── cmos.rs         CMOS/RTC
│   ├── uart.rs         16550 UART (COM1–COM4)
│   ├── pci.rs          PCI bus with PCIe MMCFG
│   ├── acpi.rs         ACPI power management
│   ├── q35.rs          Q35 chipset MCH
│   ├── fw_cfg.rs       QEMU firmware config interface
│   ├── apm.rs          Advanced Power Management
│   └── ...
│
├── memory/             Memory subsystem
│   ├── mod.rs          Main memory bus + paging
│   ├── flat.rs         Guest physical RAM
│   ├── mmio.rs         Memory-mapped I/O dispatch
│   └── segment.rs      Real-mode segment translation
│
├── runtime/            VM execution loop (std only)
│   └── mod.rs          Event loop, control interface
│
├── setup.rs            VM initialization utilities (std only)
└── net.rs              SLIRP networking (std only)
```

## Execution Flow

1. **Create** — `corevm_create(ram_mb)` allocates VM state and guest RAM
2. **Configure** — Load BIOS, set up devices (E1000, AHCI, SVGA, etc.)
3. **Attach media** — Attach disk images and ISOs
4. **Run** — `corevm_run()` enters the backend execution loop
   - KVM: `ioctl(KVM_RUN)` on the vCPU fd
   - WHP: `WHvRunVirtualProcessor` API
   - anyOS: VMd syscalls for VT-x/AMD-V
5. **Handle exits** — I/O port access, MMIO, interrupts are dispatched to device emulation
6. **Repeat** — Loop until shutdown/reboot/error

## Cargo Features

| Feature | Description |
|---------|-------------|
| `anyos` (default) | anyOS VMd backend — `no_std`, uses `libheap` and `libsyscall` |
| `linux` | KVM backend — enables `std`, `libc`, SLIRP networking, file I/O |
| `windows` | WHP backend — enables `std`, Hyper-V/WHP API |
| `std` | Standard library support (auto-enabled by `linux`/`windows`) |
| `host_test` | Host-side test utilities |

## BIOS

libcorevm includes a custom 64 KB BIOS written in NASM assembly (23 source files in `bios/`). It implements:

- **INT 10h** — Video services (text mode, VBE framebuffer)
- **INT 13h** — Disk services (HDD, CD-ROM)
- **INT 15h** — System services (memory map, A20 gate)
- **INT 16h** — Keyboard services
- **INT 19h** — Boot loader (MBR, El Torito, PXE)
- **INT 1Ah** — Time/date services

SeaBIOS is supported as an alternative firmware for maximum guest compatibility.

## See Also

- [Device Reference](devices.md) — detailed documentation for each emulated device
- [Backend Reference](backends.md) — KVM, WHP, and anyOS backend details
- [C FFI Reference](ffi.md) — complete list of exported functions
- [Memory Subsystem](memory.md) — guest RAM, MMIO, and paging
