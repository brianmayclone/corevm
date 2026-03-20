//! Server-side EventHandler for libcorevm VmRuntime.
//!
//! Routes VM events to shared state (framebuffer, serial broadcast, stats)
//! instead of to an egui UI.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use libcorevm::runtime::{EventHandler, VmEvent, VmControlHandle};
use libcorevm::ffi::{
    corevm_vga_get_framebuffer, corevm_vga_get_text_buffer, corevm_vga_get_mode,
    corevm_vga_get_fb_offset, corevm_has_virtio_gpu,
    corevm_virtio_gpu_get_framebuffer, corevm_virtio_gpu_get_mode,
    corevm_virtio_gpu_scanout_active,
};

/// Shared framebuffer data between VM thread and WebSocket handlers.
pub struct FrameBufferData {
    pub pixels: Vec<u8>,       // RGBA32
    pub width: u32,
    pub height: u32,
    pub text_mode: bool,
    pub text_buffer: Vec<u16>, // 80x25 = 2000 cells
    pub dirty: bool,
}

impl Default for FrameBufferData {
    fn default() -> Self {
        Self {
            pixels: Vec::new(), width: 0, height: 0,
            text_mode: true, text_buffer: Vec::new(), dirty: false,
        }
    }
}

/// EventHandler implementation for vmm-server.
pub struct ServerEventHandler {
    pub fb: Arc<Mutex<FrameBufferData>>,
    pub serial_tx: broadcast::Sender<Vec<u8>>,
    pub control: VmControlHandle,
    pub handle: u64,
    last_fb_update: Instant,
}

// Safety: all fields are Send-safe (Arc, broadcast::Sender, VmControlHandle).
unsafe impl Send for ServerEventHandler {}

impl ServerEventHandler {
    pub fn new(
        fb: Arc<Mutex<FrameBufferData>>,
        serial_tx: broadcast::Sender<Vec<u8>>,
        control: VmControlHandle,
        handle: u64,
    ) -> Self {
        Self { fb, serial_tx, control, handle, last_fb_update: Instant::now() }
    }
}

impl EventHandler for ServerEventHandler {
    fn on_event(&mut self, event: VmEvent) {
        match event {
            VmEvent::SerialOutput(data) => {
                let _ = self.serial_tx.send(data);
            }
            VmEvent::DebugOutput(_data) => {
                // Could log to tracing if needed
            }
            VmEvent::Shutdown => {
                tracing::info!("VM shutdown (handle={})", self.handle);
                self.control.set_exit_reason("Shutdown".into());
                self.control.set_exited();
                update_framebuffer(self.handle, &self.fb);
            }
            VmEvent::Error { message } => {
                tracing::error!("VM error (handle={}): {}", self.handle, message);
                self.control.set_exit_reason(format!("Error: {}", message));
                self.control.set_exited();
            }
            VmEvent::RebootRequested => {
                tracing::info!("VM reboot requested (handle={})", self.handle);
                self.control.set_reboot_requested();
                self.control.set_exited();
            }
            VmEvent::Diagnostic(msg) => {
                tracing::debug!("VM diag: {}", msg);
            }
        }
    }

    fn on_tick(&mut self, handle: u64) {
        // Update framebuffer at ~30fps (lower than GUI's 60fps)
        if self.last_fb_update.elapsed() >= Duration::from_millis(33) {
            update_framebuffer(handle, &self.fb);
            self.last_fb_update = Instant::now();
        }
    }
}

/// Read the current framebuffer state from libcorevm into shared FrameBufferData.
/// This is the same logic as vmmanager's update_framebuffer but without egui deps.
fn update_framebuffer(handle: u64, fb: &Arc<Mutex<FrameBufferData>>) {
    let mut fb_lock = match fb.lock() {
        Ok(l) => l,
        Err(_) => return,
    };

    // Check VirtIO GPU first (if present and active)
    if corevm_has_virtio_gpu(handle) != 0 && corevm_virtio_gpu_scanout_active(handle) != 0 {
        let mut ptr: *const u8 = std::ptr::null();
        let mut w: u32 = 0;
        let mut h: u32 = 0;
        let mut stride: u32 = 0;
        let mut format: u32 = 0;
        corevm_virtio_gpu_get_framebuffer(handle, &mut ptr, &mut w, &mut h, &mut stride, &mut format);
        if !ptr.is_null() && w > 0 && h > 0 {
            let size = (h * stride) as usize;
            let data = unsafe { std::slice::from_raw_parts(ptr, size) };
            // Convert BGRA -> RGBA if needed (VirtIO GPU format 1 = B8G8R8X8)
            let row_bytes = (w * 4) as usize;
            fb_lock.pixels.resize(row_bytes * h as usize, 0);
            for y in 0..h as usize {
                let src_off = y * stride as usize;
                let dst_off = y * row_bytes;
                for x in 0..w as usize {
                    let si = src_off + x * 4;
                    let di = dst_off + x * 4;
                    if si + 3 < data.len() && di + 3 < fb_lock.pixels.len() {
                        if format == 1 { // BGRX
                            fb_lock.pixels[di]     = data[si + 2]; // R
                            fb_lock.pixels[di + 1] = data[si + 1]; // G
                            fb_lock.pixels[di + 2] = data[si];     // B
                            fb_lock.pixels[di + 3] = 255;
                        } else { // RGBX
                            fb_lock.pixels[di..di+4].copy_from_slice(&data[si..si+4]);
                        }
                    }
                }
            }
            fb_lock.width = w;
            fb_lock.height = h;
            fb_lock.text_mode = false;
            fb_lock.dirty = true;
            return;
        }
    }

    // Fallback: VGA framebuffer
    let mut ptr: *const u8 = std::ptr::null();
    let mut w: u32 = 0;
    let mut h: u32 = 0;
    let mut bpp: u32 = 0;
    let mut stride: u32 = 0;
    corevm_vga_get_framebuffer(handle, &mut ptr, &mut w, &mut h, &mut bpp, &mut stride);

    let mode = corevm_vga_get_mode(handle);
    if mode == 3 || mode == 7 {
        // Text mode
        let mut text_ptr: *const u16 = std::ptr::null();
        let mut text_len: u32 = 0;
        corevm_vga_get_text_buffer(handle, &mut text_ptr, &mut text_len);
        if !text_ptr.is_null() && text_len > 0 {
            let text_data = unsafe { std::slice::from_raw_parts(text_ptr, text_len as usize) };
            fb_lock.text_buffer = text_data.to_vec();
            fb_lock.text_mode = true;
            fb_lock.width = 720;  // 80*9
            fb_lock.height = 400; // 25*16
            fb_lock.dirty = true;
        }
        return;
    }

    // Graphics mode
    if !ptr.is_null() && w > 0 && h > 0 && bpp >= 8 {
        let fb_offset = corevm_vga_get_fb_offset(handle) as usize;
        let row_bytes = (w * 4) as usize;
        fb_lock.pixels.resize(row_bytes * h as usize, 0);

        for y in 0..h as usize {
            let src_off = fb_offset + y * stride as usize;
            let dst_off = y * row_bytes;
            for x in 0..w as usize {
                let si = src_off + x * (bpp as usize / 8);
                let di = dst_off + x * 4;
                if di + 3 < fb_lock.pixels.len() {
                    match bpp {
                        32 => {
                            // BGRA -> RGBA
                            let src = unsafe { std::slice::from_raw_parts(ptr.add(si), 4) };
                            fb_lock.pixels[di]     = src[2]; // R
                            fb_lock.pixels[di + 1] = src[1]; // G
                            fb_lock.pixels[di + 2] = src[0]; // B
                            fb_lock.pixels[di + 3] = 255;
                        }
                        24 => {
                            let src = unsafe { std::slice::from_raw_parts(ptr.add(si), 3) };
                            fb_lock.pixels[di]     = src[2];
                            fb_lock.pixels[di + 1] = src[1];
                            fb_lock.pixels[di + 2] = src[0];
                            fb_lock.pixels[di + 3] = 255;
                        }
                        16 => {
                            let lo = unsafe { *ptr.add(si) } as u16;
                            let hi = unsafe { *ptr.add(si + 1) } as u16;
                            let pixel = lo | (hi << 8);
                            fb_lock.pixels[di]     = ((pixel >> 11) as u8) << 3;
                            fb_lock.pixels[di + 1] = (((pixel >> 5) & 0x3F) as u8) << 2;
                            fb_lock.pixels[di + 2] = ((pixel & 0x1F) as u8) << 3;
                            fb_lock.pixels[di + 3] = 255;
                        }
                        _ => {}
                    }
                }
            }
        }
        fb_lock.width = w;
        fb_lock.height = h;
        fb_lock.text_mode = false;
        fb_lock.dirty = true;
    }
}
