# libcorevm — C FFI Reference

libcorevm exposes 58 C ABI functions for dynamic loading via `dlopen`/`dlsym` (Linux) or `LoadLibrary`/`GetProcAddress` (Windows).

**Source:** `src/ffi.rs`

## VM Lifecycle

| Function | Signature | Description |
|----------|-----------|-------------|
| `corevm_create` | `(ram_mb: u32) → *mut VmHandle` | Create a new VM with the specified RAM size |
| `corevm_destroy` | `(handle: *mut VmHandle)` | Destroy VM and free all resources |
| `corevm_run` | `(handle: *mut VmHandle) → i32` | Run the VM until an exit occurs |
| `corevm_reset` | `(handle: *mut VmHandle)` | Soft reboot the VM |

## BIOS & Firmware

| Function | Signature | Description |
|----------|-----------|-------------|
| `corevm_load_bios` | `(handle, path: *const c_char)` | Load BIOS firmware from file |
| `corevm_load_bios_data` | `(handle, data: *const u8, len: usize)` | Load BIOS from memory buffer |

## Device Setup

| Function | Description |
|----------|-------------|
| `corevm_setup_e1000(handle, mac)` | Configure Intel E1000 NIC with MAC address |
| `corevm_setup_ahci(handle)` | Enable AHCI SATA controller |
| `corevm_setup_ide(handle)` | Enable legacy IDE/ATA controller |
| `corevm_setup_ac97(handle)` | Enable AC'97 audio |
| `corevm_setup_svga(handle)` | Enable VMware SVGA II GPU |
| `corevm_setup_vga(handle)` | Enable VGA/Bochs VBE |
| `corevm_setup_net(handle, mode)` | Configure networking (0=none, 1=SLIRP) |
| `corevm_setup_hpet(handle)` | Enable HPET timer |
| `corevm_setup_usb(handle)` | Enable USB controller |

## Storage

| Function | Description |
|----------|-------------|
| `corevm_attach_disk(handle, path, idx)` | Attach a disk image at the given port index |
| `corevm_attach_cdrom(handle, path)` | Attach an ISO file as CD-ROM |
| `corevm_set_boot_order(handle, order)` | Set boot device priority |
| `corevm_set_disk_cache(handle, mode)` | Set disk cache mode |

## CPU & Registers

| Function | Description |
|----------|-------------|
| `corevm_set_cpus(handle, count)` | Set number of vCPUs |
| `corevm_get_registers(handle, regs)` | Read CPU registers |
| `corevm_set_registers(handle, regs)` | Write CPU registers |

## I/O & Input

| Function | Description |
|----------|-------------|
| `corevm_inject_key(handle, scancode)` | Inject PS/2 keyboard scancode |
| `corevm_inject_mouse(handle, dx, dy, buttons)` | Inject mouse movement and button state |
| `corevm_get_framebuffer(handle) → *const u8` | Get pointer to VGA framebuffer |
| `corevm_get_framebuffer_size(handle, w, h)` | Get current framebuffer dimensions |
| `corevm_get_ram_ptr(handle) → *const u8` | Get pointer to guest RAM |
| `corevm_get_serial_output(handle) → *const c_char` | Get serial port output buffer |

## Networking

| Function | Description |
|----------|-------------|
| `corevm_net_poll(handle)` | Poll network for pending packets |
| `corevm_net_get_stats(handle, stats)` | Get network statistics (TX/RX bytes/packets) |

## Usage Example (C)

```c
#include <dlfcn.h>

// Load library
void *lib = dlopen("libcorevm.so", RTLD_NOW);

// Get function pointers
typedef void* (*create_fn)(unsigned int);
typedef int (*run_fn)(void*);
typedef void (*destroy_fn)(void*);

create_fn create = dlsym(lib, "corevm_create");
run_fn run = dlsym(lib, "corevm_run");
destroy_fn destroy = dlsym(lib, "corevm_destroy");

// Create and run VM
void *vm = create(512);  // 512 MB RAM
// ... configure devices, load BIOS, attach disks ...
int exit_reason = run(vm);
destroy(vm);

dlclose(lib);
```

## Usage Example (Rust — Direct)

When used as a Rust crate (e.g., by vmctl, vmm-server), the FFI layer is bypassed and Rust APIs are used directly via `libcorevm::vm` and `libcorevm::runtime`.
