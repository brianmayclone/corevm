//! Low-level evdev input reader for Linux (mouse + keyboard).
//!
//! Reads raw `input_event` structs directly from `/dev/input/eventX` to get
//! reliable relative mouse deltas and keyboard key-down/key-up events,
//! independent of the windowing system.
//!
//! This solves two problems:
//! 1. CursorGrab::Locked not delivering pointer.delta() on some X11/Wayland backends
//! 2. egui suppressing key-repeat events, preventing held keys from repeating in the guest
//!
//! Requires read access to `/dev/input/eventX` devices (user must be in the
//! `input` group). Use `check_access()` at startup and `grant_access()` to
//! add the user to the group via pkexec if needed.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

// Linux input event types
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;

// Relative axis codes
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;

// ioctl constants for capability queries
const EVIOCGBIT_EV: libc::c_ulong = 0x80084520;  // EVIOCGBIT(0, 32)
const EVIOCGBIT_REL: libc::c_ulong = 0x80084522;  // EVIOCGBIT(EV_REL, 32)
const EV_REL_BIT: u8 = 1 << EV_REL;
const EV_KEY_BIT: u8 = 1 << EV_KEY;

/// A keyboard event from evdev: (scancode_ps2, is_extended, pressed)
/// scancode_ps2 is the PS/2 Set 1 scancode (without E0 prefix).
/// is_extended means the E0 prefix must be sent before the scancode.
#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub scancode: u8,
    pub extended: bool,
    pub pressed: bool,
}

/// Check if the current user can read `/dev/input/event*` devices.
pub fn check_access() -> bool {
    for i in 0..32 {
        let path = format!("/dev/input/event{}", i);
        let c_path = match std::ffi::CString::new(path.as_str()) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd >= 0 {
            unsafe { libc::close(fd); }
            return true;
        }
    }
    false
}

/// Add the current user to the `input` group via pkexec (shows polkit auth dialog).
/// Returns Ok(true) if successful, Ok(false) if user cancelled, Err on failure.
pub fn grant_access() -> Result<bool, String> {
    let username = std::env::var("USER")
        .map_err(|_| "Could not determine current username".to_string())?;

    let status = std::process::Command::new("pkexec")
        .args(["usermod", "-aG", "input", &username])
        .status()
        .map_err(|e| format!("Failed to run pkexec: {}", e))?;

    if status.success() {
        Ok(true)
    } else if status.code() == Some(126) {
        Ok(false)
    } else {
        Err(format!("pkexec usermod failed with exit code: {:?}", status.code()))
    }
}

/// Shared state between the reader thread and the UI thread.
struct EvdevAccum {
    // Mouse deltas
    dx: AtomicI32,
    dy: AtomicI32,
    wheel: AtomicI32,
    // Keyboard events queue
    key_events: Mutex<Vec<KeyEvent>>,
    // Modifier state tracked by keyboard thread (for release combo detection)
    ctrl_down: AtomicBool,
    alt_down: AtomicBool,
    // Set by keyboard thread when Ctrl+Alt+G/F/Esc is detected
    release_combo: AtomicBool,
    // Thread control
    running: AtomicBool,
}

/// Combined evdev input reader for mouse and keyboard.
/// Opens separate devices for mouse (REL_X+REL_Y) and keyboard (EV_KEY),
/// reads both in background threads, accumulates events for polling.
pub struct EvdevInputReader {
    accum: Arc<EvdevAccum>,
    mouse_thread: Option<std::thread::JoinHandle<()>>,
    kbd_thread: Option<std::thread::JoinHandle<()>>,
    mouse_fd: i32,
    kbd_fd: i32,
}

impl EvdevInputReader {
    /// Find and open mouse + keyboard devices.
    /// Returns None if neither device can be opened.
    pub fn open() -> Option<Self> {
        let mouse_fd = find_mouse_device().unwrap_or(-1);
        let kbd_fd = find_keyboard_device().unwrap_or(-1);

        if mouse_fd < 0 && kbd_fd < 0 {
            return None;
        }

        if mouse_fd >= 0 {
            eprintln!("[evdev] Opened mouse device fd={}", mouse_fd);
        }
        if kbd_fd >= 0 {
            eprintln!("[evdev] Opened keyboard device fd={}", kbd_fd);
        }

        Some(Self {
            accum: Arc::new(EvdevAccum {
                dx: AtomicI32::new(0),
                dy: AtomicI32::new(0),
                wheel: AtomicI32::new(0),
                key_events: Mutex::new(Vec::new()),
                ctrl_down: AtomicBool::new(false),
                alt_down: AtomicBool::new(false),
                release_combo: AtomicBool::new(false),
                running: AtomicBool::new(false),
            }),
            mouse_thread: None,
            kbd_thread: None,
            mouse_fd,
            kbd_fd,
        })
    }

    /// Start the background reader threads.
    pub fn start(&mut self) {
        if self.accum.running.load(Ordering::Relaxed) {
            return;
        }
        self.accum.running.store(true, Ordering::SeqCst);
        self.accum.dx.store(0, Ordering::Relaxed);
        self.accum.dy.store(0, Ordering::Relaxed);
        self.accum.wheel.store(0, Ordering::Relaxed);
        self.accum.ctrl_down.store(false, Ordering::Relaxed);
        self.accum.alt_down.store(false, Ordering::Relaxed);
        self.accum.release_combo.store(false, Ordering::Relaxed);
        if let Ok(mut keys) = self.accum.key_events.lock() {
            keys.clear();
        }

        if self.mouse_fd >= 0 {
            let accum = self.accum.clone();
            let fd = self.mouse_fd;
            self.mouse_thread = Some(std::thread::spawn(move || {
                mouse_reader_loop(fd, &accum);
            }));
        }

        if self.kbd_fd >= 0 {
            let accum = self.accum.clone();
            let fd = self.kbd_fd;
            self.kbd_thread = Some(std::thread::spawn(move || {
                kbd_reader_loop(fd, &accum);
            }));
        }
    }

    /// Stop the background reader threads.
    pub fn stop(&mut self) {
        self.accum.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.mouse_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.kbd_thread.take() {
            let _ = handle.join();
        }
    }

    /// Take accumulated mouse deltas since last call (atomically resets them).
    pub fn take_mouse_deltas(&self) -> (i32, i32, i32) {
        let dx = self.accum.dx.swap(0, Ordering::Relaxed);
        let dy = self.accum.dy.swap(0, Ordering::Relaxed);
        let wheel = self.accum.wheel.swap(0, Ordering::Relaxed);
        (dx, dy, wheel)
    }

    /// Take accumulated keyboard events since last call.
    pub fn take_key_events(&self) -> Vec<KeyEvent> {
        if let Ok(mut keys) = self.accum.key_events.lock() {
            keys.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Check if the reader is running.
    pub fn is_running(&self) -> bool {
        self.accum.running.load(Ordering::Relaxed)
    }

    /// Check if the Ctrl+Alt+G/F/Esc release combo was detected.
    /// Atomically clears the flag after reading.
    pub fn check_release_combo(&self) -> bool {
        self.accum.release_combo.swap(false, Ordering::SeqCst)
    }

    /// Check if we have a mouse device.
    pub fn has_mouse(&self) -> bool {
        self.mouse_fd >= 0
    }

    /// Check if we have a keyboard device.
    pub fn has_keyboard(&self) -> bool {
        self.kbd_fd >= 0
    }
}

impl Drop for EvdevInputReader {
    fn drop(&mut self) {
        self.stop();
        if self.mouse_fd >= 0 {
            unsafe { libc::close(self.mouse_fd); }
        }
        if self.kbd_fd >= 0 {
            unsafe { libc::close(self.kbd_fd); }
        }
    }
}

/// Raw Linux `struct input_event` (24 bytes on 64-bit).
#[repr(C)]
struct InputEvent {
    _tv_sec: i64,
    _tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

// ── Mouse reader thread ──

fn mouse_reader_loop(fd: i32, accum: &EvdevAccum) {
    let event_size = std::mem::size_of::<InputEvent>();
    let mut buf = vec![0u8; event_size * 64];

    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    while accum.running.load(Ordering::Relaxed) {
        let ret = unsafe { libc::poll(&mut pollfd, 1, 50) };
        if ret <= 0 { continue; }

        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 { continue; }

        let num_events = n as usize / event_size;
        for i in 0..num_events {
            let event = unsafe { &*(buf.as_ptr().add(i * event_size) as *const InputEvent) };
            if event.type_ == EV_REL {
                match event.code {
                    REL_X => { accum.dx.fetch_add(event.value, Ordering::Relaxed); }
                    REL_Y => { accum.dy.fetch_add(event.value, Ordering::Relaxed); }
                    REL_WHEEL => { accum.wheel.fetch_add(event.value, Ordering::Relaxed); }
                    _ => {}
                }
            }
        }
    }
}

// ── Keyboard reader thread ──

fn kbd_reader_loop(fd: i32, accum: &EvdevAccum) {
    let event_size = std::mem::size_of::<InputEvent>();
    let mut buf = vec![0u8; event_size * 64];

    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    while accum.running.load(Ordering::Relaxed) {
        let ret = unsafe { libc::poll(&mut pollfd, 1, 50) };
        if ret <= 0 { continue; }

        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 { continue; }

        let num_events = n as usize / event_size;
        let mut batch = Vec::new();

        for i in 0..num_events {
            let event = unsafe { &*(buf.as_ptr().add(i * event_size) as *const InputEvent) };
            if event.type_ == EV_KEY {
                // value: 0 = release, 1 = press, 2 = repeat (auto-repeat)
                let pressed = event.value != 0; // both press and repeat count as "down"

                // Track modifier state for release combo detection
                // evdev codes: KEY_LEFTCTRL=29, KEY_RIGHTCTRL=97, KEY_LEFTALT=56, KEY_RIGHTALT=100
                match event.code {
                    29 | 97 => accum.ctrl_down.store(pressed, Ordering::SeqCst),
                    56 | 100 => accum.alt_down.store(pressed, Ordering::SeqCst),
                    _ => {}
                }

                // Check for Ctrl+Alt+G(34)/F(33)/Escape(1) release combo
                if pressed
                    && accum.ctrl_down.load(Ordering::SeqCst)
                    && accum.alt_down.load(Ordering::SeqCst)
                {
                    match event.code {
                        34 | 33 | 1 => {
                            // G, F, or Escape while Ctrl+Alt held
                            accum.release_combo.store(true, Ordering::SeqCst);
                            // Don't forward this key event to the VM
                            continue;
                        }
                        _ => {}
                    }
                }

                if let Some((scancode, extended)) = evdev_to_ps2(event.code) {
                    batch.push(KeyEvent { scancode, extended, pressed });
                }
            }
        }

        if !batch.is_empty() {
            if let Ok(mut keys) = accum.key_events.lock() {
                keys.extend(batch);
            }
        }
    }
}

// ── Device discovery ──

fn find_mouse_device() -> Option<i32> {
    for i in 0..32 {
        let path = format!("/dev/input/event{}", i);
        let c_path = std::ffi::CString::new(path.as_str()).ok()?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd < 0 { continue; }

        let mut ev_bits = [0u8; 32];
        let ret = unsafe { libc::ioctl(fd, EVIOCGBIT_EV, ev_bits.as_mut_ptr()) };
        if ret < 0 || (ev_bits[0] & EV_REL_BIT) == 0 {
            unsafe { libc::close(fd); }
            continue;
        }

        let mut rel_bits = [0u8; 32];
        let ret = unsafe { libc::ioctl(fd, EVIOCGBIT_REL, rel_bits.as_mut_ptr()) };
        if ret < 0 {
            unsafe { libc::close(fd); }
            continue;
        }

        let has_rel_x = (rel_bits[REL_X as usize / 8] & (1 << (REL_X % 8))) != 0;
        let has_rel_y = (rel_bits[REL_Y as usize / 8] & (1 << (REL_Y % 8))) != 0;

        if has_rel_x && has_rel_y {
            eprintln!("[evdev] Found mouse at {}", path);
            return Some(fd);
        }
        unsafe { libc::close(fd); }
    }
    eprintln!("[evdev] No mouse device found");
    None
}

fn find_keyboard_device() -> Option<i32> {
    // Look for a device that has EV_KEY and has key codes in the typical keyboard range.
    // We use EVIOCGBIT(EV_KEY, ...) to check for common keys like KEY_A (30).
    const EVIOCGBIT_KEY: libc::c_ulong = 0x80604521; // EVIOCGBIT(EV_KEY, 96 bytes = 768 bits)

    for i in 0..32 {
        let path = format!("/dev/input/event{}", i);
        let c_path = std::ffi::CString::new(path.as_str()).ok()?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd < 0 { continue; }

        let mut ev_bits = [0u8; 32];
        let ret = unsafe { libc::ioctl(fd, EVIOCGBIT_EV, ev_bits.as_mut_ptr()) };
        if ret < 0 || (ev_bits[0] & EV_KEY_BIT) == 0 {
            unsafe { libc::close(fd); }
            continue;
        }

        // Check for typical keyboard keys (KEY_A=30, KEY_Z=44, KEY_ENTER=28)
        let mut key_bits = [0u8; 96];
        let ret = unsafe { libc::ioctl(fd, EVIOCGBIT_KEY, key_bits.as_mut_ptr()) };
        if ret < 0 {
            unsafe { libc::close(fd); }
            continue;
        }

        let has_key_a = (key_bits[30 / 8] & (1 << (30 % 8))) != 0;
        let has_key_z = (key_bits[44 / 8] & (1 << (44 % 8))) != 0;
        let has_key_enter = (key_bits[28 / 8] & (1 << (28 % 8))) != 0;

        // Must have typical letter keys + enter to be a keyboard (not just a mouse with buttons)
        if has_key_a && has_key_z && has_key_enter {
            // Also make sure this is NOT a mouse pretending to be a keyboard
            // (mice have EV_KEY for buttons but not for letter keys, so the check above suffices)
            eprintln!("[evdev] Found keyboard at {}", path);
            return Some(fd);
        }
        unsafe { libc::close(fd); }
    }
    eprintln!("[evdev] No keyboard device found");
    None
}

// ── evdev keycode → PS/2 Set 1 scancode mapping ──
//
// Linux evdev keycodes are defined in <linux/input-event-codes.h>.
// PS/2 Set 1 scancodes are what the i8042 controller uses.
// For most keys, the evdev code IS the PS/2 Set 1 make code.
// Extended keys (arrow keys, etc.) need an E0 prefix in PS/2.

fn evdev_to_ps2(evdev_code: u16) -> Option<(u8, bool)> {
    // Standard keys: evdev code maps directly to PS/2 Set 1 scancode
    match evdev_code {
        // Row 1: Esc, F1-F12
        1 => Some((0x01, false)),   // KEY_ESC
        59 => Some((0x3B, false)),  // KEY_F1
        60 => Some((0x3C, false)),  // KEY_F2
        61 => Some((0x3D, false)),  // KEY_F3
        62 => Some((0x3E, false)),  // KEY_F4
        63 => Some((0x3F, false)),  // KEY_F5
        64 => Some((0x40, false)),  // KEY_F6
        65 => Some((0x41, false)),  // KEY_F7
        66 => Some((0x42, false)),  // KEY_F8
        67 => Some((0x43, false)),  // KEY_F9
        68 => Some((0x44, false)),  // KEY_F10
        87 => Some((0x57, false)),  // KEY_F11
        88 => Some((0x58, false)),  // KEY_F12

        // Row 2: Backtick, 1-0, Minus, Equal, Backspace
        41 => Some((0x29, false)),  // KEY_GRAVE
        2 => Some((0x02, false)),   // KEY_1
        3 => Some((0x03, false)),   // KEY_2
        4 => Some((0x04, false)),   // KEY_3
        5 => Some((0x05, false)),   // KEY_4
        6 => Some((0x06, false)),   // KEY_5
        7 => Some((0x07, false)),   // KEY_6
        8 => Some((0x08, false)),   // KEY_7
        9 => Some((0x09, false)),   // KEY_8
        10 => Some((0x0A, false)),  // KEY_9
        11 => Some((0x0B, false)),  // KEY_0
        12 => Some((0x0C, false)),  // KEY_MINUS
        13 => Some((0x0D, false)),  // KEY_EQUAL
        14 => Some((0x0E, false)),  // KEY_BACKSPACE

        // Row 3: Tab, Q-P, brackets, backslash
        15 => Some((0x0F, false)),  // KEY_TAB
        16 => Some((0x10, false)),  // KEY_Q
        17 => Some((0x11, false)),  // KEY_W
        18 => Some((0x12, false)),  // KEY_E
        19 => Some((0x13, false)),  // KEY_R
        20 => Some((0x14, false)),  // KEY_T
        21 => Some((0x15, false)),  // KEY_Y
        22 => Some((0x16, false)),  // KEY_U
        23 => Some((0x17, false)),  // KEY_I
        24 => Some((0x18, false)),  // KEY_O
        25 => Some((0x19, false)),  // KEY_P
        26 => Some((0x1A, false)),  // KEY_LEFTBRACE
        27 => Some((0x1B, false)),  // KEY_RIGHTBRACE
        43 => Some((0x2B, false)),  // KEY_BACKSLASH

        // Row 4: CapsLock, A-L, semicolon, quote, Enter
        58 => Some((0x3A, false)),  // KEY_CAPSLOCK
        30 => Some((0x1E, false)),  // KEY_A
        31 => Some((0x1F, false)),  // KEY_S
        32 => Some((0x20, false)),  // KEY_D
        33 => Some((0x21, false)),  // KEY_F
        34 => Some((0x22, false)),  // KEY_G
        35 => Some((0x23, false)),  // KEY_H
        36 => Some((0x24, false)),  // KEY_J
        37 => Some((0x25, false)),  // KEY_K
        38 => Some((0x26, false)),  // KEY_L
        39 => Some((0x27, false)),  // KEY_SEMICOLON
        40 => Some((0x28, false)),  // KEY_APOSTROPHE
        28 => Some((0x1C, false)),  // KEY_ENTER

        // Row 5: Shifts, Z-M, comma, dot, slash
        42 => Some((0x2A, false)),  // KEY_LEFTSHIFT
        44 => Some((0x2C, false)),  // KEY_Z
        45 => Some((0x2D, false)),  // KEY_X
        46 => Some((0x2E, false)),  // KEY_C
        47 => Some((0x2F, false)),  // KEY_V
        48 => Some((0x30, false)),  // KEY_B
        49 => Some((0x31, false)),  // KEY_N
        50 => Some((0x32, false)),  // KEY_M
        51 => Some((0x33, false)),  // KEY_COMMA
        52 => Some((0x34, false)),  // KEY_DOT
        53 => Some((0x35, false)),  // KEY_SLASH
        54 => Some((0x36, false)),  // KEY_RIGHTSHIFT

        // Row 6: Ctrl, Alt, Space
        29 => Some((0x1D, false)),  // KEY_LEFTCTRL
        56 => Some((0x38, false)),  // KEY_LEFTALT
        57 => Some((0x39, false)),  // KEY_SPACE

        // Non-extended special keys
        69 => Some((0x45, false)),  // KEY_NUMLOCK
        70 => Some((0x46, false)),  // KEY_SCROLLLOCK
        86 => Some((0x56, false)),  // KEY_102ND (the extra key on ISO keyboards, between LShift and Z)

        // Numpad
        71 => Some((0x47, false)),  // KEY_KP7
        72 => Some((0x48, false)),  // KEY_KP8
        73 => Some((0x49, false)),  // KEY_KP9
        74 => Some((0x4A, false)),  // KEY_KPMINUS
        75 => Some((0x4B, false)),  // KEY_KP4
        76 => Some((0x4C, false)),  // KEY_KP5
        77 => Some((0x4D, false)),  // KEY_KP6
        78 => Some((0x4E, false)),  // KEY_KPPLUS
        79 => Some((0x4F, false)),  // KEY_KP1
        80 => Some((0x50, false)),  // KEY_KP2
        81 => Some((0x51, false)),  // KEY_KP3
        82 => Some((0x52, false)),  // KEY_KP0
        83 => Some((0x53, false)),  // KEY_KPDOT
        55 => Some((0x37, false)),  // KEY_KPASTERISK

        // Extended keys (need E0 prefix)
        97 => Some((0x1D, true)),   // KEY_RIGHTCTRL
        100 => Some((0x38, true)),  // KEY_RIGHTALT (AltGr)
        96 => Some((0x1C, true)),   // KEY_KPENTER
        98 => Some((0x35, true)),   // KEY_KPSLASH
        99 => Some((0x37, true)),   // KEY_SYSRQ (PrintScreen)
        102 => Some((0x47, true)),  // KEY_HOME
        103 => Some((0x48, true)),  // KEY_UP
        104 => Some((0x49, true)),  // KEY_PAGEUP
        105 => Some((0x4B, true)),  // KEY_LEFT
        106 => Some((0x4D, true)),  // KEY_RIGHT
        107 => Some((0x4F, true)),  // KEY_END
        108 => Some((0x50, true)),  // KEY_DOWN
        109 => Some((0x51, true)),  // KEY_PAGEDOWN
        110 => Some((0x52, true)),  // KEY_INSERT
        111 => Some((0x53, true)),  // KEY_DELETE
        125 => Some((0x5B, true)),  // KEY_LEFTMETA (Super/Win)
        126 => Some((0x5C, true)),  // KEY_RIGHTMETA
        127 => Some((0x5D, true)),  // KEY_COMPOSE (Menu)

        _ => None,
    }
}
