//! Low-level evdev mouse reader for Linux.
//!
//! Reads raw `input_event` structs directly from `/dev/input/eventX` to get
//! reliable relative mouse deltas (REL_X, REL_Y, REL_WHEEL) independent of
//! the windowing system. This avoids issues with CursorGrab::Locked not
//! delivering pointer.delta() on some X11/Wayland backends.
//!
//! Requires read access to `/dev/input/eventX` devices (user must be in the
//! `input` group). Use `check_access()` at startup and `grant_access()` to
//! add the user to the group via pkexec if needed.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

// Linux input event constants
const EV_REL: u16 = 0x02;
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;

// ioctl constants for capability queries
const EVIOCGBIT_EV: libc::c_ulong = 0x80084520; // EVIOCGBIT(0, 32)
const EVIOCGBIT_REL: libc::c_ulong = 0x80084522; // EVIOCGBIT(EV_REL, 32)
// EV_REL bit in event type bitmap
const EV_REL_BIT: u8 = 1 << EV_REL;

/// Check if the current user can read `/dev/input/event*` devices.
/// Returns true if at least one event device is readable.
pub fn check_access() -> bool {
    for i in 0..32 {
        let path = format!("/dev/input/event{}", i);
        if let Ok(metadata) = std::fs::metadata(&path) {
            // Try to actually open it for reading
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
    }
    false
}

/// Check if the current user is in the `input` group.
pub fn user_in_input_group() -> bool {
    let username = std::env::var("USER").unwrap_or_default();
    if username.is_empty() {
        return false;
    }
    // Check via `groups` command output
    if let Ok(output) = std::process::Command::new("groups").arg(&username).output() {
        if let Ok(groups_str) = std::str::from_utf8(&output.stdout) {
            return groups_str.split_whitespace().any(|g| g == "input");
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
        // 126 = user dismissed the polkit dialog
        Ok(false)
    } else {
        Err(format!("pkexec usermod failed with exit code: {:?}", status.code()))
    }
}

/// Accumulated mouse deltas from the evdev reader thread.
struct EvdevAccum {
    dx: AtomicI32,
    dy: AtomicI32,
    wheel: AtomicI32,
    running: AtomicBool,
}

/// Raw evdev mouse reader. Opens `/dev/input/eventX`, reads in a background
/// thread, accumulates deltas that can be polled from the UI thread.
pub struct EvdevMouseReader {
    accum: Arc<EvdevAccum>,
    thread: Option<std::thread::JoinHandle<()>>,
    fd: i32,
}

impl EvdevMouseReader {
    /// Find and open the first mouse device (one that reports REL_X and REL_Y).
    pub fn open() -> Option<Self> {
        let fd = find_mouse_device()?;
        eprintln!("[evdev] Opened mouse device fd={}", fd);
        Some(Self {
            accum: Arc::new(EvdevAccum {
                dx: AtomicI32::new(0),
                dy: AtomicI32::new(0),
                wheel: AtomicI32::new(0),
                running: AtomicBool::new(false),
            }),
            thread: None,
            fd,
        })
    }

    /// Start the background reader thread.
    pub fn start(&mut self) {
        if self.accum.running.load(Ordering::Relaxed) {
            return;
        }
        self.accum.running.store(true, Ordering::SeqCst);
        // Reset accumulators
        self.accum.dx.store(0, Ordering::Relaxed);
        self.accum.dy.store(0, Ordering::Relaxed);
        self.accum.wheel.store(0, Ordering::Relaxed);

        let accum = self.accum.clone();
        let fd = self.fd;

        self.thread = Some(std::thread::spawn(move || {
            reader_loop(fd, &accum);
        }));
    }

    /// Stop the background reader thread.
    pub fn stop(&mut self) {
        self.accum.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    /// Take accumulated deltas since last call (atomically resets them).
    pub fn take_deltas(&self) -> (i32, i32, i32) {
        let dx = self.accum.dx.swap(0, Ordering::Relaxed);
        let dy = self.accum.dy.swap(0, Ordering::Relaxed);
        let wheel = self.accum.wheel.swap(0, Ordering::Relaxed);
        (dx, dy, wheel)
    }

    /// Check if the reader thread is running.
    pub fn is_running(&self) -> bool {
        self.accum.running.load(Ordering::Relaxed)
    }
}

impl Drop for EvdevMouseReader {
    fn drop(&mut self) {
        self.stop();
        if self.fd >= 0 {
            unsafe { libc::close(self.fd); }
        }
    }
}

/// Background reader loop: reads input_event structs and accumulates deltas.
fn reader_loop(fd: i32, accum: &EvdevAccum) {
    let event_size = std::mem::size_of::<InputEvent>();
    let mut buf = vec![0u8; event_size * 64];

    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    while accum.running.load(Ordering::Relaxed) {
        // Wait up to 50ms for data (allows checking running flag regularly)
        let ret = unsafe { libc::poll(&mut pollfd, 1, 50) };
        if ret <= 0 {
            continue;
        }

        let n = unsafe {
            libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
        };
        if n <= 0 {
            continue;
        }

        let num_events = n as usize / event_size;
        for i in 0..num_events {
            let offset = i * event_size;
            let event = unsafe {
                &*(buf.as_ptr().add(offset) as *const InputEvent)
            };

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

/// Raw Linux `struct input_event` (24 bytes on 64-bit).
#[repr(C)]
struct InputEvent {
    _tv_sec: i64,
    _tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

/// Scan `/dev/input/event*` and find the first device that reports REL_X + REL_Y.
fn find_mouse_device() -> Option<i32> {
    for i in 0..32 {
        let path = format!("/dev/input/event{}", i);
        let c_path = std::ffi::CString::new(path.as_str()).ok()?;

        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd < 0 {
            continue;
        }

        // Check if this device reports EV_REL events
        let mut ev_bits = [0u8; 32];
        let ret = unsafe {
            libc::ioctl(fd, EVIOCGBIT_EV, ev_bits.as_mut_ptr())
        };
        if ret < 0 || (ev_bits[0] & EV_REL_BIT) == 0 {
            unsafe { libc::close(fd); }
            continue;
        }

        // Check if it reports REL_X and REL_Y
        let mut rel_bits = [0u8; 32];
        let ret = unsafe {
            libc::ioctl(fd, EVIOCGBIT_REL, rel_bits.as_mut_ptr())
        };
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

    eprintln!("[evdev] No mouse device found in /dev/input/event0..31");
    None
}
