use eframe::egui;

/// Map egui Key to PS/2 scancode set 1 (default set used by the PS/2 controller).
/// Returns (scancode, is_extended) where is_extended means E0 prefix needed.
pub fn scancode_for_key(key: egui::Key) -> Option<(u8, bool)> {
    match key {
        egui::Key::A => Some((0x1E, false)),
        egui::Key::B => Some((0x30, false)),
        egui::Key::C => Some((0x2E, false)),
        egui::Key::D => Some((0x20, false)),
        egui::Key::E => Some((0x12, false)),
        egui::Key::F => Some((0x21, false)),
        egui::Key::G => Some((0x22, false)),
        egui::Key::H => Some((0x23, false)),
        egui::Key::I => Some((0x17, false)),
        egui::Key::J => Some((0x24, false)),
        egui::Key::K => Some((0x25, false)),
        egui::Key::L => Some((0x26, false)),
        egui::Key::M => Some((0x32, false)),
        egui::Key::N => Some((0x31, false)),
        egui::Key::O => Some((0x18, false)),
        egui::Key::P => Some((0x19, false)),
        egui::Key::Q => Some((0x10, false)),
        egui::Key::R => Some((0x13, false)),
        egui::Key::S => Some((0x1F, false)),
        egui::Key::T => Some((0x14, false)),
        egui::Key::U => Some((0x16, false)),
        egui::Key::V => Some((0x2F, false)),
        egui::Key::W => Some((0x11, false)),
        egui::Key::X => Some((0x2D, false)),
        egui::Key::Y => Some((0x15, false)),
        egui::Key::Z => Some((0x2C, false)),
        egui::Key::Num0 => Some((0x0B, false)),
        egui::Key::Num1 => Some((0x02, false)),
        egui::Key::Num2 => Some((0x03, false)),
        egui::Key::Num3 => Some((0x04, false)),
        egui::Key::Num4 => Some((0x05, false)),
        egui::Key::Num5 => Some((0x06, false)),
        egui::Key::Num6 => Some((0x07, false)),
        egui::Key::Num7 => Some((0x08, false)),
        egui::Key::Num8 => Some((0x09, false)),
        egui::Key::Num9 => Some((0x0A, false)),
        egui::Key::Enter => Some((0x1C, false)),
        egui::Key::Escape => Some((0x01, false)),
        egui::Key::Backspace => Some((0x0E, false)),
        egui::Key::Tab => Some((0x0F, false)),
        egui::Key::Space => Some((0x39, false)),
        egui::Key::ArrowLeft => Some((0x4B, true)),
        egui::Key::ArrowRight => Some((0x4D, true)),
        egui::Key::ArrowUp => Some((0x48, true)),
        egui::Key::ArrowDown => Some((0x50, true)),
        egui::Key::F1 => Some((0x3B, false)),
        egui::Key::F2 => Some((0x3C, false)),
        egui::Key::F3 => Some((0x3D, false)),
        egui::Key::F4 => Some((0x3E, false)),
        egui::Key::F5 => Some((0x3F, false)),
        egui::Key::F6 => Some((0x40, false)),
        egui::Key::F7 => Some((0x41, false)),
        egui::Key::F8 => Some((0x42, false)),
        egui::Key::F9 => Some((0x43, false)),
        egui::Key::F10 => Some((0x44, false)),
        egui::Key::F11 => Some((0x57, false)),
        egui::Key::F12 => Some((0x58, false)),
        egui::Key::Delete => Some((0x53, true)),
        egui::Key::Home => Some((0x47, true)),
        egui::Key::End => Some((0x4F, true)),
        egui::Key::PageUp => Some((0x49, true)),
        egui::Key::PageDown => Some((0x51, true)),
        egui::Key::Insert => Some((0x52, true)),
        // Punctuation / symbols (Set 1)
        egui::Key::Minus => Some((0x0C, false)),
        egui::Key::Equals => Some((0x0D, false)),
        egui::Key::OpenBracket => Some((0x1A, false)),
        egui::Key::CloseBracket => Some((0x1B, false)),
        egui::Key::Backslash => Some((0x2B, false)),
        egui::Key::Semicolon => Some((0x27, false)),
        egui::Key::Quote => Some((0x28, false)),
        egui::Key::Backtick => Some((0x29, false)),
        egui::Key::Comma => Some((0x33, false)),
        egui::Key::Period => Some((0x34, false)),
        egui::Key::Slash => Some((0x35, false)),
        _ => None,
    }
}

/// Track modifier key state to detect press/release transitions.
static mut PREV_SHIFT: bool = false;
static mut PREV_CTRL: bool = false;
static mut PREV_ALT: bool = false;
/// Track whether we sent Right Alt (AltGr) as E0+38 instead of plain 38.
static mut PREV_ALTGR: bool = false;
/// Track which keys are currently pressed to prevent duplicate press/release.
static mut KEYS_DOWN: [bool; 256] = [false; 256];

/// Handle keyboard events from egui and send to VM via libcorevm.
/// vm_handle: the corevm VM handle (u64)
/// display_focused: whether the display area has focus (captures all keys)
/// Returns a label for the last key pressed (if any) for status bar display.
pub fn handle_keyboard_events(ctx: &egui::Context, vm_handle: u64, display_focused: bool) -> Option<String> {
    if !display_focused {
        // Release all tracked keys and modifiers when focus is lost
        unsafe {
            for i in 0..256 {
                if KEYS_DOWN[i] {
                    libcorevm::ffi::corevm_ps2_key_release(vm_handle, i as u8);
                    KEYS_DOWN[i] = false;
                }
            }
            if PREV_SHIFT { libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x2A); PREV_SHIFT = false; }
            if PREV_CTRL  { libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x1D); PREV_CTRL = false; }
            if PREV_ALT   { libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x38); PREV_ALT = false; }
            if PREV_ALTGR {
                libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0xE0);
                libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x38);
                PREV_ALTGR = false;
            }
        }
        return None;
    }

    let has_virtio_input = libcorevm::ffi::corevm_has_virtio_input(vm_handle) != 0;

    // Helper: send a PS/2 scancode (with optional E0 prefix) to both PS/2 and VirtIO.
    let send_key_press_ext = |scancode: u8, extended: bool| {
        if extended {
            libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0xE0);
        }
        libcorevm::ffi::corevm_ps2_key_press(vm_handle, scancode);
        if has_virtio_input {
            if extended {
                libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, 0xE0);
            }
            libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, scancode);
        }
    };
    let send_key_release_ext = |scancode: u8, extended: bool| {
        if extended {
            libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0xE0);
        }
        libcorevm::ffi::corevm_ps2_key_release(vm_handle, scancode);
        if has_virtio_input {
            if extended {
                libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, 0xE0);
            }
            libcorevm::ffi::corevm_virtio_kbd_ps2(vm_handle, scancode | 0x80);
        }
    };
    let send_key_press = |scancode: u8| { send_key_press_ext(scancode, false); };
    let send_key_release = |scancode: u8| { send_key_release_ext(scancode, false); };

    // Track modifier state from egui and send press/release scancodes.
    // Detect AltGr: on Linux/X11, AltGr is reported as ctrl+alt simultaneously.
    // We detect this pattern and send Right Alt (E0 0x38) instead of Left Ctrl + Left Alt.
    let modifiers = ctx.input(|i| i.modifiers);

    // Check if this looks like AltGr (both ctrl and alt pressed simultaneously,
    // but no actual Ctrl key intent — i.e. the user pressed AltGr, not Ctrl+Alt).
    // Heuristic: if both ctrl and alt become true in the same frame and we weren't
    // tracking them individually, it's likely AltGr.
    let is_altgr = modifiers.ctrl && modifiers.alt;

    unsafe {
        if is_altgr && !PREV_ALTGR {
            // AltGr pressed — send Right Alt (E0 + 0x38)
            // First release any previously sent Left Ctrl/Alt
            if PREV_CTRL { send_key_release(0x1D); PREV_CTRL = false; }
            if PREV_ALT { send_key_release(0x38); PREV_ALT = false; }
            send_key_press_ext(0x38, true); // Right Alt = E0 0x38
            PREV_ALTGR = true;
        } else if !is_altgr && PREV_ALTGR {
            // AltGr released
            send_key_release_ext(0x38, true);
            PREV_ALTGR = false;
        }

        if !PREV_ALTGR {
            // Normal modifier handling (only when not in AltGr mode)
            if modifiers.shift && !PREV_SHIFT {
                send_key_press(0x2A); // Left Shift
                PREV_SHIFT = true;
            } else if !modifiers.shift && PREV_SHIFT {
                send_key_release(0x2A);
                PREV_SHIFT = false;
            }
            if modifiers.ctrl && !PREV_CTRL {
                send_key_press(0x1D); // Left Ctrl
                PREV_CTRL = true;
            } else if !modifiers.ctrl && PREV_CTRL {
                send_key_release(0x1D);
                PREV_CTRL = false;
            }
            if modifiers.alt && !PREV_ALT {
                send_key_press(0x38); // Left Alt
                PREV_ALT = true;
            } else if !modifiers.alt && PREV_ALT {
                send_key_release(0x38);
                PREV_ALT = false;
            }
        }
    }

    // Use input_mut to both process AND remove key events in one pass.
    // This must be called BEFORE any egui widgets are drawn, so egui
    // never sees Enter/Tab/etc. for its own navigation.
    let mut last_key: Option<String> = None;
    ctx.input_mut(|i| {
        for event in &i.events {
            match event {
                egui::Event::Key { key, pressed, repeat, .. } => {
                    if let Some((scancode, extended)) = scancode_for_key(*key) {
                        let idx = scancode as usize;
                        unsafe {
                            if *pressed {
                                // Only send press if not already down (dedup repeats)
                                if !KEYS_DOWN[idx] {
                                    KEYS_DOWN[idx] = true;
                                    send_key_press_ext(scancode, extended);
                                    last_key = Some(format!("{:?} (0x{:02X})", key, scancode));
                                }
                            } else {
                                // Only send release if currently down
                                if KEYS_DOWN[idx] {
                                    KEYS_DOWN[idx] = false;
                                    send_key_release_ext(scancode, extended);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
    });
    last_key
}

/// Map ASCII characters to PS/2 scancode set 1 (for characters not covered by egui::Key)
fn scancode_for_char(ch: char) -> Option<u8> {
    match ch {
        'a'..='z' => {
            let idx = (ch as u8) - b'a';
            let scancodes: [u8; 26] = [
                0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24,
                0x25, 0x26, 0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14,
                0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
            ];
            Some(scancodes[idx as usize])
        }
        '0' => Some(0x0B),
        '1'..='9' => Some(0x02 + (ch as u8 - b'1')),
        ' ' => Some(0x39),
        '-' => Some(0x0C),
        '=' => Some(0x0D),
        '[' => Some(0x1A),
        ']' => Some(0x1B),
        '\\' => Some(0x2B),
        ';' => Some(0x27),
        '\'' => Some(0x28),
        '`' => Some(0x29),
        ',' => Some(0x33),
        '.' => Some(0x34),
        '/' => Some(0x35),
        _ => None,
    }
}

/// Map characters that require Shift to their base scancode.
fn char_to_scancode(ch: char) -> Option<(u8, bool)> {
    // Uppercase letters
    if ch.is_ascii_uppercase() {
        return scancode_for_char(ch.to_ascii_lowercase()).map(|sc| (sc, true));
    }
    // Check if it's a direct (unshifted) character
    if let Some(sc) = scancode_for_char(ch) {
        return Some((sc, false));
    }
    // Shifted symbols
    match ch {
        '!' => Some((0x02, true)),
        '@' => Some((0x03, true)),
        '#' => Some((0x04, true)),
        '$' => Some((0x05, true)),
        '%' => Some((0x06, true)),
        '^' => Some((0x07, true)),
        '&' => Some((0x08, true)),
        '*' => Some((0x09, true)),
        '(' => Some((0x0A, true)),
        ')' => Some((0x0B, true)),
        '_' => Some((0x0C, true)),
        '+' => Some((0x0D, true)),
        '{' => Some((0x1A, true)),
        '}' => Some((0x1B, true)),
        '|' => Some((0x2B, true)),
        ':' => Some((0x27, true)),
        '"' => Some((0x28, true)),
        '~' => Some((0x29, true)),
        '<' => Some((0x33, true)),
        '>' => Some((0x34, true)),
        '?' => Some((0x35, true)),
        _ => None,
    }
}

/// Type a string into the VM by injecting PS/2 scancodes.
pub fn type_string_to_vm(vm_handle: u64, text: &str) {
    for ch in text.chars() {
        if let Some((scancode, needs_shift)) = char_to_scancode(ch) {
            if needs_shift {
                libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0x2A); // Left Shift
            }
            libcorevm::ffi::corevm_ps2_key_press(vm_handle, scancode);
            libcorevm::ffi::corevm_ps2_key_release(vm_handle, scancode);
            if needs_shift {
                libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x2A);
            }
        } else if ch == '\n' {
            libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0x1C);
            libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x1C);
        } else if ch == '\t' {
            libcorevm::ffi::corevm_ps2_key_press(vm_handle, 0x0F);
            libcorevm::ffi::corevm_ps2_key_release(vm_handle, 0x0F);
        }
    }
}
