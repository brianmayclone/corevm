//! Engine: VM lifecycle, platform abstraction, keyboard input, diagnostics.

pub mod diagnostics;
#[cfg(target_os = "linux")]
pub mod evdev_input;
#[cfg(target_os = "linux")]
pub mod evdev_mouse;
pub mod input;
pub mod iso_detect;
pub mod platform;
pub mod vm;
