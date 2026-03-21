//! Translate browser keyboard/mouse events to libcorevm input calls.

use serde::Deserialize;

/// Input event from WebSocket client.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ConsoleInput {
    #[serde(rename = "key")]
    Key { code: u8, pressed: bool },

    #[serde(rename = "mouse_move")]
    MouseMove { x: u32, y: u32, buttons: u8 },

    #[serde(rename = "mouse_rel")]
    MouseRel { dx: i32, dy: i32, buttons: u8 },

    #[serde(rename = "mouse_wheel")]
    MouseWheel { delta: i32 },

    #[serde(rename = "ctrl_alt_del")]
    CtrlAltDel,

    #[serde(rename = "set_fps")]
    SetFps { fps: u32 },
}

/// Inject a console input event into the VM.
/// Sends to both USB tablet (absolute) and PS/2 (relative) —
/// the VM ignores whichever device isn't configured.
pub fn inject_input(handle: u64, input: &ConsoleInput, fb_width: u32, fb_height: u32) {
    use libcorevm::ffi::*;

    match input {
        ConsoleInput::Key { code, pressed } => {
            if *pressed {
                corevm_ps2_key_press(handle, *code);
            } else {
                corevm_ps2_key_release(handle, *code);
            }
        }
        ConsoleInput::MouseMove { x, y, buttons } => {
            // Try USB tablet (absolute positioning) — works if UHCI is enabled
            if fb_width > 0 && fb_height > 0 {
                let abs_x = ((*x as u64) * 32767 / fb_width as u64) as u16;
                let abs_y = ((*y as u64) * 32767 / fb_height as u64) as u16;
                corevm_usb_tablet_move(handle, abs_x, abs_y, *buttons);
            }
        }
        ConsoleInput::MouseRel { dx, dy, buttons } => {
            // PS/2 relative mouse — always works
            corevm_ps2_mouse_move(handle, *dx as i16, *dy as i16, *buttons);
        }
        ConsoleInput::MouseWheel { delta } => {
            // Try USB tablet wheel first, then PS/2 with wheel
            corevm_usb_tablet_move_wheel(handle, 0, 0, 0, *delta as i8);
        }
        ConsoleInput::CtrlAltDel => {
            corevm_ps2_key_press(handle, 0x1D);
            corevm_ps2_key_press(handle, 0x38);
            corevm_ps2_key_press(handle, 0x53);
            corevm_ps2_key_release(handle, 0x53);
            corevm_ps2_key_release(handle, 0x38);
            corevm_ps2_key_release(handle, 0x1D);
        }
        ConsoleInput::SetFps { .. } => {}
    }
}
