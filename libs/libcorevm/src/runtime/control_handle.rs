//! Thread-safe handle for controlling a running VmRuntime from another thread.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use alloc::string::String;

use super::RuntimeControl;

/// Clonable, Send+Sync handle that lets any thread stop/pause/resume the VM
/// and query its exit state.
///
/// Obtained via [`VmRuntime::control_handle`](super::VmRuntime::control_handle)
/// **before** moving the runtime into a worker thread.
#[derive(Clone)]
pub struct VmControlHandle {
    pub(super) control: Arc<RuntimeControl>,
    handle: u64,
    num_cpus: u32,
}

impl VmControlHandle {
    pub(super) fn new(control: Arc<RuntimeControl>, handle: u64, num_cpus: u32) -> Self {
        Self { control, handle, num_cpus }
    }

    /// Signal all vCPU threads to stop and kick them out of KVM_RUN.
    pub fn request_stop(&self) {
        self.control.stop.store(true, Ordering::Relaxed);
        for cpu_id in 0..self.num_cpus {
            crate::ffi::corevm_cancel_vcpu(self.handle, cpu_id);
        }
    }

    /// Pause VM execution.
    pub fn request_pause(&self) {
        self.control.pause.store(true, Ordering::Relaxed);
    }

    /// Resume VM execution.
    pub fn request_resume(&self) {
        self.control.pause.store(false, Ordering::Relaxed);
    }

    /// Returns `true` if the VM has exited (shutdown, error, or reboot).
    pub fn is_exited(&self) -> bool {
        self.control.exited.load(Ordering::Relaxed)
    }

    /// Returns `true` if the guest requested a reboot.
    pub fn reboot_requested(&self) -> bool {
        self.control.reboot_requested.load(Ordering::Relaxed)
    }

    /// Get the exit reason string.
    pub fn exit_reason(&self) -> String {
        self.control.exit_reason.lock().unwrap().clone()
    }

    /// Set the exit reason string (used by event handlers).
    pub fn set_exit_reason(&self, reason: String) {
        if let Ok(mut r) = self.control.exit_reason.lock() {
            *r = reason;
        }
    }

    /// Mark the VM as exited.
    pub fn set_exited(&self) {
        self.control.exited.store(true, Ordering::Relaxed);
    }

    /// Mark a reboot as requested.
    pub fn set_reboot_requested(&self) {
        self.control.reboot_requested.store(true, Ordering::Relaxed);
    }
}
