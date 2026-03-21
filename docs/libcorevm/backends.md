# libcorevm — Execution Backends

libcorevm supports three execution backends that provide hardware-accelerated VM execution on different platforms. All backends implement the same trait interface, ensuring consistent behavior across platforms.

## Backend Overview

| Backend | Feature Flag | Platform | Acceleration | `no_std` |
|---------|-------------|----------|--------------|----------|
| **KVM** | `linux` | Linux | Intel VT-x / AMD-V via `/dev/kvm` | No |
| **WHP** | `windows` | Windows | Intel VT-x / AMD-V via Hyper-V | No |
| **anyOS VMd** | `anyos` | anyOS | Direct VT-x / AMD-V | Yes |

## KVM Backend (Linux)

**Source:** `src/backend/kvm.rs`

Uses the Linux Kernel-based Virtual Machine (KVM) API to run guest code natively on the CPU.

### Requirements

- Linux kernel with KVM support enabled
- `/dev/kvm` device accessible (user must be in `kvm` group or root)
- Intel VT-x or AMD-V capable CPU

### How It Works

1. Opens `/dev/kvm` device
2. Creates a VM via `ioctl(KVM_CREATE_VM)`
3. Configures guest memory regions via `KVM_SET_USER_MEMORY_REGION`
4. Creates vCPU via `KVM_CREATE_VCPU`
5. Sets up initial CPU state (registers, segments, control registers)
6. Enters execution loop: `ioctl(KVM_RUN)` → handle exit → repeat

### Exit Handling

KVM exits occur when the guest executes:
- **I/O port access** (`IN`/`OUT`) — dispatched to device emulation via `io.rs`
- **MMIO access** — dispatched to device emulation via `memory/mmio.rs`
- **HLT instruction** — guest idle, returns to host
- **Shutdown/reset** — VM lifecycle events
- **Internal errors** — reported as errors

### Building

```bash
cd libs/libcorevm
cargo build --release --no-default-features --features linux
```

---

## WHP Backend (Windows)

**Source:** `src/backend/whp.rs`

Uses the Windows Hypervisor Platform (WHP) API, which is part of Hyper-V.

### Requirements

- Windows 10/11 with Hyper-V enabled
- Windows Hypervisor Platform feature enabled
- Intel VT-x or AMD-V capable CPU

### How It Works

1. Creates partition via `WHvCreatePartition`
2. Configures partition properties (processor count, etc.)
3. Maps guest memory via `WHvMapGpaRange`
4. Creates virtual processor via `WHvCreateVirtualProcessor`
5. Enters execution loop: `WHvRunVirtualProcessor` → handle exit → repeat

### Building

```bash
cd libs/libcorevm
cargo build --release --no-default-features --features windows
```

---

## anyOS VMd Backend

**Source:** `src/backend/anyos.rs`

Bare-metal backend for the anyOS operating system. Uses VT-x/AMD-V directly through anyOS VMd syscalls.

### Key Properties

- **`no_std` compatible** — compiles without the Rust standard library
- **Direct hardware access** — no host OS kernel abstraction layer
- Uses `libheap` for memory allocation and `libsyscall` for VMd system calls

### Dependencies

- `libheap` — heap allocator for `no_std` environment
- `libsyscall` — anyOS syscall interface

### Building

```bash
cd libs/libcorevm
cargo build --release  # anyos is the default feature
```

---

## Backend Trait

All backends implement a common interface that provides:

- VM creation and destruction
- Memory region mapping
- vCPU register get/set (GPR, segment, control, MSR)
- Execution loop with exit reason reporting
- Interrupt injection

This allows the upper layers (device emulation, I/O dispatch, memory management) to remain completely backend-agnostic.
