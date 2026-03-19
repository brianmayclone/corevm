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
    loop {
        if control.stop.load(Ordering::Relaxed) {
            break;
        }

        let mut exit = CExitReason::default();
        let rc = corevm_run_vcpu(handle, cpu_id, &mut exit);

        if rc != 0 {
            // Run error — brief sleep and retry (backend may be resetting).
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }

        match loop_core::dispatch_exit(handle, cpu_id, &exit) {
            loop_core::ExitAction::Continue => {}
            loop_core::ExitAction::Shutdown
            | loop_core::ExitAction::Error
            | loop_core::ExitAction::Reboot => {
                control.stop.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
}
