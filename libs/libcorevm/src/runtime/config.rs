//! VM runtime configuration and input event types.
//!
//! [`VmRuntimeConfig`] controls which subsystems the runtime polls each
//! iteration and sets timing parameters. [`InputEvent`] represents input
//! that applications inject into the VM (keyboard, mouse, serial).

use std::time::Duration;

/// Configuration for the VM runtime execution loop.
///
/// Built by the application before calling [`VmRuntime::new`]. All fields
/// have sensible defaults via [`Default`].
///
/// # Example
///
/// ```ignore
/// let config = VmRuntimeConfig {
///     handle: vm_handle,
///     num_cpus: 4,
///     usb_tablet: true,
///     audio_enabled: true,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct VmRuntimeConfig {
    /// VM handle returned by `corevm_create`.
    pub handle: u64,

    /// Number of vCPUs (1-32). vCPU 0 is the BSP, 1+ are APs.
    pub num_cpus: u32,

    /// Poll UHCI USB frames on each I/O exit.
    /// Enable when USB tablet is configured for absolute mouse positioning.
    pub usb_tablet: bool,

    /// Process AC97 audio DMA periodically (~every 10ms).
    pub audio_enabled: bool,

    /// Poll network backend each iteration.
    pub net_enabled: bool,

    /// Process VirtIO GPU virtqueue commands.
    pub virtio_gpu: bool,

    /// Process VirtIO Input events.
    pub virtio_input: bool,

    /// Enable diagnostic event emission (CPU state dumps, exit counts).
    pub diagnostics: bool,

    /// Cancel interval: how often to kick vCPUs out of KVM_RUN for timer
    /// advancement and device polling. Default: 10ms on Linux, 1ms on Windows.
    pub cancel_interval: Duration,

    /// Optional execution timeout. `Duration::ZERO` means run forever.
    pub timeout: Duration,
}

impl Default for VmRuntimeConfig {
    fn default() -> Self {
        Self {
            handle: 0,
            num_cpus: 1,
            usb_tablet: false,
            audio_enabled: false,
            net_enabled: false,
            virtio_gpu: false,
            virtio_input: false,
            diagnostics: false,
            #[cfg(target_os = "windows")]
            cancel_interval: Duration::from_millis(1),
            #[cfg(not(target_os = "windows"))]
            cancel_interval: Duration::from_millis(10),
            timeout: Duration::ZERO,
        }
    }
}

/// Input events injected by the application into the VM.
///
/// Thread-safe: events are queued and drained by the BSP thread each
/// iteration. Applications call [`VmRuntime::inject_input`] from any thread.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// PS/2 keyboard scancode press.
    Ps2KeyPress(u8),

    /// PS/2 keyboard scancode release.
    Ps2KeyRelease(u8),

    /// PS/2 mouse relative movement.
    Ps2MouseMove { dx: i16, dy: i16, buttons: u8 },

    /// USB tablet absolute position (used with UHCI USB tablet device).
    UsbTabletMove { x: u16, y: u16, buttons: u8 },

    /// VirtIO keyboard PS/2 scancode (forwarded to VirtIO input device).
    VirtioKeyPs2(u8),

    /// VirtIO tablet absolute position.
    VirtioTabletMove { x: u32, y: u32, buttons: u8 },

    /// Serial port (COM1) input bytes.
    SerialInput(Vec<u8>),
}
