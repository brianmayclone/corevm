# vmctl — User Guide

vmctl is the command-line interface for CoreVM. It allows you to run virtual machines headlessly, making it ideal for scripting, automation, and server environments.

## Installation

### Building

```bash
cd apps/vmctl
cargo build --release
```

The binary is output to `target/x86_64-unknown-linux-gnu/release/corevm-vmctl`.

## Usage

```bash
corevm-vmctl run [OPTIONS]
```

### Basic Examples

```bash
# Boot an ISO with 512 MB RAM
corevm-vmctl run -r 512 -i debian-netinst.iso -b seabios -g

# Boot a disk image with 1 GB RAM
corevm-vmctl run -r 1024 -d disk.img -b seabios -g

# Boot with multiple CPUs
corevm-vmctl run -r 2048 -d disk.img -b seabios --cpus 4 -g

# Boot with timeout (auto-stop after 60 seconds)
corevm-vmctl run -r 512 -i tinycore.iso -b seabios -t 60

# Boot with keyboard input injection
corevm-vmctl run -r 512 -d disk.img -b seabios -k "5000:enter"
```

## Command-Line Options

### Required

| Option | Description |
|--------|-------------|
| `-r <MB>` | RAM allocation in megabytes |

### Media

| Option | Description |
|--------|-------------|
| `-d <path>` | Attach a disk image |
| `-i <path>` | Attach an ISO image (CD-ROM) |

### Configuration

| Option | Description |
|--------|-------------|
| `-b <bios>` | BIOS type: `seabios` or `corevm` |
| `--cpus <n>` | Number of vCPUs (default: 1) |
| `--boot-order <order>` | Boot order: `disk`, `cd`, `floppy` |
| `--cache <mode>` | Disk cache mode: `writeback`, `writethrough`, `none` |
| `--vram <MB>` | Video RAM size |
| `--append <string>` | Kernel append parameters |

### Display

| Option | Description |
|--------|-------------|
| `-g` | Show screen (render VGA framebuffer) |
| `-s` | Show framebuffer in terminal (ASCII art) |

### Debug

| Option | Description |
|--------|-------------|
| `--show-regs` | Display CPU registers on exit |
| `--serial` | Show serial port output |

### Automation

| Option | Description |
|--------|-------------|
| `-t <seconds>` | Timeout — auto-stop after N seconds |
| `-k <delay:key>` | Inject keyboard input after delay (milliseconds) |

### Network

| Option | Description |
|--------|-------------|
| `--net <mode>` | Network mode: `none`, `slirp` |

## Keyboard Input Injection

The `-k` flag allows injecting keyboard input with a delay:

```bash
# Press Enter after 5 seconds
corevm-vmctl run -r 512 -d disk.img -b seabios -k "5000:enter"

# Type a command after 10 seconds
corevm-vmctl run -r 512 -d disk.img -b seabios -k "10000:h,e,l,l,o,enter"
```

Format: `<delay_ms>:<keyname>[,<keyname>,...]`

### Key Names

Common key names: `enter`, `esc`, `tab`, `space`, `backspace`, `up`, `down`, `left`, `right`, `f1`–`f12`, and single characters (`a`–`z`, `0`–`9`).

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Normal shutdown |
| 1 | Error |
| 2 | Timeout reached |

## Use Cases

### Automated Testing

```bash
# Boot a test image, run for 30 seconds, check exit code
corevm-vmctl run -r 256 -d test-disk.img -b seabios -t 30
echo "Exit code: $?"
```

### CI/CD Pipelines

```bash
# Boot and inject commands for automated setup
corevm-vmctl run -r 1024 -d build-env.img -b seabios \
  -k "10000:root,enter" \
  -k "12000:./run-tests.sh,enter" \
  -t 300
```

### Headless Server

```bash
# Run a VM in the background with serial output
corevm-vmctl run -r 2048 -d server.img -b seabios --net slirp --serial &
```
