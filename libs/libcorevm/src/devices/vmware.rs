//! VMware Backdoor emulation — absolute pointer + version detection.
//!
//! The VMware backdoor uses x86 `IN` instructions on port 0x5658 with
//! magic value 0x564D5868 ("VMXh") in EDX. Guest registers carry the
//! command and parameters:
//!
//! | Register | Input | Output |
//! |----------|-------|--------|
//! | EAX | Command ID | Result |
//! | EBX | Parameter 1 | Result 1 |
//! | ECX | Parameter 2 | Result 2 |
//! | EDX | 0x564D5868 (magic) | Port number |
//! | ESI | Parameter 3 | - |
//! | EDI | Parameter 4 | - |
//!
//! # Supported commands
//!
//! | CMD | Name | Purpose |
//! |-----|------|---------|
//! | 0x01 | GET_VERSION | VMware detection + version |
//! | 0x0A | GET_CURSOR_POS | Absolute cursor query |
//! | 0x27 | ABSPOINTER_DATA | Read absolute pointer data |
//! | 0x28 | ABSPOINTER_STATUS | Pointer status / pending data |
//! | 0x29 | ABSPOINTER_COMMAND | Enable/disable/reset pointer |

use core::sync::atomic::{AtomicU16, AtomicU8, AtomicBool, Ordering};

/// VMware magic constant (ASCII "VMXh").
const VMWARE_MAGIC: u32 = 0x564D5868;

/// VMware backdoor I/O port.
pub const VMWARE_PORT: u16 = 0x5658;

// Command IDs
const CMD_GET_VERSION: u32 = 0x01;
const CMD_GET_CURSOR_POS: u32 = 0x0A;
const CMD_ABSPOINTER_DATA: u32 = 0x27;
const CMD_ABSPOINTER_STATUS: u32 = 0x28;
const CMD_ABSPOINTER_COMMAND: u32 = 0x29;

// ABSPOINTER_COMMAND sub-commands (in EBX)
const ABSPOINTER_ENABLE: u32 = 0x45414552;   // "REAE" — enable
const ABSPOINTER_RELATIVE: u32 = 0x4C455252; // "RREL" — relative mode
const ABSPOINTER_ABSOLUTE: u32 = 0x53424152; // "RABS" — absolute mode

/// VMware backdoor device state.
///
/// Thread-safe: mouse position is updated from the UI/input thread
/// via atomic operations, read from the vCPU thread during I/O exits.
pub struct VmwareBackdoor {
    /// Absolute pointer enabled by the guest.
    enabled: AtomicBool,
    /// Absolute mode (vs relative).
    absolute: AtomicBool,
    /// Current absolute X position (0–65535).
    abs_x: AtomicU16,
    /// Current absolute Y position (0–65535).
    abs_y: AtomicU16,
    /// Current button state (bit 0=left, bit 1=right, bit 2=middle).
    buttons: AtomicU8,
    /// New data available (set by host, cleared after guest reads).
    data_pending: AtomicBool,
    /// Scroll wheel delta (accumulated, signed).
    wheel: core::sync::atomic::AtomicI8,
}

impl VmwareBackdoor {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            absolute: AtomicBool::new(false),
            abs_x: AtomicU16::new(0),
            abs_y: AtomicU16::new(0),
            buttons: AtomicU8::new(0),
            data_pending: AtomicBool::new(false),
            wheel: core::sync::atomic::AtomicI8::new(0),
        }
    }

    /// Update the absolute pointer position (called from input/UI thread).
    ///
    /// `x` and `y` are in the range 0–65535 (0%–100% of screen).
    pub fn set_position(&self, x: u16, y: u16, buttons: u8) {
        self.abs_x.store(x, Ordering::Relaxed);
        self.abs_y.store(y, Ordering::Relaxed);
        self.buttons.store(buttons, Ordering::Relaxed);
        self.data_pending.store(true, Ordering::Release);
    }

    /// Update position with wheel delta.
    pub fn set_position_wheel(&self, x: u16, y: u16, buttons: u8, wheel: i8) {
        self.set_position(x, y, buttons);
        self.wheel.store(wheel, Ordering::Relaxed);
    }

    /// Whether the guest has enabled the absolute pointer.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Whether the guest is in absolute mode.
    pub fn is_absolute(&self) -> bool {
        self.absolute.load(Ordering::Relaxed)
    }

    /// Handle a VMware backdoor I/O exit.
    ///
    /// Called from the I/O exit handler when port == 0x5658 and EDX == VMWARE_MAGIC.
    /// Reads the full register state, processes the command, and modifies
    /// registers for the response.
    ///
    /// Returns `true` if the command was handled (caller should skip normal I/O dispatch).
    pub fn handle_command(&self, regs: &mut crate::backend::VcpuRegs) -> bool {
        let edx = regs.rdx as u32;
        if edx != VMWARE_MAGIC {
            return false;
        }

        let cmd = (regs.rcx as u32) & 0xFFFF;  // Low 16 bits of ECX = command
        let ebx = regs.rbx as u32;

        match cmd {
            CMD_GET_VERSION => {
                // Return VMware magic in EBX, version 6 in EAX, product=1 (WS) in ECX
                regs.rax = 6; // VMware version 6
                regs.rbx = VMWARE_MAGIC as u64;
                regs.rcx = 1; // Product: Workstation
                true
            }

            CMD_GET_CURSOR_POS => {
                // Return cursor position in EAX (x) and EBX (y)
                regs.rax = self.abs_x.load(Ordering::Relaxed) as u64;
                regs.rbx = self.abs_y.load(Ordering::Relaxed) as u64;
                true
            }

            CMD_ABSPOINTER_DATA => {
                // Read absolute pointer data packet.
                // The vmmouse driver reads 4 dwords:
                //   word 0 (EAX): status/buttons
                //   word 1 (EBX): X position
                //   word 2 (ECX): Y position
                //   word 3 (EDX): wheel
                let buttons = self.buttons.load(Ordering::Relaxed) as u32;
                let x = self.abs_x.load(Ordering::Relaxed) as u32;
                let y = self.abs_y.load(Ordering::Relaxed) as u32;
                let wheel = self.wheel.swap(0, Ordering::Relaxed) as i32 as u32;

                // Status word: bits 16-19 = number of words available (4)
                // bits 0-2 = button state
                regs.rax = (4 << 16) | (buttons & 0x07) as u64;
                regs.rbx = x as u64;
                regs.rcx = y as u64;
                regs.rdx = wheel as u64;

                self.data_pending.store(false, Ordering::Release);
                true
            }

            CMD_ABSPOINTER_STATUS => {
                // Return status: number of data words available.
                // If data_pending, return 4 (4 dwords available).
                // Otherwise return 0.
                let words = if self.data_pending.load(Ordering::Acquire) { 4u32 } else { 0 };
                regs.rax = words as u64;
                true
            }

            CMD_ABSPOINTER_COMMAND => {
                // Sub-command in EBX
                match ebx {
                    ABSPOINTER_ENABLE => {
                        self.enabled.store(true, Ordering::Relaxed);
                        self.absolute.store(true, Ordering::Relaxed);
                        regs.rax = 0; // success
                    }
                    ABSPOINTER_RELATIVE => {
                        self.absolute.store(false, Ordering::Relaxed);
                        regs.rax = 0;
                    }
                    ABSPOINTER_ABSOLUTE => {
                        self.absolute.store(true, Ordering::Relaxed);
                        regs.rax = 0;
                    }
                    0 => {
                        // Disable / reset
                        self.enabled.store(false, Ordering::Relaxed);
                        self.absolute.store(false, Ordering::Relaxed);
                        regs.rax = 0;
                    }
                    _ => {
                        regs.rax = 0; // unknown sub-command, ignore
                    }
                }
                true
            }

            _ => {
                // Unknown command — return magic anyway so guest knows we're VMware
                regs.rax = 0;
                regs.rbx = VMWARE_MAGIC as u64;
                true
            }
        }
    }
}
