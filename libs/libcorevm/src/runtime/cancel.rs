//! Cancel thread for periodic vCPU preemption.
//!
//! KVM_RUN blocks until a VM exit occurs. To ensure timely timer advancement
//! and device polling, a background thread periodically calls
//! `corevm_cancel_vcpu` which sets `immediate_exit = 1` on the kvm_run
//! shared page and sends SIGUSR1 to the vCPU thread. This causes KVM_RUN
//! to return with exit reason `Cancelled` (13).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crate::ffi::corevm_cancel_vcpu;
use super::RuntimeControl;

/// Spawn the cancel thread that periodically kicks the BSP out of KVM_RUN.
///
/// Only the BSP (vCPU 0) needs periodic cancel kicks — it uses them to
/// advance timers (PIT, RTC), poll device IRQs, and drain I/O buffers.
///
/// APs are NOT cancelled: they run in KVM_RUN and block on HLT until
/// an IPI arrives. Cancelling APs forces unnecessary userspace round-trips
/// that contend on the I/O dispatch lock, causing severe disk I/O
/// performance degradation with SMP.
///
/// The thread runs until `control.stop` is set to `true`.
pub(crate) fn spawn_cancel_thread(
    handle: u64,
    _num_cpus: u32,
    interval: Duration,
    control: Arc<RuntimeControl>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("vm-cancel".into())
        .spawn(move || {
            while !control.stop.load(Ordering::Relaxed) {
                thread::sleep(interval);
                // Only cancel BSP (vCPU 0) — APs don't need periodic kicks.
                corevm_cancel_vcpu(handle, 0);
            }
        })
        .expect("failed to spawn cancel thread")
}
