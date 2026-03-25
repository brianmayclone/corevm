//! C FFI layer for libcorevm.
//!
//! All `extern "C"` functions that form the public API consumed by the VM
//! daemon (vmd) and other C/C++ callers. A global VM registry maps opaque
//! `u64` handles to [`Vm`] instances.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::vm::Vm;
use crate::backend::types::*;
#[cfg(feature = "linux")]
use crate::backend::VmBackend;
use crate::backend::VmExitReason;

// ── I/O dispatch lock ───────────────────────────────────────────────────────
//
// Multiple vCPU threads call handle_io_exit / handle_mmio_exit concurrently.
// Device handlers are NOT thread-safe, so access must be serialised.
//
// Spin briefly for the common uncontended case, then sleep to avoid burning
// CPU cycles when another vCPU holds the lock (especially during AHCI DMA).
// Using sleep(1μs) instead of yield_now() is critical: yield_now() calls
// sched_yield() which doesn't actually sleep — it just moves the thread to
// the end of the runqueue, wasting CPU time that the lock holder needs.
static IO_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
fn io_lock() {
    if IO_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        return;
    }
    io_lock_slow();
}

#[cold]
fn io_lock_slow() {
    let mut spin_count = 0u32;
    loop {
        if IO_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return;
        }
        spin_count += 1;
        if spin_count < 64 {
            core::hint::spin_loop();
        } else {
            // Sleep briefly so the lock holder can make progress.
            #[cfg(feature = "std")]
            std::thread::sleep(core::time::Duration::from_micros(1));
            #[cfg(not(feature = "std"))]
            core::hint::spin_loop();
            spin_count = 0;
        }
    }
}

#[inline]
fn io_unlock() {
    IO_LOCK.store(false, Ordering::Release);
}

// ── AHCI device lock ────────────────────────────────────────────────────────
//
// Dedicated lock for AHCI device state, separate from IO_LOCK so that
// port I/O exits don't block on AHCI disk I/O and vice versa.
// AHCI command processing does synchronous disk I/O (pread/pwrite)
// which can block for milliseconds.
static AHCI_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
pub(crate) fn ahci_lock() {
    if AHCI_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        return;
    }
    ahci_lock_slow();
}

#[cold]
fn ahci_lock_slow() {
    #[cfg(feature = "std")]
    let t0 = std::time::Instant::now();
    let mut spin_count = 0u32;
    let mut sleep_count = 0u32;
    loop {
        if AHCI_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            #[cfg(feature = "std")]
            {
                let wait_us = t0.elapsed().as_micros() as u64;
                if wait_us > 1000 {
                    static CONTENTION_N: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
                    let n = CONTENTION_N.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    if n < 50 || n % 200 == 0 {
                        eprintln!("[ahci-diag] LOCK contention: waited {}us (sleeps={}) #{}", wait_us, sleep_count, n);
                    }
                }
            }
            return;
        }
        spin_count += 1;
        if spin_count < 64 {
            core::hint::spin_loop();
        } else {
            #[cfg(feature = "std")]
            std::thread::sleep(core::time::Duration::from_micros(1));
            #[cfg(not(feature = "std"))]
            core::hint::spin_loop();
            spin_count = 0;
            sleep_count += 1;
        }
    }
}

#[inline]
pub(crate) fn ahci_unlock() {
    AHCI_LOCK.store(false, Ordering::Release);
}

/// Try to acquire AHCI_LOCK without blocking. Returns true if acquired.
#[inline]
fn ahci_try_lock() -> bool {
    AHCI_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok()
}

// ── MMIO device lock ───────────────────────────────────────────────────────
//
// Serialises MMIO access to non-AHCI devices (VGA/SVGA, E1000, VirtIO).
// AHCI has its own AHCI_LOCK and is excluded. Without this lock, multiple
// vCPU threads can race on device state (VBE registers, framebuffer Vec,
// E1000 ring descriptors) causing segfaults and data corruption.
//
// This lock is NOT held for AHCI MMIO — AHCI uses deferred I/O with
// AHCI_LOCK to avoid blocking other vCPUs during disk I/O.
static MMIO_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
fn mmio_lock() {
    if MMIO_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        return;
    }
    mmio_lock_slow();
}

#[cold]
fn mmio_lock_slow() {
    let mut spin_count = 0u32;
    loop {
        if MMIO_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            return;
        }
        spin_count += 1;
        if spin_count < 64 {
            core::hint::spin_loop();
        } else {
            #[cfg(feature = "std")]
            std::thread::sleep(core::time::Duration::from_micros(1));
            #[cfg(not(feature = "std"))]
            core::hint::spin_loop();
            spin_count = 0;
        }
    }
}

#[inline]
fn mmio_unlock() {
    MMIO_LOCK.store(false, Ordering::Release);
}

// ── Per-vCPU I/O context ────────────────────────────────────────────────────
//
// Tracks which vCPU is currently executing a port I/O handler.
// Protected by IO_LOCK (only one vCPU dispatches port I/O at a time).
// Used by PciBus to maintain per-vCPU PCI config address latches,
// preventing the 0xCF8/0xCFC TOCTOU race between vCPUs.
use core::sync::atomic::AtomicU32;
static CURRENT_IO_VCPU: AtomicU32 = AtomicU32::new(0);

/// Returns the vCPU ID currently holding IO_LOCK.
pub(crate) fn current_io_vcpu() -> u32 {
    CURRENT_IO_VCPU.load(Ordering::Relaxed)
}

// ── Global VM registry ──────────────────────────────────────────────────────

static mut VMS: Option<Vec<Option<Vm>>> = None;
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static mut LAST_ERROR: Option<String> = None;

fn set_last_error(msg: String) {
    unsafe { LAST_ERROR = Some(msg); }
}

fn clear_last_error() {
    unsafe { LAST_ERROR = None; }
}

fn vm_list() -> &'static mut Vec<Option<Vm>> {
    unsafe {
        if VMS.is_none() {
            VMS = Some(Vec::new());
        }
        VMS.as_mut().unwrap()
    }
}

pub fn get_vm(handle: u64) -> Option<&'static mut Vm> {
    if handle == 0 {
        return None;
    }
    let idx = (handle - 1) as usize;
    vm_list().get_mut(idx).and_then(|slot| slot.as_mut())
}

// ── C-compatible exit reason ────────────────────────────────────────────────

/// C-compatible tagged struct for VM exit reasons.
///
/// The `reason` field selects which union members are valid:
/// 0=IoIn, 1=IoOut, 2=MmioRead, 3=MmioWrite, 4=MsrRead, 5=MsrWrite,
/// 6=Cpuid, 7=Halted, 8=InterruptWindow, 9=Shutdown, 10=Debug, 11=Error,
/// 12=StringIo
#[repr(C)]
#[derive(Default)]
pub struct CExitReason {
    pub reason: u32,
    pub port: u16,
    pub size: u8,
    pub _pad: u8,
    pub data_u32: u32,
    pub io_count: u32,
    pub addr: u64,
    pub data_u64: u64,
    pub msr_index: u32,
    pub cpuid_fn: u32,
    pub cpuid_idx: u32,
    pub mmio_dest_reg: u8,
    pub mmio_instr_len: u8,
    pub _reserved: [u8; 2],
    // StringIo fields (reason=12)
    pub string_io_count: u64,
    pub string_io_gpa: u64,
    pub string_io_step: i64,
    pub string_io_instr_len: u64,
    pub string_io_is_write: u8,
    pub string_io_addr_size: u8,
    pub _reserved2: [u8; 6],
}

fn fill_exit(e: &mut CExitReason, reason: VmExitReason) {
    *e = CExitReason::default();
    match reason {
        VmExitReason::IoIn { port, size, count } => {
            e.reason = 0; e.port = port; e.size = size; e.io_count = count;
        }
        VmExitReason::IoOut { port, size, data, count } => {
            e.reason = 1; e.port = port; e.size = size; e.data_u32 = data; e.io_count = count;
        }
        VmExitReason::MmioRead { addr, size, dest_reg, instr_len } => {
            e.reason = 2; e.addr = addr; e.size = size;
            e.mmio_dest_reg = dest_reg; e.mmio_instr_len = instr_len;
        }
        VmExitReason::MmioWrite { addr, size, data } => {
            e.reason = 3; e.addr = addr; e.size = size; e.data_u64 = data;
        }
        VmExitReason::MsrRead { index } => {
            e.reason = 4; e.msr_index = index;
        }
        VmExitReason::MsrWrite { index, value } => {
            e.reason = 5; e.msr_index = index; e.data_u64 = value;
        }
        VmExitReason::CpuidExit { function, index } => {
            e.reason = 6; e.cpuid_fn = function; e.cpuid_idx = index;
        }
        VmExitReason::StringIo { port, is_write, count, gpa, step, instr_len, addr_size, access_size } => {
            e.reason = 12; e.port = port; e.size = access_size;
            e.string_io_count = count;
            e.string_io_gpa = gpa;
            e.string_io_step = step;
            e.string_io_instr_len = instr_len;
            e.string_io_is_write = if is_write { 1 } else { 0 };
            e.string_io_addr_size = addr_size;
        }
        VmExitReason::Halted => e.reason = 7,
        VmExitReason::InterruptWindow => e.reason = 8,
        VmExitReason::Shutdown => e.reason = 9,
        VmExitReason::Debug => e.reason = 10,
        VmExitReason::Error => e.reason = 11,
        VmExitReason::Cancelled => e.reason = 13,
    }
}

// ── VM lifecycle ────────────────────────────────────────────────────────────

/// Create a new VM with the given RAM size in megabytes.
/// Returns a non-zero handle on success, 0 on failure.
#[no_mangle]
pub extern "C" fn corevm_create(ram_mb: u32) -> u64 {
    clear_last_error();
    match Vm::new(ram_mb) {
        Ok(vm) => {
            let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
            let idx = (handle - 1) as usize;
            let list = vm_list();
            while list.len() <= idx {
                list.push(None);
            }
            list[idx] = Some(vm);
            handle
        }
        Err(e) => {
            set_last_error(format!("{}", e));
            0
        }
    }
}

/// Destroy a VM and release all resources.
#[no_mangle]
pub extern "C" fn corevm_destroy(handle: u64) {
    if handle == 0 { return; }
    let idx = (handle - 1) as usize;
    if let Some(slot) = vm_list().get_mut(idx) {
        if let Some(mut vm) = slot.take() {
            vm.destroy_backend();
        }
    }
}

/// Get the last error message. Returns a pointer to a null-terminated UTF-8
/// string, or null if no error. The pointer is valid until the next FFI call.
#[no_mangle]
pub extern "C" fn corevm_last_error() -> *const u8 {
    unsafe {
        match &LAST_ERROR {
            Some(s) => s.as_ptr(),
            None => core::ptr::null(),
        }
    }
}

/// Get the length of the last error message (excluding null terminator).
/// Returns 0 if no error.
#[no_mangle]
pub extern "C" fn corevm_last_error_len() -> u32 {
    unsafe {
        match &LAST_ERROR {
            Some(s) => s.len() as u32,
            None => 0,
        }
    }
}

/// Reset the VM. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn corevm_reset(handle: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => if vm.reset().is_ok() { 0 } else { -1 },
        None => -1,
    }
}

// ── vCPU management ─────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn corevm_create_vcpu(handle: u64, vcpu_id: u32) -> i32 {
    match get_vm(handle) {
        Some(vm) => match vm.create_vcpu(vcpu_id) {
            Ok(()) => {
                // APs (vcpu_id > 0): set to INIT_RECEIVED so they wait
                // for SIPI from the BSP.
                #[cfg(feature = "linux")]
                if vcpu_id > 0 {
                    let apic_base = 0xFEE0_0000u64 | (1 << 11);
                    let _ = vm.backend.set_msrs(vcpu_id, &[
                        (0x1B, apic_base),     // IA32_APIC_BASE: APIC enabled, not BSP
                        (0x10, 0),             // IA32_TSC: synchronize with BSP (start at 0)
                        (0x3B, 0),             // IA32_TSC_ADJUST: no offset
                    ]);
                    // KVM_MP_STATE_INIT_RECEIVED (2) — AP waits for SIPI
                    let _ = vm.backend.set_mp_state(vcpu_id, 2);
                }
                // BSP: also ensure TSC starts at 0 for consistency
                #[cfg(feature = "linux")]
                if vcpu_id == 0 {
                    let _ = vm.backend.set_msrs(vcpu_id, &[
                        (0x10, 0),             // IA32_TSC: start at 0
                        (0x3B, 0),             // IA32_TSC_ADJUST: no offset
                    ]);
                }
                0
            }
            Err(e) => { set_last_error(format!("{}", e)); -1 }
        },
        None => { set_last_error("no VM handle".into()); -1 },
    }
}

#[no_mangle]
pub extern "C" fn corevm_destroy_vcpu(handle: u64, vcpu_id: u32) -> i32 {
    match get_vm(handle) {
        Some(vm) => if vm.destroy_vcpu(vcpu_id).is_ok() { 0 } else { -1 },
        None => -1,
    }
}

/// Run a vCPU until it exits. Fills `exit` with the exit reason.
#[no_mangle]
pub extern "C" fn corevm_run_vcpu(handle: u64, vcpu_id: u32, exit: *mut CExitReason) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    match vm.run_vcpu(vcpu_id) {
        Ok(reason) => {
            if !exit.is_null() {
                fill_exit(unsafe { &mut *exit }, reason);
            }
            0
        }
        Err(e) => { set_last_error(format!("{}", e)); -1 }
    }
}

// ── Register access ─────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn corevm_get_vcpu_regs(handle: u64, vcpu_id: u32, regs: *mut VcpuRegs) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if regs.is_null() { return -1; }
    match vm.get_vcpu_regs(vcpu_id) {
        Ok(r) => { unsafe { *regs = r; } 0 }
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn corevm_set_vcpu_regs(handle: u64, vcpu_id: u32, regs: *const VcpuRegs) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if regs.is_null() { return -1; }
    if vm.set_vcpu_regs(vcpu_id, unsafe { &*regs }).is_ok() { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn corevm_get_vcpu_sregs(handle: u64, vcpu_id: u32, sregs: *mut VcpuSregs) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if sregs.is_null() { return -1; }
    match vm.get_vcpu_sregs(vcpu_id) {
        Ok(s) => { unsafe { *sregs = s; } 0 }
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn corevm_set_vcpu_sregs(handle: u64, vcpu_id: u32, sregs: *const VcpuSregs) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => { set_last_error("no VM handle".into()); return -1 } };
    if sregs.is_null() { set_last_error("null sregs".into()); return -1; }
    match vm.set_vcpu_sregs(vcpu_id, unsafe { &*sregs }) {
        Ok(()) => 0,
        Err(e) => { set_last_error(format!("{}", e)); -1 }
    }
}

/// Read the in-kernel LAPIC register page (1024 bytes).
/// Only available on Linux/KVM. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn corevm_get_lapic(handle: u64, vcpu_id: u32, buf: *mut u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if buf.is_null() { return -1; }
    #[cfg(feature = "linux")]
    {
        match vm.backend.get_lapic(vcpu_id) {
            Ok(data) => { unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), buf, 1024); } 0 }
            Err(_) => -1,
        }
    }
    #[cfg(not(feature = "linux"))]
    { -1 }
}

/// Read the in-kernel irqchip state (512 bytes).
/// chip_id: 0=PIC master, 1=PIC slave, 2=IOAPIC.
#[no_mangle]
pub extern "C" fn corevm_get_irqchip(handle: u64, chip_id: u32, buf: *mut u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if buf.is_null() { return -1; }
    #[cfg(feature = "linux")]
    {
        match vm.backend.get_irqchip(chip_id) {
            Ok(data) => {
                let len = data.len().min(512);
                unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), buf, len); }
                0
            }
            Err(_) => -1,
        }
    }
    #[cfg(not(feature = "linux"))]
    { -1 }
}

// ── Interrupt injection ─────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn corevm_inject_interrupt(handle: u64, vcpu_id: u32, vector: u8) -> i32 {
    match get_vm(handle) {
        Some(vm) => if vm.inject_interrupt(vcpu_id, vector).is_ok() { 0 } else { -1 },
        None => -1,
    }
}

/// Cancel a running vCPU, causing run_vcpu to return with Cancelled.
/// Safe to call from any thread — uses per-VM atomic slots, no VM registry access needed.
#[no_mangle]
pub extern "C" fn corevm_cancel_vcpu(handle: u64, vcpu_id: u32) -> i32 {
    #[cfg(feature = "linux")]
    {
        // Look up the VM's slot index from its backend
        let vm_slot = match get_vm(handle) {
            Some(vm) => vm.backend.vm_slot,
            None => return -1,
        };
        return crate::backend::kvm::cancel_vcpu_kvm(vm_slot, vcpu_id);
    }
    #[cfg(feature = "anyos")]
    { return 0; }
    #[allow(unreachable_code)]
    { let _ = (handle, vcpu_id); 0 }
}

/// Advance the PIT timer by `ticks` clock cycles.
/// If channel 0 fires, raises IRQ0 on the PIC and injects the resulting
/// interrupt vector into vCPU 0. Returns the number of IRQ0 fires.
///
/// On Linux/KVM: the in-kernel PIT handles channel 0 IRQ 0 automatically,
/// but we still need to tick the userspace PIT for channel 2 (used by
/// port 0x61 for BIOS/bootloader delay loops).
#[no_mangle]
pub extern "C" fn corevm_pit_advance(handle: u64, ticks: u32) -> u32 {
    #[cfg(feature = "linux")]
    {
        // Sync channel 2 config from in-kernel PIT, then tick userspace PIT.
        // The in-kernel PIT handles port 0x40-0x43 writes (mode, count config),
        // but port 0x61 reads the userspace PIT's channel 2 output.
        match get_vm(handle) {
            Some(vm) => {
                let (count, _status, mode, gate, ret) = vm.backend.get_pit2_debug();
                if ret >= 0 {
                    if let Some(pit) = vm.pit_mut() {
                        let ch = &mut pit.channels[2];
                        // Sync configuration from in-kernel PIT to userspace PIT
                        // only if config changed (mode or count written by guest)
                        if mode < 6 && (!ch.enabled || ch.mode != mode || ch.count != count as u16) {
                            ch.mode = mode;
                            ch.count = count as u16;
                            ch.current = count as u16;
                            ch.gate = gate != 0;
                            ch.enabled = true;
                            // Modes 2,3 start with output HIGH; mode 0 starts LOW
                            ch.output = matches!(mode, 2 | 3);
                        }
                        ch.gate = gate != 0;
                        // Tick channel 2 only (not channel 0 — in-kernel PIT does that)
                        for _ in 0..ticks {
                            ch.tick();
                        }
                    }
                }
                0
            }
            None => 0,
        }
    }

    #[cfg(not(feature = "linux"))]
    match get_vm(handle) {
        Some(vm) => {
            let fires = if let Some(pit) = vm.pit_mut() {
                pit.advance(ticks)
            } else {
                return 0;
            };
            if fires > 0 {
                // Route PIT timer through IOAPIC *or* PIC, not both.
                // Using both causes PendingEvent conflicts: the PIC injection
                // in poll_irqs overwrites the IOAPIC injection with a different
                // vector, breaking Linux's check_timer() which expects the
                // IOAPIC vector specifically.
                {
                    if !vm.pic_ptr.is_null() {
                        let pic = unsafe { &mut *vm.pic_ptr };
                        pic.raise_irq(0);
                    }
                }
            }
            fires
        }
        None => 0,
    }
}

/// Return PIT channel 0 debug info: mode | (enabled << 8) | (output << 9) | (current << 16)
#[no_mangle]
pub extern "C" fn corevm_pit_debug(handle: u64) -> u64 {
    match get_vm(handle) {
        Some(vm) => {
            if let Some(pit) = vm.pit_mut() {
                let ch = &pit.channels[0];
                (ch.mode as u64)
                    | ((ch.enabled as u64) << 8)
                    | ((ch.output as u64) << 9)
                    | ((ch.current as u64) << 16)
                    | ((ch.count as u64) << 32)
            } else { 0 }
        }
        None => 0,
    }
}

/// Advance the CMOS RTC periodic timer by `ticks_32768` ticks of the
/// 32.768 kHz base clock. Returns 1 if IRQ 8 should fire, 0 otherwise.
/// On KVM, raises IRQ 8 via KVM_IRQ_LINE when the periodic timer fires.
#[no_mangle]
pub extern "C" fn corevm_cmos_advance(handle: u64, ticks_32768: u64) -> u32 {
    match get_vm(handle) {
        Some(vm) => {
            let fired = if let Some(cmos) = vm.cmos_mut() {
                cmos.advance(ticks_32768)
            } else {
                return 0;
            };
            if fired {
                #[cfg(feature = "linux")]
                {
                    let _ = vm.backend.set_irq_line(8, true);
                    let _ = vm.backend.set_irq_line(8, false);
                }
                #[cfg(not(feature = "linux"))]
                {
                    if !vm.pic_ptr.is_null() {
                        let pic = unsafe { &mut *vm.pic_ptr };
                        pic.raise_irq(8);
                    }
                }
                1
            } else {
                0
            }
        }
        None => 0,
    }
}

/// Poll all device IRQ sources and inject any pending interrupts.
///
/// Checks PS/2 keyboard (IRQ 1) and mouse (IRQ 12). Proactively drains
/// device buffers and fires IRQs — does NOT rely on the `irq_needed` flag
/// alone, since it may be set from a different thread (UI thread) without
/// memory barriers.
///
/// On Linux/KVM: uses KVM_IRQ_LINE to signal the in-kernel irqchip.
/// The in-kernel PIC/IOAPIC/LAPIC handles vector injection automatically.
#[no_mangle]
pub extern "C" fn corevm_poll_irqs(handle: u64) -> u32 {
    let result = match get_vm(handle) {
        Some(vm) => {
            let mut injected = 0u32;

            // IO_LOCK: PS/2 and Serial device state is shared with AP threads
            // via I/O port exits (port 0x60/0x64 for PS/2, 0x3F8 for Serial).
            // Hold io_lock while accessing these devices to prevent data races.
            io_lock();

            // Drain pending mouse events from the thread-safe queue into
            // the PS/2 controller.
            #[cfg(feature = "std")]
            {
                let events: alloc::vec::Vec<(i16, i16, u8, i8)> = {
                    if let Ok(mut queue) = vm.pending_mouse.lock() {
                        queue.drain(..).collect()
                    } else {
                        alloc::vec::Vec::new()
                    }
                };
                if !events.is_empty() {
                    if let Some(ps2) = vm.ps2() {
                        for (dx, dy, buttons, wheel) in events {
                            ps2.mouse_move_wheel(dx, dy, buttons, wheel);
                        }
                    }
                }
            }

            // PS/2 keyboard → IRQ 1, mouse → IRQ 12
            if let Some(ps2) = vm.ps2() {
                // Proactively try to fill output buffer from device buffers.
                ps2.try_fill_output();

                // Fire IRQ if output buffer has data ready for the guest.
                // Check the actual buffer state rather than relying solely
                // on irq_needed (which may not be visible cross-thread).
                let need_irq = ps2.irq_needed
                    || (ps2.status & 0x01 != 0); // STATUS_OUTPUT_FULL
                if need_irq {
                    ps2.irq_needed = false;
                    // Use IRQ 12 for mouse data, IRQ 1 for keyboard data.
                    // Linux's i8042 driver registers separate handlers for
                    // IRQ 1 (KBD) and IRQ 12 (AUX). During AUX detection,
                    // i8042_check_aux registers a test handler on IRQ 12
                    // and expects IRQ 12 to fire for loopback data.
                    // KVM_IRQ_LINE signals BOTH PIC and IOAPIC, so even if
                    // IOAPIC pin 12 is masked, the PIC still delivers it.
                    let is_mouse_data = ps2.status & 0x20 != 0; // STATUS_MOUSE_DATA
                    let irq: u8 = if is_mouse_data { 12 } else { 1 };
                    #[cfg(feature = "linux")]
                    {
                        let _ = vm.backend.set_irq_line(irq as u32, true);
                        let _ = vm.backend.set_irq_line(irq as u32, false);
                        injected += 1;
                    }
                    #[cfg(not(feature = "linux"))]
                    {
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(irq);
                        }
                    }
                }
            }

            // Serial COM1 IRQ → IRQ 4 (edge-triggered)
            // Fire when the serial device has a new interrupt pending (THRE or RXDA).
            if let Some(serial) = vm.serial() {
                if serial.irq_pending {
                    serial.irq_pending = false;
                    #[cfg(feature = "linux")]
                    {
                        let _ = vm.backend.set_irq_line(4, true);
                        let _ = vm.backend.set_irq_line(4, false);
                        injected += 1;
                    }
                    #[cfg(not(feature = "linux"))]
                    {
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(4);
                        }
                    }
                }
            }

            io_unlock();

            // AHCI IRQ → MSI (preferred) or legacy IRQ 11 (level-triggered)
            // Use try_lock: if an AP holds AHCI_LOCK (doing disk I/O), skip
            // this iteration. The AP delivers IRQs via corevm_ahci_poll_irq().
            // fix_stuck_irq() runs on every successful lock to recover from
            // any inconsistency between port IS and irq_pending.
            if !vm.ahci_ptr.is_null() && ahci_try_lock() {
                let ahci = unsafe { &mut *vm.ahci_ptr };
                // Auto-recover stuck IRQ: is!=0 but irq_pending=false
                let was_stuck = ahci.fix_stuck_irq();
                let want_asserted = ahci.irq_raised();

                #[cfg(feature = "linux")]
                {
                    if ahci.msi_enabled {
                        if want_asserted {
                            let _ = vm.backend.signal_msi(ahci.msi_address, ahci.msi_data);
                            ahci.clear_irq();
                            injected += 1;
                        }
                    } else {
                        // Legacy level-triggered IRQ 11.
                        if want_asserted && !vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                            let _ = vm.backend.set_irq_line(11, true);
                            vm.ahci_irq_asserted.store(true, Ordering::Relaxed);
                            injected += 1;
                        } else if want_asserted && was_stuck && vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                            // Toggle low→high to clear IOAPIC Remote IRR
                            let _ = vm.backend.set_irq_line(11, false);
                            let _ = vm.backend.set_irq_line(11, true);
                            injected += 1;
                        } else if !want_asserted && vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                            vm.ahci_irq_asserted.store(false, Ordering::Relaxed);
                            if !vm.e1000_irq_asserted.load(Ordering::Relaxed) && !vm.virtio_net_irq_asserted.load(Ordering::Relaxed) {
                                let _ = vm.backend.set_irq_line(11, false);
                            }
                        }
                    }
                }
                #[cfg(not(feature = "linux"))]
                {
                    if want_asserted {
                        ahci.clear_irq();
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(11);
                        }
                    }
                }
                ahci_unlock();
            }

            // E1000 NIC — MSI or legacy IRQ 11
            // Under std: E1000 is protected by Arc<Mutex> for SMP-safe access.
            // Under no_std: single-core, raw pointer access.
            #[cfg(feature = "std")]
            if let Some(e1000_arc) = vm.e1000.as_ref() {
                let mut e1000 = e1000_arc.lock().unwrap();

                // Check if guest enabled MSI (read MSI control from PCI config)
                if !vm.pci_bus_ptr.is_null() {
                    let bus = unsafe { &mut *vm.pci_bus_ptr };
                    let e1000_slot = vm.chipset.slots.e1000;
                    let mcr = bus.mmcfg_read(0, e1000_slot, 0, 0xD0 + 2, 2) as u16;
                    e1000.msi_enabled = (mcr & 0x01) != 0;
                    if e1000.msi_enabled {
                        let addr = bus.mmcfg_read(0, e1000_slot, 0, 0xD0 + 4, 4) as u64;
                        let data = bus.mmcfg_read(0, e1000_slot, 0, 0xD0 + 8, 2) as u32;
                        e1000.msi_address = addr;
                        e1000.msi_data = data;
                    }
                }

                // Deliver any pending RX packets to guest via DMA.
                if !e1000.rx_buffer.is_empty() {
                    e1000.process_rx_ring();
                }
                let icr = e1000.regs[0x00C0 / 4];
                let ims = e1000.regs[0x00D0 / 4];
                let want_asserted = (icr & ims) != 0;
                // Extract MSI state before dropping e1000 lock for backend calls
                let msi_enabled = e1000.msi_enabled;
                let msi_address = e1000.msi_address;
                let msi_data = e1000.msi_data;
                if want_asserted && msi_enabled {
                    e1000.regs[0x00C0 / 4] &= !ims;
                }
                drop(e1000); // Release lock before backend calls

                #[cfg(feature = "linux")]
                {
                    if msi_enabled {
                        if want_asserted {
                            let _ = vm.backend.signal_msi(msi_address, msi_data);
                            injected += 1;
                        }
                    } else {
                        let e1000_irq = vm.chipset.irqs.e1000 as u32;
                        if want_asserted {
                            let _ = vm.backend.set_irq_line(e1000_irq, true);
                            let _ = vm.backend.set_irq_line(e1000_irq, false);
                            injected += 1;
                        }
                    }
                }
            }
            #[cfg(not(feature = "std"))]
            if !vm.e1000_ptr.is_null() {
                let e1000 = unsafe { &mut *vm.e1000_ptr };

                if !vm.pci_bus_ptr.is_null() {
                    let bus = unsafe { &mut *vm.pci_bus_ptr };
                    let e1000_slot = vm.chipset.slots.e1000;
                    let mcr = bus.mmcfg_read(0, e1000_slot, 0, 0xD0 + 2, 2) as u16;
                    e1000.msi_enabled = (mcr & 0x01) != 0;
                    if e1000.msi_enabled {
                        let addr = bus.mmcfg_read(0, e1000_slot, 0, 0xD0 + 4, 4) as u64;
                        let data = bus.mmcfg_read(0, e1000_slot, 0, 0xD0 + 8, 2) as u32;
                        e1000.msi_address = addr;
                        e1000.msi_data = data;
                    }
                }

                if !e1000.rx_buffer.is_empty() {
                    e1000.process_rx_ring();
                }
                let icr = e1000.regs[0x00C0 / 4];
                let ims = e1000.regs[0x00D0 / 4];
                let want_asserted = (icr & ims) != 0;
                if want_asserted {
                    if !vm.pic_ptr.is_null() {
                        let pic = unsafe { &mut *vm.pic_ptr };
                        pic.raise_irq(vm.chipset.irqs.e1000);
                    }
                }
            }

            // HPET Timer 0 → IRQ (edge-triggered pulse on KVM)
            if !vm.hpet_ptr.is_null() {
                let hpet = unsafe { &mut *vm.hpet_ptr };
                if hpet.check_timer() {
                    let irq = hpet.timer0_irq();
                    #[cfg(feature = "linux")]
                    {
                        let _ = vm.backend.set_irq_line(irq, true);
                        let _ = vm.backend.set_irq_line(irq, false);
                        injected += 1;
                    }
                    #[cfg(not(feature = "linux"))]
                    {
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(irq as u8);
                        }
                    }
                }
            }

            // UHCI USB → IRQ 9 (edge-triggered)
            if !vm.uhci_ptr.is_null() {
                let uhci = unsafe { &mut *vm.uhci_ptr };
                if uhci.irq_pending {
                    uhci.irq_pending = false;
                    #[cfg(feature = "linux")]
                    {
                        let _ = vm.backend.set_irq_line(9, true);
                        let _ = vm.backend.set_irq_line(9, false);
                        injected += 1;
                    }
                    #[cfg(not(feature = "linux"))]
                    {
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(9);
                        }
                    }
                }
            }

            // VirtIO GPU → IRQ 11 (level-triggered, shares with AHCI/E1000)
            // Only deliver IRQ if the guest driver is active (DRIVER_OK).
            // If no driver is loaded (e.g. Windows using standard VGA),
            // isr_status may be non-zero but nobody reads it to clear it,
            // causing IRQ 11 to stay permanently asserted → IRQ storm that
            // makes the kernel disable IRQ 11 entirely, killing AHCI too.
            // VirtIO GPU — level-triggered IRQ.
            // Read the actual IRQ line from PCI config space (SeaBIOS may
            // remap it via PIRQ routing, overriding our initial value).
            if !vm.virtio_gpu_ptr.is_null() {
                mmio_lock();
                let gpu = unsafe { &mut *vm.virtio_gpu_ptr };
                let want_asserted = gpu.isr_status != 0;
                let gpu_irq = if !vm.pci_bus_ptr.is_null() {
                    let bus = unsafe { &mut *vm.pci_bus_ptr };
                    let line = bus.mmcfg_read(0, vm.chipset.slots.virtio_gpu, 0, 0x3C, 1);
                    if line > 0 && line < 256 { line as u32 } else { vm.chipset.irqs.virtio_gpu as u32 }
                } else {
                    vm.chipset.irqs.virtio_gpu as u32
                };
                #[cfg(feature = "linux")]
                {
                    if want_asserted && !vm.virtio_gpu_irq_asserted.load(Ordering::Relaxed) {
                        vm.virtio_gpu_irq_asserted.store(true, Ordering::Relaxed);
                        let _ = vm.backend.set_irq_line(gpu_irq, true);
                        injected += 1;
                    } else if !want_asserted && vm.virtio_gpu_irq_asserted.load(Ordering::Relaxed) {
                        vm.virtio_gpu_irq_asserted.store(false, Ordering::Relaxed);
                        let _ = vm.backend.set_irq_line(gpu_irq, false);
                    }
                }
                #[cfg(not(feature = "linux"))]
                {
                    if want_asserted {
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(gpu_irq as u8);
                        }
                    }
                }
                mmio_unlock();
            }

            // VirtIO-Net → IRQ 11 (level-triggered, shares with AHCI/E1000/VirtIO GPU)
            if !vm.virtio_net_ptr.is_null() {
                mmio_lock();
                let net = unsafe { &mut *vm.virtio_net_ptr };
                let want_asserted = net.isr_status != 0;
                #[cfg(feature = "linux")]
                {
                    if want_asserted && !vm.virtio_net_irq_asserted.load(Ordering::Relaxed) {
                        vm.virtio_net_irq_asserted.store(true, Ordering::Relaxed);
                        let _ = vm.backend.set_irq_line(11, true);
                        injected += 1;
                    } else if !want_asserted && vm.virtio_net_irq_asserted.load(Ordering::Relaxed) {
                        vm.virtio_net_irq_asserted.store(false, Ordering::Relaxed);
                        if !vm.ahci_irq_asserted.load(Ordering::Relaxed) && !vm.e1000_irq_asserted.load(Ordering::Relaxed) {
                            let _ = vm.backend.set_irq_line(11, false);
                        }
                    }
                }
                #[cfg(not(feature = "linux"))]
                {
                    if want_asserted {
                        if !vm.pic_ptr.is_null() {
                            let pic = unsafe { &mut *vm.pic_ptr };
                            pic.raise_irq(11);
                        }
                    }
                }
                mmio_unlock();
            }

            // VirtIO Input (Keyboard + Tablet) → IRQ 10 (edge-triggered)
            // MMIO_LOCK: VirtIO input devices may be accessed via MMIO from APs.
            if !vm.virtio_kbd_ptr.is_null() {
                mmio_lock();
                let kbd = unsafe { &mut *vm.virtio_kbd_ptr };
                if kbd.isr_status != 0 {
                    #[cfg(feature = "linux")]
                    {
                        let _ = vm.backend.set_irq_line(10, true);
                        let _ = vm.backend.set_irq_line(10, false);
                        injected += 1;
                    }
                }
                mmio_unlock();
            }
            if !vm.virtio_tablet_ptr.is_null() {
                mmio_lock();
                let tablet = unsafe { &mut *vm.virtio_tablet_ptr };
                if tablet.isr_status != 0 {
                    #[cfg(feature = "linux")]
                    {
                        let _ = vm.backend.set_irq_line(10, true);
                        let _ = vm.backend.set_irq_line(10, false);
                        injected += 1;
                    }
                }
                mmio_unlock();
            }

            // Software PIC injection (non-Linux only).
            // On KVM/Linux the in-kernel irqchip handles injection automatically.
            #[cfg(not(feature = "linux"))]
            if !vm.pic_ptr.is_null() {
                let pic = unsafe { &mut *vm.pic_ptr };

                let can_inject = vm.get_vcpu_regs(0)
                    .map(|r| r.rflags & 0x200 != 0)
                    .unwrap_or(false);
                if can_inject {
                    if let Some(vector) = pic.get_interrupt_vector() {
                        if vm.inject_interrupt(0, vector).is_ok() {
                            let irq = pic.irq_for_vector(vector).unwrap_or(0);
                            pic.lower_irq(irq);
                            injected += 1;
                            if pic.get_interrupt_vector().is_some() {
                                let _ = vm.request_interrupt_window(0, true);
                            }
                        }
                    }
                } else {
                    if pic.get_interrupt_vector().is_some() {
                        let _ = vm.request_interrupt_window(0, true);
                    }
                }
            }

            injected
        }
        None => 0,
    };
    result
}

/// Immediately check and update the AHCI IRQ on the in-kernel irqchip.
/// This must be called after every MMIO exit to ensure timely IRQ delivery,
/// because AHCI commands are processed synchronously during MMIO writes and
/// the guest may acknowledge the interrupt before poll_irqs runs.
///
/// Supports both legacy level-triggered IRQ 11 and MSI when the guest has
/// enabled MSI via the PCI capability registers.
#[no_mangle]
pub extern "C" fn corevm_ahci_poll_irq(handle: u64) {
    // Called from BSP and AP threads after MMIO exits. AHCI_LOCK serialises
    // access to AHCI device state and ahci_irq_asserted.
    let vm = match get_vm(handle) { Some(v) => v, None => { return } };
    if vm.ahci_ptr.is_null() { return; }
    ahci_lock();

    // Periodic heartbeat — print AHCI diagnostic summary every ~5 seconds
    #[cfg(feature = "std")]
    {
        static LAST_HEARTBEAT: AtomicU64 = AtomicU64::new(0);
        let now_ms = {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
        };
        let last = LAST_HEARTBEAT.load(Ordering::Relaxed);
        if last == 0 {
            LAST_HEARTBEAT.store(now_ms, Ordering::Relaxed);
        } else if now_ms.wrapping_sub(last) > 5000 {
            LAST_HEARTBEAT.store(now_ms, Ordering::Relaxed);
            let ahci_diag = unsafe { &*vm.ahci_ptr };
            ahci_diag.diag_heartbeat();
        }
    }

    // Sync MSI state from PCI config space and set up irqfd if needed.
    // AHCI is PCI device 00:02.0, MSI cap at offset 0x80.
    if !vm.pci_bus_ptr.is_null() {
        let bus = unsafe { &mut *vm.pci_bus_ptr };
        let msi_cap = crate::devices::ahci::AHCI_MSI_CAP_OFFSET;
        let ahci_slot = vm.chipset.slots.ahci;
        let mcr = bus.mmcfg_read(0, ahci_slot, 0, msi_cap + 2, 2) as u16;
        let ahci = unsafe { &mut *vm.ahci_ptr };
        let was_enabled = ahci.msi_enabled;
        ahci.msi_enabled = (mcr & 0x01) != 0;
        if ahci.msi_enabled {
            let addr_lo = bus.mmcfg_read(0, ahci_slot, 0, msi_cap + 4, 4) as u32;
            let data = bus.mmcfg_read(0, ahci_slot, 0, msi_cap + 8, 2) as u32;
            let addr_changed = ahci.msi_address != addr_lo as u64 || ahci.msi_data != data;
            ahci.msi_address = addr_lo as u64;
            ahci.msi_data = data;

            // Set up or update irqfd routing when MSI is first enabled
            // or when the guest changes the MSI address/data.
            #[cfg(feature = "linux")]
            if (!was_enabled || addr_changed) && addr_lo != 0 {
                if vm.backend.ahci_msi_fd < 0 {
                    let _ = vm.backend.setup_ahci_msi_irqfd(addr_lo as u64, data);
                } else if addr_changed {
                    let _ = vm.backend.update_ahci_msi_route(addr_lo as u64, data);
                }
            }
        }
    }

    let ahci = unsafe { &mut *vm.ahci_ptr };
    // Auto-recover stuck IRQ: is!=0 but irq_pending=false
    let was_stuck = ahci.fix_stuck_irq();
    let want_asserted = ahci.irq_raised();

    #[cfg(feature = "linux")]
    {
        if ahci.msi_enabled {
            // MSI + legacy IRQ belt-and-suspenders approach.
            //
            // MSI with Lowest Priority delivery can lose interrupts on SMP
            // when the target vCPU has IF=0 (edge-triggered = fire-and-forget).
            // KVM_IRQFD would fix this but KVM_SET_GSI_ROUTING fails on some
            // configurations. Instead: send MSI for speed AND assert legacy
            // IRQ 11 as backup. The guest handles whichever arrives first;
            // the other is a harmless no-op (ISR finds PORT_IS already cleared).
            if want_asserted {
                ahci.diag_irqs_delivered += 1;
                // Try irqfd first (kernel-level delivery), fallback to signal_msi
                if vm.backend.ahci_msi_fd >= 0 {
                    let _ = vm.backend.trigger_ahci_msi();
                } else {
                    let _ = vm.backend.signal_msi(ahci.msi_address, ahci.msi_data);
                }
                // Also assert legacy IRQ as safety net
                if !vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                    let _ = vm.backend.set_irq_line(11, true);
                    vm.ahci_irq_asserted.store(true, Ordering::Relaxed);
                }
                ahci.clear_irq();
            } else if vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                // Deassert legacy IRQ when no longer needed
                vm.ahci_irq_asserted.store(false, Ordering::Relaxed);
                if !vm.e1000_irq_asserted.load(Ordering::Relaxed) && !vm.virtio_net_irq_asserted.load(Ordering::Relaxed) {
                    let _ = vm.backend.set_irq_line(11, false);
                }
            }
        } else {
            // Legacy level-triggered mode (MSI not enabled by guest).
            if want_asserted && !vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                ahci.diag_irqs_delivered += 1;
                let _ = vm.backend.set_irq_line(11, true);
                vm.ahci_irq_asserted.store(true, Ordering::Relaxed);
            } else if want_asserted && was_stuck && vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                // IRQ line is already high but guest isn't responding.
                // Toggle low→high to clear IOAPIC Remote IRR and force
                // re-delivery. Without this, a stuck Remote IRR permanently
                // prevents the guest from receiving the level-triggered IRQ.
                ahci.diag_irqs_delivered += 1;
                let _ = vm.backend.set_irq_line(11, false);
                let _ = vm.backend.set_irq_line(11, true);
            } else if !want_asserted && vm.ahci_irq_asserted.load(Ordering::Relaxed) {
                vm.ahci_irq_asserted.store(false, Ordering::Relaxed);
                if !vm.e1000_irq_asserted.load(Ordering::Relaxed) && !vm.virtio_net_irq_asserted.load(Ordering::Relaxed) {
                    let _ = vm.backend.set_irq_line(11, false);
                }
            }
        }
    }
    ahci_unlock();
}

/// Check if the guest has requested a system reset (e.g. PS/2 0xFE, port 0xCF9).
/// Returns 1 if reset was requested (and clears the flag), 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_check_reset(handle: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => {
            // Check PS/2 controller reset request (0xFE to port 0x64)
            if let Some(ps2) = vm.ps2() {
                if ps2.reset_requested {
                    ps2.reset_requested = false;
                    return 1;
                }
            }
            // Check port 0xCF9 reset (stored in VM state)
            if vm.cf9_reset_pending.load(Ordering::Relaxed) {
                vm.cf9_reset_pending.store(false, Ordering::Relaxed);
                return 1;
            }
            0
        }
        None => 0,
    }
}

/// Check if the guest has requested ACPI shutdown (SLP_EN + SLP_TYP=S5).
/// Returns 1 if shutdown was requested (and clears the flag), 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_check_acpi_shutdown(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.acpi_pm_ptr.is_null() { return 0; }
    let acpi = unsafe { &mut *vm.acpi_pm_ptr };
    if acpi.shutdown_requested {
        acpi.shutdown_requested = false;
        return 1;
    }
    0
}

/// Debug: return PIC master state as packed u32.
/// bits 0-7: IRR, 8-15: IMR, 16-23: ISR, 24: icw_step>0
#[no_mangle]
pub extern "C" fn corevm_pic_debug(handle: u64) -> u32 {
    match get_vm(handle) {
        Some(vm) => {
            if vm.pic_ptr.is_null() { return 0xDEAD; }
            let pic = unsafe { &*vm.pic_ptr };
            (pic.master.irr as u32)
                | ((pic.master.imr as u32) << 8)
                | ((pic.master.isr as u32) << 16)
                | if pic.master.icw_step > 0 { 1 << 24 } else { 0 }
        }
        None => 0xDEAD,
    }
}

/// Poll LAPIC timer (TSC-based). Injects interrupt if timer fired and IF=1.
/// Returns the vector injected (>0) or 0.
///
/// On KVM, the in-kernel LAPIC handles the timer internally.
///
/// This function is only useful for the anyOS backend's software LAPIC.
#[no_mangle]
pub extern "C" fn corevm_lapic_timer_advance(handle: u64, _ticks: u64) -> u32 {
    // KVM in-kernel LAPIC handles the timer internally.
    // Only the anyOS software LAPIC path would need polling here.
    let _ = (handle, _ticks);
    0
}

/// Debug: return LAPIC timer state.
/// Returns [armed:1|pending:1|divide:8|mode:2|vec:8|masked:1] in low bits,
/// and writes initial_count and current_count to out pointers.
#[no_mangle]
pub extern "C" fn corevm_lapic_debug(handle: u64, out_initial: *mut u32, out_current: *mut u32, out_lvt: *mut u32) -> u32 {
    let _ = (handle, out_initial, out_current, out_lvt); return 0;
}

/// Inject an exception. Pass `error_code` < 0 for no error code.
#[no_mangle]
pub extern "C" fn corevm_inject_exception(handle: u64, vcpu_id: u32, vector: u8, error_code: i64) -> i32 {
    let ec = if error_code < 0 { None } else { Some(error_code as u32) };
    match get_vm(handle) {
        Some(vm) => if vm.inject_exception(vcpu_id, vector, ec).is_ok() { 0 } else { -1 },
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn corevm_inject_nmi(handle: u64, vcpu_id: u32) -> i32 {
    match get_vm(handle) {
        Some(vm) => if vm.inject_nmi(vcpu_id).is_ok() { 0 } else { -1 },
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn corevm_request_interrupt_window(handle: u64, vcpu_id: u32, enable: u8) -> i32 {
    match get_vm(handle) {
        Some(vm) => if vm.request_interrupt_window(vcpu_id, enable != 0).is_ok() { 0 } else { -1 },
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn corevm_set_cpuid(handle: u64, entries: *const CpuidEntry, count: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if entries.is_null() && count > 0 { return -1; }
    let slice = if count > 0 {
        unsafe { core::slice::from_raw_parts(entries, count as usize) }
    } else {
        &[]
    };
    if vm.set_cpuid(slice).is_ok() { 0 } else { -1 }
}

// ── Memory ──────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn corevm_set_memory_region(
    handle: u64, slot: u32, guest_phys: u64, size: u64, host_ptr: *mut u8,
) -> i32 {
    match get_vm(handle) {
        Some(vm) => if vm.set_memory_region(slot, guest_phys, size, host_ptr).is_ok() { 0 } else { -1 },
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn corevm_read_phys(handle: u64, addr: u64, buf: *mut u8, len: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if buf.is_null() { return -1; }
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, len as usize) };
    if vm.read_phys(addr, slice).is_ok() { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn corevm_write_phys(handle: u64, addr: u64, buf: *const u8, len: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if buf.is_null() { return -1; }
    let slice = unsafe { core::slice::from_raw_parts(buf, len as usize) };
    if vm.write_phys(addr, slice).is_ok() { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn corevm_load_binary(handle: u64, guest_phys: u64, data: *const u8, len: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if data.is_null() && len > 0 { return -1; }
    let slice = if len > 0 {
        unsafe { core::slice::from_raw_parts(data, len as usize) }
    } else {
        &[]
    };
    if vm.load_binary(guest_phys, slice).is_ok() { 0 } else { -1 }
}

// ── Hardware support ────────────────────────────────────────────────────────

/// Returns the hardware virtualization type:
///   0 = none / not available
///   1 = Intel VT-x (VMX)
///   2 = AMD-V (SVM)
///
/// A return value != 0 means hardware virtualization is available.
#[no_mangle]
pub extern "C" fn corevm_has_hw_support() -> i32 {
    #[cfg(feature = "linux")]
    {
        return match crate::backend::kvm::KvmBackend::new() {
            Ok(mut b) => { b.destroy(); 1 }
            Err(_) => 0,
        };
    }
    #[cfg(feature = "anyos")]
    {
        // Ask the kernel: 0=none, 1=VMX, 2=SVM.
        return unsafe { crate::backend::anyos::syscall_vm_hw_info() as i32 };
    }
    #[allow(unreachable_code)]
    0
}

// ── I/O and MMIO exit dispatch ──────────────────────────────────────────────

/// Dispatch a port I/O exit to the registered device handler.
///
/// Handle a bulk string I/O exit (REP INSB/OUTSB).
///
/// Performs the entire transfer in one call: reads/writes guest memory and
/// invokes the I/O handler for each byte. Updates guest registers afterward.
#[no_mangle]
pub extern "C" fn corevm_handle_string_io_exit(
    handle: u64, vcpu_id: u32, port: u16, is_write: u8, count: u64, gpa: u64,
    step: i64, instr_len: u64, addr_size: u8, access_size: u8,
) -> i32 {
    io_lock();
    CURRENT_IO_VCPU.store(vcpu_id, Ordering::Relaxed);
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    vm.handle_string_io(vcpu_id, port, is_write != 0, count, gpa, step, instr_len, addr_size, access_size);
    io_unlock();
    0
}

/// For reads (`is_write`=0), `data` is filled with the result.
/// For writes (`is_write`=1), `data` contains the guest-written value.
#[no_mangle]
pub extern "C" fn corevm_handle_io_exit(
    handle: u64, vcpu_id: u32, port: u16, is_write: u8, size: u8, data: *mut u8,
) -> i32 {
    io_lock();
    CURRENT_IO_VCPU.store(vcpu_id, Ordering::Relaxed);
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    if data.is_null() { io_unlock(); return -1; }

    // ── VMware Backdoor intercept ──
    // The VMware backdoor protocol uses port 0x5658 with full register state
    // (EAX=cmd, EBX/ECX/EDX/ESI/EDI=params). We intercept here before normal
    // I/O dispatch because IoHandler only sees (port, val), not all registers.
    if port == crate::devices::vmware::VMWARE_PORT && is_write == 0 {
        if let Ok(mut regs) = vm.get_vcpu_regs(vcpu_id) {
            // Check if EDX contains VMware magic (0x564D5868 = "VMXh")
            if (regs.rdx as u32) == 0x564D5868 {
                if vm.vmware_backdoor.handle_command(&mut regs) {
                    let _ = vm.set_vcpu_regs(vcpu_id, &regs);
                    // Also write EAX result into the I/O response buffer for KVM
                    let result = regs.rax as u32;
                    #[cfg(feature = "linux")]
                    {
                        let result_bytes = result.to_le_bytes();
                        let resp = &result_bytes[..size as usize];
                        vm.set_io_response(vcpu_id, resp);
                    }
                    io_unlock();
                    return 0;
                }
            }
        }
    }

    let buf = unsafe { core::slice::from_raw_parts_mut(data, size as usize) };
    vm.handle_io(port, is_write != 0, size, buf);

    // For IN (read), write response back to the backend.
    // KVM: write into kvm_run shared page.
    // anyOS: write response value into guest RAX via set_vcpu_regs.
    if is_write == 0 {
        #[cfg(feature = "linux")]
        {
            vm.set_io_response(vcpu_id, buf);
        }
        #[cfg(not(feature = "linux"))]
        {
            // Write result into guest RAX
            if let Ok(mut regs) = vm.get_vcpu_regs(vcpu_id) {
                let val = match size {
                    1 => buf[0] as u64,
                    2 => u16::from_le_bytes([buf[0], buf[1]]) as u64,
                    4 => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64,
                    _ => 0,
                };
                regs.rax = (regs.rax & !((1u64 << (size as u64 * 8)) - 1)) | val;
                let _ = vm.set_vcpu_regs(vcpu_id, &regs);
            }
        }
    }
    io_unlock();
    0
}

/// Handle a KVM string I/O exit (REP INSB/OUTSB) with count > 1.
/// Loops count times, calling the IO port handler for each iteration.
/// Results are written directly to the kvm_run shared page.
#[no_mangle]
pub extern "C" fn corevm_complete_string_io(
    handle: u64, vcpu_id: u32, port: u16, is_write: u8, size: u8, count: u32,
) -> i32 {
    io_lock();
    CURRENT_IO_VCPU.store(vcpu_id, Ordering::Relaxed);
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    #[cfg(feature = "linux")]
    {
        if is_write != 0 {
            vm.complete_string_io_out(vcpu_id, port, size, count);
        } else {
            vm.complete_string_io_in(vcpu_id, port, size, count);
        }
    }
    #[cfg(not(feature = "linux"))]
    {
        let _ = (vm, vcpu_id, port, is_write, size, count);
    }
    io_unlock();
    0
}

/// Check if a physical address falls in the AHCI BAR5 MMIO region.
/// Used to skip MMIO_LOCK for AHCI accesses (AHCI has its own AHCI_LOCK).
///
/// # Safety
/// Uses `&mut *pci_bus_ptr` because `mmcfg_read` takes `&mut self` (even
/// though it only reads). This is safe here because we are only reading
/// the PCI config space BAR value — no mutation occurs.
#[inline]
fn is_ahci_mmio(vm: &Vm, addr: u64) -> bool {
    if vm.ahci_ptr.is_null() || vm.pci_bus_ptr.is_null() {
        return false;
    }
    let bus = unsafe { &mut *vm.pci_bus_ptr };
    let ahci_bar5 = bus.mmcfg_read(0, vm.chipset.slots.ahci, 0, 0x24, 4) & 0xFFFFFFF0;
    ahci_bar5 != 0 && addr >= ahci_bar5 && addr < ahci_bar5 + 0x1000
}

/// Dispatch an MMIO exit to the registered device handler.
///
/// For reads (`is_write`=0), `data` is filled with the result.
/// For writes (`is_write`=1), `data` contains the guest-written value.
/// `dest_reg` indicates which GP register receives the read result (0=RAX..7=RDI).
/// `instr_len` is the instruction length for RIP advancement (non-KVM reads only).
#[no_mangle]
pub extern "C" fn corevm_handle_mmio_exit(
    handle: u64, vcpu_id: u32, addr: u64, is_write: u8, size: u8, data: *mut u8,
    dest_reg: u8, instr_len: u8,
) -> i32 {
    // SMP MMIO locking: MMIO_LOCK serialises non-AHCI device access.
    // AHCI uses its own AHCI_LOCK with deferred I/O inside PciMmioRouter.
    let vm = match get_vm(handle) { Some(v) => v, None => { return -1 } };
    if data.is_null() { return -1; }

    let is_ahci = is_ahci_mmio(vm, addr);
    if !is_ahci {
        mmio_lock();
    }
    let buf = unsafe { core::slice::from_raw_parts_mut(data, size as usize) };
    vm.handle_mmio(addr, is_write != 0, size, buf);
    if !is_ahci {
        mmio_unlock();
    }

    // For MMIO reads, write response back to backend.
    if is_write == 0 {
        let val = match size {
            1 => buf[0] as u64,
            2 => u16::from_le_bytes([buf[0], buf[1]]) as u64,
            4 => u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64,
            8 => u64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]]),
            _ => 0,
        };
        #[cfg(feature = "linux")]
        {
            let _ = val; // used by other paths
            vm.set_mmio_response(vcpu_id, buf);
        }
        #[cfg(not(feature = "linux"))]
        {
            // anyOS or other: set register directly
            if let Ok(mut regs) = vm.get_vcpu_regs(vcpu_id) {
                if instr_len > 0 {
                    regs.rip += instr_len as u64;
                }
                match dest_reg {
                    0 => regs.rax = val,
                    1 => regs.rcx = val,
                    2 => regs.rdx = val,
                    3 => regs.rbx = val,
                    4 => regs.rsp = val,
                    5 => regs.rbp = val,
                    6 => regs.rsi = val,
                    7 => regs.rdi = val,
                    _ => regs.rax = val,
                }
                let _ = vm.set_vcpu_regs(vcpu_id, &regs);
            }
        }
    }
    0
}

// ── Standard device setup ───────────────────────────────────────────────────

/// Add a named file to the fw_cfg device (e.g., "vgaroms/vgabios.bin").
#[no_mangle]
pub extern "C" fn corevm_fw_cfg_add_file(
    handle: u64, name: *const u8, name_len: u32, data: *const u8, data_len: u32,
) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if name.is_null() || data.is_null() || vm.fw_cfg_ptr.is_null() { return -1; }
    let name_slice = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    let name_str = match core::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let data_slice = unsafe { core::slice::from_raw_parts(data, data_len as usize) };
    let fw_cfg = unsafe { &mut *vm.fw_cfg_ptr };
    fw_cfg.add_file(name_str, data_slice.to_vec());
    0
}

/// Set up direct kernel boot via fw_cfg legacy selectors.
/// Parses a Linux bzImage, splits it into setup + kernel, computes addresses.
#[no_mangle]
pub extern "C" fn corevm_fw_cfg_set_kernel(
    handle: u64,
    kernel: *const u8, kernel_len: u32,
    initrd: *const u8, initrd_len: u32,
    cmdline: *const u8, cmdline_len: u32,
) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if vm.fw_cfg_ptr.is_null() { return -1; }
    let kernel_slice = if kernel.is_null() || kernel_len == 0 { &[] }
        else { unsafe { core::slice::from_raw_parts(kernel, kernel_len as usize) } };
    let initrd_slice = if initrd.is_null() || initrd_len == 0 { &[] }
        else { unsafe { core::slice::from_raw_parts(initrd, initrd_len as usize) } };
    let cmdline_slice = if cmdline.is_null() || cmdline_len == 0 { &[] }
        else { unsafe { core::slice::from_raw_parts(cmdline, cmdline_len as usize) } };
    let fw_cfg = unsafe { &mut *vm.fw_cfg_ptr };
    fw_cfg.set_kernel(kernel_slice, initrd_slice, cmdline_slice);
    0
}

/// Register all standard chipset devices into the VM.
#[no_mangle]
/// Set vCPU MP state. For APs (vcpu_id > 0), set to 1 (UNINITIALIZED) so they
/// wait for SIPI from the BSP instead of running immediately.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_set_mp_state(handle: u64, vcpu_id: u32, state: u32) -> i32 {
    match get_vm(handle) {
        Some(vm) => {
            #[cfg(feature = "linux")]
            match vm.backend.set_mp_state(vcpu_id, state) {
                Ok(_) => 0,
                Err(_) => -1,
            }
            #[cfg(not(feature = "linux"))]
            { let _ = (vcpu_id, state); 0 }
        }
        None => -1,
    }
}

/// Synchronize TSC across all vCPUs just before starting VM execution.
///
/// Sets IA32_TSC and IA32_TSC_ADJUST to 0 on all vCPUs in a tight loop
/// to minimize inter-core TSC skew. Also sets a fixed TSC frequency via
/// KVM_SET_TSC_KHZ so all cores run at the same rate regardless of host
/// P-state changes.
///
/// Without this, guest OSes detect TSC desynchronization:
/// - Linux reports "Firmware Bug: TSC not synchronous to P0"
/// - Windows hangs in APIC timer calibration loops
///
/// Must be called AFTER all vCPUs are created and BEFORE any vCPU thread
/// starts running.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_sync_tsc(handle: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => {
            #[cfg(feature = "linux")]
            {
                vm.backend.sync_tsc();
            }
            0
        }
        None => -1,
    }
}

/// Deprecated: ACPI _PRT is now auto-detected from device pointers.
/// Kept for ABI compatibility — does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_set_acpi_devices(_handle: u64, _has_e1000: i32, _has_ac97: i32, _has_uhci: i32) -> i32 {
    0
}

/// Set the number of CPU cores. Must be called BEFORE corevm_setup_acpi_tables().
/// Re-loads host CPUID with topology adjusted for the new core count.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_set_cpu_count(handle: u64, count: u32) -> i32 {
    match get_vm(handle) {
        Some(vm) => {
            let c = count.max(1).min(32);
            vm.cpu_count = c;
            #[cfg(feature = "linux")]
            {
                vm.backend.cpu_count = c;
                // Re-load host CPUID so topology leaves (1, 4, 0xB,
                // 0x80000008) are filtered with the correct cpu_count.
                let _ = vm.backend.load_host_cpuid();
            }
            0
        }
        None => -1,
    }
}

/// Set VRAM size in MiB. Must be called BEFORE corevm_setup_standard_devices().
/// Valid range: 8-256 MiB. Pass 0 for default (16 MiB).
#[unsafe(no_mangle)]
pub extern "C" fn corevm_set_vram_mb(handle: u64, vram_mb: u32) -> i32 {
    match get_vm(handle) {
        Some(vm) => { vm.vram_mb = vram_mb; 0 }
        None => -1,
    }
}

/// Mark this VM as UEFI boot mode. Must be called BEFORE
/// corevm_setup_standard_devices() so that the VGA LFB at 0xE0000000 is NOT
/// mapped as a KVM memory region — OVMF relocates PCIEXBAR there and needs
/// MMIO traps, not RAM.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_set_uefi_boot(handle: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => { vm.uefi_boot = true; 0 }
        None => -1,
    }
}

/// Configure disk cache for an AHCI port.
/// `port`: AHCI port number (0-based).
/// `cache_mb`: Cache size in MiB (0 = disable cache).
/// `mode`: 0 = WriteBack (best perf), 1 = WriteThrough (safe), 2 = None (no cache).
/// Must be called AFTER corevm_setup_ahci() and disk attachment.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_ahci_set_cache(handle: u64, port: u32, cache_mb: u32, mode: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if vm.ahci_ptr.is_null() { return -1; }
    let ahci = unsafe { &mut *vm.ahci_ptr };
    let cache_mode = match mode {
        0 => crate::devices::disk_cache::CacheMode::WriteBack,
        1 => crate::devices::disk_cache::CacheMode::WriteThrough,
        _ => crate::devices::disk_cache::CacheMode::None,
    };
    ahci.configure_cache(port as usize, cache_mb, cache_mode);
    0
}

/// Flush all dirty cache blocks to host for all AHCI ports.
/// Should be called periodically from the VM loop.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_ahci_flush_caches(handle: u64) {
    let vm = match get_vm(handle) { Some(v) => v, None => return };
    if vm.ahci_ptr.is_null() { return; }
    ahci_lock();
    let ahci = unsafe { &mut *vm.ahci_ptr };
    ahci.flush_caches();
    ahci_unlock();
}

/// Check if any AHCI port has dirty cache blocks that need flushing.
#[unsafe(no_mangle)]
pub extern "C" fn corevm_ahci_needs_flush(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.ahci_ptr.is_null() { return 0; }
    // Non-blocking: if AHCI_LOCK is held (AP doing disk I/O), skip the check.
    if !ahci_try_lock() { return 0; }
    let ahci = unsafe { &*vm.ahci_ptr };
    let result = if ahci.any_cache_needs_flush() { 1 } else { 0 };
    ahci_unlock();
    result
}

pub extern "C" fn corevm_setup_standard_devices(handle: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => {
            vm.setup_standard_devices();
            // Map VGA LFB as a hypervisor memory region for fast access.
            #[cfg(feature = "linux")]
            if let Err(_) = vm.setup_vga_lfb_mapping() {
                #[cfg(feature = "std")]
                eprintln!("[corevm] Warning: failed to map VGA LFB as hypervisor region");
            }
            0
        }
        None => -1,
    }
}

/// Enable the HPET (High Precision Event Timer) device.
/// Must be called after corevm_setup_standard_devices.
/// Required for Windows guests; optional for Linux.
#[no_mangle]
pub extern "C" fn corevm_setup_hpet(handle: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => { vm.setup_hpet(); 0 }
        None => -1,
    }
}

/// Attach a CDROM image to the IDE controller (slave device).
/// This uses the legacy ISA IDE controller which has built-in Windows drivers.
/// `fd` is a file descriptor, `size` is the image size in bytes.
#[no_mangle]
pub extern "C" fn corevm_ide_attach_cdrom(handle: u64, fd: i32, size: u64) -> i32 {
    match get_vm(handle) {
        Some(vm) => {
            if vm.ide_ptr.is_null() {
                set_last_error("IDE controller not initialized".into());
                return -1;
            }
            let ide = unsafe { &mut *vm.ide_ptr };
            ide.attach_slave_fd(fd, size);
            0
        }
        None => -1,
    }
}

/// Generate and register ACPI tables via fw_cfg.
/// Must be called after corevm_setup_standard_devices (needs fw_cfg device).
#[no_mangle]
pub extern "C" fn corevm_setup_acpi_tables(handle: u64) -> i32 {
    fn dbg(_msg: &str) {
    }
    dbg("corevm_setup_acpi_tables called");
    let vm = match get_vm(handle) {
        Some(v) => v,
        None => { dbg("get_vm returned None"); return -1; }
    };
    if vm.fw_cfg_ptr.is_null() {
        dbg("fw_cfg_ptr is NULL");
        return -1;
    }
    let fw_cfg = unsafe { &mut *vm.fw_cfg_ptr };

    let num_cpus = vm.cpu_count.max(1);

    // Tell fw_cfg the actual CPU count so SeaBIOS discovers and starts APs.
    fw_cfg.set_cpu_count(num_cpus as u16);

    // Auto-detect which PCI devices are present from their pointers
    let devices = crate::devices::acpi_tables::AcpiDeviceConfig {
        #[cfg(feature = "std")]
        has_e1000: vm.e1000.is_some(),
        #[cfg(not(feature = "std"))]
        has_e1000: !vm.e1000_ptr.is_null(),
        has_ac97: !vm.ac97_ptr.is_null(),
        has_uhci: !vm.uhci_ptr.is_null(),
        has_virtio_gpu: !vm.virtio_gpu_ptr.is_null(),
        has_virtio_net: !vm.virtio_net_ptr.is_null(),
        has_virtio_input: !vm.virtio_kbd_ptr.is_null(),
        pci_mmio_start: vm.chipset.mmio.pci_mmio_start,
        pci_mmio_end: vm.chipset.mmio.pci_mmio_end,
        slots: vm.chipset.slots,
        irqs: vm.chipset.irqs,
        // UEFI/OVMF uses ICH9 PMBASE at 0x600; SeaBIOS uses 0xB000
        pm_base: if vm.uefi_boot { 0x600 } else { 0xB000 },
        // Include MCFG table for UEFI so OVMF discovers PCI MMCONFIG
        pci_mmconfig_base: if vm.uefi_boot { vm.chipset.mmio.pci_mmconfig_base } else { 0 },
    };
    let (rsdp, tables, loader) = crate::devices::acpi_tables::generate_acpi_tables_configured(false, num_cpus, &devices);

    fw_cfg.add_file("etc/acpi/rsdp", rsdp);
    fw_cfg.add_file("etc/acpi/tables", tables);
    fw_cfg.add_file("etc/table-loader", loader);

    // Set CMOS register 0x5F = CPU count for SeaBIOS SMP detection
    if !vm.cmos_ptr.is_null() {
        let cmos = unsafe { &mut *vm.cmos_ptr };
        if num_cpus > 1 { cmos.data[0x5F] = (num_cpus - 1) as u8; }
    }

    dbg("ACPI files registered in fw_cfg");
    0
}

/// Set up ACPI tables WITH HPET table included.
/// Required for Windows 7/8/10 guests that need HPET for timer source.
/// Linux guests should use corevm_setup_acpi_tables() without HPET
/// (HPET Legacy Replacement mode conflicts with PIT-based timer test).
#[no_mangle]
pub extern "C" fn corevm_setup_acpi_tables_with_hpet(handle: u64) -> i32 {
    let vm = match get_vm(handle) {
        Some(v) => v,
        None => return -1,
    };
    if vm.fw_cfg_ptr.is_null() {
        return -1;
    }
    let fw_cfg = unsafe { &mut *vm.fw_cfg_ptr };
    let num_cpus = vm.cpu_count.max(1);

    // Tell fw_cfg the actual CPU count so SeaBIOS discovers and starts APs.
    fw_cfg.set_cpu_count(num_cpus as u16);

    // Auto-detect which PCI devices are present from their pointers
    let devices = crate::devices::acpi_tables::AcpiDeviceConfig {
        #[cfg(feature = "std")]
        has_e1000: vm.e1000.is_some(),
        #[cfg(not(feature = "std"))]
        has_e1000: !vm.e1000_ptr.is_null(),
        has_ac97: !vm.ac97_ptr.is_null(),
        has_uhci: !vm.uhci_ptr.is_null(),
        has_virtio_gpu: !vm.virtio_gpu_ptr.is_null(),
        has_virtio_net: !vm.virtio_net_ptr.is_null(),
        has_virtio_input: !vm.virtio_kbd_ptr.is_null(),
        pci_mmio_start: vm.chipset.mmio.pci_mmio_start,
        pci_mmio_end: vm.chipset.mmio.pci_mmio_end,
        slots: vm.chipset.slots,
        irqs: vm.chipset.irqs,
        pm_base: if vm.uefi_boot { 0x600 } else { 0xB000 },
        pci_mmconfig_base: if vm.uefi_boot { vm.chipset.mmio.pci_mmconfig_base } else { 0 },
    };
    let (rsdp, tables, loader) = crate::devices::acpi_tables::generate_acpi_tables_configured(true, num_cpus, &devices);
    fw_cfg.add_file("etc/acpi/rsdp", rsdp);
    fw_cfg.add_file("etc/acpi/tables", tables);
    fw_cfg.add_file("etc/table-loader", loader);

    // Set CMOS register 0x5F = CPU count for SeaBIOS SMP detection
    if !vm.cmos_ptr.is_null() {
        let cmos = unsafe { &mut *vm.cmos_ptr };
        if num_cpus > 1 { cmos.data[0x5F] = (num_cpus - 1) as u8; }
    }

    0
}

// ── Device-specific FFI ─────────────────────────────────────────────────────

/// Set up the E1000 NIC with the given MAC address (6 bytes).
///
/// Registers the E1000 as:
/// - Routed via the PCI MMIO router (created by `corevm_setup_ahci`) which
///   dynamically reads BAR0 from PCI config to forward MMIO accesses
/// - PCI device 00:04.0 (Intel 82540EM, 8086:100E) so the guest can discover it
///
/// **Must be called after `corevm_setup_ahci()`** so the PCI MMIO router exists.
#[no_mangle]
pub extern "C" fn corevm_setup_e1000(handle: u64, mac: *const u8) -> i32 {
    const E1000_MMIO_SIZE: u64 = 0x2_0000; // 128 KB
    const E1000_IO_BASE: u16 = 0xC000; // I/O BAR for indirect register access

    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let e1000_mmio_base = vm.chipset.mmio.e1000_bar0;
    if mac.is_null() { return -1; }
    let m: [u8; 6] = unsafe { [*mac, *mac.add(1), *mac.add(2), *mac.add(3), *mac.add(4), *mac.add(5)] };
    let mut e1000_dev = crate::devices::e1000::E1000::new(m);
    // Give E1000 access to guest RAM for DMA (TX/RX descriptor rings).
    let (ram_ptr, ram_len) = vm.memory.ram_mut_ptr();
    e1000_dev.guest_mem_ptr = ram_ptr;
    e1000_dev.guest_mem_len = ram_len;
    // Set up IRQ callback for immediate interrupt delivery.
    // The E1000 driver's interrupt test requires the interrupt to fire
    // synchronously during the ICS write — not deferred to poll_irqs.
    #[cfg(feature = "linux")]
    {
        let backend_addr = &mut vm.backend as *mut crate::backend::kvm::KvmBackend as usize;
        e1000_dev.irq_callback = Some(alloc::boxed::Box::new(move |asserted: bool| {
            let backend = unsafe { &mut *(backend_addr as *mut crate::backend::kvm::KvmBackend) };
            let _ = backend.set_irq_line(11, asserted);
        }));
    }

    // Wrap E1000 in Arc<Mutex> for SMP-safe access from multiple vCPU threads
    // and the net_poll path. Under no_std, use a raw pointer (single-core).
    #[cfg(feature = "std")]
    let e1000_arc = alloc::sync::Arc::new(std::sync::Mutex::new(e1000_dev));
    #[cfg(feature = "std")]
    {
        vm.e1000 = Some(e1000_arc.clone());

        // Add E1000 to the PCI MMIO router so accesses are routed dynamically
        // based on the current BAR0 address (which SeaBIOS may remap).
        if !vm.pci_mmio_router_ptr.is_null() {
            let router = unsafe { &mut *vm.pci_mmio_router_ptr };
            router.e1000 = Some(e1000_arc.clone());
        }

        // Add E1000 to the PCI I/O router for BAR2 indirect register access.
        if vm.pci_io_router_ptr.is_null() {
            let router = Box::new(PciIoRouter {
                uhci: vm.uhci_ptr,
                ac97: vm.ac97_ptr,
                e1000: Some(e1000_arc.clone()),
                pci_bus: vm.pci_bus_ptr,
                chipset: vm.chipset,
            });
            let router_ptr = &*router as *const PciIoRouter as *mut PciIoRouter;
            vm.pci_io_router_ptr = router_ptr;
            vm.io.register(PCI_IO_ROUTER_BASE, PCI_IO_ROUTER_SIZE, router);
        } else {
            let router = unsafe { &mut *vm.pci_io_router_ptr };
            router.e1000 = Some(e1000_arc.clone());
        }
    }

    #[cfg(not(feature = "std"))]
    {
        let e1000 = Box::new(e1000_dev);
        let e1000_ptr = &*e1000 as *const crate::devices::e1000::E1000 as *mut crate::devices::e1000::E1000;
        vm.e1000_ptr = e1000_ptr;

        if !vm.pci_mmio_router_ptr.is_null() {
            let router = unsafe { &mut *vm.pci_mmio_router_ptr };
            router.e1000 = e1000_ptr;
        }

        if vm.pci_io_router_ptr.is_null() {
            let router = Box::new(PciIoRouter {
                uhci: vm.uhci_ptr,
                ac97: vm.ac97_ptr,
                e1000: e1000_ptr,
                pci_bus: vm.pci_bus_ptr,
                chipset: vm.chipset,
            });
            let router_ptr = &*router as *const PciIoRouter as *mut PciIoRouter;
            vm.pci_io_router_ptr = router_ptr;
            vm.io.register(PCI_IO_ROUTER_BASE, PCI_IO_ROUTER_SIZE, router);
        } else {
            let router = unsafe { &mut *vm.pci_io_router_ptr };
            router.e1000 = e1000_ptr;
        }

        core::mem::forget(e1000);
    }

    // Register E1000 as a PCI device so the guest can discover it via PCI scan.
    if !vm.pci_bus_ptr.is_null() {
        let pci_bus = unsafe { &mut *vm.pci_bus_ptr };
        // Intel 82540EM: vendor 8086, device 100E, class 02 (Network), subclass 00 (Ethernet)
        let mut pci_dev = crate::devices::bus::PciDevice::new(0x8086, 0x100E, 0x02, 0x00, 0x00);
        pci_dev.device = vm.chipset.slots.e1000;
        // BAR0: MMIO at 0xF0000000, size 128 KB
        pci_dev.set_bar(0, e1000_mmio_base as u32, E1000_MMIO_SIZE as u32, true);
        // BAR2: I/O ports for indirect register access (8 bytes: IOADDR+IODATA)
        pci_dev.set_bar(2, E1000_IO_BASE as u32, 8, false);
        // Interrupt: IRQ 11, pin INTA (fallback for legacy mode)
        pci_dev.set_interrupt(11, 1);
        // Subsystem ID (common for 82540EM)
        pci_dev.set_subsystem(0x8086, 0x001E);
        // MSI capability at offset 0xD0 (standard for Intel NICs)
        pci_dev.add_msi_capability(0xD0);
        pci_bus.add_device(pci_dev);
    }
    0
}

/// Base address for the PCI MMIO catch-all region.
/// Covers 0xF0000000-0xFEBFFFFF (~236MB) to catch PCI BARs wherever SeaBIOS
/// remaps them.  SeaBIOS allocates BARs downward from just below the IOAPIC
/// (0xFEC00000), so both AHCI BAR5 and E1000 BAR0 typically land here.
const PCI_MMIO_CATCHALL_BASE: u64 = 0xF000_0000;
const PCI_MMIO_CATCHALL_SIZE: u64 = 0xEC0_0000; // up to 0xFEBFFFFF

/// MMIO router that forwards accesses to AHCI or E1000 based on their current
/// PCI BAR addresses.  Registered over a wide catch-all range; dynamically
/// reads BAR values from PCI config space to route each access.
pub struct PciMmioRouter {
    pub ahci: *mut crate::devices::ahci::Ahci,
    #[cfg(feature = "std")]
    pub e1000: Option<alloc::sync::Arc<std::sync::Mutex<crate::devices::e1000::E1000>>>,
    #[cfg(not(feature = "std"))]
    pub e1000: *mut crate::devices::e1000::E1000,
    pub svga: *mut crate::devices::svga::Svga,
    pub virtio_gpu: *mut crate::devices::virtio_gpu::VirtioGpu,
    pub intel_gpu: *mut crate::devices::intel_gpu::IntelGpu,
    pub virtio_net: *mut crate::devices::virtio_net::VirtioNet,
    pub virtio_kbd: *mut crate::devices::virtio_input::VirtioInput,
    pub virtio_tablet: *mut crate::devices::virtio_input::VirtioInput,
    pci_bus: *mut crate::devices::bus::PciBus,
    /// Chipset config for PCI slot lookups.
    pub chipset: &'static crate::devices::chipset::ChipsetConfig,
}

unsafe impl Send for PciMmioRouter {}

impl PciMmioRouter {
    /// Read a BAR value from PCI config space for a device at the given slot.
    fn read_bar(&self, slot: u8, bar_offset: usize, align_mask: u64) -> u64 {
        if self.pci_bus.is_null() { return 0; }
        let bus = unsafe { &mut *self.pci_bus };
        bus.mmcfg_read(0, slot, 0, bar_offset, 4) & align_mask
    }

    fn ahci_bar5(&self) -> u64 {
        if self.ahci.is_null() { return 0; }
        self.read_bar(self.chipset.slots.ahci, 0x24, 0xFFFFFFF0)
    }
    fn e1000_bar0(&self) -> u64 {
        #[cfg(feature = "std")]
        { if self.e1000.is_none() { return 0; } }
        #[cfg(not(feature = "std"))]
        { if self.e1000.is_null() { return 0; } }
        self.read_bar(self.chipset.slots.e1000, 0x10, 0xFFFFFFF0)
    }
    #[cfg(feature = "std")]
    fn has_e1000(&self) -> bool { self.e1000.is_some() }
    #[cfg(not(feature = "std"))]
    fn has_e1000(&self) -> bool { !self.e1000.is_null() }
    fn vga_bar2(&self) -> u64 {
        if self.svga.is_null() { return 0; }
        // When Intel GPU is active, it replaces VGA on the same PCI slot.
        // Don't route VGA BAR2 accesses to SVGA — they belong to the Intel GPU.
        if !self.intel_gpu.is_null() { return 0; }
        self.read_bar(self.chipset.slots.vga, 0x18, 0xFFFFF000)
    }
    /// Intel GPU BAR0 (MMIO register space, 4 MB).
    fn intel_gpu_bar0(&self) -> u64 {
        if self.intel_gpu.is_null() { return 0; }
        // Intel GPU uses the VGA slot — BAR0 at PCI config offset 0x10
        self.read_bar(self.chipset.slots.vga, 0x10, 0xFFC00000) // 4 MB aligned
    }
    /// Intel GPU BAR2 (VRAM aperture).
    fn intel_gpu_bar2(&self) -> u64 {
        if self.intel_gpu.is_null() { return 0; }
        self.read_bar(self.chipset.slots.vga, 0x18, 0xFE000000) // 32 MB aligned minimum
    }
    fn virtio_gpu_bar0(&self) -> u64 {
        if self.virtio_gpu.is_null() { return 0; }
        self.read_bar(self.chipset.slots.virtio_gpu, 0x10, 0xFFFFC000)
    }
    fn virtio_net_bar0(&self) -> u64 {
        if self.virtio_net.is_null() { return 0; }
        self.read_bar(self.chipset.slots.virtio_net, 0x10, 0xFFFFC000)
    }
    fn virtio_kbd_bar0(&self) -> u64 {
        if self.virtio_kbd.is_null() { return 0; }
        self.read_bar(self.chipset.slots.virtio_kbd, 0x10, 0xFFFFC000)
    }
    fn virtio_tablet_bar0(&self) -> u64 {
        if self.virtio_tablet.is_null() { return 0; }
        self.read_bar(self.chipset.slots.virtio_tablet, 0x10, 0xFFFFC000)
    }
}

impl crate::memory::mmio::MmioHandler for PciMmioRouter {
    fn read(&mut self, offset: u64, size: u8) -> crate::error::Result<u64> {
        let abs_addr = PCI_MMIO_CATCHALL_BASE + offset;

        // Debug: log accesses in Intel GPU BAR0 range
        #[cfg(feature = "std")]
        if abs_addr >= 0xFC000000 && abs_addr < 0xFC400000 {
            static ROUTER_DBG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = ROUTER_DBG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 20 {
                eprintln!("[router] GPU range read abs=0x{:X} igpu_null={} bar0=0x{:X}",
                    abs_addr, self.intel_gpu.is_null(), self.intel_gpu_bar0());
            }
        }

        // Check E1000 BAR0 (128KB region)
        if self.has_e1000() {
            let e1000_base = self.e1000_bar0();
            if e1000_base != 0 && abs_addr >= e1000_base && abs_addr < e1000_base + 0x2_0000 {
                let reg = abs_addr - e1000_base;
                #[cfg(feature = "std")]
                {
                    // E1000 reads MUST acquire the lock because some registers
                    // are clear-on-read (ICR). Returning 0 on contention would
                    // cause the guest driver to miss interrupts permanently.
                    let mut e1000 = self.e1000.as_ref().unwrap().lock().unwrap();
                    return e1000.read(reg, size);
                }
                #[cfg(not(feature = "std"))]
                {
                    let e1000 = unsafe { &mut *self.e1000 };
                    return e1000.read(reg, size);
                }
            }
        }

        // Check AHCI BAR5 (4KB region).
        // Reads don't need AHCI_LOCK: port registers (is, ci, tfd, etc.)
        // are plain u32 values, which are atomic on x86_64. Even during
        // concurrent writes, reads get a consistent (old or new) value.
        // This eliminates the worst SMP bottleneck: status polling from
        // other vCPUs no longer blocks on multi-ms disk I/O.
        let ahci_base = self.ahci_bar5();
        if ahci_base != 0 && abs_addr >= ahci_base && abs_addr < ahci_base + 0x1000 {
            let ahci = unsafe { &mut *self.ahci };
            let result = ahci.read(abs_addr - ahci_base, size);
            return result;
        }

        // Check VGA BAR2 — Bochs VBE DISPI MMIO registers (4KB region).
        // SeaBIOS may remap BAR2 from 0xFEBE0000 to another address; bochs-drm
        // (Linux kernel) reads VBE registers via MMIO here. Without this routing,
        // the ID check fails ("ID mismatch") and bochs-drm won't load.
        let vga_bar2 = self.vga_bar2();
        if vga_bar2 != 0 && abs_addr >= vga_bar2 && abs_addr < vga_bar2 + 0x1000 {
            let svga = unsafe { &mut *self.svga };
            let bar2_off = abs_addr - vga_bar2;
            #[cfg(feature = "std")]
            {
                static DISPI_RD_LOG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = DISPI_RD_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n < 30 {
                    eprintln!("[vga-bar2] MMIO read off=0x{:X} size={} (bar2=0x{:X})", bar2_off, size, vga_bar2);
                }
            }
            return svga_dispi_mmio_read(svga, bar2_off, size);
        }

        // Check Intel GPU BAR0 (4 MB MMIO register space)
        if !self.intel_gpu.is_null() {
            let igpu_bar0 = self.intel_gpu_bar0();
            let igpu_mmio_size = crate::devices::intel_gpu::MMIO_SIZE as u64;

            #[cfg(feature = "std")]
            {
                static IGPU_ROUTE_LOG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = IGPU_ROUTE_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n < 10 {
                    eprintln!("[igpu-route] read abs=0x{:X} bar0=0x{:X} size={} match={}",
                        abs_addr, igpu_bar0, igpu_mmio_size,
                        igpu_bar0 != 0 && abs_addr >= igpu_bar0 && abs_addr < igpu_bar0 + igpu_mmio_size);
                }
            }

            if igpu_bar0 != 0 && abs_addr >= igpu_bar0 && abs_addr < igpu_bar0 + igpu_mmio_size {
                let gpu = unsafe { &mut *self.intel_gpu };
                return gpu.read(abs_addr - igpu_bar0, size);
            }
            // Check Intel GPU BAR2 (VRAM aperture)
            let igpu_bar2 = self.intel_gpu_bar2();
            if igpu_bar2 != 0 && abs_addr >= igpu_bar2 {
                let gpu = unsafe { &mut *self.intel_gpu };
                let vram_size = gpu.vram_size as u64;
                if abs_addr < igpu_bar2 + vram_size {
                    let offset = (abs_addr - igpu_bar2) as usize;
                    let val = match size {
                        1 => gpu.vram.get(offset).copied().unwrap_or(0) as u64,
                        2 if offset + 1 < gpu.vram_size => {
                            u16::from_le_bytes([gpu.vram[offset], gpu.vram[offset + 1]]) as u64
                        }
                        4 if offset + 3 < gpu.vram_size => {
                            u32::from_le_bytes([
                                gpu.vram[offset], gpu.vram[offset + 1],
                                gpu.vram[offset + 2], gpu.vram[offset + 3],
                            ]) as u64
                        }
                        _ => 0,
                    };
                    return Ok(val);
                }
            }
        }

        // Check VirtIO GPU BAR0 (16KB region)
        if !self.virtio_gpu.is_null() {
            let gpu_base = self.virtio_gpu_bar0();
            if gpu_base != 0 && abs_addr >= gpu_base && abs_addr < gpu_base + 0x4000 {
                let gpu = unsafe { &mut *self.virtio_gpu };
                return gpu.read(abs_addr - gpu_base, size);
            }
        }

        // Check VirtIO-Net BAR0 (16KB region)
        if !self.virtio_net.is_null() {
            let net_base = self.virtio_net_bar0();
            if net_base != 0 && abs_addr >= net_base && abs_addr < net_base + 0x4000 {
                let net = unsafe { &mut *self.virtio_net };
                return net.read(abs_addr - net_base, size);
            }
        }

        // Check VirtIO Input Keyboard BAR0 (16KB region)
        if !self.virtio_kbd.is_null() {
            let kbd_base = self.virtio_kbd_bar0();
            if kbd_base != 0 && abs_addr >= kbd_base && abs_addr < kbd_base + 0x4000 {
                let kbd = unsafe { &mut *self.virtio_kbd };
                return kbd.read(abs_addr - kbd_base, size);
            }
        }

        // Check VirtIO Input Tablet BAR0 (16KB region)
        if !self.virtio_tablet.is_null() {
            let tab_base = self.virtio_tablet_bar0();
            if tab_base != 0 && abs_addr >= tab_base && abs_addr < tab_base + 0x4000 {
                let tab = unsafe { &mut *self.virtio_tablet };
                return tab.read(abs_addr - tab_base, size);
            }
        }

        Ok(0xFFFFFFFF)
    }

    fn write(&mut self, offset: u64, size: u8, val: u64) -> crate::error::Result<()> {
        let abs_addr = PCI_MMIO_CATCHALL_BASE + offset;

        // Check E1000 BAR0 (128KB region)
        if self.has_e1000() {
            let e1000_base = self.e1000_bar0();
            if e1000_base != 0 && abs_addr >= e1000_base && abs_addr < e1000_base + 0x2_0000 {
                let reg = abs_addr - e1000_base;
                #[cfg(feature = "std")]
                {
                    // Writes must acquire the lock (dropping writes would
                    // break driver state). Use try_lock with brief spin to
                    // reduce contention, then fall back to blocking lock.
                    let arc = self.e1000.as_ref().unwrap();
                    let mut e1000 = match arc.try_lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            for _ in 0..8 { core::hint::spin_loop(); }
                            arc.lock().unwrap()
                        }
                    };
                    return e1000.write(reg, size, val);
                }
                #[cfg(not(feature = "std"))]
                {
                    let e1000 = unsafe { &mut *self.e1000 };
                    return e1000.write(reg, size, val);
                }
            }
        }

        // Check AHCI BAR5 (4KB region) — deferred I/O pattern:
        // 1. ahci_lock → parse command, clear CI, queue I/O → ahci_unlock
        // 2. Execute pread/pwrite WITHOUT lock (other vCPUs can access AHCI)
        // 3. ahci_lock → apply completions (FIS, IS, IRQ) → ahci_unlock
        let ahci_base = self.ahci_bar5();
        if ahci_base != 0 && abs_addr >= ahci_base && abs_addr < ahci_base + 0x1000 {
            ahci_lock();
            let ahci = unsafe { &mut *self.ahci };
            let result = ahci.write(abs_addr - ahci_base, size, val);
            let mut pending = ahci.take_pending_io();
            ahci_unlock();

            if !pending.is_empty() {
                // Disk I/O without AHCI_LOCK — the critical optimization.
                for req in &mut pending {
                    req.execute();
                }
                // Apply completions (brief lock — only state updates)
                ahci_lock();
                let ahci = unsafe { &mut *self.ahci };
                for req in &pending {
                    ahci.complete_io(req);
                }
                ahci_unlock();
            }
            return result;
        }

        // Check VGA BAR2 — Bochs VBE DISPI MMIO registers (4KB region)
        let vga_bar2 = self.vga_bar2();
        if vga_bar2 != 0 && abs_addr >= vga_bar2 && abs_addr < vga_bar2 + 0x1000 {
            let svga = unsafe { &mut *self.svga };
            let bar2_off = abs_addr - vga_bar2;
            #[cfg(feature = "std")]
            {
                static DISPI_WR_LOG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = DISPI_WR_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n < 30 {
                    eprintln!("[vga-bar2] MMIO write off=0x{:X} size={} val=0x{:X} (bar2=0x{:X})", bar2_off, size, val, vga_bar2);
                }
            }
            return svga_dispi_mmio_write(svga, bar2_off, size, val);
        }

        // Check Intel GPU BAR0 (4 MB MMIO register space)
        if !self.intel_gpu.is_null() {
            let igpu_bar0 = self.intel_gpu_bar0();
            let igpu_mmio_size = crate::devices::intel_gpu::MMIO_SIZE as u64;
            if igpu_bar0 != 0 && abs_addr >= igpu_bar0 && abs_addr < igpu_bar0 + igpu_mmio_size {
                let gpu = unsafe { &mut *self.intel_gpu };
                return gpu.write(abs_addr - igpu_bar0, size, val);
            }
            // Check Intel GPU BAR2 (VRAM aperture)
            let igpu_bar2 = self.intel_gpu_bar2();
            if igpu_bar2 != 0 && abs_addr >= igpu_bar2 {
                let gpu = unsafe { &mut *self.intel_gpu };
                let vram_size = gpu.vram_size as u64;
                if abs_addr < igpu_bar2 + vram_size {
                    let offset = (abs_addr - igpu_bar2) as usize;
                    match size {
                        1 => { if offset < gpu.vram_size { gpu.vram[offset] = val as u8; } }
                        2 => { if offset + 1 < gpu.vram_size { gpu.vram[offset..offset+2].copy_from_slice(&(val as u16).to_le_bytes()); } }
                        4 => { if offset + 3 < gpu.vram_size { gpu.vram[offset..offset+4].copy_from_slice(&(val as u32).to_le_bytes()); } }
                        _ => {}
                    }
                    return Ok(());
                }
            }
        }

        // Check VirtIO GPU BAR0 (16KB region)
        if !self.virtio_gpu.is_null() {
            let gpu_base = self.virtio_gpu_bar0();
            if gpu_base != 0 && abs_addr >= gpu_base && abs_addr < gpu_base + 0x4000 {
                let gpu = unsafe { &mut *self.virtio_gpu };
                return gpu.write(abs_addr - gpu_base, size, val);
            }
        }

        // Check VirtIO-Net BAR0 (16KB region)
        if !self.virtio_net.is_null() {
            let net_base = self.virtio_net_bar0();
            if net_base != 0 && abs_addr >= net_base && abs_addr < net_base + 0x4000 {
                let net = unsafe { &mut *self.virtio_net };
                return net.write(abs_addr - net_base, size, val);
            }
        }

        // Check VirtIO Input Keyboard BAR0 (16KB region)
        if !self.virtio_kbd.is_null() {
            let kbd_base = self.virtio_kbd_bar0();
            if kbd_base != 0 && abs_addr >= kbd_base && abs_addr < kbd_base + 0x4000 {
                let kbd = unsafe { &mut *self.virtio_kbd };
                return kbd.write(abs_addr - kbd_base, size, val);
            }
        }

        // Check VirtIO Input Tablet BAR0 (16KB region)
        if !self.virtio_tablet.is_null() {
            let tab_base = self.virtio_tablet_bar0();
            if tab_base != 0 && abs_addr >= tab_base && abs_addr < tab_base + 0x4000 {
                let tab = unsafe { &mut *self.virtio_tablet };
                return tab.write(abs_addr - tab_base, size, val);
            }
        }

        Ok(())
    }
}

/// Read from VGA BAR2 MMIO (Bochs VBE DISPI registers).
/// Same layout as SvgaDispiMmioProxy in vm.rs but callable from PciMmioRouter.
fn svga_dispi_mmio_read(svga: &mut crate::devices::svga::Svga, offset: u64, size: u8) -> crate::error::Result<u64> {
    if offset >= 0x500 && offset < 0x600 {
        let idx = ((offset - 0x500) / 2) as usize;
        if idx < svga.vbe_regs.len() {
            let val = svga.vbe_regs[idx] as u64;
            return Ok(match size {
                1 => if offset & 1 == 0 { val & 0xFF } else { (val >> 8) & 0xFF },
                2 => val,
                4 => {
                    let hi = if idx + 1 < svga.vbe_regs.len() { svga.vbe_regs[idx + 1] as u64 } else { 0 };
                    val | (hi << 16)
                }
                _ => val,
            });
        }
        return Ok(0);
    } else if offset < 0x400 {
        let port = 0x3C0 + offset as u16;
        return <crate::devices::svga::Svga as crate::io::IoHandler>::read(svga, port, size).map(|v| v as u64);
    }
    Ok(0xFFFF_FFFF)
}

/// Write to VGA BAR2 MMIO (Bochs VBE DISPI registers).
fn svga_dispi_mmio_write(svga: &mut crate::devices::svga::Svga, offset: u64, size: u8, val: u64) -> crate::error::Result<()> {
    if offset >= 0x500 && offset < 0x600 {
        let idx = ((offset - 0x500) / 2) as usize;
        let v = val as u16;
        if idx < svga.vbe_regs.len() {
            svga.vbe_regs[idx] = v;
            if idx == 4 && (v & 0x01) != 0 {
                let w = svga.vbe_regs[1] as u32;
                let h = svga.vbe_regs[2] as u32;
                let bpp = svga.vbe_regs[3] as u8;
                if w > 0 && h > 0 && bpp > 0 {
                    svga.set_mode(crate::devices::svga::VgaMode::LinearFramebuffer { width: w, height: h, bpp });
                }
            } else if idx == 4 && (v & 0x01) == 0 {
                svga.set_mode(crate::devices::svga::VgaMode::Text80x25);
            }
        }
        return Ok(());
    } else if offset < 0x400 {
        let port = 0x3C0 + offset as u16;
        return <crate::devices::svga::Svga as crate::io::IoHandler>::write(svga, port, size, val as u32);
    }
    Ok(())
}

/// Set up the AHCI SATA controller with the given number of ports.
#[no_mangle]
pub extern "C" fn corevm_setup_ahci(handle: u64, num_ports: u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let ahci = Box::new(crate::devices::ahci::Ahci::new(num_ports));
    vm.ahci_ptr = &*ahci as *const crate::devices::ahci::Ahci as *mut crate::devices::ahci::Ahci;

    // Create the PCI MMIO router covering the full PCI BAR allocation area.
    // The router dynamically reads BAR values from PCI config to route accesses
    // to the correct device (AHCI, E1000, etc.).
    let router = Box::new(PciMmioRouter {
        ahci: vm.ahci_ptr,
        #[cfg(feature = "std")]
        e1000: vm.e1000.clone(),
        #[cfg(not(feature = "std"))]
        e1000: core::ptr::null_mut(),
        svga: vm.svga_ptr,
        virtio_gpu: vm.virtio_gpu_ptr,
        intel_gpu: vm.intel_gpu_ptr,
        virtio_net: vm.virtio_net_ptr,
        virtio_kbd: vm.virtio_kbd_ptr,
        virtio_tablet: vm.virtio_tablet_ptr,
        pci_bus: vm.pci_bus_ptr,
        chipset: vm.chipset,
    });
    let router_ptr = &*router as *const PciMmioRouter as *mut PciMmioRouter;
    vm.pci_mmio_router_ptr = router_ptr;
    vm.memory.add_mmio(PCI_MMIO_CATCHALL_BASE, PCI_MMIO_CATCHALL_SIZE, router);

    // Give AHCI access to guest RAM for DMA transfers.
    let (ram_ptr, ram_len) = vm.memory.ram_mut_ptr();
    unsafe { &mut *vm.ahci_ptr }.set_guest_memory(ram_ptr, ram_len);

    // Keep the AHCI Box alive by leaking it (wrapper uses raw pointer)
    core::mem::forget(ahci);

    // Register AHCI as a PCI device so SeaBIOS can discover it
    if !vm.pci_bus_ptr.is_null() {
        let pci_bus = unsafe { &mut *vm.pci_bus_ptr };
        let mut pci_dev = crate::devices::ahci::create_ahci_pci_device(PCI_MMIO_CATCHALL_BASE as u32);
        pci_dev.device = vm.chipset.slots.ahci;
        pci_bus.add_device(pci_dev);
    }
    0
}

/// Attach a disk image to an AHCI port.
#[no_mangle]
pub extern "C" fn corevm_ahci_attach_disk(handle: u64, port: u32, fd: i32, size: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    match vm.ahci() {
        Some(ahci) => {
            ahci.attach_disk_fd(port as usize, fd, size, crate::devices::ahci::AhciDriveKind::AtaDisk);
            0
        }
        None => -1,
    }
}

/// Attach a CD-ROM image to an AHCI port.
#[no_mangle]
pub extern "C" fn corevm_ahci_attach_cdrom(handle: u64, port: u32, fd: i32, size: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    match vm.ahci() {
        Some(ahci) => {
            ahci.attach_disk_fd(port as usize, fd, size, crate::devices::ahci::AhciDriveKind::AtapiCdrom);
            0
        }
        None => -1,
    }
}

/// Send input bytes to the serial port (COM1).
#[no_mangle]
pub extern "C" fn corevm_serial_send_input(handle: u64, data: *const u8, len: u32) -> i32 {
    io_lock();
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    if data.is_null() && len > 0 { io_unlock(); return -1; }
    let result = match vm.serial() {
        Some(serial) => {
            let slice = if len > 0 {
                unsafe { core::slice::from_raw_parts(data, len as usize) }
            } else {
                &[]
            };
            serial.send_input(slice);
            0
        }
        None => -1,
    };
    io_unlock();
    result
}

/// Take output bytes from the serial port. Returns number of bytes written to `buf`,
/// or -1 on error.
#[no_mangle]
pub extern "C" fn corevm_serial_take_output(handle: u64, buf: *mut u8, max_len: u32) -> i32 {
    io_lock();
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    if buf.is_null() { io_unlock(); return -1; }
    let result = match vm.serial() {
        Some(serial) => {
            let output = serial.take_output();
            let copy_len = output.len().min(max_len as usize);
            if copy_len > 0 {
                unsafe {
                    core::ptr::copy_nonoverlapping(output.as_ptr(), buf, copy_len);
                }
            }
            copy_len as i32
        }
        None => -1,
    };
    io_unlock();
    result
}

/// Drain buffered debug port (0x402) output. Returns number of bytes copied,
/// or -1 on error.
#[no_mangle]
pub extern "C" fn corevm_debug_port_take_output(handle: u64, buf: *mut u8, max_len: u32) -> i32 {
    // IO_LOCK: DebugPort::write() is called from AP threads via port_out.
    // take_output() swaps the Vec, so we must hold the lock to avoid racing
    // with concurrent push() calls from AP I/O exits.
    io_lock();
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    if buf.is_null() || vm.debug_port_ptr.is_null() { io_unlock(); return -1; }
    let dbg = unsafe { &mut *vm.debug_port_ptr };
    let output = dbg.take_output();
    io_unlock();
    let copy_len = output.len().min(max_len as usize);
    if copy_len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(output.as_ptr(), buf, copy_len); }
    }
    copy_len as i32
}

/// Send a PS/2 key press scancode.
#[no_mangle]
pub extern "C" fn corevm_ps2_key_press(handle: u64, scancode: u8) -> i32 {
    io_lock();
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    let result = match vm.ps2() {
        Some(ps2) => { ps2.key_press(scancode); 0 }
        None => -1,
    };
    io_unlock();
    result
}

/// Send a PS/2 key release scancode.
#[no_mangle]
pub extern "C" fn corevm_ps2_key_release(handle: u64, scancode: u8) -> i32 {
    io_lock();
    let vm = match get_vm(handle) { Some(v) => v, None => { io_unlock(); return -1 } };
    let result = match vm.ps2() {
        Some(ps2) => { ps2.key_release(scancode); 0 }
        None => -1,
    };
    io_unlock();
    result
}

/// Send a PS/2 mouse movement.
///
/// Thread-safe: pushes the event into a Mutex-protected queue.
/// The VM loop drains it in `corevm_poll_irqs` (single-threaded).
#[no_mangle]
pub extern "C" fn corevm_ps2_mouse_move(handle: u64, dx: i16, dy: i16, buttons: u8) -> i32 {
    corevm_ps2_mouse_move_wheel(handle, dx, dy, buttons, 0)
}

/// Send PS/2 relative mouse move with scroll wheel.
/// `wheel`: positive = scroll up, negative = scroll down (clamped to -8..7).
#[no_mangle]
pub extern "C" fn corevm_ps2_mouse_move_wheel(handle: u64, dx: i16, dy: i16, buttons: u8, wheel: i8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    #[cfg(feature = "std")]
    {
        if let Ok(mut queue) = vm.pending_mouse.lock() {
            queue.push((dx, dy, buttons, wheel));
            return 0;
        }
        return -1;
    }
    #[cfg(not(feature = "std"))]
    {
        match vm.ps2() {
            Some(ps2) => { ps2.mouse_move_wheel(dx, dy, buttons, wheel); 0 }
            None => -1,
        }
    }
}

/// Debug: query PS/2 mouse state.
/// Returns: bit 0 = mouse_enabled, bits 8..15 = mouse_buffer length, bits 16..23 = keyboard_buffer length.
/// Returns 0xFFFFFFFF if no PS/2 controller.
#[no_mangle]
pub extern "C" fn corevm_ps2_mouse_state(handle: u64) -> u32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0xFFFFFFFF };
    match vm.ps2() {
        Some(ps2) => {
            let enabled = if ps2.mouse_enabled { 1u32 } else { 0 };
            let mbuf = (ps2.mouse_buffer.len() as u32 & 0xFF) << 8;
            let kbuf = (ps2.keyboard_buffer.len() as u32 & 0xFF) << 16;
            enabled | mbuf | kbuf
        }
        None => 0xFFFFFFFF,
    }
}

/// Debug: dump KVM in-kernel IOAPIC redirection table entry for a given pin.
/// Returns the 64-bit redirection entry value, or 0xFFFFFFFF_FFFFFFFF on error.
#[no_mangle]
pub extern "C" fn corevm_ioapic_pin_state(handle: u64, pin: u32) -> u64 {
    #[cfg(feature = "linux")]
    {
        let vm = match get_vm(handle) { Some(v) => v, None => return u64::MAX };
        // KVM IOAPIC chip_id = 2
        match vm.backend.get_irqchip(2) {
            Ok(data) => {
                // KVM ioapic state layout:
                // u64 base_address (8 bytes)
                // u32 ioregsel (4 bytes)
                // u32 id (4 bytes)
                // u32 irr (4 bytes)
                // u32 pad (4 bytes)
                // Then 24 entries of: union { u64 bits; struct { u8 vector, ... } } (8 bytes each)
                // = kvm_ioapic_state
                let entry_offset = 24 + (pin as usize) * 8; // 8+4+4+4+4=24 header bytes
                if entry_offset + 8 <= data.len() {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&data[entry_offset..entry_offset + 8]);
                    u64::from_le_bytes(bytes)
                } else {
                    u64::MAX
                }
            }
            Err(_) => u64::MAX,
        }
    }
    #[cfg(not(feature = "linux"))]
    {
        let _ = (handle, pin);
        u64::MAX
    }
}

/// Get a pointer to the VGA framebuffer pixel data.
/// Sets `*out_ptr` and `*out_len`. Returns 0 on success, -1 on error.
/// Returns len=0 when in text mode (caller should use get_text_buffer instead).
#[no_mangle]
pub extern "C" fn corevm_vga_get_framebuffer(
    handle: u64, out_ptr: *mut *const u8, out_len: *mut u32,
) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if out_ptr.is_null() || out_len.is_null() { return -1; }
    match vm.svga() {
        Some(svga) => {
            // Always return the full VRAM buffer. On KVM it is mapped as
            // a hypervisor memory region and the guest writes directly to it.
            // The caller can check corevm_vga_get_mode() if it needs to know
            // whether the guest is in text or graphics mode.
            let fb = svga.get_framebuffer();
            unsafe {
                *out_ptr = fb.as_ptr();
                *out_len = fb.len() as u32;
            }
            0
        }
        None => -1,
    }
}

/// Get the current VGA display mode dimensions.
/// Returns 0 on success and fills out_width, out_height, out_bpp.
/// Returns 1 if in text mode (out_width=80, out_height=25, out_bpp=0).
/// Returns -1 on error.
#[no_mangle]
pub extern "C" fn corevm_vga_get_mode(
    handle: u64, out_width: *mut u32, out_height: *mut u32, out_bpp: *mut u8,
) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if out_width.is_null() || out_height.is_null() || out_bpp.is_null() { return -1; }
    match vm.svga() {
        Some(svga) => {
            match &svga.mode {
                crate::devices::svga::VgaMode::Text80x25 => {
                    unsafe { *out_width = 80; *out_height = 25; *out_bpp = 0; }
                    1
                }
                crate::devices::svga::VgaMode::Graphics320x200x256 => {
                    unsafe { *out_width = 320; *out_height = 200; *out_bpp = 8; }
                    0
                }
                crate::devices::svga::VgaMode::Graphics640x480x16 => {
                    unsafe { *out_width = 640; *out_height = 480; *out_bpp = 4; }
                    0
                }
                crate::devices::svga::VgaMode::LinearFramebuffer { width, height, bpp } => {
                    unsafe { *out_width = *width; *out_height = *height; *out_bpp = *bpp; }
                    0
                }
            }
        }
        None => -1,
    }
}

/// Get the current VGA linear framebuffer physical address from PCI BAR0.
/// SeaBIOS may relocate BARs during PCI enumeration, so this can differ
/// from the initial 0xFD000000.  Returns the BAR0 address, or 0 on error.
#[no_mangle]
pub extern "C" fn corevm_vga_get_lfb_addr(handle: u64) -> u64 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.pci_bus_ptr.is_null() { return 0; }
    let pci_bus = unsafe { &*vm.pci_bus_ptr };
    // VGA device is at bus 0, device 2, function 0
    for dev in &pci_bus.devices {
        if dev.bus == 0 && dev.device == 2 && dev.function == 0 {
            // BAR0 at offset 0x10 (32-bit MMIO BAR)
            let bar0 = (dev.config_space[0x10] as u32)
                | ((dev.config_space[0x11] as u32) << 8)
                | ((dev.config_space[0x12] as u32) << 16)
                | ((dev.config_space[0x13] as u32) << 24);
            // Mask off type bits (bits 0-3 for MMIO BAR)
            return (bar0 & 0xFFFF_FFF0) as u64;
        }
    }
    0
}

/// Get the byte offset into VRAM where the display framebuffer starts.
///
/// bochs-drm (Linux DRM driver) uses VBE_DISPI_INDEX_X_OFFSET (reg 8) and
/// VBE_DISPI_INDEX_Y_OFFSET (reg 9) to place its framebuffer at an arbitrary
/// offset within VRAM.  The display start is at:
///   `(y_offset * virt_width + x_offset) * bytes_per_pixel`
///
/// Returns the byte offset, or 0 if the offset registers are zero or on error.
#[no_mangle]
pub extern "C" fn corevm_vga_get_fb_offset(handle: u64) -> u64 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    match vm.svga() {
        Some(svga) => {
            let x_off = svga.vbe_regs[8] as u64;   // VBE_DISPI_INDEX_X_OFFSET
            let y_off = svga.vbe_regs[9] as u64;   // VBE_DISPI_INDEX_Y_OFFSET
            let virt_w = svga.vbe_regs[6] as u64;  // VBE_DISPI_INDEX_VIRT_WIDTH
            let bpp = svga.vbe_regs[3] as u64;     // VBE_DISPI_INDEX_BPP
            let bytes_pp = (bpp + 7) / 8;
            (y_off * virt_w + x_off) * bytes_pp
        }
        None => 0,
    }
}

/// Get a pointer to the VGA text buffer (array of u16: char+attr pairs).
/// Sets `*out_ptr` and `*out_len` (number of u16 entries). Returns 0 on success, -1 on error.
/// In hardware-virt mode, syncs the text buffer from guest RAM first.
#[no_mangle]
pub extern "C" fn corevm_vga_get_text_buffer(
    handle: u64, out_ptr: *mut *const u16, out_len: *mut u32,
) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if out_ptr.is_null() || out_len.is_null() { return -1; }

    // In hardware-virt mode (KVM), sync text buffer from guest RAM
    // since VGA memory writes bypass the MMIO handler.
    let (ram_ptr, ram_size) = vm.memory.ram_ptr();
    if ram_size > 0xB8000 + 80 * 25 * 2 {
        if let Some(svga) = vm.svga_mut() {
            unsafe { svga.sync_text_buffer_from_ram(ram_ptr); }
        }
    }

    match vm.svga() {
        Some(svga) => {
            let tb = svga.get_text_buffer();
            unsafe {
                *out_ptr = tb.as_ptr();
                *out_len = tb.len() as u32;
            }
            0
        }
        None => -1,
    }
}

// ── E1000 Network Packet Exchange ──

/// Take all packets transmitted by the guest.
/// Returns the number of packets written to the output buffer.
/// Each packet is prefixed by a 2-byte little-endian length.
/// Format: [len_lo, len_hi, data...] repeated.
#[no_mangle]
pub extern "C" fn corevm_e1000_take_tx(handle: u64, buf: *mut u8, buf_len: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let e1000_arc = match vm.e1000.as_ref() { Some(a) => a, None => return 0 };
    let mut e1000 = e1000_arc.lock().unwrap();
    let packets = e1000.take_tx_packets();
    if packets.is_empty() { return 0; }
    let out = unsafe { core::slice::from_raw_parts_mut(buf, buf_len as usize) };
    let mut offset = 0;
    let mut count = 0;
    for pkt in &packets {
        let needed = 2 + pkt.len();
        if offset + needed > out.len() { break; }
        let len = pkt.len() as u16;
        out[offset] = len as u8;
        out[offset + 1] = (len >> 8) as u8;
        out[offset + 2..offset + 2 + pkt.len()].copy_from_slice(pkt);
        offset += needed;
        count += 1;
    }
    count
}

/// Deliver a received packet to the E1000 NIC for guest consumption.
/// The packet should be a raw Ethernet frame (no length prefix).
#[no_mangle]
pub extern "C" fn corevm_e1000_receive(handle: u64, data: *const u8, len: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if data.is_null() || len == 0 { return -1; }
    let pkt = unsafe { core::slice::from_raw_parts(data, len as usize) };
    let e1000_arc = match vm.e1000.as_ref() { Some(a) => a, None => return -1 };
    let mut e1000 = e1000_arc.lock().unwrap();
    e1000.receive_packet(pkt);
    // Immediately try to deliver to guest via RX descriptor ring DMA.
    e1000.process_rx_ring();
    0
}

/// Check if E1000 has pending RX interrupt and return ICR value.
/// Returns 0 if no pending interrupt, or the ICR bits if interrupt pending.
#[no_mangle]
pub extern "C" fn corevm_e1000_has_rx_irq(handle: u64) -> u32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let e1000_arc = match vm.e1000.as_ref() { Some(a) => a, None => return 0 };
    let e1000 = e1000_arc.lock().unwrap();
    let icr = e1000.regs[0x00C0 / 4];
    let ims = e1000.regs[0x00D0 / 4];
    icr & ims // Only report masked-in interrupts
}

// ── Network Backend ──

/// Set up the network backend for the VM.
/// mode: 0 = none, 1 = user-mode NAT (SLIRP).
/// For TAP/bridge mode use [`corevm_setup_net_tap`] instead.
/// Must be called AFTER corevm_setup_e1000().
#[no_mangle]
pub extern "C" fn corevm_setup_net(handle: u64, mode: i32) -> i32 {
    #[cfg(feature = "std")]
    {
        let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
        match mode {
            0 => {
                vm.net_backend = Some(alloc::boxed::Box::new(crate::devices::net::NullNet));
                0
            }
            1 => {
                #[cfg(feature = "linux")]
                {
                    vm.net_backend = Some(alloc::boxed::Box::new(crate::devices::slirp::SlirpNet::new()));
                    0
                }
                #[cfg(not(feature = "linux"))]
                { -1 }
            }
            _ => -1, // unknown mode
        }
    }
    #[cfg(not(feature = "std"))]
    { let _ = (handle, mode); -1 }
}

/// Set up TAP/bridge network backend.
///
/// Creates a TAP device, brings it up, and optionally joins it to a Linux
/// bridge.  The guest gets a real Layer-2 presence on the host network.
///
/// * `tap_name_ptr`    — C string: requested TAP name (empty = kernel assigns).
/// * `bridge_name_ptr` — C string: bridge to join (empty = standalone TAP).
///
/// Requires CAP_NET_ADMIN or root.  Returns 0 on success, -1 on error.
#[cfg(feature = "linux")]
#[no_mangle]
pub extern "C" fn corevm_setup_net_tap(
    handle: u64,
    tap_name_ptr: *const u8,
    tap_name_len: u32,
    bridge_name_ptr: *const u8,
    bridge_name_len: u32,
) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };

    let tap_name = if tap_name_ptr.is_null() || tap_name_len == 0 {
        ""
    } else {
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(tap_name_ptr, tap_name_len as usize)) }
    };

    let bridge_name = if bridge_name_ptr.is_null() || bridge_name_len == 0 {
        ""
    } else {
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(bridge_name_ptr, bridge_name_len as usize)) }
    };

    match crate::devices::net::TapNet::new(tap_name, bridge_name) {
        Ok(tap_net) => {
            vm.net_backend = Some(alloc::boxed::Box::new(tap_net));
            0
        }
        Err(e) => {
            eprintln!("[corevm] TAP setup failed: {}", e);
            -1
        }
    }
}

/// Stub for non-Linux platforms (TAP not available).
#[cfg(not(feature = "linux"))]
#[no_mangle]
pub extern "C" fn corevm_setup_net_tap(
    handle: u64,
    _tap_name_ptr: *const u8,
    _tap_name_len: u32,
    _bridge_name_ptr: *const u8,
    _bridge_name_len: u32,
) -> i32 {
    let _ = handle;
    -1
}

/// Setup user-mode networking with custom SDN configuration.
/// The config pointer must point to a valid SlirpConfig struct.
/// Returns 0 on success.
#[cfg(feature = "linux")]
#[no_mangle]
pub extern "C" fn corevm_setup_net_sdn(handle: u64, config_ptr: *const crate::devices::slirp::SlirpConfig) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if config_ptr.is_null() { return -1; }
    let config = unsafe { &*config_ptr };
    let sdn_config = crate::devices::slirp::SlirpConfig {
        net_prefix: config.net_prefix,
        gateway_ip: config.gateway_ip,
        dns_ip: config.dns_ip,
        guest_ip: config.guest_ip,
        netmask: config.netmask,
        gw_mac: config.gw_mac,
        custom_dns: config.custom_dns,
        pxe_boot_file: config.pxe_boot_file.clone(),
        pxe_next_server: config.pxe_next_server,
    };
    vm.net_backend = Some(alloc::boxed::Box::new(
        crate::devices::slirp::SlirpNet::with_config(sdn_config)
    ));
    0
}

/// Stub for non-Linux platforms (SLIRP not available).
#[cfg(not(feature = "linux"))]
#[no_mangle]
pub extern "C" fn corevm_setup_net_sdn(handle: u64, config_ptr: *const core::ffi::c_void) -> i32 {
    let _ = (handle, config_ptr);
    -1
}

/// Poll the network backend: move TX packets from E1000 to backend,
/// move RX packets from backend to E1000. Call periodically from VM loop.
/// Returns the number of RX packets delivered to the guest.
#[no_mangle]
pub extern "C" fn corevm_net_poll(handle: u64) -> i32 {
    #[cfg(feature = "std")]
    {
        let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
        let use_e1000 = vm.e1000.is_some();
        let use_virtio_net = !vm.virtio_net_ptr.is_null();

        // Run a TX→backend→RX loop. This allows ACKs from the guest to
        // immediately trigger new data reads from host sockets within the
        // same poll call, eliminating the 10ms+ round-trip latency that
        // would otherwise occur between polls.
        let mut total_rx: i32 = 0;
        let mut total_tx: i32 = 0;
        for round in 0..4 {
            // Take TX packets from the active NIC and send to backend.
            // Lock is held only for the brief take_tx_packets() call.
            let tx_packets = if use_virtio_net {
                let vnet = unsafe { &mut *vm.virtio_net_ptr };
                vnet.take_tx_packets()
            } else if use_e1000 {
                let mut e1000 = vm.e1000.as_ref().unwrap().lock().unwrap();
                e1000.take_tx_packets()
                // Lock released here
            } else {
                alloc::vec::Vec::new()
            };

            let backend = match &mut vm.net_backend {
                Some(b) => b,
                None => return 0,
            };

            total_tx += tx_packets.len() as i32;
            for pkt in &tx_packets {
                backend.send(pkt);
            }

            // Poll backend for periodic work (timers, TCP reads, etc.)
            backend.poll();

            // Receive packets from backend and inject into the active NIC.
            let rx_packets = backend.recv();
            if rx_packets.is_empty() && tx_packets.is_empty() {
                break; // Nothing happening — no need for more rounds
            }
            let rx_count = rx_packets.len() as i32;
            total_rx += rx_count;

            if !rx_packets.is_empty() {
                if use_virtio_net {
                    let vnet = unsafe { &mut *vm.virtio_net_ptr };
                    for pkt in &rx_packets {
                        vnet.receive_packet(pkt);
                    }
                    vnet.process_rx();
                } else if use_e1000 {
                    // Lock briefly to enqueue packets, then release before
                    // process_rx_ring to minimize contention with AP threads.
                    let mut e1000 = vm.e1000.as_ref().unwrap().lock().unwrap();
                    const RX_BUFFER_LIMIT: usize = 512;
                    for pkt in &rx_packets {
                        if e1000.rx_buffer.len() >= RX_BUFFER_LIMIT { break; }
                        e1000.receive_packet(pkt);
                    }
                    // process_rx_ring does DMA into guest memory — keep lock
                    // held here since it modifies E1000 state (head pointer,
                    // ICR bits). The operation is fast (memcpy to guest RAM).
                    e1000.process_rx_ring();
                    // Lock released here
                }
            }

            // Net-poll diagnostic logging disabled — too noisy.
            // Uncomment for debugging:
            // if total_tx > 0 || total_rx > 0 {
            //     static mut NET_POLL_DIAG: u32 = 0;
            //     unsafe { NET_POLL_DIAG += 1; }
            //     if unsafe { NET_POLL_DIAG } % 200 == 0 {
            //         eprintln!("[net-poll] round={} tx={} rx={} total_tx={} total_rx={}",
            //             round, tx_packets.len(), rx_count, total_tx, total_rx);
            //     }
            // }
        }

        total_rx
    }
    #[cfg(not(feature = "std"))]
    { let _ = handle; 0 }
}

// ── AC97 Audio ──

/// Set up the AC97 audio controller on the VM.
/// Registers as PCI device 00:05.0 (Intel 82801AA, 8086:2415).
/// NAM I/O ports at 0x1C00 (256 bytes), NABM at 0x1D00 (64 bytes).
#[no_mangle]
pub extern "C" fn corevm_setup_ac97(handle: u64) -> i32 {
    const AC97_NAM_BASE: u16 = 0x1C00;
    const AC97_NABM_BASE: u16 = 0x1D00;

    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };

    let mut ac97 = Box::new(crate::devices::ac97::Ac97::new());
    // Give AC97 access to guest RAM for DMA reads
    let (ram_ptr, ram_size) = vm.memory.ram_mut_ptr();
    ac97.set_ram(ram_ptr as *const u8, ram_size);

    let ac97_ptr = &*ac97 as *const crate::devices::ac97::Ac97 as *mut crate::devices::ac97::Ac97;
    vm.ac97_ptr = ac97_ptr;

    // Register NAM/NABM at initial I/O ports (for direct access before BAR remap)
    vm.io.register(AC97_NAM_BASE, 256, Box::new(crate::devices::ac97::Ac97Nam(ac97_ptr)));
    vm.io.register(AC97_NABM_BASE, 64, Box::new(crate::devices::ac97::Ac97Nabm(ac97_ptr)));

    // Also register in PCI I/O router for after SeaBIOS remaps BARs
    if !vm.pci_io_router_ptr.is_null() {
        let router = unsafe { &mut *vm.pci_io_router_ptr };
        router.ac97 = ac97_ptr;
    }

    // Leak the Ac97 box — it lives as long as the VM
    core::mem::forget(ac97);

    // Register as PCI device
    if !vm.pci_bus_ptr.is_null() {
        let pci_bus = unsafe { &mut *vm.pci_bus_ptr };
        // Intel 82801AA AC97 Audio: vendor 8086, device 2415, class 04 (Multimedia), subclass 01 (Audio)
        let mut pci_dev = crate::devices::bus::PciDevice::new(0x8086, 0x2415, 0x04, 0x01, 0x00);
        pci_dev.device = vm.chipset.slots.ac97;
        // BAR0: NAM I/O at 0x1C00, 256 bytes
        pci_dev.set_bar(0, AC97_NAM_BASE as u32, 256, false); // false = I/O space
        // BAR1: NABM I/O at 0x1D00, 64 bytes
        pci_dev.set_bar(1, AC97_NABM_BASE as u32, 64, false);
        // Interrupt: IRQ 5, pin INTA
        pci_dev.set_interrupt(5, 1);
        pci_dev.set_subsystem(0x8086, 0x0000);
        pci_bus.add_device(pci_dev);
    }
    0
}

/// Process AC97 DMA: read audio data from guest buffers.
/// Call periodically (every 10-20ms). Returns 1 if interrupt pending, 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_ac97_process(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let ac97 = match vm.ac97() { Some(a) => a, None => return 0 };
    if ac97.process_po() { 1 } else { 0 }
}

/// Take buffered audio samples for host playback.
/// Writes interleaved 16-bit stereo PCM to the output buffer.
/// Returns number of samples written (not bytes — multiply by 2 for byte count).
#[no_mangle]
pub extern "C" fn corevm_ac97_take_audio(handle: u64, buf: *mut i16, max_samples: u32) -> u32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let ac97 = match vm.ac97() { Some(a) => a, None => return 0 };
    let samples = ac97.take_audio();
    if samples.is_empty() || buf.is_null() { return 0; }
    let count = samples.len().min(max_samples as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(samples.as_ptr(), buf, count);
    }
    count as u32
}

/// Get the AC97 configured sample rate.
#[no_mangle]
pub extern "C" fn corevm_ac97_sample_rate(handle: u64) -> u32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 48000 };
    let ac97 = match vm.ac97() { Some(a) => a, None => return 48000 };
    ac97.sample_rate()
}

// ── PCI I/O Port Router ──
// SeaBIOS remaps PCI BAR I/O addresses. This catch-all I/O handler covers
// the typical PCI I/O allocation range (0xC000-0xCFFF) and dynamically
// routes accesses to UHCI and AC97 based on their current PCI BAR values.

const PCI_IO_ROUTER_BASE: u16 = 0xC000;
const PCI_IO_ROUTER_SIZE: u16 = 0x1000; // 4KB: 0xC000-0xCFFF

pub struct PciIoRouter {
    pub uhci: *mut crate::devices::uhci::Uhci,
    pub ac97: *mut crate::devices::ac97::Ac97,
    #[cfg(feature = "std")]
    pub e1000: Option<alloc::sync::Arc<std::sync::Mutex<crate::devices::e1000::E1000>>>,
    #[cfg(not(feature = "std"))]
    pub e1000: *mut crate::devices::e1000::E1000,
    pci_bus: *mut crate::devices::bus::PciBus,
    pub chipset: &'static crate::devices::chipset::ChipsetConfig,
}

unsafe impl Send for PciIoRouter {}

impl PciIoRouter {
    /// Read UHCI BAR4 (device 00:06.0, config offset 0x20) from PCI config.
    fn read_io_bar(&self, slot: u8, bar_offset: usize, align_mask: u16) -> u16 {
        if self.pci_bus.is_null() { return 0; }
        let bus = unsafe { &mut *self.pci_bus };
        (bus.mmcfg_read(0, slot, 0, bar_offset, 4) as u16) & align_mask
    }

    fn uhci_bar4(&self) -> u16 {
        if self.uhci.is_null() { return 0; }
        self.read_io_bar(self.chipset.slots.uhci, 0x20, 0xFFE0)
    }
    fn ac97_bar0(&self) -> u16 {
        if self.ac97.is_null() { return 0; }
        self.read_io_bar(self.chipset.slots.ac97, 0x10, 0xFF00)
    }
    fn e1000_bar2(&self) -> u16 {
        #[cfg(feature = "std")]
        { if self.e1000.is_none() { return 0; } }
        #[cfg(not(feature = "std"))]
        { if self.e1000.is_null() { return 0; } }
        self.read_io_bar(self.chipset.slots.e1000, 0x18, 0xFFF8)
    }
    #[cfg(feature = "std")]
    fn has_e1000(&self) -> bool { self.e1000.is_some() }
    #[cfg(not(feature = "std"))]
    fn has_e1000(&self) -> bool { !self.e1000.is_null() }
    fn ac97_bar1(&self) -> u16 {
        if self.ac97.is_null() { return 0; }
        self.read_io_bar(self.chipset.slots.ac97, 0x14, 0xFFC0)
    }
}

impl crate::io::IoHandler for PciIoRouter {
    fn read(&mut self, port: u16, size: u8) -> crate::error::Result<u32> {
        // Check E1000 BAR2 (8 bytes: IOADDR at +0, IODATA at +4)
        // The 82540EM driver uses E1000_WRITE_REG_IO for reset — this is
        // critical for e1000_reset_hw() to work.
        if self.has_e1000() {
            let e1000_io = self.e1000_bar2();
            if e1000_io != 0 && port >= e1000_io && port < e1000_io + 8 {
                let offset = port - e1000_io;
                #[cfg(feature = "std")]
                {
                    let mut e1000 = self.e1000.as_ref().unwrap().lock().unwrap();
                    if offset < 4 {
                        return Ok(e1000.io_addr);
                    } else {
                        use crate::memory::mmio::MmioHandler;
                        let reg = e1000.io_addr as u64;
                        let val = e1000.read(reg, 4)?;
                        return Ok(val as u32);
                    }
                }
                #[cfg(not(feature = "std"))]
                {
                    let e1000 = unsafe { &mut *self.e1000 };
                    if offset < 4 {
                        return Ok(e1000.io_addr);
                    } else {
                        use crate::memory::mmio::MmioHandler;
                        let val = e1000.read(e1000.io_addr as u64, 4)?;
                        return Ok(val as u32);
                    }
                }
            }
        }

        // Check UHCI BAR4 (32 bytes)
        let uhci_base = self.uhci_bar4();
        if uhci_base != 0 && port >= uhci_base && port < uhci_base + 32 {
            let uhci = unsafe { &mut *self.uhci };
            return uhci.read(port - uhci_base, size);
        }

        // Check AC97 NAM BAR0 (256 bytes)
        let ac97_nam = self.ac97_bar0();
        if ac97_nam != 0 && port >= ac97_nam && port < ac97_nam + 256 {
            let ac97 = unsafe { &mut *self.ac97 };
            return crate::devices::ac97::Ac97Nam::read_static(ac97, port - ac97_nam, size);
        }

        // Check AC97 NABM BAR1 (64 bytes)
        let ac97_nabm = self.ac97_bar1();
        if ac97_nabm != 0 && port >= ac97_nabm && port < ac97_nabm + 64 {
            let ac97 = unsafe { &mut *self.ac97 };
            return crate::devices::ac97::Ac97Nabm::read_static(ac97, port - ac97_nabm, size);
        }

        Ok(0xFFFFFFFF) // bus float
    }

    fn write(&mut self, port: u16, size: u8, val: u32) -> crate::error::Result<()> {
        // Check E1000 BAR2 (8 bytes: IOADDR at +0, IODATA at +4)
        if self.has_e1000() {
            let e1000_io = self.e1000_bar2();
            if e1000_io != 0 && port >= e1000_io && port < e1000_io + 8 {
                let offset = port - e1000_io;
                #[cfg(feature = "std")]
                {
                    let mut e1000 = self.e1000.as_ref().unwrap().lock().unwrap();
                    if offset < 4 {
                        e1000.io_addr = val;
                    } else {
                        use crate::memory::mmio::MmioHandler;
                        let reg = e1000.io_addr;
                        let _ = e1000.write(reg as u64, 4, val as u64);
                    }
                    return Ok(());
                }
                #[cfg(not(feature = "std"))]
                {
                    let e1000 = unsafe { &mut *self.e1000 };
                    if offset < 4 {
                        e1000.io_addr = val;
                    } else {
                        use crate::memory::mmio::MmioHandler;
                        let reg = e1000.io_addr;
                        let _ = e1000.write(reg as u64, 4, val as u64);
                    }
                    return Ok(());
                }
            }
        }

        // Check UHCI BAR4 (32 bytes)
        let uhci_base = self.uhci_bar4();
        if uhci_base != 0 && port >= uhci_base && port < uhci_base + 32 {
            let uhci = unsafe { &mut *self.uhci };
            return uhci.write(port - uhci_base, size, val);
        }

        // Check AC97 NAM BAR0 (256 bytes)
        let ac97_nam = self.ac97_bar0();
        if ac97_nam != 0 && port >= ac97_nam && port < ac97_nam + 256 {
            let ac97 = unsafe { &mut *self.ac97 };
            return crate::devices::ac97::Ac97Nam::write_static(ac97, port - ac97_nam, size, val);
        }

        // Check AC97 NABM BAR1 (64 bytes)
        let ac97_nabm = self.ac97_bar1();
        if ac97_nabm != 0 && port >= ac97_nabm && port < ac97_nabm + 64 {
            let ac97 = unsafe { &mut *self.ac97 };
            return crate::devices::ac97::Ac97Nabm::write_static(ac97, port - ac97_nabm, size, val);
        }

        Ok(())
    }
}

// ── UHCI USB Controller ──

/// Set up the UHCI USB 1.1 controller with an integrated USB tablet device.
/// Registers as PCI device 00:06.0 (Intel PIIX3 UHCI, 8086:7020).
/// I/O ports are dynamically routed via the PCI I/O Router. IRQ 9.
#[no_mangle]
pub extern "C" fn corevm_setup_uhci(handle: u64) -> i32 {
    const UHCI_IO_BASE: u16 = 0xC100; // initial BAR value (SeaBIOS will remap)

    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };

    let mut uhci = Box::new(crate::devices::uhci::Uhci::new());
    let (ram_ptr, ram_size) = vm.memory.ram_mut_ptr();
    uhci.set_guest_memory(ram_ptr, ram_size);
    // Connect the USB tablet device on port 1.
    // The UHCI controller is only set up when the user enables USB tablet,
    // so always connect the tablet here.
    uhci.connect_tablet();

    let uhci_ptr = &*uhci as *const crate::devices::uhci::Uhci as *mut crate::devices::uhci::Uhci;
    vm.uhci_ptr = uhci_ptr;
    core::mem::forget(uhci);

    // Register PCI I/O Router if not yet registered
    // (covers 0xC000-0xCFFF for all PCI I/O BAR devices)
    if vm.pci_io_router_ptr.is_null() {
        let router = Box::new(PciIoRouter {
            uhci: uhci_ptr,
            ac97: vm.ac97_ptr,
            #[cfg(feature = "std")]
            e1000: vm.e1000.clone(),
            #[cfg(not(feature = "std"))]
            e1000: vm.e1000_ptr,
            pci_bus: vm.pci_bus_ptr,
            chipset: vm.chipset,
        });
        let router_ptr = &*router as *const PciIoRouter as *mut PciIoRouter;
        vm.pci_io_router_ptr = router_ptr;
        vm.io.register(PCI_IO_ROUTER_BASE, PCI_IO_ROUTER_SIZE, router);
    } else {
        // Router already exists — just add UHCI pointer
        let router = unsafe { &mut *vm.pci_io_router_ptr };
        router.uhci = uhci_ptr;
    }

    // Register as PCI device
    if !vm.pci_bus_ptr.is_null() {
        let pci_bus = unsafe { &mut *vm.pci_bus_ptr };
        let mut pci_dev = crate::devices::bus::PciDevice::new(0x8086, 0x7020, 0x0C, 0x03, 0x00);
        pci_dev.device = vm.chipset.slots.uhci;
        pci_dev.set_bar(4, UHCI_IO_BASE as u32, 32, false);
        // Interrupt: INTD → PIRQB → IRQ 5 (via PIIX3 swizzle: (6+3)%4=1)
        pci_dev.set_interrupt(5, 4);
        pci_dev.set_subsystem(0x8086, 0x7020);
        // PIIX3 UHCI-specific: Serial Bus Release Number (USB 1.1 = 0x10)
        pci_dev.config_space[0x60] = 0x10;
        // LEGSUP register (Legacy Support) at 0xC0 — required by Windows UHCI driver
        pci_dev.config_space[0xC0] = 0x00;
        pci_dev.config_space[0xC1] = 0x20; // LEGSUP: bit 13 = USBPIRQ routed
        pci_bus.add_device(pci_dev);
    }
    0
}

/// Send absolute tablet coordinates to the USB tablet device.
/// x, y are in range 0..32767. buttons: bit0=left, bit1=right, bit2=middle.
#[no_mangle]
pub extern "C" fn corevm_usb_tablet_move(handle: u64, x: u16, y: u16, buttons: u8) -> i32 {
    corevm_usb_tablet_move_wheel(handle, x, y, buttons, 0)
}

#[no_mangle]
pub extern "C" fn corevm_usb_tablet_move_wheel(handle: u64, x: u16, y: u16, buttons: u8, wheel: i8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let uhci = match vm.uhci() { Some(u) => u, None => return -1 };
    uhci.tablet_move(x, y, buttons, wheel);
    0
}

/// Process one UHCI frame (call periodically, ~1kHz or at least every 10ms).
/// Returns 1 if IRQ pending, 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_uhci_process(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let uhci = match vm.uhci() { Some(u) => u, None => return 0 };
    if uhci.process_frame() { 1 } else { 0 }
}

// ── VMware Backdoor (absolute pointer) ──────────────────────────────────────

/// Send absolute mouse coordinates via the VMware backdoor.
/// x, y are in range 0..65535 (0%–100% of screen).
/// buttons: bit0=left, bit1=right, bit2=middle.
/// This is thread-safe (uses atomics internally).
#[no_mangle]
pub extern "C" fn corevm_vmware_mouse_move(handle: u64, x: u16, y: u16, buttons: u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    vm.vmware_backdoor.set_position(x, y, buttons);
    0
}

/// Send absolute mouse coordinates + wheel via the VMware backdoor.
#[no_mangle]
pub extern "C" fn corevm_vmware_mouse_move_wheel(handle: u64, x: u16, y: u16, buttons: u8, wheel: i8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    vm.vmware_backdoor.set_position_wheel(x, y, buttons, wheel);
    0
}

/// Check if the guest has enabled the VMware absolute pointer.
/// Returns 1 if enabled, 0 if not.
#[no_mangle]
pub extern "C" fn corevm_vmware_pointer_enabled(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.vmware_backdoor.is_enabled() { 1 } else { 0 }
}

// ── VirtIO GPU FFI ──

/// Set up the VirtIO GPU device at PCI slot 00:07.0.
/// Must be called AFTER corevm_setup_standard_devices() and
/// AFTER corevm_setup_ahci() (which creates the PCI MMIO router).
/// `vram_mb`: VRAM size in MiB (0 = default 256).
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn corevm_setup_virtio_gpu(handle: u64, vram_mb: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };

    // Remove VGA PCI device from the bus so the guest only sees VirtIO GPU.
    // The VGA I/O ports and SVGA device remain active for BIOS text-mode boot.
    if !vm.pci_bus_ptr.is_null() {
        let pci_bus = unsafe { &mut *vm.pci_bus_ptr };
        let vga_slot = vm.chipset.slots.vga;
        pci_bus.devices.retain(|d| d.device != vga_slot);
    }

    vm.setup_virtio_gpu(vram_mb);

    // Update the PCI MMIO router if it exists.
    if !vm.pci_mmio_router_ptr.is_null() && !vm.virtio_gpu_ptr.is_null() {
        let router = unsafe { &mut *vm.pci_mmio_router_ptr };
        router.virtio_gpu = vm.virtio_gpu_ptr;
    }

    // Register the notify callback: kick vCPU 0 so poll_irqs runs promptly.
    // Do NOT call set_irq_line here — IRQ 11 is shared between AHCI, E1000,
    // VirtIO-GPU, and VirtIO-Net. Direct assert/de-assert from the callback
    // races with poll_irqs and causes either lost AHCI interrupts (if we
    // de-assert) or stuck IRQ line (if we only assert). Instead, the GPU
    // sets isr_status in virtio_notify() and poll_irqs handles the shared
    // IRQ 11 line correctly by checking all devices before assert/de-assert.
    #[cfg(feature = "linux")]
    if !vm.virtio_gpu_ptr.is_null() {
        let vs = vm.backend.vm_slot;
        let gpu = unsafe { &mut *vm.virtio_gpu_ptr };
        gpu.notify_callback = Some(alloc::boxed::Box::new(move || {
            // Just kick the BSP so it re-enters the poll loop quickly.
            // poll_irqs will see gpu.isr_status and deliver IRQ 11.
            crate::backend::kvm::cancel_vcpu_kvm(vs, 0);
        }));
    }

    0
}

/// Process pending VirtIO GPU virtqueue commands.
/// Call periodically from the VM run loop (e.g., every vCPU exit or ~60Hz).
/// Returns 1 if IRQ pending (needs delivery), 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_virtio_gpu_process(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let gpu = match vm.virtio_gpu() { Some(g) => g, None => return 0 };
    gpu.process();
    // Don't clear irq_pending here — poll_irqs handles IRQ delivery
    // and clears it when pulsing the IRQ line.
    if gpu.irq_pending { 1 } else { 0 }
}

/// Get VirtIO GPU scanout framebuffer pointer and size.
/// Sets *out_ptr and *out_len. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn corevm_virtio_gpu_get_framebuffer(
    handle: u64,
    out_ptr: *mut *const u8,
    out_len: *mut u32,
) -> i32 {
    if out_ptr.is_null() || out_len.is_null() { return -1; }
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let gpu = match vm.virtio_gpu() { Some(g) => g, None => return -1 };
    unsafe {
        *out_ptr = gpu.framebuffer.as_ptr();
        *out_len = gpu.framebuffer.len() as u32;
    }
    0
}

/// Get VirtIO GPU current scanout dimensions.
/// Returns 0 if GPU mode is active, -1 on error.
/// Sets width, height, bpp (always 32 for VirtIO GPU BGRA32 framebuffer).
#[no_mangle]
pub extern "C" fn corevm_virtio_gpu_get_mode(
    handle: u64,
    out_width: *mut u32,
    out_height: *mut u32,
    out_bpp: *mut u8,
) -> i32 {
    if out_width.is_null() || out_height.is_null() || out_bpp.is_null() { return -1; }
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let gpu = match vm.virtio_gpu_ref() { Some(g) => g, None => return -1 };
    unsafe {
        *out_width = gpu.fb_width;
        *out_height = gpu.fb_height;
        *out_bpp = 32;
    }
    0
}

/// Check if the VirtIO GPU device is set up and active.
/// Returns 1 if active, 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_has_virtio_gpu(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.virtio_gpu_ptr.is_null() { 0 } else { 1 }
}

/// Check if the VirtIO GPU scanout is active (guest driver has configured display).
/// Returns 1 if the guest driver has set up a scanout with a valid resource, 0 otherwise.
/// Use this to decide whether to show the VirtIO GPU framebuffer or fall back to VGA.
#[no_mangle]
pub extern "C" fn corevm_virtio_gpu_scanout_active(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let gpu = match vm.virtio_gpu_ref() { Some(g) => g, None => return 0 };
    if gpu.scanout_active { 1 } else { 0 }
}

// ── Intel HD Graphics FFI ──

/// Set up the Intel HD Graphics device, replacing the standard VGA adapter.
/// `vram_mb` is VRAM size in MiB (clamped to 64-512).
/// Must be called AFTER corevm_setup_standard_devices().
#[no_mangle]
pub extern "C" fn corevm_setup_intel_gpu(handle: u64, vram_mb: u32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };

    // Remove standard VGA PCI device
    if !vm.pci_bus_ptr.is_null() {
        let pci_bus = unsafe { &mut *vm.pci_bus_ptr };
        let vga_slot = vm.chipset.slots.vga;
        pci_bus.devices.retain(|d| d.device != vga_slot);
    }

    vm.setup_intel_gpu(vram_mb);

    // Update the PCI MMIO router so it routes Intel GPU BAR accesses
    if !vm.pci_mmio_router_ptr.is_null() && !vm.intel_gpu_ptr.is_null() {
        let router = unsafe { &mut *vm.pci_mmio_router_ptr };
        router.intel_gpu = vm.intel_gpu_ptr;
    }

    0
}

/// Get Intel GPU framebuffer pointer and size.
#[no_mangle]
pub extern "C" fn corevm_intel_gpu_get_framebuffer(
    handle: u64, out_ptr: *mut *const u8, out_len: *mut u32,
) -> i32 {
    if out_ptr.is_null() || out_len.is_null() { return -1; }
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let gpu = match vm.intel_gpu_ref() { Some(g) => g, None => return -1 };
    let (ptr, len) = gpu.framebuffer_ptr();
    unsafe { *out_ptr = ptr; *out_len = len as u32; }
    0
}

/// Get Intel GPU current display mode.
#[no_mangle]
pub extern "C" fn corevm_intel_gpu_get_mode(
    handle: u64, out_width: *mut u32, out_height: *mut u32, out_bpp: *mut u8,
) -> i32 {
    if out_width.is_null() || out_height.is_null() || out_bpp.is_null() { return -1; }
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let gpu = match vm.intel_gpu_ref() { Some(g) => g, None => return -1 };
    let (w, h, bpp) = gpu.display_mode();
    unsafe { *out_width = w; *out_height = h; *out_bpp = bpp as u8; }
    0
}

/// Refresh Intel GPU framebuffer from VRAM.
/// Call periodically (~60 Hz) to update the host display.
#[no_mangle]
pub extern "C" fn corevm_intel_gpu_refresh(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let gpu = match vm.intel_gpu() { Some(g) => g, None => return -1 };
    gpu.refresh_framebuffer();
    0
}

/// Check if the Intel GPU device is set up.
#[no_mangle]
pub extern "C" fn corevm_has_intel_gpu(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.intel_gpu_ptr.is_null() { 0 } else { 1 }
}

// ── VirtIO-Net FFI ──

/// Set up the VirtIO-Net device at PCI slot 00:08.0.
/// Must be called AFTER corevm_setup_standard_devices() and
/// AFTER corevm_setup_ahci() (which creates the PCI MMIO router).
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn corevm_setup_virtio_net(handle: u64, mac: *const u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    if mac.is_null() { return -1; }
    let m: [u8; 6] = unsafe { [*mac, *mac.add(1), *mac.add(2), *mac.add(3), *mac.add(4), *mac.add(5)] };

    vm.setup_virtio_net(m);

    // Update the PCI MMIO router if it exists.
    if !vm.pci_mmio_router_ptr.is_null() && !vm.virtio_net_ptr.is_null() {
        let router = unsafe { &mut *vm.pci_mmio_router_ptr };
        router.virtio_net = vm.virtio_net_ptr;
    }

    0
}

/// Process pending VirtIO-Net RX delivery (host → guest).
/// Call periodically from the VM run loop alongside corevm_net_poll.
/// Returns 1 if IRQ pending, 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_virtio_net_process_rx(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let net = match vm.virtio_net() { Some(n) => n, None => return 0 };
    net.process_rx();
    if net.irq_pending {
        net.irq_pending = false;
        1
    } else {
        0
    }
}

/// Check if the VirtIO-Net device is set up and active.
/// Returns 1 if active, 0 otherwise.
#[no_mangle]
pub extern "C" fn corevm_has_virtio_net(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.virtio_net_ptr.is_null() { 0 } else { 1 }
}

// ── VirtIO Input FFI ──

/// Set up VirtIO Input devices (keyboard + tablet).
/// Creates two PCI devices at slots 00:09.0 and 00:0A.0.
#[no_mangle]
pub extern "C" fn corevm_setup_virtio_input(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    vm.setup_virtio_input();

    // Update PCI MMIO router with both input devices.
    if !vm.pci_mmio_router_ptr.is_null() {
        let router = unsafe { &mut *vm.pci_mmio_router_ptr };
        router.virtio_kbd = vm.virtio_kbd_ptr;
        router.virtio_tablet = vm.virtio_tablet_ptr;
    }

    // Register notify callbacks for keyboard and tablet: pulse IRQ 10 and kick vCPU 0.
    #[cfg(feature = "linux")]
    {
        let backend_ptr = &mut vm.backend as *mut crate::backend::kvm::KvmBackend;
        let vm_slot = vm.backend.vm_slot;

        if !vm.virtio_kbd_ptr.is_null() {
            let kbd = unsafe { &mut *vm.virtio_kbd_ptr };
            let bp = backend_ptr as usize;
            let vs = vm_slot;
            kbd.notify_callback = Some(alloc::boxed::Box::new(move || {
                let backend = unsafe { &mut *(bp as *mut crate::backend::kvm::KvmBackend) };
                let _ = backend.set_irq_line(10, true);
                let _ = backend.set_irq_line(10, false);
                crate::backend::kvm::cancel_vcpu_kvm(vs, 0);
            }));
        }
        if !vm.virtio_tablet_ptr.is_null() {
            let tablet = unsafe { &mut *vm.virtio_tablet_ptr };
            let bp = backend_ptr as usize;
            let vs = vm_slot;
            tablet.notify_callback = Some(alloc::boxed::Box::new(move || {
                let backend = unsafe { &mut *(bp as *mut crate::backend::kvm::KvmBackend) };
                let _ = backend.set_irq_line(10, true);
                let _ = backend.set_irq_line(10, false);
                crate::backend::kvm::cancel_vcpu_kvm(vs, 0);
            }));
        }
    }

    0
}

/// Inject a key event into the VirtIO keyboard.
/// `key`: Linux KEY_* code. `pressed`: 1=press, 0=release.
#[no_mangle]
pub extern "C" fn corevm_virtio_kbd_key(handle: u64, key: u16, pressed: i32) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let kbd = match vm.virtio_kbd() { Some(k) => k, None => return -1 };
    kbd.inject_key(key, pressed != 0);
    kbd.process_eventq();
    0
}

/// Inject a PS/2 scancode into the VirtIO keyboard (auto-converts to Linux KEY_*).
/// `scancode`: PS/2 Set 1 scancode (bit 7 = break).
#[no_mangle]
pub extern "C" fn corevm_virtio_kbd_ps2(handle: u64, scancode: u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let kbd = match vm.virtio_kbd() { Some(k) => k, None => return -1 };
    let pressed = scancode & 0x80 == 0;
    let raw_sc = scancode & 0x7F;
    match crate::devices::virtio_input::ps2_to_linux_key(raw_sc) {
        Some(key) => {
            #[cfg(feature = "std")]
            {
                static mut PS2_LOG: u32 = 0;
                unsafe { PS2_LOG += 1; if PS2_LOG <= 20 {
                    eprintln!("[virtio-kbd-ps2] sc=0x{:02X} → key={} pressed={}", scancode, key, pressed);
                }}
            }
            kbd.inject_key(key, pressed);
            kbd.process_eventq();
            // notify_callback handles IRQ pulse + vCPU kick automatically
        }
        None => {
            #[cfg(feature = "std")]
            {
                static mut PS2_MISS: u32 = 0;
                unsafe { PS2_MISS += 1; if PS2_MISS <= 10 {
                    eprintln!("[virtio-kbd-ps2] sc=0x{:02X} → NO MAPPING (raw=0x{:02X})", scancode, raw_sc);
                }}
            }
        }
    }
    0
}

/// Inject an absolute tablet position into the VirtIO tablet.
/// `x`, `y`: position in range 0..32767. `buttons`: bitmask (bit0=left, bit1=right, bit2=middle).
#[no_mangle]
pub extern "C" fn corevm_virtio_tablet_move(handle: u64, x: u32, y: u32, buttons: u8) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return -1 };
    let tablet = match vm.virtio_tablet() { Some(t) => t, None => return -1 };
    tablet.inject_abs_tablet(x, y, buttons);
    tablet.process_eventq();
    // notify_callback handles IRQ pulse + vCPU kick automatically
    0
}

/// Process VirtIO Input event delivery for both keyboard and tablet.
/// Call periodically from the VM loop. Returns 1 if any IRQ pending.
#[no_mangle]
pub extern "C" fn corevm_virtio_input_process(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    let mut irq = false;
    if let Some(kbd) = vm.virtio_kbd() {
        kbd.process_eventq();
        if kbd.irq_pending { kbd.irq_pending = false; irq = true; }
    }
    if let Some(tablet) = vm.virtio_tablet() {
        tablet.process_eventq();
        if tablet.irq_pending { tablet.irq_pending = false; irq = true; }
    }
    if irq { 1 } else { 0 }
}

/// Check if VirtIO Input devices are set up.
#[no_mangle]
pub extern "C" fn corevm_has_virtio_input(handle: u64) -> i32 {
    let vm = match get_vm(handle) { Some(v) => v, None => return 0 };
    if vm.virtio_kbd_ptr.is_null() { 0 } else { 1 }
}

/// Set I/O activity callbacks on AHCI, E1000, and VirtioNet devices.
/// `disk_cb(ctx, port_index)` is called on AHCI disk/cdrom I/O.
/// `net_cb(ctx)` is called on network TX/RX.
#[cfg(feature = "std")]
pub fn corevm_set_io_activity_callbacks(
    handle: u64,
    disk_cb: Option<fn(*mut (), u8)>,
    disk_ctx: *mut (),
    net_cb: Option<fn(*mut ())>,
    net_ctx: *mut (),
) {
    let vm = match get_vm(handle) { Some(v) => v, None => return };

    if !vm.ahci_ptr.is_null() {
        let ahci = unsafe { &mut *vm.ahci_ptr };
        ahci.io_activity_cb = disk_cb;
        ahci.io_activity_ctx = disk_ctx;
    }

    #[cfg(feature = "std")]
    if let Some(e1000_arc) = vm.e1000.as_ref() {
        let mut e1000 = e1000_arc.lock().unwrap();
        e1000.io_activity_cb = net_cb;
        e1000.io_activity_ctx = net_ctx;
    }
    #[cfg(not(feature = "std"))]
    if !vm.e1000_ptr.is_null() {
        let e1000 = unsafe { &mut *vm.e1000_ptr };
        e1000.io_activity_cb = net_cb;
        e1000.io_activity_ctx = net_ctx;
    }

    if !vm.virtio_net_ptr.is_null() {
        let vnet = unsafe { &mut *vm.virtio_net_ptr };
        vnet.io_activity_cb = net_cb;
        vnet.io_activity_ctx = net_ctx;
    }
}
