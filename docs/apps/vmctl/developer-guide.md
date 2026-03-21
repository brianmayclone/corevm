# vmctl — Developer Guide

This guide covers the internal architecture of the vmctl CLI tool.

## Source Structure

```
apps/vmctl/src/
└── main.rs         Single-file application (~1400 lines)
```

vmctl is intentionally a single-file application — all CLI parsing, VM setup, and execution logic is in `main.rs`.

## Architecture

### Flow

```
CLI args parsing
      │
      ▼
VM configuration
(RAM, BIOS, disks, network, etc.)
      │
      ▼
libcorevm setup
(create VM, configure devices, load BIOS, attach media)
      │
      ▼
Execution loop
(run VM, handle exits, render framebuffer, inject input)
      │
      ▼
Shutdown / timeout
```

### CLI Parsing

Arguments are parsed manually from `std::env::args()`. The `run` subcommand is the primary (and currently only) command.

### VM Setup

vmctl uses libcorevm's Rust API directly (not FFI):

1. Creates a `Vm` with the specified RAM
2. Loads BIOS firmware (CoreVM or SeaBIOS)
3. Configures devices based on CLI flags:
   - E1000 NIC (if networking enabled)
   - AHCI or IDE controller
   - SVGA or VGA display
   - AC'97 audio
4. Attaches disk images and ISOs
5. Sets boot order and cache mode

### Execution

The VM runs in the main thread:

1. Calls `corevm_run()` in a loop
2. Handles VM exits (I/O, HLT, shutdown)
3. If `-g` flag: renders framebuffer
4. If `-k` flag: injects keyboard input at scheduled times
5. If `-t` flag: checks timeout and stops if exceeded
6. Continues until shutdown, error, or timeout

### Platform Support

| Platform | Feature | Backend |
|----------|---------|---------|
| Linux | `libcorevm/linux` | KVM |
| Windows | `libcorevm/windows` | WHP |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `libcorevm` | VM engine (with platform feature) |

vmctl has minimal dependencies — only libcorevm itself.

## Extending

### Adding a New Subcommand

1. Add argument parsing for the new subcommand in `main.rs`
2. Implement the subcommand logic
3. Call from the main dispatch

### Adding a New CLI Option

1. Add the option to the argument parser
2. Pass the value to the VM configuration
3. Document the option in the help text and user guide
