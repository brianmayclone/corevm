# libcorevm — Memory Subsystem

The memory subsystem manages guest physical RAM, memory-mapped I/O, and x86 paging/segmentation.

**Source:** `src/memory/`

## Architecture

```
Guest Virtual Address
        │
        ▼
┌──────────────────┐
│  Segmentation     │  (real mode: segment:offset → linear)
│  src/memory/      │
│  segment.rs       │
└────────┬─────────┘
         ▼
  Guest Linear Address
         │
         ▼
┌──────────────────┐
│  Paging           │  (protected/long mode: page table walk)
│  src/memory/      │
│  mod.rs           │
└────────┬─────────┘
         ▼
  Guest Physical Address
         │
    ┌────┴────┐
    ▼         ▼
┌────────┐ ┌────────┐
│  RAM   │ │  MMIO  │
│ flat.rs│ │ mmio.rs│
└────────┘ └────────┘
```

## Components

### Flat Memory (`flat.rs`)

Guest physical RAM implemented as a contiguous host memory allocation.

- Allocates guest RAM as a single host mmap/VirtualAlloc region
- Provides byte, word, dword, and qword read/write operations
- Supports direct pointer access for backend memory mapping

### MMIO Dispatch (`mmio.rs`)

Memory-mapped I/O routing for device registers.

- Maintains a table of MMIO regions (base address, size, device handler)
- Dispatches reads/writes to the appropriate device
- Key MMIO regions:
  - `0xFEC00000` — I/O APIC
  - `0xFED00000` — HPET
  - `0xFEE00000` — Local APIC
  - PCI BAR regions (per device)

### Segment Translation (`segment.rs`)

Real-mode segmentation: converts `segment:offset` pairs to linear addresses.

- Used in 16-bit real mode (BIOS, bootloader)
- `linear = segment * 16 + offset`

### Paging (`mod.rs`)

x86 page table walks with full enforcement of protection bits.

| Mode | Levels | Page Sizes | Bits |
|------|--------|------------|------|
| 32-bit | 2 (PD → PT) | 4 KB, 4 MB (PSE) | 32-bit physical |
| PAE | 3 (PDPT → PD → PT) | 4 KB, 2 MB | 36-bit physical (NX) |
| Long mode | 4 (PML4 → PDPT → PD → PT) | 4 KB, 2 MB, 1 GB | 48-bit virtual, 52-bit physical |

**Protection bits enforced:**
- **NX (No Execute)** — page-level execute disable
- **WP (Write Protect)** — supervisor write protection
- **U/S (User/Supervisor)** — privilege level access control
- **Present** — page fault on non-present pages

## PCI Memory Hole

The PCI hole (typically 0xC0000000–0xFFFFFFFF) maps MMIO ranges for PCI device BARs. Guest physical addresses in this range are routed to MMIO dispatch instead of flat RAM.

The Q35 MCH configures the PCI hole boundaries.
