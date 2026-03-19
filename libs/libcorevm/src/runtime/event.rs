//! VM runtime events and event handler trait.
//!
//! The runtime emits [`VmEvent`]s during execution. Applications implement
//! [`EventHandler`] to receive serial output, framebuffer updates, shutdown
//! notifications, and diagnostic messages.
//!
//! # Thread Safety
//!
//! `EventHandler::on_event` is called from the BSP vCPU thread only.
//! Implementations that need to forward events to other threads (e.g. a GUI
//! thread) should use channels or shared atomic state internally.

use alloc::string::String;
use alloc::vec::Vec;

/// Events emitted by the VM runtime during execution.
#[derive(Debug)]
pub enum VmEvent {
    /// Serial port (COM1) output bytes.
    SerialOutput(Vec<u8>),

    /// Debug port (0xE9 / 0x402) output bytes.
    DebugOutput(Vec<u8>),

    /// VM shut down cleanly (ACPI shutdown or triple fault).
    Shutdown,

    /// VM encountered a fatal error (emulation failure).
    Error { message: String },

    /// Guest requested a system reset (port 0xCF9 or PS/2 keyboard controller).
    RebootRequested,

    /// Diagnostic / status information (CPU state, exit counts, etc.).
    /// Only emitted when diagnostics are enabled in the config.
    Diagnostic(String),
}

/// Trait for receiving VM runtime events.
///
/// Applications implement this to handle serial output, shutdown notifications,
/// etc. The trait requires `Send + 'static` because the handler is moved into
/// the BSP thread.
///
/// # Example
///
/// ```ignore
/// struct HeadlessHandler;
///
/// impl EventHandler for HeadlessHandler {
///     fn on_event(&mut self, event: VmEvent) {
///         match event {
///             VmEvent::SerialOutput(data) => {
///                 let s = String::from_utf8_lossy(&data);
///                 print!("{}", s);
///             }
///             VmEvent::Shutdown => eprintln!("VM shutdown"),
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait EventHandler: Send + 'static {
    /// Called when the VM runtime produces an event.
    fn on_event(&mut self, event: VmEvent);

    /// Called once per BSP loop iteration, after exit dispatch and device polling.
    ///
    /// Use this for periodic work like framebuffer updates (~60fps) or audio
    /// sample draining. The VM handle is provided for direct FFI calls
    /// (e.g. `corevm_vga_get_framebuffer`).
    ///
    /// Default implementation does nothing.
    fn on_tick(&mut self, _handle: u64) {}
}

/// A no-op event handler that discards all events.
///
/// Useful for testing or when no event handling is needed.
pub struct NullEventHandler;

impl EventHandler for NullEventHandler {
    fn on_event(&mut self, _event: VmEvent) {}
    fn on_tick(&mut self, _handle: u64) {}
}
