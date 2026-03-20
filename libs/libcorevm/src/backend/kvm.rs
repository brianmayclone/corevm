//! Linux KVM backend for hardware-accelerated virtualization.

use super::{CpuidEntry, DescriptorTable, SegmentReg, VcpuRegs, VcpuSregs, VmBackend, VmError, VmExitReason};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI32, AtomicU64, AtomicU32, Ordering};

// --- Per-VM thread-safe cancel support ---
// Supports up to MAX_VMS concurrent VMs, each with up to 4 vCPUs.
// Indexed as [vm_slot][vcpu_id].
const MAX_VMS: usize = 8;
const MAX_VCPUS: usize = 4;

/// Per-VM slot data for cancel and PIT support.
struct VmSlotData {
    cancel_kvm_run: [AtomicU64; MAX_VCPUS],
    vcpu_tid: [AtomicI32; MAX_VCPUS],
    vm_fd: AtomicI32,
}

impl VmSlotData {
    const fn new() -> Self {
        Self {
            cancel_kvm_run: [
                AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0),
            ],
            vcpu_tid: [
                AtomicI32::new(0), AtomicI32::new(0),
                AtomicI32::new(0), AtomicI32::new(0),
            ],
            vm_fd: AtomicI32::new(0),
        }
    }

    fn clear(&self) {
        self.vm_fd.store(0, Ordering::Relaxed);
        for t in &self.vcpu_tid { t.store(0, Ordering::Relaxed); }
        for c in &self.cancel_kvm_run { c.store(0, Ordering::Relaxed); }
    }
}

static VM_SLOTS: [VmSlotData; MAX_VMS] = [
    VmSlotData::new(), VmSlotData::new(), VmSlotData::new(), VmSlotData::new(),
    VmSlotData::new(), VmSlotData::new(), VmSlotData::new(), VmSlotData::new(),
];

/// Allocates a VM slot index. Each KvmBackend gets a unique slot so
/// multiple VMs don't interfere with each other's cancel/PIT state.
static NEXT_VM_SLOT: AtomicU32 = AtomicU32::new(0);

fn alloc_vm_slot() -> usize {
    loop {
        let cur = NEXT_VM_SLOT.load(Ordering::Relaxed);
        let slot = cur as usize % MAX_VMS;
        if NEXT_VM_SLOT.compare_exchange(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            return slot;
        }
    }
}

fn get_vm_slot(slot: usize) -> &'static VmSlotData {
    &VM_SLOTS[slot % MAX_VMS]
}

// Thread-local storage for the current VM slot, used by PIT channel 2
// callbacks which don't receive a handle parameter.
// Set by run_vcpu() before entering KVM_RUN.
static CURRENT_VM_SLOT: AtomicU32 = AtomicU32::new(0);

/// Sync PIT channel 2 gate to in-kernel PIT.
/// Called from Port61 write handler via gate_sync callback.
/// Uses the current VM slot (set by run_vcpu) to find the correct VM fd.
pub fn kvm_sync_pit_ch2_gate(gate: bool) {
    let slot = CURRENT_VM_SLOT.load(Ordering::Relaxed) as usize;
    let vm_fd = get_vm_slot(slot).vm_fd.load(Ordering::Relaxed);
    if vm_fd <= 0 { return; }
    unsafe {
        let mut state = KvmPitState2::default();
        let ret = sys_ioctl(vm_fd, KVM_GET_PIT2, &mut state as *mut _ as u64);
        if ret < 0 { return; }
        state.channels[2].gate = if gate { 1 } else { 0 };
        sys_ioctl(vm_fd, KVM_SET_PIT2, &state as *const _ as u64);
    }
}

/// Read PIT channel 2 output from the in-kernel KVM PIT.
/// Called from Port61 read handler to return bit 5 (channel 2 OUT).
///
/// KVM_GET_PIT2 doesn't expose the live output state directly.
/// We must compute it from the channel state (mode, count, gate, count_load_time).
pub fn kvm_pit_ch2_output() -> bool {
    let slot = CURRENT_VM_SLOT.load(Ordering::Relaxed) as usize;
    let vm_fd = get_vm_slot(slot).vm_fd.load(Ordering::Relaxed);
    if vm_fd <= 0 { return false; }
    unsafe {
        let mut state = KvmPitState2::default();
        let ret = sys_ioctl(vm_fd, KVM_GET_PIT2, &mut state as *mut _ as u64);
        if ret < 0 { return false; }

        let ch = &state.channels[2];
        if ch.gate == 0 { return false; } // gate must be high for counting

        // Get current time (CLOCK_MONOTONIC nanoseconds, same as ktime_t)
        let now_ns = clock_gettime_mono_ns();
        let load_ns = ch.count_load_time;
        if load_ns == 0 { return false; } // not yet programmed

        let elapsed_ns = if now_ns > load_ns { (now_ns - load_ns) as u64 } else { 0 };
        // PIT frequency: 1193182 Hz → one tick every ~838.1 ns
        let elapsed_ticks = elapsed_ns * 1193182 / 1_000_000_000;

        let count = if ch.count == 0 { 0x10000u32 } else { ch.count }; // 0 means 65536

        pit_output_for_mode(ch.mode, count, elapsed_ticks)
    }
}

/// Compute PIT output for a given mode based on elapsed ticks since count load.
fn pit_output_for_mode(mode: u8, count: u32, elapsed_ticks: u64) -> bool {
    match mode {
        0 => {
            // Mode 0: Interrupt on terminal count.
            // Output starts LOW, goes HIGH when count reaches 0.
            elapsed_ticks >= count as u64
        }
        1 => {
            // Mode 1: Hardware-retriggerable one-shot.
            // Output goes LOW on gate trigger, HIGH when count reaches 0.
            elapsed_ticks >= count as u64
        }
        2 | 6 => {
            // Mode 2: Rate generator.
            // Output is HIGH, goes LOW for one tick when count reaches 1, then reloads.
            let pos = elapsed_ticks % count as u64;
            pos != (count as u64 - 1)
        }
        3 | 7 => {
            // Mode 3: Square wave.
            // Output toggles every count/2 ticks.
            let half = count as u64 / 2;
            if half == 0 { return true; }
            let pos = elapsed_ticks % count as u64;
            pos < half
        }
        4 => {
            // Mode 4: Software-triggered strobe.
            // Output HIGH, goes LOW for one tick at terminal count.
            elapsed_ticks != count as u64
        }
        5 => {
            // Mode 5: Hardware-triggered strobe.
            // Same as mode 4 but gate-triggered.
            elapsed_ticks != count as u64
        }
        _ => false,
    }
}

/// Get current CLOCK_MONOTONIC time in nanoseconds via clock_gettime syscall.
unsafe fn clock_gettime_mono_ns() -> i64 {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 228_u64, // __NR_clock_gettime
        in("rdi") 1_u64,   // CLOCK_MONOTONIC
        in("rsi") &mut ts as *mut Timespec as u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    if ret < 0 { return 0; }
    ts.tv_sec * 1_000_000_000 + ts.tv_nsec
}

/// Cancel a running KVM_RUN from any thread.
/// Sets immediate_exit = 1 AND sends SIGUSR1 to the vCPU thread.
/// immediate_exit alone only works at KVM_RUN entry; the signal is needed
/// to kick the vCPU out of an already-running KVM_RUN.
///
/// `vm_slot` identifies which VM instance to cancel (from KvmBackend::vm_slot).
pub fn cancel_vcpu_kvm(vm_slot: usize, vcpu_id: u32) -> i32 {
    let slot = get_vm_slot(vm_slot);
    if vcpu_id as usize >= MAX_VCPUS { return -1; }
    let ptr = slot.cancel_kvm_run[vcpu_id as usize].load(Ordering::Relaxed);
    if ptr == 0 { return -1; }
    unsafe {
        let kvm_run = ptr as *mut KvmRun;
        (*kvm_run).immediate_exit = 1;
    }
    // Send SIGUSR1 to interrupt a blocked KVM_RUN ioctl.
    // The signal handler is a no-op; the ioctl returns -EINTR.
    let tid = slot.vcpu_tid[vcpu_id as usize].load(Ordering::Relaxed);
    if tid > 0 {
        unsafe { sys_tgkill(sys_getpid(), tid, 10); } // 10 = SIGUSR1
    }
    0
}

// ── KVM ioctl numbers (x86_64 Linux) ──────────────────────────────────────

const KVM_GET_API_VERSION: u64 = 0xAE00;
const KVM_CREATE_VM: u64 = 0xAE01;
const KVM_GET_VCPU_MMAP_SIZE: u64 = 0xAE04;
const KVM_CREATE_VCPU: u64 = 0xAE41;
const KVM_SET_USER_MEMORY_REGION: u64 = 0x4020_AE46;
const KVM_RUN: u64 = 0xAE80;
const KVM_GET_REGS: u64 = 0x8090_AE81;
const KVM_SET_REGS: u64 = 0x4090_AE82;
const KVM_GET_SREGS: u64 = 0x8138_AE83;
const KVM_SET_SREGS: u64 = 0x4138_AE84;
const KVM_INTERRUPT: u64 = 0x4004_AE86;
const KVM_SET_CPUID2: u64 = 0x4008_AE90;
const KVM_GET_CPUID2: u64 = 0xC008_AE91;
const KVM_GET_VCPU_EVENTS: u64 = 0x8040_AE9F;
const KVM_SET_VCPU_EVENTS: u64 = 0x4040_AEA0;
const KVM_CREATE_IRQCHIP: u64 = 0xAE60;
const KVM_IRQ_LINE: u64 = 0x4008_AE61;
const KVM_ENABLE_CAP: u64 = 0x4068_AEA3;
const KVM_SIGNAL_MSI: u64 = 0x4020_AEA5;
const KVM_IRQFD: u64 = 0x4020_AE76;
const KVM_SET_GSI_ROUTING: u64 = 0x4008_AE6A;
const KVM_CREATE_PIT2: u64 = 0x4040_AE77;
const KVM_GET_LAPIC: u64 = 0x8400_AE8E;
const KVM_SET_LAPIC: u64 = 0x4400_AE8F;
const KVM_GET_IRQCHIP: u64 = 0xC208_AE62;
const KVM_GET_SUPPORTED_CPUID: u64 = 0xC008_AE05;
const KVM_SET_MP_STATE: u64 = 0x4004_AE99;
const KVM_SET_MSRS: u64 = 0x4008_AE89;
const KVM_SET_TSS_ADDR: u64 = 0xAE47;
const KVM_SET_IDENTITY_MAP_ADDR: u64 = 0x4008_AE48;
const KVM_GET_TSC_KHZ: u64 = 0xAEA3;
const KVM_SET_TSC_KHZ: u64 = 0xAEA2;
const KVM_CHECK_EXTENSION: u64 = 0xAE03;
const KVM_GET_PIT2: u64 = 0x8070_AE9F;
const KVM_SET_PIT2: u64 = 0x4070_AEA0;

// Exit reasons
const KVM_EXIT_IO: u32 = 2;
const KVM_EXIT_DEBUG: u32 = 4;
const KVM_EXIT_HLT: u32 = 5;
const KVM_EXIT_MMIO: u32 = 6;
const KVM_EXIT_IRQ_WINDOW_OPEN: u32 = 7;
const KVM_EXIT_SHUTDOWN: u32 = 8;
const KVM_EXIT_INTERNAL_ERROR: u32 = 17;

const KVM_EXIT_IO_IN: u8 = 0;
const KVM_EXIT_IO_OUT: u8 = 1;

// mmap constants
const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_SHARED: i32 = 1;
const O_RDWR: i32 = 2;

// ── Raw syscall helpers ───────────────────────────────────────────────────

unsafe fn sys_ioctl(fd: i32, request: u64, arg: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 16_u64,
        in("rdi") fd as u64,
        in("rsi") request,
        in("rdx") arg,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

unsafe fn sys_mmap(addr: u64, len: u64, prot: i32, flags: i32, fd: i32, offset: i64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 9_u64,
        in("rdi") addr,
        in("rsi") len,
        in("rdx") prot as u64,
        in("r10") flags as u64,
        in("r8") fd as u64,
        in("r9") offset as u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

unsafe fn sys_munmap(addr: u64, len: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 11_u64,
        in("rdi") addr,
        in("rsi") len,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

unsafe fn sys_getpid() -> i32 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 39_u64, // getpid
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret as i32
}

unsafe fn sys_gettid() -> i32 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 186_u64, // gettid
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret as i32
}

unsafe fn sys_tgkill(tgid: i32, tid: i32, sig: i32) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 234_u64, // tgkill
        in("rdi") tgid as u64,
        in("rsi") tid as u64,
        in("rdx") sig as u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

/// rt_sigaction syscall for installing signal handlers.
unsafe fn sys_rt_sigaction(sig: i32, act: *const Sigaction, oldact: *mut Sigaction) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 13_u64, // rt_sigaction
        in("rdi") sig as u64,
        in("rsi") act as u64,
        in("rdx") oldact as u64,
        in("r10") 8_u64, // sizeof(sigset_t) = 8 on x86_64
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

/// Minimal sigaction struct for rt_sigaction syscall on x86_64 Linux.
#[repr(C)]
struct Sigaction {
    sa_handler: u64,    // function pointer or SIG_DFL/SIG_IGN
    sa_flags: u64,
    sa_restorer: u64,
    sa_mask: u64,       // sigset_t (8 bytes on x86_64)
}

/// Empty SIGUSR1 handler — just needs to exist so the signal interrupts KVM_RUN.
unsafe extern "C" fn sigusr1_handler(_sig: i32) {}

/// Install SIGUSR1 handler. Must be called once before using signal-based cancel.
pub fn install_sigusr1_handler() {
    extern "C" {
        fn kvm_sigreturn_trampoline();
    }
    unsafe {
        let act = Sigaction {
            sa_handler: sigusr1_handler as u64,
            sa_flags: 0x04000000, // SA_RESTORER — required on x86_64
            sa_restorer: kvm_sigreturn_trampoline as u64,
            sa_mask: 0,
        };
        sys_rt_sigaction(10, &act, core::ptr::null_mut()); // 10 = SIGUSR1
    }
}

// Sigreturn trampoline for SA_RESTORER (required on x86_64 Linux).
core::arch::global_asm!(
    ".global kvm_sigreturn_trampoline",
    "kvm_sigreturn_trampoline:",
    "mov rax, 15", // __NR_rt_sigreturn
    "syscall",
);

unsafe fn sys_open(path: *const u8, flags: i32) -> i32 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 2_u64,
        in("rdi") path as u64,
        in("rsi") flags as u64,
        in("rdx") 0_u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret as i32
}

unsafe fn sys_close(fd: i32) -> i32 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 3_u64,
        in("rdi") fd as u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret as i32
}

unsafe fn sys_write(fd: i32, buf: *const u8, len: usize) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 1_u64, // SYS_write
        in("rdi") fd as u64,
        in("rsi") buf as u64,
        in("rdx") len as u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret
}

unsafe fn sys_eventfd(initval: u32, flags: i32) -> i32 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 290_u64, // SYS_eventfd2
        in("rdi") initval as u64,
        in("rsi") flags as u64,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
        options(nostack)
    );
    ret as i32
}

// ── KVM structs (repr C, matching Linux headers) ──────────────────────────

#[repr(C)]
struct KvmUserspaceMemoryRegion {
    slot: u32,
    flags: u32,
    guest_phys_addr: u64,
    memory_size: u64,
    userspace_addr: u64,
}

/// kvm_regs — note KVM field order: rax rbx rcx rdx rsi rdi rsp rbp r8-r15 rip rflags
#[repr(C)]
#[derive(Default)]
struct KvmRegs {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rsp: u64,
    rbp: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
    rflags: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct KvmSegment {
    base: u64,
    limit: u32,
    selector: u16,
    type_: u8,
    present: u8,
    dpl: u8,
    db: u8,
    s: u8,
    l: u8,
    g: u8,
    avl: u8,
    unusable: u8,
    _padding: u8,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct KvmDtable {
    base: u64,
    limit: u16,
    _padding: [u16; 3],
}

#[repr(C)]
#[derive(Default)]
struct KvmSregs {
    cs: KvmSegment,
    ds: KvmSegment,
    es: KvmSegment,
    fs: KvmSegment,
    gs: KvmSegment,
    ss: KvmSegment,
    tr: KvmSegment,
    ldt: KvmSegment,
    gdt: KvmDtable,
    idt: KvmDtable,
    cr0: u64,
    cr2: u64,
    cr3: u64,
    cr4: u64,
    cr8: u64,
    efer: u64,
    apic_base: u64,
    interrupt_bitmap: [u64; 4], // 256 bits
}

#[repr(C)]
struct KvmVcpuEvents {
    exception: KvmVcpuEventException,
    interrupt: KvmVcpuEventInterrupt,
    nmi: KvmVcpuEventNmi,
    sipi_vector: u32,
    flags: u32,
    smi: KvmVcpuEventSmi,
    _reserved: [u8; 27],
    exception_has_payload: u8,
    exception_payload: u64,
}

#[repr(C)]
struct KvmVcpuEventException {
    injected: u8,
    nr: u8,
    has_error_code: u8,
    pending: u8,
    error_code: u32,
}

#[repr(C)]
struct KvmVcpuEventInterrupt {
    injected: u8,
    nr: u8,
    soft: u8,
    shadow: u8,
}

#[repr(C)]
struct KvmVcpuEventNmi {
    injected: u8,
    pending: u8,
    masked: u8,
    _pad: u8,
}

#[repr(C)]
struct KvmVcpuEventSmi {
    smm: u8,
    pending: u8,
    smm_inside_nmi: u8,
    latched_init: u8,
}

/// kvm_run shared page — offsets for exit data sub-structs.
#[repr(C)]
struct KvmRun {
    request_interrupt_window: u8,
    immediate_exit: u8,
    _padding1: [u8; 6],
    exit_reason: u32,
    ready_for_interrupt_injection: u8,
    if_flag: u8,
    flags: u16,
    cr8: u64,
    apic_base: u64,
    // offset 32: union of exit info — we access via raw pointer offsets
    exit_data: [u8; 256],
}

/// IO exit sub-struct at kvm_run offset 32
#[repr(C)]
struct KvmRunExitIo {
    direction: u8,
    size: u8,
    port: u16,
    count: u32,
    data_offset: u64,
}

/// MMIO exit sub-struct at kvm_run offset 32
#[repr(C)]
struct KvmRunExitMmio {
    phys_addr: u64,
    data: [u8; 8],
    len: u32,
    is_write: u8,
}

#[repr(C)]
struct KvmCpuidEntry2 {
    function: u32,
    index: u32,
    flags: u32,
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
    _padding: [u32; 3],
}

/// kvm_pit_config for KVM_CREATE_PIT2
#[repr(C)]
struct KvmPitConfig {
    flags: u32,
    _pad: [u32; 15],
}

/// kvm_pit_channel_state — per-channel state in kvm_pit_state2
#[repr(C)]
#[derive(Default)]
struct KvmPitChannelState {
    count: u32,
    latched_count: u16,
    count_latched: u8,
    status_latched: u8,
    status: u8,
    read_state: u8,
    write_state: u8,
    write_latch: u8,
    rw_mode: u8,
    mode: u8,
    bcd: u8,
    gate: u8,
    count_load_time: i64,
}

/// kvm_pit_state2 for KVM_GET_PIT2
#[repr(C)]
#[derive(Default)]
struct KvmPitState2 {
    channels: [KvmPitChannelState; 3],
    flags: u32,
    _reserved: [u32; 9],
}

/// kvm_enable_cap for KVM_ENABLE_CAP
#[repr(C)]
struct KvmEnableCap {
    cap: u32,
    flags: u32,
    args: [u64; 4],
    _pad: [u8; 64],
}

/// kvm_irq_level for KVM_IRQ_LINE
#[repr(C)]
struct KvmIrqLevel {
    /// IRQ number (bits 31:0) — for irqchip: GSI number.
    irq: u32,
    /// 1 = assert, 0 = deassert.
    level: u32,
}

/// kvm_msi for KVM_SIGNAL_MSI
#[repr(C)]
struct KvmMsi {
    address_lo: u32,
    address_hi: u32,
    data: u32,
    flags: u32,
    devid: u32,
    _pad: [u8; 12],
}

/// kvm_irqfd for KVM_IRQFD
#[repr(C)]
struct KvmIrqfd {
    fd: u32,
    gsi: u32,
    flags: u32,
    resamplefd: u32,
    _pad: [u8; 16],
}

/// kvm_irq_routing for KVM_SET_GSI_ROUTING
#[repr(C)]
struct KvmIrqRouting {
    nr: u32,
    flags: u32,
    // entries follow (variable length)
}

/// kvm_irq_routing_entry
#[repr(C)]
#[derive(Clone, Copy)]
struct KvmIrqRoutingEntry {
    gsi: u32,
    entry_type: u32,
    flags: u32,
    pad: u32,
    // union: MSI fields
    address_lo: u32,
    address_hi: u32,
    data: u32,
    /// devid for KVM_MSI_VALID_DEVID
    union_pad: [u32; 5],
}

const KVM_IRQ_ROUTING_MSI: u32 = 1;
const KVM_IRQ_ROUTING_IRQCHIP: u32 = 0;

// ── Memory region tracking for read_phys / write_phys ─────────────────────

struct MemorySlot {
    guest_phys: u64,
    size: u64,
    host_ptr: *mut u8,
}

// ── vCPU ──────────────────────────────────────────────────────────────────

struct KvmVcpu {
    fd: i32,
    kvm_run: *mut KvmRun,
    mmap_size: usize,
}

impl Drop for KvmVcpu {
    fn drop(&mut self) {
        unsafe {
            if !self.kvm_run.is_null() {
                sys_munmap(self.kvm_run as u64, self.mmap_size as u64);
            }
            sys_close(self.fd);
        }
    }
}

// ── KvmBackend ────────────────────────────────────────────────────────────

pub struct KvmBackend {
    kvm_fd: i32,
    vm_fd: i32,
    vcpus: Vec<Option<KvmVcpu>>,
    mmap_size: usize,
    memory_slots: Vec<MemorySlot>,
    stored_cpuid: Option<Vec<CpuidEntry>>,
    /// Number of configured CPU cores (for CPUID topology).
    pub cpu_count: u32,
    /// Slot index into VM_SLOTS for per-VM cancel/PIT state.
    pub vm_slot: usize,
    /// Fixed TSC frequency in kHz for all vCPUs (0 = not set).
    tsc_khz: u32,
    /// AHCI MSI irqfd — eventfd used for kernel-level MSI delivery.
    /// When set, writing 1 to this fd triggers an MSI via KVM_IRQFD.
    /// The GSI routing entry maps this to the correct MSI address/data.
    pub ahci_msi_fd: i32,
    /// GSI number allocated for AHCI MSI routing.
    ahci_msi_gsi: u32,
}

impl KvmBackend {
    pub fn new() -> Result<Self, VmError> {
        unsafe {
            let kvm_fd = sys_open(b"/dev/kvm\0".as_ptr(), O_RDWR);
            if kvm_fd < 0 {
                return Err(VmError::NoHardwareSupport);
            }

            let api_ver = sys_ioctl(kvm_fd, KVM_GET_API_VERSION, 0);
            if api_ver != 12 {
                sys_close(kvm_fd);
                return Err(VmError::NoHardwareSupport);
            }

            let mmap_size = sys_ioctl(kvm_fd, KVM_GET_VCPU_MMAP_SIZE, 0);
            if mmap_size <= 0 {
                sys_close(kvm_fd);
                return Err(VmError::BackendError(mmap_size as i32));
            }

            let vm_fd = sys_ioctl(kvm_fd, KVM_CREATE_VM, 0) as i32;
            if vm_fd < 0 {
                sys_close(kvm_fd);
                return Err(VmError::BackendError(vm_fd));
            }

            // Set TSS address (required by KVM on Intel before creating vCPUs).
            // Place at 0xFFFBD000 (same as QEMU) — just below 4GB, outside normal RAM.
            let ret = sys_ioctl(vm_fd, KVM_SET_TSS_ADDR, 0xFEFF_D000u64);
            if ret < 0 {
                sys_close(vm_fd);
                sys_close(kvm_fd);
                return Err(VmError::BackendError(ret as i32));
            }

            // Set identity map address (required by KVM for real-mode emulation).
            // Place at 0xFFFBC000 (page below TSS).
            let identity_addr: u64 = 0xFEFF_C000;
            let ret = sys_ioctl(vm_fd, KVM_SET_IDENTITY_MAP_ADDR, &identity_addr as *const _ as u64);
            if ret < 0 {
                sys_close(vm_fd);
                sys_close(kvm_fd);
                return Err(VmError::BackendError(ret as i32));
            }

            // Create in-kernel irqchip (PIC, IOAPIC, LAPIC).
            let ret = sys_ioctl(vm_fd, KVM_CREATE_IRQCHIP, 0);
            if ret < 0 {
                sys_close(vm_fd);
                sys_close(kvm_fd);
                return Err(VmError::BackendError(ret as i32));
            }

            // Enable PIT reinjection control (KVM_CAP_REINJECT_CONTROL = 41).
            // Required for correct PIT timer behavior — without this,
            // Windows 10 bootmgr hangs waiting for timer interrupts.
            {
                let mut cap = KvmEnableCap { cap: 41, flags: 0, args: [0; 4], _pad: [0; 64] };
                let _ = sys_ioctl(vm_fd, KVM_ENABLE_CAP, &cap as *const _ as u64);
            }

            // Create in-kernel PIT (i8254 timer).
            let pit_config = KvmPitConfig {
                flags: 1, // KVM_PIT_SPEAKER_DUMMY = 1 (match QEMU)
                _pad: [0; 15],
            };
            let ret = sys_ioctl(
                vm_fd,
                KVM_CREATE_PIT2,
                &pit_config as *const _ as u64,
            );
            if ret < 0 {
                sys_close(vm_fd);
                sys_close(kvm_fd);
                return Err(VmError::BackendError(ret as i32));
            }

            let slot = alloc_vm_slot();
            // Query host TSC frequency via KVM_GET_TSC_KHZ (vCPU-level ioctl
            // requires a vCPU — use KVM_CHECK_EXTENSION on the VM fd instead).
            // KVM_CAP_GET_TSC_KHZ = 61
            let has_tsc_khz = sys_ioctl(vm_fd, KVM_CHECK_EXTENSION, 61) > 0;

            let mut backend = Self {
                kvm_fd,
                vm_fd,
                vcpus: Vec::new(),
                mmap_size: mmap_size as usize,
                memory_slots: Vec::new(),
                stored_cpuid: None,
                cpu_count: 1,
                vm_slot: slot,
                tsc_khz: 0,
                ahci_msi_fd: -1,
                ahci_msi_gsi: 0,
            };

            // Store VM fd in per-VM slot for PIT channel 2 gate sync
            let slot_data = get_vm_slot(slot);
            slot_data.clear(); // Ensure clean state from any previous VM in this slot
            slot_data.vm_fd.store(vm_fd, Ordering::Relaxed);

            // Auto-load host CPUID so vCPUs get proper feature flags (APIC, etc.)
            let _ = backend.load_host_cpuid();

            Ok(backend)
        }
    }

    /// Fetch host-supported CPUID entries via KVM_GET_SUPPORTED_CPUID and store them.
    /// Must be called before creating vCPUs.
    pub fn load_host_cpuid(&mut self) -> Result<(), VmError> {
        unsafe {
            // Allocate buffer for up to 256 CPUID entries
            const MAX_ENTRIES: usize = 256;
            let buf_size = 8 + MAX_ENTRIES * core::mem::size_of::<KvmCpuidEntry2>();
            let mut buf = alloc::vec![0u8; buf_size];
            // nent = MAX_ENTRIES at offset 0 (u32)
            let nent_ptr = buf.as_mut_ptr() as *mut u32;
            *nent_ptr = MAX_ENTRIES as u32;

            let ret = sys_ioctl(self.kvm_fd, KVM_GET_SUPPORTED_CPUID, buf.as_mut_ptr() as u64);
            if ret < 0 {
                return Err(VmError::BackendError(ret as i32));
            }

            let nent = *nent_ptr as usize;
            let entries_ptr = buf.as_ptr().add(8) as *const KvmCpuidEntry2;

            // Detect AMD vs Intel from CPUID leaf 0 vendor string.
            // AMD: "AuthenticAMD" (EBX=0x68747541), Intel: "GenuineIntel"
            let mut is_amd = false;
            for i in 0..nent {
                let e = &*entries_ptr.add(i);
                if e.function == 0 {
                    is_amd = e.ebx == 0x6874_7541; // "Auth" in EBX
                    break;
                }
            }

            let max_cpus = self.cpu_count.max(1);
            let mut cpuid_entries = Vec::new();
            for i in 0..nent {
                let e = &*entries_ptr.add(i);
                let mut eax = e.eax;
                let mut ebx = e.ebx;
                let mut ecx = e.ecx;
                let mut edx = e.edx;

                // Filter CPUID: hide features not supported in our VM.
                if e.function == 1 {
                    ecx &= !(1 << 5);   // VMX — not useful inside guest
                    // x2APIC (bit 21): keep enabled — Q35/ICH9 chipset supports it.
                    // If using i440FX, the caller can re-filter via set_cpuid().
                    // Keep TSC-Deadline (bit 24) — Windows 10 needs it for
                    // the LAPIC timer with SMP. KVM handles TSC-Deadline
                    // in the in-kernel LAPIC automatically.
                    // Keep bit 31 (hypervisor present) — needed for KVM PV
                    // features including SMP wakeup used by SeaBIOS.
                    // EBX[31:24] and EBX[23:16] are set per-vCPU in create_vcpu
                    ebx &= 0x0000_FFFF;

                    // HTT (EDX bit 28): must be set when num_cpus > 1.
                    // Despite its name ("Hyper-Threading Technology"), this bit
                    // indicates that the package contains multiple logical
                    // processors. Windows HAL checks this to decide whether
                    // to start APs — without it, Windows stays uniprocessor.
                    if max_cpus > 1 {
                        edx |= 1 << 28;
                    }
                }

                // Leaf 4 (Deterministic Cache Parameters, Intel):
                // EAX[25:14] = max addressable IDs for cores in physical package
                // Must match VM core count so Windows Task Manager shows correct CPUs.
                // EAX[31:26] = max logical CPUs sharing this cache — set to max_cpus-1
                // (all cores share each cache level in our simple topology).
                if e.function == 4 {
                    eax = (eax & 0x03FF_FFFF) | (((max_cpus - 1) & 0x3F) << 26);
                    eax = (eax & 0xFC00_3FFF) | (((max_cpus - 1) & 0xFFF) << 14);
                }

                // Leaf 0x6 (Thermal/Power Management):
                // Clear hardware P-state/frequency features that don't apply
                // to a VM. Keep ARAT (bit 2) — Always Running APIC Timer.
                if e.function == 0x6 {
                    eax = eax & (1 << 2);
                    ebx = 0;
                    ecx = 0;
                }

                // Leaf 0x15 (Time Stamp Counter / Core Crystal Clock):
                // Clear to prevent guest deriving TSC from non-existent crystal.
                if e.function == 0x15 {
                    eax = 0;
                    ebx = 0;
                    ecx = 0;
                }

                // Leaf 0x16 (Processor Frequency Information):
                // Clear to prevent TSC vs P0-frequency mismatch warnings.
                if e.function == 0x16 {
                    eax = 0;
                    ebx = 0;
                    ecx = 0;
                }

                // Leaf 0xB (Extended Topology Enumeration).
                // Synthesize VM-specific topology instead of passing through
                // host values. Our topology: 1 thread per core, max_cpus cores.
                // EDX = x2APIC ID (set per-vCPU in create_vcpu).
                // Windows BSODs with MULTIPROCESSOR_CONFIGURATION_NOT_SUPPORTED
                // if leaf 0xB reports host topology that doesn't match VM vCPU count.
                if e.function == 0xB {
                    let subleaf = e.index;
                    if subleaf == 0 {
                        // SMT level: 1 thread per core
                        eax = 0; // bits to shift = 0 (no SMT subdivision)
                        ebx = 1; // 1 logical processor at this level
                        ecx = (1 << 8) | 0; // level type = SMT (1), level number = 0
                    } else if subleaf == 1 {
                        // Core level: max_cpus cores per package
                        let mut shift = 0u32;
                        while (1u32 << shift) < max_cpus { shift += 1; }
                        eax = shift;
                        ebx = max_cpus; // total logical processors
                        ecx = (2 << 8) | 1; // level type = Core (2), level number = 1
                    } else {
                        // Invalid level: terminate enumeration
                        eax = 0;
                        ebx = 0;
                        ecx = subleaf; // level type = 0 (invalid)
                    }
                    edx = 0; // fixed per-vCPU in create_vcpu
                }

                // Leaf 0x80000007 (Advanced Power Management):
                // Keep only Invariant TSC (EDX bit 8). Clear hardware P-state
                // bits that don't apply to a VM (CPB, EffFreqRO, etc.).
                if e.function == 0x80000007 {
                    eax = 0;
                    ebx = 0;
                    ecx = 0;
                    edx = edx & (1 << 8); // keep only InvariantTSC
                }

                // Leaf 0x80000008 (AMD Processor Capacity):
                // ECX[7:0] = NC = number of physical cores - 1
                // ECX[15:12] = ApicIdSize = bits needed for APIC IDs
                // Must reflect VM topology for Windows to show correct CPU count.
                // MSI delivery is now handled via KVM_IRQFD (kernel-level),
                // so CPUID topology changes no longer affect interrupt routing.
                if e.function == 0x8000_0008 {
                    let nc = max_cpus - 1;
                    let mut apic_id_size = 0u32;
                    while (1u32 << apic_id_size) <= nc { apic_id_size += 1; }
                    ebx = 0; // clear CPPC/RDPRU bits that don't work in a VM
                    ecx = (ecx & 0xFFFF_0000) | ((apic_id_size & 0xF) << 12) | (nc & 0xFF);
                }

                // AMD Extended APIC ID (leaf 0x8000001E).
                // EAX = Extended APIC ID (set per-vCPU in create_vcpu)
                // EBX[7:0] = Compute Unit ID, EBX[15:8] = threads per unit - 1
                if e.function == 0x8000_001E {
                    eax = 0; // set per-vCPU in create_vcpu
                    ebx = 0; // 1 thread per core (threads_per_unit - 1 = 0)
                }

                // Keep KVM hypervisor leaves (0x40000000+) — SeaBIOS uses
                // the KVM signature for PV SMP wakeup.
                // Disable kvmclock (leaf 0x40000001 EAX bit 3) to prevent
                // SeaBIOS from replacing the PIT timer with kvmclock,
                // which breaks the timer when APs are present.
                // Keep all KVM features including kvmclock — SeaBIOS needs
                // kvmclock for time measurement during SMP init.

                cpuid_entries.push(CpuidEntry {
                    function: e.function,
                    index: e.index,
                    flags: e.flags,
                    eax,
                    ebx,
                    ecx,
                    edx,
                });
            }

            // On AMD, KVM may not provide leaf 0xB at all. Synthesize it
            // so Windows can enumerate the topology. Without it, Windows
            // BSODs with MULTIPROCESSOR_CONFIGURATION_NOT_SUPPORTED.
            if is_amd && !cpuid_entries.iter().any(|e| e.function == 0xB) && max_cpus > 1 {
                let flags = 1u32; // KVM_CPUID_FLAG_SIGNIFCANT_INDEX
                let mut shift = 0u32;
                while (1u32 << shift) < max_cpus { shift += 1; }
                cpuid_entries.push(CpuidEntry {
                    function: 0xB, index: 0, flags,
                    eax: 0, ebx: 1, ecx: (1 << 8) | 0, edx: 0,
                });
                cpuid_entries.push(CpuidEntry {
                    function: 0xB, index: 1, flags,
                    eax: shift, ebx: max_cpus, ecx: (2 << 8) | 1, edx: 0,
                });
                cpuid_entries.push(CpuidEntry {
                    function: 0xB, index: 2, flags,
                    eax: 0, ebx: 0, ecx: 2, edx: 0,
                });
            }

            self.stored_cpuid = Some(cpuid_entries);
            Ok(())
        }
    }

    /// Query the in-kernel PIT channel 2 output state via KVM_GET_PIT2.
    /// Returns true if the channel 2 output pin is high.
    pub fn get_pit2_channel2_output(&self) -> bool {
        unsafe {
            let mut state = KvmPitState2::default();
            let ret = sys_ioctl(self.vm_fd, KVM_GET_PIT2, &mut state as *mut _ as u64);
            if ret < 0 { return false; }
            // Channel 2 output is encoded in the status byte, bit 7 = OUT pin
            (state.channels[2].status & 0x80) != 0
        }
    }

    /// Debug: return raw PIT channel 2 state for diagnostics.
    /// Returns (count, status, mode, gate, ret_code)
    pub fn get_pit2_debug(&self) -> (u32, u8, u8, u8, i64) {
        unsafe {
            let mut state = KvmPitState2::default();
            let ret = sys_ioctl(self.vm_fd, KVM_GET_PIT2, &mut state as *mut _ as u64);
            let ch = &state.channels[2];
            (ch.count, ch.status, ch.mode, ch.gate, ret)
        }
    }

    /// Set the in-kernel PIT channel 2 gate via KVM_SET_PIT2.
    /// Called when port 0x61 bit 0 changes.
    pub fn set_pit2_channel2_gate(&self, gate: bool) {
        unsafe {
            let mut state = KvmPitState2::default();
            let ret = sys_ioctl(self.vm_fd, KVM_GET_PIT2, &mut state as *mut _ as u64);
            if ret < 0 { return; }
            state.channels[2].gate = if gate { 1 } else { 0 };
            sys_ioctl(self.vm_fd, KVM_SET_PIT2, &state as *const _ as u64);
        }
    }

    fn get_vcpu(&self, id: u32) -> Result<&KvmVcpu, VmError> {
        self.vcpus
            .get(id as usize)
            .and_then(|v| v.as_ref())
            .ok_or(VmError::InvalidVcpuId)
    }

    fn get_vcpu_mut(&mut self, id: u32) -> Result<&mut KvmVcpu, VmError> {
        self.vcpus
            .get_mut(id as usize)
            .and_then(|v| v.as_mut())
            .ok_or(VmError::InvalidVcpuId)
    }

    /// Write I/O response data into the kvm_run shared page for an IoIn exit.
    /// Must be called before the next `run_vcpu`.
    pub fn set_io_response(&mut self, vcpu_id: u32, data: &[u8]) {
        if let Ok(vcpu) = self.get_vcpu(vcpu_id) {
            unsafe {
                let run = &*vcpu.kvm_run;
                let io = &*(run.exit_data.as_ptr() as *const KvmRunExitIo);
                let dst = (vcpu.kvm_run as *mut u8).add(io.data_offset as usize);
                let len = data.len().min(io.size as usize * io.count as usize);
                core::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
            }
        }
    }

    /// Write one iteration's data at a specific index in the kvm_run IO data buffer.
    /// For string I/O (REP INSB) where count > 1.
    pub fn set_io_response_at(&mut self, vcpu_id: u32, index: u32, data: &[u8]) {
        if let Ok(vcpu) = self.get_vcpu(vcpu_id) {
            unsafe {
                let run = &*vcpu.kvm_run;
                let io = &*(run.exit_data.as_ptr() as *const KvmRunExitIo);
                let base = (vcpu.kvm_run as *mut u8).add(io.data_offset as usize);
                let dst = base.add(index as usize * io.size as usize);
                let len = data.len().min(io.size as usize);
                core::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
            }
        }
    }

    /// Read one iteration's data from the kvm_run IO data buffer for string OUT.
    pub fn get_io_data_at(&self, vcpu_id: u32, index: u32) -> u32 {
        if let Ok(vcpu) = self.get_vcpu(vcpu_id) {
            unsafe {
                let run = &*vcpu.kvm_run;
                let io = &*(run.exit_data.as_ptr() as *const KvmRunExitIo);
                let base = (run as *const KvmRun as *const u8).add(io.data_offset as usize);
                let src = base.add(index as usize * io.size as usize);
                let mut val: u32 = 0;
                core::ptr::copy_nonoverlapping(src, &mut val as *mut u32 as *mut u8, io.size as usize);
                val
            }
        } else {
            0
        }
    }

    /// Write MMIO response data into the kvm_run shared page for an MmioRead exit.
    /// Must be called before the next `run_vcpu`.
    pub fn set_mmio_response(&mut self, vcpu_id: u32, data: &[u8]) {
        if let Ok(vcpu) = self.get_vcpu(vcpu_id) {
            unsafe {
                let mmio = &mut *((*vcpu.kvm_run).exit_data.as_ptr() as *mut KvmRunExitMmio);
                let len = data.len().min(mmio.len as usize).min(8);
                mmio.data[..len].copy_from_slice(&data[..len]);
            }
        }
    }

    fn build_cpuid_buf(entries: &[CpuidEntry]) -> Vec<u8> {
        let header_size = 8usize;
        let entry_size = core::mem::size_of::<KvmCpuidEntry2>();
        let total = header_size + entries.len() * entry_size;
        let mut buf = vec![0u8; total];
        let nent = entries.len() as u32;
        buf[0..4].copy_from_slice(&nent.to_ne_bytes());
        for (i, e) in entries.iter().enumerate() {
            let off = header_size + i * entry_size;
            let ke = KvmCpuidEntry2 {
                function: e.function,
                index: e.index,
                flags: e.flags,
                eax: e.eax, ebx: e.ebx, ecx: e.ecx, edx: e.edx,
                _padding: [0; 3],
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &ke as *const _ as *const u8,
                    buf.as_mut_ptr().add(off),
                    entry_size,
                );
            }
        }
        buf
    }

    fn translate_phys(&self, addr: u64, len: usize) -> Option<*mut u8> {
        for slot in &self.memory_slots {
            if addr >= slot.guest_phys && addr + len as u64 <= slot.guest_phys + slot.size {
                let offset = (addr - slot.guest_phys) as usize;
                return Some(unsafe { slot.host_ptr.add(offset) });
            }
        }
        None
    }

    /// Read the in-kernel LAPIC register page (1024 bytes) for a vCPU.
    pub fn get_lapic(&self, vcpu_idx: u32) -> Result<[u8; 1024], VmError> {
        let vcpu = self.vcpus.get(vcpu_idx as usize)
            .and_then(|v| v.as_ref())
            .ok_or(VmError::BackendError(-1))?;
        let mut regs = [0u8; 1024];
        let ret = unsafe { sys_ioctl(vcpu.fd, KVM_GET_LAPIC, regs.as_mut_ptr() as u64) };
        if ret < 0 { return Err(VmError::BackendError(ret as i32)); }
        Ok(regs)
    }

    /// Read the in-kernel irqchip state.
    /// chip_id: 0=PIC master, 1=PIC slave, 2=IOAPIC.
    /// Returns up to 512 bytes of chip-specific state.
    pub fn get_irqchip(&self, chip_id: u32) -> Result<Vec<u8>, VmError> {
        // kvm_irqchip: u32 chip_id, u32 pad, then 512 bytes of data
        let mut buf = vec![0u8; 4 + 4 + 512];
        buf[0..4].copy_from_slice(&chip_id.to_le_bytes());
        let ret = unsafe { sys_ioctl(self.vm_fd, KVM_GET_IRQCHIP, buf.as_mut_ptr() as u64) };
        if ret < 0 { return Err(VmError::BackendError(ret as i32)); }
        Ok(buf[8..].to_vec()) // skip chip_id + pad
    }

    /// Assert or deassert an IRQ line on the in-kernel irqchip.
    /// `irq` is the GSI (Global System Interrupt) number.
    pub fn set_irq_line(&self, irq: u32, level: bool) -> Result<(), VmError> {
        let irq_level = KvmIrqLevel {
            irq,
            level: if level { 1 } else { 0 },
        };
        let ret = unsafe {
            sys_ioctl(self.vm_fd, KVM_IRQ_LINE, &irq_level as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    /// Set MSRs for a vCPU. Each entry is (index, value).
    pub fn set_msrs(&self, vcpu_idx: u32, msrs: &[(u32, u64)]) -> Result<(), VmError> {
        let vcpu = self.vcpus.get(vcpu_idx as usize)
            .and_then(|v| v.as_ref())
            .ok_or(VmError::BackendError(-1))?;
        // kvm_msrs header: nmsrs(u32) + pad(u32) + entries[]
        // kvm_msr_entry: index(u32) + reserved(u32) + data(u64) = 16 bytes
        let buf_size = 8 + msrs.len() * 16;
        let mut buf = vec![0u8; buf_size];
        let nmsrs = msrs.len() as u32;
        buf[0..4].copy_from_slice(&nmsrs.to_ne_bytes());
        for (i, &(index, data)) in msrs.iter().enumerate() {
            let off = 8 + i * 16;
            buf[off..off+4].copy_from_slice(&index.to_ne_bytes());
            buf[off+8..off+16].copy_from_slice(&data.to_ne_bytes());
        }
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_SET_MSRS, buf.as_ptr() as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    /// Set the MP state of a vCPU.
    /// 0 = RUNNABLE, 1 = UNINITIALIZED, 2 = INIT_RECEIVED, 3 = HALTED, 4 = SIPI_RECEIVED
    pub fn set_mp_state(&self, vcpu_idx: u32, state: u32) -> Result<(), VmError> {
        let vcpu = self.vcpus.get(vcpu_idx as usize)
            .and_then(|v| v.as_ref())
            .ok_or(VmError::BackendError(-1))?;
        let mp_state: u32 = state;
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_SET_MP_STATE, &mp_state as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    /// Synchronize TSC across all vCPUs.
    ///
    /// Sets IA32_TSC (MSR 0x10) and IA32_TSC_ADJUST (MSR 0x3B) to 0 on all
    /// created vCPUs in a tight loop, minimizing the time skew between them.
    /// Must be called just before starting the VM execution threads.
    pub fn sync_tsc(&self) {
        for (idx, slot) in self.vcpus.iter().enumerate() {
            if let Some(vcpu) = slot {
                // Build a KVM_SET_MSRS buffer for IA32_TSC=0, IA32_TSC_ADJUST=0
                let mut buf = [0u8; 8 + 2 * 16]; // header + 2 entries
                buf[0..4].copy_from_slice(&2u32.to_ne_bytes()); // nmsrs = 2
                // Entry 0: IA32_TSC (0x10) = 0
                buf[8..12].copy_from_slice(&0x10u32.to_ne_bytes());
                // data at offset 16..24 is already 0
                // Entry 1: IA32_TSC_ADJUST (0x3B) = 0
                buf[24..28].copy_from_slice(&0x3Bu32.to_ne_bytes());
                // data at offset 32..40 is already 0
                unsafe {
                    let ret = sys_ioctl(vcpu.fd, KVM_SET_MSRS, buf.as_ptr() as u64);
                    if ret < 0 {
                        eprintln!("[kvm] sync_tsc: KVM_SET_MSRS failed for vcpu {}: {}", idx, ret);
                    }
                }
            }
        }
    }

    /// Return the fixed TSC frequency in kHz (0 if not detected).
    pub fn tsc_khz(&self) -> u32 {
        self.tsc_khz
    }

    /// Inject an MSI interrupt into the guest via KVM_SIGNAL_MSI.
    /// `address`: MSI address (written by guest to MSI capability register)
    /// `data`: MSI data (written by guest to MSI capability register)
    pub fn signal_msi(&self, address: u64, data: u32) -> Result<i64, VmError> {
        let msi = KvmMsi {
            address_lo: address as u32,
            address_hi: (address >> 32) as u32,
            data,
            flags: 0,
            devid: 0,
            _pad: [0; 12],
        };
        let ret = unsafe {
            sys_ioctl(self.vm_fd, KVM_SIGNAL_MSI, &msi as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        // ret=1: delivered to a LAPIC, ret=0: no matching LAPIC found
        Ok(ret)
    }

    /// Build a minimal GSI routing table with just the MSI entry for AHCI.
    ///
    /// Two strategies are tried:
    /// 1. Full table (PIC + IOAPIC + MSI) — works with full in-kernel irqchip
    /// 2. MSI-only table — works with split irqchip (irqchip_split=true
    ///    rejects KVM_IRQ_ROUTING_IRQCHIP entries with EINVAL)
    fn build_gsi_routing(&self, msi_address: u64, msi_data: u32, msi_only: bool) -> Vec<u8> {
        let entry_size = core::mem::size_of::<KvmIrqRoutingEntry>();
        let header_size = core::mem::size_of::<KvmIrqRouting>();

        if msi_only {
            // MSI-only: just 1 entry
            let nr_entries = 1u32;
            let total_size = header_size + nr_entries as usize * entry_size;
            let mut buf = alloc::vec![0u8; total_size];
            let routing = buf.as_mut_ptr() as *mut KvmIrqRouting;
            unsafe {
                (*routing).nr = nr_entries;
                (*routing).flags = 0;
                let e = &mut *(buf.as_mut_ptr().add(header_size) as *mut KvmIrqRoutingEntry);
                *e = core::mem::zeroed();
                e.gsi = 24;
                e.entry_type = KVM_IRQ_ROUTING_MSI;
                e.address_lo = msi_address as u32;
                e.address_hi = (msi_address >> 32) as u32;
                e.data = msi_data;
            }
            return buf;
        }

        // Full table: KVM default (PIC + IOAPIC) + MSI
        let nr_entries = 41u32;
        let total_size = header_size + nr_entries as usize * entry_size;
        let mut buf = alloc::vec![0u8; total_size];
        let routing = buf.as_mut_ptr() as *mut KvmIrqRouting;
        unsafe { (*routing).nr = nr_entries; }
        unsafe { (*routing).flags = 0; }

        let entries = unsafe { buf.as_mut_ptr().add(header_size) as *mut KvmIrqRoutingEntry };
        let mut idx = 0usize;

        // GSI 0-7: PIC_MASTER + IOAPIC
        for i in 0..8u32 {
            unsafe {
                let e = &mut *entries.add(idx);
                *e = core::mem::zeroed();
                e.gsi = i;
                e.entry_type = KVM_IRQ_ROUTING_IRQCHIP;
                e.address_lo = 0; // PIC_MASTER
                e.address_hi = i;
            }
            idx += 1;
            unsafe {
                let e = &mut *entries.add(idx);
                *e = core::mem::zeroed();
                e.gsi = i;
                e.entry_type = KVM_IRQ_ROUTING_IRQCHIP;
                e.address_lo = 2; // IOAPIC
                e.address_hi = i;
            }
            idx += 1;
        }
        // GSI 8-15: PIC_SLAVE + IOAPIC
        for i in 8..16u32 {
            unsafe {
                let e = &mut *entries.add(idx);
                *e = core::mem::zeroed();
                e.gsi = i;
                e.entry_type = KVM_IRQ_ROUTING_IRQCHIP;
                e.address_lo = 1; // PIC_SLAVE
                e.address_hi = i - 8;
            }
            idx += 1;
            unsafe {
                let e = &mut *entries.add(idx);
                *e = core::mem::zeroed();
                e.gsi = i;
                e.entry_type = KVM_IRQ_ROUTING_IRQCHIP;
                e.address_lo = 2; // IOAPIC
                e.address_hi = i;
            }
            idx += 1;
        }
        // GSI 16-23: IOAPIC only
        for i in 16..24u32 {
            unsafe {
                let e = &mut *entries.add(idx);
                *e = core::mem::zeroed();
                e.gsi = i;
                e.entry_type = KVM_IRQ_ROUTING_IRQCHIP;
                e.address_lo = 2; // IOAPIC
                e.address_hi = i;
            }
            idx += 1;
        }
        // GSI 24: AHCI MSI
        unsafe {
            let e = &mut *entries.add(idx);
            *e = core::mem::zeroed();
            e.gsi = 24;
            e.entry_type = KVM_IRQ_ROUTING_MSI;
            e.address_lo = msi_address as u32;
            e.address_hi = (msi_address >> 32) as u32;
            e.data = msi_data;
        }
        buf
    }

    /// Set up kernel-level MSI delivery for AHCI using KVM_IRQFD + GSI routing.
    pub fn setup_ahci_msi_irqfd(&mut self, address: u64, data: u32) -> Result<(), VmError> {
        unsafe {
            let fd = sys_eventfd(0, 0);
            if fd < 0 {
                return Err(VmError::BackendError(fd as i32));
            }

            let gsi: u32 = 24;

            // Try to set up GSI routing for irqfd.
            // Build MSI-only routing (1 entry). Full PIC+IOAPIC routing
            // fails on some kernels/CPUs (e.g. AMD with specific irqchip config).
            #[repr(C)]
            struct MsiOnlyRouting {
                header: KvmIrqRouting,
                entry: KvmIrqRoutingEntry,
            }
            let mut routing: MsiOnlyRouting = core::mem::zeroed();
            routing.header.nr = 1;
            routing.entry.gsi = gsi;
            routing.entry.entry_type = KVM_IRQ_ROUTING_MSI;
            routing.entry.address_lo = address as u32;
            routing.entry.address_hi = (address >> 32) as u32;
            routing.entry.data = data;

            let ret = sys_ioctl(self.vm_fd, KVM_SET_GSI_ROUTING, &routing as *const _ as u64);
            if ret < 0 {
                // GSI routing not available — irqfd won't work.
                // Fall back to signal_msi + legacy IRQ belt-and-suspenders.
                sys_close(fd);
                eprintln!("[kvm] KVM_SET_GSI_ROUTING unavailable ({}), using signal_msi+legacy fallback", ret);
                return Err(VmError::BackendError(ret as i32));
            }

            let irqfd = KvmIrqfd {
                fd: fd as u32,
                gsi,
                flags: 0,
                resamplefd: 0,
                _pad: [0; 16],
            };
            let ret = sys_ioctl(self.vm_fd, KVM_IRQFD, &irqfd as *const _ as u64);
            if ret < 0 {
                sys_close(fd);
                eprintln!("[kvm] KVM_IRQFD failed: {}", ret);
                return Err(VmError::BackendError(ret as i32));
            }

            if self.ahci_msi_fd >= 0 {
                sys_close(self.ahci_msi_fd);
            }

            self.ahci_msi_fd = fd;
            self.ahci_msi_gsi = gsi;

            eprintln!("[kvm] AHCI MSI irqfd setup: fd={} gsi={} addr=0x{:X} data=0x{:X}",
                fd, gsi, address, data);
            Ok(())
        }
    }

    /// Update the AHCI MSI routing entry when the guest changes MSI address/data.
    pub fn update_ahci_msi_route(&mut self, address: u64, data: u32) -> Result<(), VmError> {
        if self.ahci_msi_fd < 0 {
            return self.setup_ahci_msi_irqfd(address, data);
        }
        unsafe {
            // Try full, then MSI-only
            let buf = self.build_gsi_routing(address, data, false);
            let routing = buf.as_ptr() as *const KvmIrqRouting;
            let mut ret = sys_ioctl(self.vm_fd, KVM_SET_GSI_ROUTING, routing as u64);
            if ret < 0 {
                let buf2 = self.build_gsi_routing(address, data, true);
                let routing2 = buf2.as_ptr() as *const KvmIrqRouting;
                ret = sys_ioctl(self.vm_fd, KVM_SET_GSI_ROUTING, routing2 as u64);
            }
            if ret < 0 {
                return Err(VmError::BackendError(ret as i32));
            }
            Ok(())
        }
    }

    /// Trigger AHCI MSI interrupt via the kernel-level irqfd (eventfd write).
    /// This is much more reliable than KVM_SIGNAL_MSI because the kernel
    /// handles delivery retries and interrupt coalescing.
    pub fn trigger_ahci_msi(&self) -> Result<(), VmError> {
        if self.ahci_msi_fd < 0 {
            return Err(VmError::BackendError(-1));
        }
        let val: u64 = 1;
        let ret = unsafe {
            sys_write(self.ahci_msi_fd, &val as *const u64 as *const u8, 8)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }
}

impl Drop for KvmBackend {
    fn drop(&mut self) {
        if self.ahci_msi_fd >= 0 {
            unsafe { sys_close(self.ahci_msi_fd); }
        }
        self.destroy();
    }
}

// ── Segment conversion helpers ────────────────────────────────────────────

fn seg_to_kvm(s: &SegmentReg) -> KvmSegment {
    KvmSegment {
        base: s.base,
        limit: s.limit,
        selector: s.selector,
        type_: s.type_,
        present: s.present,
        dpl: s.dpl,
        db: s.db,
        s: s.s,
        l: s.l,
        g: s.g,
        avl: s.avl,
        unusable: if s.present != 0 { 0 } else { 1 },
        _padding: 0,
    }
}

fn seg_from_kvm(k: &KvmSegment) -> SegmentReg {
    SegmentReg {
        base: k.base,
        limit: k.limit,
        selector: k.selector,
        type_: k.type_,
        present: k.present,
        dpl: k.dpl,
        db: k.db,
        s: k.s,
        l: k.l,
        g: k.g,
        avl: k.avl,
    }
}

fn dt_from_kvm(k: &KvmDtable) -> DescriptorTable {
    DescriptorTable {
        base: k.base,
        limit: k.limit,
    }
}

fn dt_to_kvm(d: &DescriptorTable) -> KvmDtable {
    KvmDtable {
        base: d.base,
        limit: d.limit,
        _padding: [0; 3],
    }
}

// ── VmBackend implementation ──────────────────────────────────────────────

impl VmBackend for KvmBackend {
    fn destroy(&mut self) {
        self.vcpus.clear();
        // Clear this VM's slot so no stale state remains
        get_vm_slot(self.vm_slot).clear();
        if self.vm_fd >= 0 {
            unsafe { sys_close(self.vm_fd); }
            self.vm_fd = -1;
        }
        if self.kvm_fd >= 0 {
            unsafe { sys_close(self.kvm_fd); }
            self.kvm_fd = -1;
        }
    }

    fn reset(&mut self) -> Result<(), VmError> {
        // KVM doesn't have a global VM reset; vCPUs must be re-created.
        Ok(())
    }

    fn set_memory_region(
        &mut self,
        slot: u32,
        guest_phys: u64,
        size: u64,
        host_ptr: *mut u8,
    ) -> Result<(), VmError> {
        let region = KvmUserspaceMemoryRegion {
            slot,
            flags: 0,
            guest_phys_addr: guest_phys,
            memory_size: size,
            userspace_addr: host_ptr as u64,
        };
        let ret = unsafe {
            sys_ioctl(
                self.vm_fd,
                KVM_SET_USER_MEMORY_REGION,
                &region as *const _ as u64,
            )
        };
        if ret < 0 {
            eprintln!("[kvm] KVM_SET_USER_MEMORY_REGION failed: slot={} gpa=0x{:x} size=0x{:x} ret={}", slot, guest_phys, size, ret);
            return Err(VmError::MemoryMapFailed);
        }
        // Track for read_phys/write_phys
        self.memory_slots.retain(|s| s.guest_phys != guest_phys);
        if size > 0 {
            self.memory_slots.push(MemorySlot {
                guest_phys,
                size,
                host_ptr,
            });
        }
        Ok(())
    }

    fn read_phys(&self, addr: u64, buf: &mut [u8]) -> Result<(), VmError> {
        let ptr = self.translate_phys(addr, buf.len()).ok_or(VmError::MemoryMapFailed)?;
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), buf.len());
        }
        Ok(())
    }

    fn write_phys(&mut self, addr: u64, buf: &[u8]) -> Result<(), VmError> {
        let ptr = self.translate_phys(addr, buf.len()).ok_or(VmError::MemoryMapFailed)?;
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, buf.len());
        }
        Ok(())
    }

    fn create_vcpu(&mut self, id: u32) -> Result<(), VmError> {
        unsafe {
            let vcpu_fd = sys_ioctl(self.vm_fd, KVM_CREATE_VCPU, id as u64) as i32;
            if vcpu_fd < 0 {
                return Err(VmError::BackendError(vcpu_fd));
            }

            let run_ptr = sys_mmap(
                0,
                self.mmap_size as u64,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                vcpu_fd,
                0,
            );
            if run_ptr < 0 || (run_ptr as u64) >= 0xFFFF_FFFF_FFFF_F000 {
                eprintln!("[kvm] mmap kvm_run failed: ptr=0x{:x}", run_ptr as u64);
                sys_close(vcpu_fd);
                return Err(VmError::BackendError(-1));
            }

            let vcpu = KvmVcpu {
                fd: vcpu_fd,
                kvm_run: run_ptr as *mut KvmRun,
                mmap_size: self.mmap_size,
            };

            // Apply stored CPUID with per-vCPU adjustments
            if let Some(ref entries) = self.stored_cpuid {
                // Clone entries and fix per-vCPU fields
                let mut adjusted = entries.clone();
                let max_cpus = self.cpu_count.max(1);
                for e in &mut adjusted {
                    if e.function == 1 {
                        // EBX[31:24] = Initial APIC ID = vcpu_id
                        // EBX[23:16] = Max logical processors = cpu_count
                        e.ebx = (e.ebx & 0x0000_FFFF)
                            | ((max_cpus & 0xFF) << 16)
                            | ((id as u32 & 0xFF) << 24);
                    }
                    if e.function == 0xB {
                        // x2APIC topology: EDX = x2APIC ID = vcpu_id
                        e.edx = id as u32;
                    }
                    if e.function == 0x8000001E {
                        // AMD Extended APIC ID: EAX = APIC ID = vcpu_id
                        e.eax = id as u32;
                        // EBX[7:0] = compute unit ID = vcpu_id (1 thread per core)
                        e.ebx = id as u32;
                    }
                }
                // CPUID configured for vCPU with correct APIC ID and topology
                let buf = Self::build_cpuid_buf(&adjusted);
                let ret = sys_ioctl(vcpu_fd, KVM_SET_CPUID2, buf.as_ptr() as u64);
                if ret < 0 {
                    eprintln!("[kvm] KVM_SET_CPUID2 failed: ret={}", ret);
                    sys_close(vcpu_fd);
                    return Err(VmError::BackendError(ret as i32));
                }
            }

            // Explicitly set LAPIC ID for this vCPU.
            // KVM initializes the LAPIC ID from the vcpu_id passed to
            // KVM_CREATE_VCPU, but we read-modify-write it here to ensure
            // correctness (matching what QEMU does).
            {
                let mut lapic_regs = [0u8; 1024];
                let lr = sys_ioctl(vcpu_fd, KVM_GET_LAPIC, lapic_regs.as_mut_ptr() as u64);
                if lr >= 0 {
                    // LAPIC ID register is at offset 0x20, bits 31:24 = APIC ID
                    lapic_regs[0x23] = id as u8; // byte 3 of u32 at 0x20 = bits 31:24
                    sys_ioctl(vcpu_fd, KVM_SET_LAPIC, lapic_regs.as_ptr() as u64);
                }
            }

            // Fix TSC frequency for SMP: read from first vCPU, set on all.
            // Without a fixed TSC frequency, KVM inherits the host's current
            // P-state frequency which can vary between cores. This causes
            // Linux to report "Firmware Bug: TSC not synchronous" and Windows
            // to hang in timer calibration loops.
            {
                if self.tsc_khz == 0 {
                    // First vCPU: read the host TSC frequency
                    let khz = sys_ioctl(vcpu_fd, KVM_GET_TSC_KHZ, 0);
                    if khz > 0 {
                        self.tsc_khz = khz as u32;
                    }
                }
                if self.tsc_khz > 0 {
                    sys_ioctl(vcpu_fd, KVM_SET_TSC_KHZ, self.tsc_khz as u64);
                }
            }

            // Register kvm_run pointer for thread-safe cancel (per-VM slot)
            let slot = get_vm_slot(self.vm_slot);
            if (id as usize) < MAX_VCPUS {
                slot.cancel_kvm_run[id as usize].store(vcpu.kvm_run as u64, Ordering::Relaxed);
            }

            let idx = id as usize;
            while self.vcpus.len() <= idx {
                self.vcpus.push(None);
            }
            self.vcpus[idx] = Some(vcpu);
        }
        Ok(())
    }

    fn destroy_vcpu(&mut self, id: u32) -> Result<(), VmError> {
        let idx = id as usize;
        if idx < self.vcpus.len() {
            // Clear cancel pointer and thread ID in per-VM slot
            let slot = get_vm_slot(self.vm_slot);
            if idx < MAX_VCPUS {
                slot.cancel_kvm_run[idx].store(0, Ordering::Relaxed);
                slot.vcpu_tid[idx].store(0, Ordering::Relaxed);
            }
            self.vcpus[idx] = None; // Drop handles cleanup
            Ok(())
        } else {
            Err(VmError::InvalidVcpuId)
        }
    }

    fn run_vcpu(&mut self, id: u32) -> Result<VmExitReason, VmError> {
        let vcpu = self.get_vcpu(id)?;
        let fd = vcpu.fd;
        let run = vcpu.kvm_run;

        // Set current VM slot for PIT channel 2 callbacks
        CURRENT_VM_SLOT.store(self.vm_slot as u32, Ordering::Relaxed);

        // Store this thread's TID on first call so cancel can send SIGUSR1
        let slot = get_vm_slot(self.vm_slot);
        if (id as usize) < MAX_VCPUS {
            let tid = slot.vcpu_tid[id as usize].load(Ordering::Relaxed);
            if tid == 0 {
                slot.vcpu_tid[id as usize].store(unsafe { sys_gettid() }, Ordering::Relaxed);
            }
        }

        // Reset immediate_exit before entering KVM_RUN
        unsafe { (*run).immediate_exit = 0; }

        let ret = unsafe { sys_ioctl(fd, KVM_RUN, 0) };
        if ret < 0 {
            let errno = (-ret) as i32;
            // EINTR means we were cancelled via immediate_exit — not an error
            if errno == 4 { // EINTR
                return Ok(VmExitReason::Cancelled);
            }
            return Err(VmError::BackendError(errno));
        }

        unsafe {
            let exit_reason = (*run).exit_reason;
            match exit_reason {
                KVM_EXIT_IO => {
                    let io = &*((*run).exit_data.as_ptr() as *const KvmRunExitIo);
                    let data_ptr = (run as *const u8).add(io.data_offset as usize);
                    if io.direction == KVM_EXIT_IO_OUT {
                        let mut val: u32 = 0;
                        core::ptr::copy_nonoverlapping(
                            data_ptr,
                            &mut val as *mut u32 as *mut u8,
                            io.size as usize,
                        );
                        Ok(VmExitReason::IoOut {
                            port: io.port,
                            size: io.size,
                            data: val,
                            count: io.count,
                        })
                    } else {
                        Ok(VmExitReason::IoIn {
                            port: io.port,
                            size: io.size,
                            count: io.count,
                        })
                    }
                }
                KVM_EXIT_MMIO => {
                    let mmio = &*((*run).exit_data.as_ptr() as *const KvmRunExitMmio);
                    if mmio.is_write != 0 {
                        let mut val: u64 = 0;
                        core::ptr::copy_nonoverlapping(
                            mmio.data.as_ptr(),
                            &mut val as *mut u64 as *mut u8,
                            (mmio.len as usize).min(8),
                        );
                        Ok(VmExitReason::MmioWrite {
                            addr: mmio.phys_addr,
                            size: mmio.len as u8,
                            data: val,
                        })
                    } else {
                        Ok(VmExitReason::MmioRead {
                            addr: mmio.phys_addr,
                            size: mmio.len as u8,
                            dest_reg: 0,
                            instr_len: 0,
                        })
                    }
                }
                KVM_EXIT_HLT => Ok(VmExitReason::Halted),
                KVM_EXIT_SHUTDOWN => Ok(VmExitReason::Shutdown),
                KVM_EXIT_IRQ_WINDOW_OPEN => Ok(VmExitReason::InterruptWindow),
                KVM_EXIT_DEBUG => Ok(VmExitReason::Debug),
                KVM_EXIT_INTERNAL_ERROR => Ok(VmExitReason::Error),
                _ => Ok(VmExitReason::Error),
            }
        }
    }

    fn get_vcpu_regs(&self, id: u32) -> Result<VcpuRegs, VmError> {
        let vcpu = self.get_vcpu(id)?;
        let mut kregs = KvmRegs::default();
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_GET_REGS, &mut kregs as *mut _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(VcpuRegs {
            rax: kregs.rax,
            rbx: kregs.rbx,
            rcx: kregs.rcx,
            rdx: kregs.rdx,
            rsi: kregs.rsi,
            rdi: kregs.rdi,
            rbp: kregs.rbp,
            rsp: kregs.rsp,
            r8: kregs.r8,
            r9: kregs.r9,
            r10: kregs.r10,
            r11: kregs.r11,
            r12: kregs.r12,
            r13: kregs.r13,
            r14: kregs.r14,
            r15: kregs.r15,
            rip: kregs.rip,
            rflags: kregs.rflags,
        })
    }

    fn set_vcpu_regs(&mut self, id: u32, regs: &VcpuRegs) -> Result<(), VmError> {
        let vcpu = self.get_vcpu(id)?;
        let kregs = KvmRegs {
            rax: regs.rax,
            rbx: regs.rbx,
            rcx: regs.rcx,
            rdx: regs.rdx,
            rsi: regs.rsi,
            rdi: regs.rdi,
            rsp: regs.rsp,
            rbp: regs.rbp,
            r8: regs.r8,
            r9: regs.r9,
            r10: regs.r10,
            r11: regs.r11,
            r12: regs.r12,
            r13: regs.r13,
            r14: regs.r14,
            r15: regs.r15,
            rip: regs.rip,
            rflags: regs.rflags,
        };
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_SET_REGS, &kregs as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    fn get_vcpu_sregs(&self, id: u32) -> Result<VcpuSregs, VmError> {
        let vcpu = self.get_vcpu(id)?;
        let mut ks = KvmSregs::default();
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_GET_SREGS, &mut ks as *mut _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(VcpuSregs {
            cs: seg_from_kvm(&ks.cs),
            ds: seg_from_kvm(&ks.ds),
            es: seg_from_kvm(&ks.es),
            fs: seg_from_kvm(&ks.fs),
            gs: seg_from_kvm(&ks.gs),
            ss: seg_from_kvm(&ks.ss),
            tr: seg_from_kvm(&ks.tr),
            ldt: seg_from_kvm(&ks.ldt),
            gdt: dt_from_kvm(&ks.gdt),
            idt: dt_from_kvm(&ks.idt),
            cr0: ks.cr0,
            cr2: ks.cr2,
            cr3: ks.cr3,
            cr4: ks.cr4,
            efer: ks.efer,
        })
    }

    fn set_vcpu_sregs(&mut self, id: u32, sregs: &VcpuSregs) -> Result<(), VmError> {
        let vcpu = self.get_vcpu(id)?;
        // Read current to preserve fields we don't expose (cr8, apic_base, interrupt_bitmap)
        let mut ks = KvmSregs::default();
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_GET_SREGS, &mut ks as *mut _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }

        ks.cs = seg_to_kvm(&sregs.cs);
        ks.ds = seg_to_kvm(&sregs.ds);
        ks.es = seg_to_kvm(&sregs.es);
        ks.fs = seg_to_kvm(&sregs.fs);
        ks.gs = seg_to_kvm(&sregs.gs);
        ks.ss = seg_to_kvm(&sregs.ss);
        ks.tr = seg_to_kvm(&sregs.tr);
        ks.ldt = seg_to_kvm(&sregs.ldt);
        ks.gdt = dt_to_kvm(&sregs.gdt);
        ks.idt = dt_to_kvm(&sregs.idt);
        ks.cr0 = sregs.cr0;
        ks.cr2 = sregs.cr2;
        ks.cr3 = sregs.cr3;
        ks.cr4 = sregs.cr4;
        ks.efer = sregs.efer;

        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_SET_SREGS, &ks as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    fn inject_interrupt(&mut self, id: u32, vector: u8) -> Result<(), VmError> {
        let vcpu = self.get_vcpu(id)?;
        let irq: u32 = vector as u32;
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_INTERRUPT, &irq as *const u32 as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    fn inject_exception(&mut self, id: u32, vector: u8, error_code: Option<u32>) -> Result<(), VmError> {
        let vcpu = self.get_vcpu(id)?;

        // Read current events
        let mut events: KvmVcpuEvents = unsafe { core::mem::zeroed() };
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_GET_VCPU_EVENTS, &mut events as *mut _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }

        events.exception.pending = 0;
        events.exception.injected = 1;
        events.exception.nr = vector;
        events.exception.has_error_code = if error_code.is_some() { 1 } else { 0 };
        events.exception.error_code = error_code.unwrap_or(0);

        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_SET_VCPU_EVENTS, &events as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    fn inject_nmi(&mut self, id: u32) -> Result<(), VmError> {
        let vcpu = self.get_vcpu(id)?;

        let mut events: KvmVcpuEvents = unsafe { core::mem::zeroed() };
        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_GET_VCPU_EVENTS, &mut events as *mut _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }

        events.nmi.injected = 1;

        let ret = unsafe {
            sys_ioctl(vcpu.fd, KVM_SET_VCPU_EVENTS, &events as *const _ as u64)
        };
        if ret < 0 {
            return Err(VmError::BackendError(ret as i32));
        }
        Ok(())
    }

    fn request_interrupt_window(&mut self, id: u32, enable: bool) -> Result<(), VmError> {
        let vcpu = self.get_vcpu_mut(id)?;
        unsafe {
            (*vcpu.kvm_run).request_interrupt_window = if enable { 1 } else { 0 };
        }
        Ok(())
    }

    fn set_cpuid(&mut self, entries: &[CpuidEntry]) -> Result<(), VmError> {
        // Store for later vCPU creation
        self.stored_cpuid = Some(entries.to_vec());

        let buf = Self::build_cpuid_buf(entries);

        // Apply to all existing vCPUs
        for vcpu_opt in &self.vcpus {
            if let Some(vcpu) = vcpu_opt {
                let ret = unsafe {
                    sys_ioctl(vcpu.fd, KVM_SET_CPUID2, buf.as_ptr() as u64)
                };
                if ret < 0 {
                    return Err(VmError::BackendError(ret as i32));
                }
            }
        }
        Ok(())
    }
}
