//! VM runtime — unified execution engine for all frontends.
//!
//! This module provides [`VmRuntime`], a self-contained VM execution engine
//! that manages vCPU threads, timer advancement, device polling, and I/O
//! dispatch. Applications configure it via [`VmRuntimeConfig`], receive
//! events via [`EventHandler`], and inject input via [`InputEvent`].
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────┐
//! │                    Application                      │
//! │  (vmctl CLI / vmmanager GUI / custom frontend)      │
//! │                                                     │
//! │  ┌──────────────┐   ┌─────────────────────────────┐│
//! │  │ VmRuntimeConfig  │   │ impl EventHandler         ││
//! │  │ (what to poll,│   │ (serial→stdout, fb→window)  ││
//! │  │  num_cpus,    │   │                             ││
//! │  │  usb, audio)  │   └─────────────────────────────┘│
//! │  └──────┬────────┘                                  │
//! │         │                                           │
//! │  ┌──────▼───────────────────────────────────────┐   │
//! │  │            VmRuntime                          │   │
//! │  │  .start()  .inject_input()  .request_stop()  │   │
//! │  └──────────────────────────────────────────────┘   │
//! └─────────────────────┬───────────────────────────────┘
//!                       │
//! ┌─────────────────────▼───────────────────────────────┐
//! │                   libcorevm                          │
//! │                                                     │
//! │  ┌─────────┐  ┌─────────┐  ┌──────────────────────┐│
//! │  │  BSP    │  │  AP     │  │  Cancel thread       ││
//! │  │  thread │  │  threads│  │  (periodic kick)     ││
//! │  │  (vCPU0)│  │  (1..N) │  │                      ││
//! │  └────┬────┘  └────┬────┘  └──────────────────────┘│
//! │       │            │                                │
//! │  ┌────▼────────────▼────────────────────────────┐   │
//! │  │  loop_core::dispatch_exit()                   │   │
//! │  │  (IO / MMIO / StringIO / HLT / Shutdown)      │   │
//! │  └───────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use libcorevm::runtime::*;
//!
//! // 1. Create VM via FFI (corevm_create, setup devices, etc.)
//! let handle = corevm_create(512);
//! // ... setup devices, load BIOS ...
//!
//! // 2. Configure runtime
//! let config = VmRuntimeConfig {
//!     handle,
//!     num_cpus: 4,
//!     usb_tablet: true,
//!     audio_enabled: true,
//!     ..Default::default()
//! };
//!
//! // 3. Create runtime with event handler
//! let mut runtime = VmRuntime::new(config, MyHandler::new());
//!
//! // 4. Start execution (spawns vCPU + cancel threads)
//! runtime.start();
//!
//! // 5. Inject input from UI thread
//! runtime.inject_ps2_key_press(0x1C); // Enter
//!
//! // 6. Wait for completion
//! runtime.wait();
//! ```

pub mod config;
pub mod event;
pub(crate) mod loop_core;
pub(crate) mod smp;
pub(crate) mod cancel;

pub use config::{VmRuntimeConfig, InputEvent};
pub use event::{EventHandler, VmEvent, NullEventHandler};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;

use alloc::string::String;
use alloc::vec::Vec;

// ── Internal shared state ───────────────────────────────────────────────────

/// Thread-safe control flags shared between all runtime threads.
pub(crate) struct RuntimeControl {
    /// Signal all threads to stop.
    pub stop: AtomicBool,
    /// Pause execution (BSP sleeps instead of running vCPU).
    pub pause: AtomicBool,
    /// Set when the VM has exited (shutdown, error, or reboot).
    pub exited: AtomicBool,
    /// Human-readable exit reason.
    pub exit_reason: Mutex<String>,
    /// Set when the guest requested a reboot.
    pub reboot_requested: AtomicBool,
}

impl RuntimeControl {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            pause: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            exit_reason: Mutex::new(String::new()),
            reboot_requested: AtomicBool::new(false),
        }
    }
}

// ── VmRuntime ───────────────────────────────────────────────────────────────

/// Unified VM execution engine.
///
/// Owns the execution threads and provides a thread-safe API for input
/// injection and lifecycle control. See [module-level documentation](self)
/// for architecture overview and usage examples.
pub struct VmRuntime {
    config: VmRuntimeConfig,
    control: Arc<RuntimeControl>,
    input_queue: Arc<Mutex<Vec<InputEvent>>>,
    handler: Option<Box<dyn EventHandler>>,
    bsp_thread: Option<thread::JoinHandle<()>>,
    ap_threads: Vec<thread::JoinHandle<()>>,
    cancel_thread: Option<thread::JoinHandle<()>>,
}

impl VmRuntime {
    /// Create a new VM runtime with the given configuration and event handler.
    ///
    /// Does NOT start execution — call [`start`](Self::start) to spawn threads.
    pub fn new(config: VmRuntimeConfig, handler: impl EventHandler) -> Self {
        Self {
            config,
            control: Arc::new(RuntimeControl::new()),
            input_queue: Arc::new(Mutex::new(Vec::new())),
            handler: Some(Box::new(handler)),
            bsp_thread: None,
            ap_threads: Vec::new(),
            cancel_thread: None,
        }
    }

    /// Start VM execution.
    ///
    /// Spawns the BSP thread (vCPU 0), AP threads (vCPU 1..N-1), and the
    /// cancel timer thread. The BSP thread runs the canonical VM loop from
    /// [`loop_core::bsp_loop`].
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn start(&mut self) {
        assert!(self.bsp_thread.is_none(), "VmRuntime::start() called twice");

        let handle = self.config.handle;
        let num_cpus = self.config.num_cpus;

        // Install SIGUSR1 handler for cancel support (Linux/KVM only).
        #[cfg(target_os = "linux")]
        crate::backend::kvm::install_sigusr1_handler();

        // Spawn cancel timer thread.
        self.cancel_thread = Some(cancel::spawn_cancel_thread(
            handle,
            num_cpus,
            self.config.cancel_interval,
            self.control.clone(),
        ));

        // Spawn AP threads (vCPU 1..N-1).
        if num_cpus > 1 {
            self.ap_threads = smp::spawn_ap_threads(
                handle,
                num_cpus,
                self.control.clone(),
            );
        }

        // Spawn BSP thread (vCPU 0).
        let config = self.config.clone();
        let control = self.control.clone();
        let input_queue = self.input_queue.clone();
        let mut handler = self.handler.take().expect("handler already consumed");

        self.bsp_thread = Some(
            thread::Builder::new()
                .name("vcpu-0".into())
                .spawn(move || {
                    loop_core::bsp_loop(&config, &control, handler.as_mut(), &input_queue);
                })
                .expect("failed to spawn BSP thread"),
        );
    }

    // ── Lifecycle control ───────────────────────────────────────────────

    /// Signal all threads to stop and exit the VM loop.
    pub fn request_stop(&self) {
        self.control.stop.store(true, Ordering::Relaxed);
        // Kick vCPUs to wake them from KVM_RUN.
        for cpu_id in 0..self.config.num_cpus {
            crate::ffi::corevm_cancel_vcpu(self.config.handle, cpu_id);
        }
    }

    /// Pause VM execution. The BSP thread sleeps instead of running the vCPU.
    pub fn request_pause(&self) {
        self.control.pause.store(true, Ordering::Relaxed);
    }

    /// Resume VM execution after a pause.
    pub fn request_resume(&self) {
        self.control.pause.store(false, Ordering::Relaxed);
    }

    /// Block until all threads have exited, consuming the runtime.
    ///
    /// Call [`request_stop`](Self::request_stop) first, or wait for the VM
    /// to exit naturally (shutdown, error, reboot).
    pub fn wait(mut self) {
        if let Some(t) = self.bsp_thread.take() {
            let _ = t.join();
        }
        for t in self.ap_threads.drain(..) {
            let _ = t.join();
        }
        // Stop cancel thread after vCPU threads are done.
        self.control.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.cancel_thread.take() {
            let _ = t.join();
        }
    }

    /// Returns `true` if the VM is still running (threads spawned, not exited).
    pub fn is_running(&self) -> bool {
        self.bsp_thread.is_some() && !self.control.exited.load(Ordering::Relaxed)
    }

    /// Returns `true` if the VM has exited (shutdown, error, or reboot).
    pub fn is_exited(&self) -> bool {
        self.control.exited.load(Ordering::Relaxed)
    }

    /// Returns the human-readable exit reason, or an empty string if still running.
    pub fn exit_reason(&self) -> String {
        self.control.exit_reason.lock().unwrap().clone()
    }

    /// Returns `true` if the guest requested a system reboot.
    pub fn reboot_requested(&self) -> bool {
        self.control.reboot_requested.load(Ordering::Relaxed)
    }

    // ── Input injection (thread-safe) ───────────────────────────────────

    /// Queue an input event for processing by the BSP thread.
    ///
    /// Can be called from any thread. Events are drained each BSP iteration.
    pub fn inject_input(&self, event: InputEvent) {
        self.input_queue.lock().unwrap().push(event);
    }

    /// Convenience: inject a PS/2 key press scancode.
    pub fn inject_ps2_key_press(&self, scancode: u8) {
        self.inject_input(InputEvent::Ps2KeyPress(scancode));
    }

    /// Convenience: inject a PS/2 key release scancode.
    pub fn inject_ps2_key_release(&self, scancode: u8) {
        self.inject_input(InputEvent::Ps2KeyRelease(scancode));
    }

    /// Convenience: inject a PS/2 mouse relative movement.
    pub fn inject_mouse_move(&self, dx: i16, dy: i16, buttons: u8) {
        self.inject_input(InputEvent::Ps2MouseMove { dx, dy, buttons });
    }

    /// Convenience: inject serial port (COM1) input bytes.
    pub fn inject_serial_input(&self, data: &[u8]) {
        self.inject_input(InputEvent::SerialInput(data.to_vec()));
    }

    /// Convenience: inject a USB tablet absolute position.
    pub fn inject_usb_tablet_move(&self, x: u16, y: u16, buttons: u8) {
        self.inject_input(InputEvent::UsbTabletMove { x, y, buttons });
    }

    /// Convenience: inject a VirtIO tablet absolute position.
    pub fn inject_virtio_tablet_move(&self, x: u32, y: u32, buttons: u8) {
        self.inject_input(InputEvent::VirtioTabletMove { x, y, buttons });
    }

    /// Get the VM handle (for direct FFI calls not covered by the runtime API).
    pub fn handle(&self) -> u64 {
        self.config.handle
    }
}

impl Drop for VmRuntime {
    fn drop(&mut self) {
        // Ensure threads are stopped on drop.
        self.control.stop.store(true, Ordering::Relaxed);
        for cpu_id in 0..self.config.num_cpus {
            crate::ffi::corevm_cancel_vcpu(self.config.handle, cpu_id);
        }
    }
}
