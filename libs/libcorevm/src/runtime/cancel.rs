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

/// Spawn the cancel thread that periodically kicks all vCPUs out of KVM_RUN.
///
/// The thread runs until `control.stop` is set to `true`.
///
/// # Arguments
///
/// * `handle` - VM handle for `corevm_cancel_vcpu` calls
/// * `num_cpus` - Number of vCPUs to cancel each interval
/// * `interval` - Time between cancel kicks
/// * `control` - Shared control flags (stop signal)
pub(crate) fn spawn_cancel_thread(
    handle: u64,
    num_cpus: u32,
    interval: Duration,
    control: Arc<RuntimeControl>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("vm-cancel".into())
        .spawn(move || {
            while !control.stop.load(Ordering::Relaxed) {
                thread::sleep(interval);
                for cpu_id in 0..num_cpus {
                    corevm_cancel_vcpu(handle, cpu_id);
                }
            }
        })
        .expect("failed to spawn cancel thread")
}
