//! SMP (Symmetric Multi-Processing) support — AP vCPU threads.
//!
//! Each Application Processor (AP, vCPU 1..N-1) runs a simplified exit
//! dispatch loop. APs handle I/O and MMIO exits identically to the BSP
//! but do NOT advance timers, poll devices, or emit events. Timer and
//! device polling is the BSP's responsibility — this avoids contention
//! and keeps the model simple.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use crate::ffi::*;
use super::loop_core;
use super::RuntimeControl;

/// Spawn AP vCPU threads for CPUs 1..num_cpus-1.
///
/// Each AP thread runs [`loop_core::dispatch_exit`] in a tight loop,
/// re-entering KVM_RUN after each exit. The threads exit when
/// `control.stop` is set.
///
/// Returns a `Vec` of `JoinHandle`s for the caller to join on shutdown.
pub(crate) fn spawn_ap_threads(
    handle: u64,
    num_cpus: u32,
    control: Arc<RuntimeControl>,
) -> Vec<thread::JoinHandle<()>> {
    let mut handles = Vec::new();

    for cpu_id in 1..num_cpus {
        let ctrl = control.clone();
        let h = thread::Builder::new()
            .name(format!("vcpu-{}", cpu_id))
            .spawn(move || {
                ap_loop(handle, cpu_id, &ctrl);
            })
            .expect("failed to spawn AP thread");
        handles.push(h);
    }

    handles
}

/// AP vCPU exit-dispatch loop.
///
/// Runs until `control.stop` is set or a fatal exit (Shutdown/Error) occurs.
/// Only dispatches I/O and MMIO exits — no timer advancement or device polling.
fn ap_loop(handle: u64, cpu_id: u32, control: &RuntimeControl) {
    let mut iterations: u64 = 0;
    let mut errors: u64 = 0;
    let mut exit_counts = [0u64; 16]; // track exit reasons 0..15
    let start = std::time::Instant::now();

    loop {
        if control.stop.load(Ordering::Relaxed) {
            break;
        }

        let mut exit = CExitReason::default();
        let rc = corevm_run_vcpu(handle, cpu_id, &mut exit);

        iterations += 1;

        // Log first 20 exits in detail, then periodic summary
        if iterations <= 20 {
            eprintln!("[smp] AP{} iter={} rc={} exit.reason={} port={:#06x} addr={:#010x} size={}",
                cpu_id, iterations, rc, exit.reason, exit.port, exit.addr, exit.size);
        } else if iterations % 100_000 == 0 || (iterations < 1000 && iterations % 100 == 0) {
            let elapsed = start.elapsed().as_secs();
            eprintln!("[smp] AP{} alive: {}s iters={} errs={} exits=[io:{} mmio:{} hlt:{} cancel:{} other:{}]",
                cpu_id, elapsed, iterations, errors,
                exit_counts[0] + exit_counts[1],
                exit_counts[2] + exit_counts[3],
                exit_counts[7],
                exit_counts[13],
                iterations - exit_counts[0] - exit_counts[1] - exit_counts[2]
                    - exit_counts[3] - exit_counts[7] - exit_counts[13] - errors,
            );
        }

        if rc != 0 {
            errors += 1;
            if errors <= 10 {
                eprintln!("[smp] AP{} run_vcpu ERROR rc={} (error #{}) after {}us",
                    cpu_id, rc, errors, start.elapsed().as_micros());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }

        let reason = exit.reason as usize;
        if reason < exit_counts.len() {
            exit_counts[reason] += 1;
        }

        match loop_core::dispatch_exit(handle, cpu_id, &exit) {
            loop_core::ExitAction::Continue
            | loop_core::ExitAction::Halted => {
                // Log when AP enters HLT for the first time
                if exit.reason == 7 && exit_counts[7] <= 3 {
                    eprintln!("[smp] AP{} HLT #{} at iter={} ({}ms since start)",
                        cpu_id, exit_counts[7], iterations, start.elapsed().as_millis());
                }

                // Deliver AHCI IRQ immediately after MMIO exits from APs.
                // Without this, AHCI completion IRQs from AP-submitted
                // disk commands are delayed until the next BSP poll cycle
                // (~1ms), causing severe disk I/O latency with SMP.
                if exit.reason == 2 || exit.reason == 3 {
                    corevm_ahci_poll_irq(handle);
                }
            }
            loop_core::ExitAction::Shutdown
            | loop_core::ExitAction::Error
            | loop_core::ExitAction::Reboot => {
                eprintln!("[smp] AP{} exiting: reason={} after {} iterations", cpu_id, exit.reason, iterations);
                control.stop.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
    eprintln!("[smp] AP{} loop ended: {} iterations, {} errors", cpu_id, iterations, errors);
}
