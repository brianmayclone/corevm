//! Core VM execution loop — the single source of truth.
//!
//! This module contains the canonical exit dispatch logic and the BSP main
//! loop. Both vmctl and vmmanager delegate to these functions, ensuring
//! identical behavior across all frontends.
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────────┐
//!  │  Application  │  (vmctl / vmmanager)
//!  │  EventHandler │◄── VmEvent (serial, shutdown, etc.)
//!  └──────┬───────┘
//!         │ VmRuntime::start()
//!  ┌──────▼───────┐
//!  │   bsp_loop   │  BSP thread (vCPU 0)
//!  │  ┌─────────┐ │
//!  │  │ run_vcpu │─┤── dispatch_exit()
//!  │  └─────────┘ │
//!  │  advance_pit │
//!  │  advance_rtc │
//!  │  poll_irqs   │
//!  │  poll_devices│
//!  │  drain_io    │
//!  └──────────────┘
//! ```

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ffi::*;
use super::config::{VmRuntimeConfig, InputEvent};
use super::event::{EventHandler, VmEvent};
use super::RuntimeControl;

// ── Exit dispatch ───────────────────────────────────────────────────────────

/// Action to take after dispatching a VM exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitAction {
    /// Continue running (normal I/O, MMIO, cancel handled).
    Continue,
    /// vCPU executed HLT. KVM blocks in-kernel until an interrupt arrives.
    /// BSP should briefly sleep for timer responsivity; APs must NOT sleep
    /// (sleeping delays IPI delivery and kills SMP performance).
    Halted,
    /// VM shut down (triple fault or ACPI shutdown).
    Shutdown,
    /// VM encountered a fatal error.
    Error,
    /// Guest requested a system reboot (port 0xCF9).
    Reboot,
}

/// Dispatch a single VM exit to the appropriate handler.
///
/// Called by both BSP and AP threads. Handles IoIn, IoOut, MmioRead,
/// MmioWrite, StringIo, and pass-through exits (Halted, Cancelled,
/// InterruptWindow).
///
/// Returns [`ExitAction`] to tell the caller what to do next.
pub(crate) fn dispatch_exit(
    handle: u64,
    vcpu_id: u32,
    exit: &CExitReason,
) -> ExitAction {
    match exit.reason {
        0 => {
            // IoIn
            if exit.io_count > 1 {
                corevm_complete_string_io(handle, vcpu_id, exit.port, 0, exit.size, exit.io_count);
            } else {
                let mut data = [0u8; 4];
                corevm_handle_io_exit(handle, vcpu_id, exit.port, 0, exit.size, data.as_mut_ptr());
            }
            ExitAction::Continue
        }
        1 => {
            // IoOut
            if exit.io_count > 1 {
                corevm_complete_string_io(handle, vcpu_id, exit.port, 1, exit.size, exit.io_count);
            } else {
                let mut data = exit.data_u32.to_le_bytes();
                corevm_handle_io_exit(handle, vcpu_id, exit.port, 1, exit.size, data.as_mut_ptr());
            }
            ExitAction::Continue
        }
        2 => {
            // MmioRead
            let mut data = [0u8; 8];
            corevm_handle_mmio_exit(
                handle, vcpu_id, exit.addr, 0, exit.size,
                data.as_mut_ptr(), exit.mmio_dest_reg, exit.mmio_instr_len,
            );
            ExitAction::Continue
        }
        3 => {
            // MmioWrite
            let mut data = exit.data_u64.to_le_bytes();
            corevm_handle_mmio_exit(
                handle, vcpu_id, exit.addr, 1, exit.size,
                data.as_mut_ptr(), 0, 0,
            );
            ExitAction::Continue
        }
        7 => {
            // Halted — KVM blocks in-kernel until next interrupt.
            // Do NOT sleep here: on APs, sleeping delays IPI delivery
            // by up to 1ms causing severe SMP performance degradation.
            // On the BSP, the cancel thread kicks us out periodically
            // for timer advancement anyway.
            ExitAction::Halted
        }
        8 => {
            // InterruptWindow — guest is ready to accept interrupts.
            // poll_irqs (called after dispatch) will inject the pending one.
            ExitAction::Continue
        }
        9 => {
            // Shutdown (triple fault)
            ExitAction::Shutdown
        }
        11 => {
            // Error (emulation failure)
            ExitAction::Error
        }
        12 => {
            // StringIo — bulk REP INSB/OUTSB via guest physical memory
            corevm_handle_string_io_exit(
                handle, vcpu_id, exit.port, exit.string_io_is_write,
                exit.string_io_count, exit.string_io_gpa,
                exit.string_io_step, exit.string_io_instr_len,
                exit.string_io_addr_size, exit.size,
            );
            ExitAction::Continue
        }
        13 => {
            // Cancelled — timer thread kicked us out of KVM_RUN.
            ExitAction::Continue
        }
        _ => {
            // Unknown exit — ignore and continue.
            ExitAction::Continue
        }
    }
}

// ── Timer advancement ───────────────────────────────────────────────────────

/// Advance PIT timer based on wall-clock elapsed time.
///
/// PIT runs at 1.193182 MHz. Ticks every >=100us to keep channel 2 output
/// responsive for port 0x61 delay loops used by SeaBIOS and guest OSes.
fn advance_pit(handle: u64, last_tick: &mut Instant) {
    let now = Instant::now();
    let elapsed_us = now.duration_since(*last_tick).as_micros() as u64;
    if elapsed_us >= 100 {
        let pit_ticks = ((elapsed_us * 1193) / 1000) as u32;
        corevm_pit_advance(handle, pit_ticks);
        *last_tick = now;
    }
}

/// Advance CMOS RTC periodic timer based on wall-clock elapsed time.
///
/// RTC base clock is 32.768 kHz. Uses its own timestamp — NOT shared with
/// PIT — to ensure independent tick accumulation.
fn advance_rtc(handle: u64, last_tick: &mut Instant) {
    let now = Instant::now();
    let elapsed_us = now.duration_since(*last_tick).as_micros() as u64;
    if elapsed_us >= 100 {
        let rtc_ticks = (elapsed_us * 32768) / 1_000_000;
        if rtc_ticks > 0 {
            corevm_cmos_advance(handle, rtc_ticks);
            *last_tick = now;
        }
    }
}

// ── Input injection ─────────────────────────────────────────────────────────

/// Process queued input events from the application.
fn drain_input_queue(handle: u64, queue: &std::sync::Mutex<Vec<InputEvent>>) {
    let events: Vec<InputEvent> = {
        let mut q = queue.lock().unwrap();
        if q.is_empty() { return; }
        core::mem::take(&mut *q)
    };
    for event in events {
        match event {
            InputEvent::Ps2KeyPress(sc) => { corevm_ps2_key_press(handle, sc); }
            InputEvent::Ps2KeyRelease(sc) => { corevm_ps2_key_release(handle, sc); }
            InputEvent::Ps2MouseMove { dx, dy, buttons } => { corevm_ps2_mouse_move(handle, dx, dy, buttons); }
            InputEvent::UsbTabletMove { x, y, buttons } => { corevm_usb_tablet_move(handle, x, y, buttons); }
            InputEvent::VirtioKeyPs2(sc) => { corevm_virtio_kbd_ps2(handle, sc); }
            InputEvent::VirtioTabletMove { x, y, buttons } => { corevm_virtio_tablet_move(handle, x, y, buttons); }
            InputEvent::SerialInput(data) => {
                if !data.is_empty() {
                    corevm_serial_send_input(handle, data.as_ptr(), data.len() as u32);
                }
            }
        }
    }
}

// ── Serial/Debug output draining ────────────────────────────────────────────

/// Drain serial COM1 output and emit as VmEvent.
fn drain_serial(handle: u64, handler: &mut dyn EventHandler) {
    let mut buf = [0u8; 4096];
    let n = corevm_serial_take_output(handle, buf.as_mut_ptr(), buf.len() as u32);
    if n > 0 {
        handler.on_event(VmEvent::SerialOutput(buf[..n as usize].to_vec()));
    }
}

/// Drain debug port output and emit as VmEvent.
fn drain_debug_port(handle: u64, handler: &mut dyn EventHandler) {
    let mut buf = [0u8; 1024];
    let n = corevm_debug_port_take_output(handle, buf.as_mut_ptr(), buf.len() as u32);
    if n > 0 {
        handler.on_event(VmEvent::DebugOutput(buf[..n as usize].to_vec()));
    }
}

// ── Device polling ──────────────────────────────────────────────────────────

/// Poll optional device subsystems based on config flags.
fn poll_devices(
    handle: u64,
    config: &VmRuntimeConfig,
    exit_reason: u32,
    last_audio: &mut Instant,
) {
    // Note: AHCI IRQ polling is done in bsp_loop BEFORE poll_irqs,
    // not here — ordering matters for interrupt latency.

    // UHCI USB frame processing on I/O exits.
    if config.usb_tablet && (exit_reason == 0 || exit_reason == 1) {
        corevm_uhci_process(handle);
    }

    // VirtIO GPU virtqueue processing.
    if config.virtio_gpu {
        corevm_virtio_gpu_process(handle);
    }

    // Intel GPU: no periodic refresh needed — the native driver (i915/igfx)
    // manages the display pipeline. VGA/SVGA handles the boot display.
    // Framebuffer refresh will be triggered by DSPASURF writes from the driver.

    // VirtIO Input event processing.
    if config.virtio_input {
        corevm_virtio_input_process(handle);
    }

    // Network backend polling.
    if config.net_enabled {
        corevm_net_poll(handle);
    }

    // AC97 audio DMA processing (~every 10ms).
    if config.audio_enabled {
        let now = Instant::now();
        if now.duration_since(*last_audio).as_millis() >= 10 {
            corevm_ac97_process(handle);
            *last_audio = now;
        }
    }
}

// ── BSP main loop ───────────────────────────────────────────────────────────

/// The canonical BSP (Bootstrap Processor, vCPU 0) main loop.
///
/// This is the single source of truth for VM execution. It:
///
/// 1. Runs `corevm_run_vcpu(handle, 0)` and dispatches the exit
/// 2. Advances PIT and CMOS RTC timers (wall-clock based)
/// 3. Polls device IRQs via `corevm_poll_irqs`
/// 4. Polls optional subsystems (AHCI, UHCI, AC97, VirtIO, network)
/// 5. Checks for guest-initiated system reset
/// 6. Flushes disk caches periodically
/// 7. Drains serial/debug port output and emits events
/// 8. Processes queued input events from the application
///
/// The loop runs until `control.stop` is set, a fatal exit occurs,
/// or the optional timeout expires.
pub(crate) fn bsp_loop(
    config: &VmRuntimeConfig,
    control: &Arc<RuntimeControl>,
    handler: &mut dyn EventHandler,
    input_queue: &Arc<std::sync::Mutex<Vec<InputEvent>>>,
) {
    let handle = config.handle;
    let start = Instant::now();
    let mut last_pit_tick = Instant::now();
    let mut last_rtc_tick = Instant::now();
    let mut last_audio = Instant::now();
    let mut cache_flush_counter: u32 = 0;
    let mut consecutive_errors: u32 = 0;
    let mut bsp_iterations: u64 = 0;
    let mut bsp_halts: u64 = 0;
    let mut last_diag = Instant::now();
    let mut last_stuck_rip: u64 = 0;
    let mut stuck_count: u32 = 0;
    let mut bsp_exit_counts = [0u64; 16];
    let mut last_bsp_hb = Instant::now();

    loop {
        bsp_iterations += 1;
        if last_diag.elapsed().as_secs() >= 2 {
            // Early-boot spin-wait unstick (first 15 seconds only).
            // OVMF's MpInitLib waits for AP wakeup by polling a RAM variable
            // (pattern: mov rax,[rip+disp32]; cmp rdx,rax; jne done; pause; jmp).
            // This has no timeout and hangs forever if CPUID topology reports
            // more logical CPUs than vCPUs created. We detect the stuck RIP and
            // patch the polled variable to break the spin-wait.
            // After 15s the OS is running and same-RIP is normal (idle loop).
            if start.elapsed().as_secs() <= 15 {
                let mut regs = crate::backend::types::VcpuRegs::default();
                corevm_get_vcpu_regs(handle, 0, &mut regs);
                if regs.rip == last_stuck_rip && bsp_iterations > 50 {
                    stuck_count += 1;
                    // Scan backwards from RIP for: REX.W MOV RAX,[RIP+disp32] (48 8B 05)
                    for back in &[14u64, 12, 16, 18, 20, 10, 22, 8] {
                        let insn_addr = regs.rip.wrapping_sub(*back);
                        let mut insn = [0u8; 7];
                        corevm_read_phys(handle, insn_addr, insn.as_mut_ptr(), 7);
                        if insn[0] == 0x48 && insn[1] == 0x8B && insn[2] == 0x05 {
                            let disp = i32::from_le_bytes([insn[3], insn[4], insn[5], insn[6]]);
                            let next_rip = insn_addr.wrapping_add(7);
                            let poll_addr = (next_rip as i64).wrapping_add(disp as i64) as u64;
                            let val: u64 = stuck_count as u64;
                            corevm_write_phys(handle, poll_addr, &val as *const u64 as *const u8, 8);
                            eprintln!("[bsp] UNSTUCK: wrote {} to 0x{:X} (RIP=0x{:X})", val, poll_addr, regs.rip);
                            break;
                        }
                    }
                } else if regs.rip != last_stuck_rip {
                    stuck_count = 0;
                }
                last_stuck_rip = regs.rip;
            }
            if last_diag.elapsed().as_secs() >= 5 {
                eprintln!("[bsp] alive: {}s iters={} halts={}", start.elapsed().as_secs(), bsp_iterations, bsp_halts);
            }
            last_diag = Instant::now();
        }
        // Check stop flag.
        if control.stop.load(Ordering::Relaxed) {
            break;
        }

        // Check pause flag.
        if control.pause.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }

        // Check timeout.
        if config.timeout > std::time::Duration::ZERO && start.elapsed() >= config.timeout {
            handler.on_event(VmEvent::Shutdown);
            break;
        }

        // ── Run vCPU ────────────────────────────────────────────────────

        let mut exit = CExitReason::default();
        let rc = corevm_run_vcpu(handle, 0, &mut exit);

        if rc != 0 {
            consecutive_errors += 1;
            if consecutive_errors >= 10 {
                handler.on_event(VmEvent::Error {
                    message: format!("Too many consecutive run_vcpu errors ({})", consecutive_errors),
                });
                control.exited.store(true, Ordering::Relaxed);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        consecutive_errors = 0;

        // ── Dispatch exit ───────────────────────────────────────────────

        let reason_idx = exit.reason as usize;
        if reason_idx < bsp_exit_counts.len() {
            bsp_exit_counts[reason_idx] += 1;
        }

        // Track last MMIO address for diagnostics
        let last_mmio_addr = if exit.reason == 2 || exit.reason == 3 { exit.addr } else { 0 };

        // BSP heartbeat every 5 seconds — exit reason distribution + vCPU state
        if last_bsp_hb.elapsed().as_secs() >= 5 {
            let mut regs = crate::backend::types::VcpuRegs::default();
            corevm_get_vcpu_regs(handle, 0, &mut regs);
            eprintln!("[bsp-hb] {}s iters={} halts={} exits=[io:{} mmio:{} hlt:{} cancel:{} irqwin:{}] rip=0x{:X} rflags=0x{:X} mmio_addr=0x{:X}",
                start.elapsed().as_secs(), bsp_iterations, bsp_halts,
                bsp_exit_counts[0] + bsp_exit_counts[1],
                bsp_exit_counts[2] + bsp_exit_counts[3],
                bsp_exit_counts[7],
                bsp_exit_counts[13],
                bsp_exit_counts[8],
                regs.rip, regs.rflags, last_mmio_addr,
            );
            last_bsp_hb = Instant::now();
        }

        let action = dispatch_exit(handle, 0, &exit);

        match action {
            ExitAction::Continue => {}
            ExitAction::Halted => {
                bsp_halts += 1;
                // BSP: brief sleep so we don't busy-loop when the guest is idle.
                // The cancel thread will kick us out for timer advancement.
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            ExitAction::Shutdown => {
                corevm_ahci_flush_caches(handle);
                handler.on_event(VmEvent::Shutdown);
                control.exited.store(true, Ordering::Relaxed);
                break;
            }
            ExitAction::Error => {
                corevm_ahci_flush_caches(handle);
                handler.on_event(VmEvent::Error {
                    message: String::from("VM error exit (emulation failure)"),
                });
                control.exited.store(true, Ordering::Relaxed);
                break;
            }
            ExitAction::Reboot => {
                corevm_ahci_flush_caches(handle);
                handler.on_event(VmEvent::RebootRequested);
                control.reboot_requested.store(true, Ordering::Relaxed);
                control.exited.store(true, Ordering::Relaxed);
                break;
            }
        }

        // ── AHCI IRQ update (must happen BEFORE poll_irqs) ────────────
        // AHCI uses level-triggered interrupts. After an MMIO exit that
        // touches AHCI registers (e.g. command completion), the IRQ state
        // must be updated before poll_irqs checks it. Without this ordering,
        // poll_irqs sees stale IRQ state and the interrupt is delayed until
        // the next cancel-kick — causing ~10x slower disk I/O.
        if exit.reason == 2 || exit.reason == 3 {
            corevm_ahci_poll_irq(handle);
        }

        // ── Timer advancement ───────────────────────────────────────────

        advance_pit(handle, &mut last_pit_tick);
        advance_rtc(handle, &mut last_rtc_tick);

        // ── IRQ polling ─────────────────────────────────────────────────

        corevm_poll_irqs(handle);

        // ── Device polling ──────────────────────────────────────────────

        poll_devices(handle, config, exit.reason, &mut last_audio);

        // ── Second IRQ poll ─────────────────────────────────────────────
        // Network RX and other device events from poll_devices may have
        // set new interrupt causes (e.g. E1000 ICR_RXT0 from net_poll).
        // Poll again so these interrupts are delivered immediately instead
        // of waiting for the next cancel-kick (10ms delay kills TCP throughput).
        corevm_poll_irqs(handle);

        // ── Periodic AHCI stuck-IRQ recovery ──────────────────────────
        // Every ~5000 iterations, take the AHCI lock and run fix_stuck_irq
        // to catch any IRQ delivery race that try_lock in poll_irqs missed.
        if bsp_iterations % 5000 == 0 {
            corevm_ahci_poll_irq(handle);
        }

        // ── Disk cache flush ────────────────────────────────────────────

        cache_flush_counter += 1;
        if cache_flush_counter >= 200 || corevm_ahci_needs_flush(handle) != 0 {
            corevm_ahci_flush_caches(handle);
            cache_flush_counter = 0;
        }

        // ── System reset check ──────────────────────────────────────────

        if corevm_check_reset(handle) != 0 {
            corevm_ahci_flush_caches(handle);
            handler.on_event(VmEvent::RebootRequested);
            control.reboot_requested.store(true, Ordering::Relaxed);
            control.exited.store(true, Ordering::Relaxed);
            break;
        }

        // ── ACPI shutdown check (guest wrote SLP_EN + S5) ────────────
        if corevm_check_acpi_shutdown(handle) != 0 {
            corevm_ahci_flush_caches(handle);
            handler.on_event(VmEvent::Shutdown);
            control.exited.store(true, Ordering::Relaxed);
            break;
        }

        // ── I/O draining ────────────────────────────────────────────────

        drain_serial(handle, handler);
        drain_debug_port(handle, handler);

        // ── Input injection ─────────────────────────────────────────────

        drain_input_queue(handle, input_queue);

        // ── Per-iteration callback ──────────────────────────────────────
        // Called every iteration for app-specific periodic work
        // (framebuffer updates, audio sample draining, etc.)

        handler.on_tick(handle);
    }
}
