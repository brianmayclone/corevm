//! Server-side EventHandler for libcorevm VmRuntime.
//!
//! Routes VM events to shared state (framebuffer, serial broadcast)
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
    corevm_read_phys,
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
            VmEvent::DebugOutput(_) => {}
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
        if self.last_fb_update.elapsed() >= Duration::from_millis(33) {
            update_framebuffer(handle, &self.fb);
            self.last_fb_update = Instant::now();
        }
    }
}

/// Read the current framebuffer from libcorevm into shared FrameBufferData.
/// Mirrors vmmanager's update_framebuffer but without egui/display deps.
fn update_framebuffer(handle: u64, fb: &Arc<Mutex<FrameBufferData>>) {
    let mut fb_lock = match fb.lock() {
        Ok(l) => l,
        Err(_) => return,
    };

    // VirtIO GPU (if active)
    if corevm_has_virtio_gpu(handle) != 0 && corevm_virtio_gpu_scanout_active(handle) != 0 {
        let mut gpu_w: u32 = 0;
        let mut gpu_h: u32 = 0;
        let mut gpu_bpp: u8 = 0;
        if corevm_virtio_gpu_get_mode(handle, &mut gpu_w, &mut gpu_h, &mut gpu_bpp) == 0
            && gpu_w > 0 && gpu_h > 0
        {
            let mut fb_ptr: *const u8 = std::ptr::null();
            let mut fb_len: u32 = 0;
            if corevm_virtio_gpu_get_framebuffer(handle, &mut fb_ptr, &mut fb_len) == 0
                && !fb_ptr.is_null() && fb_len > 0
            {
                let fb_size = (gpu_w as usize) * (gpu_h as usize) * 4;
                if (fb_len as usize) >= fb_size {
                    let raw = unsafe { std::slice::from_raw_parts(fb_ptr, fb_size) };
                    fb_lock.text_mode = false;
                    fb_lock.width = gpu_w;
                    fb_lock.height = gpu_h;
                    fb_lock.pixels.resize(fb_size, 0);
                    fb_lock.pixels.copy_from_slice(raw);
                    fb_lock.dirty = true;
                    return;
                }
            }
        }
    }

    // VGA mode
    let mut vga_w: u32 = 0;
    let mut vga_h: u32 = 0;
    let mut vga_bpp: u8 = 0;
    let mode_ret = corevm_vga_get_mode(handle, &mut vga_w, &mut vga_h, &mut vga_bpp);

    if mode_ret == 1 {
        // Text mode
        let mut text_ptr: *const u16 = std::ptr::null();
        let mut text_len: u32 = 0;
        let ret = corevm_vga_get_text_buffer(handle, &mut text_ptr, &mut text_len);
        if ret == 0 && !text_ptr.is_null() && text_len > 0 {
            let text_cells = unsafe { std::slice::from_raw_parts(text_ptr, text_len as usize) };
            fb_lock.text_mode = true;
            fb_lock.text_buffer = text_cells.to_vec();
            let text_buf = fb_lock.text_buffer.clone();
            render_text_to_rgba(&text_buf, &mut fb_lock.pixels);
            fb_lock.width = 720;
            fb_lock.height = 400;
            fb_lock.dirty = true;
        }
    } else if mode_ret == 0 && vga_w > 0 && vga_h > 0 && vga_bpp > 0 {
        // Graphics mode — read VGA framebuffer via corevm_read_phys
        let bytes_per_pixel = (vga_bpp as usize + 7) / 8;
        let fb_size = vga_w as usize * vga_h as usize * bytes_per_pixel;
        let vram_offset = corevm_vga_get_fb_offset(handle);
        let read_addr = 0xE000_0000u64 + vram_offset;

        let mut raw_pixels = vec![0u8; fb_size];
        let phys_ret = corevm_read_phys(handle, read_addr, raw_pixels.as_mut_ptr(), fb_size as u32);

        if phys_ret != 0 {
            // Fallback: read internal SVGA buffer
            let mut fb_ptr: *const u8 = std::ptr::null();
            let mut fb_len: u32 = 0;
            corevm_vga_get_framebuffer(handle, &mut fb_ptr, &mut fb_len);
            if !fb_ptr.is_null() && fb_len > 0 {
                let off = vram_offset as usize;
                let avail = (fb_len as usize).saturating_sub(off);
                let len = avail.min(fb_size);
                if len > 0 {
                    let raw = unsafe { std::slice::from_raw_parts(fb_ptr.add(off), len) };
                    raw_pixels[..len].copy_from_slice(raw);
                }
            }
        }

        // Convert to RGBA32
        let rgba_size = vga_w as usize * vga_h as usize * 4;
        fb_lock.pixels.resize(rgba_size, 0);
        bgr_to_rgba(&raw_pixels, &mut fb_lock.pixels, vga_w as usize, vga_h as usize, vga_bpp);
        fb_lock.text_mode = false;
        fb_lock.width = vga_w;
        fb_lock.height = vga_h;
        fb_lock.dirty = true;
    }
}

/// Convert BGR/BGRA raw pixels to RGBA32.
fn bgr_to_rgba(src: &[u8], dst: &mut [u8], w: usize, h: usize, bpp: u8) {
    let bpp_bytes = (bpp as usize + 7) / 8;
    for y in 0..h {
        for x in 0..w {
            let si = (y * w + x) * bpp_bytes;
            let di = (y * w + x) * 4;
            if si + bpp_bytes <= src.len() && di + 4 <= dst.len() {
                match bpp {
                    32 => {
                        dst[di]     = src[si + 2]; // R
                        dst[di + 1] = src[si + 1]; // G
                        dst[di + 2] = src[si];     // B
                        dst[di + 3] = 255;
                    }
                    24 => {
                        dst[di]     = src[si + 2];
                        dst[di + 1] = src[si + 1];
                        dst[di + 2] = src[si];
                        dst[di + 3] = 255;
                    }
                    16 => {
                        let pixel = (src[si] as u16) | ((src[si + 1] as u16) << 8);
                        dst[di]     = ((pixel >> 11) as u8) << 3;
                        dst[di + 1] = (((pixel >> 5) & 0x3F) as u8) << 2;
                        dst[di + 2] = ((pixel & 0x1F) as u8) << 3;
                        dst[di + 3] = 255;
                    }
                    _ => {
                        dst[di..di+4].fill(0);
                    }
                }
            }
        }
    }
}

/// Render text buffer (80x25 cells) to RGBA pixels (720x400) using a built-in
/// CP437 8x16 font. This is a simplified version — the full VGA font is in vmmanager.
fn render_text_to_rgba(text: &[u16], pixels: &mut Vec<u8>) {
    const COLS: usize = 80;
    const ROWS: usize = 25;
    const CHAR_W: usize = 9;
    const CHAR_H: usize = 16;
    const FB_W: usize = COLS * CHAR_W;
    const FB_H: usize = ROWS * CHAR_H;

    // VGA 16-color palette (standard CGA/EGA colors)
    const PALETTE: [[u8; 3]; 16] = [
        [0, 0, 0],       [0, 0, 170],     [0, 170, 0],     [0, 170, 170],
        [170, 0, 0],     [170, 0, 170],   [170, 85, 0],    [170, 170, 170],
        [85, 85, 85],    [85, 85, 255],   [85, 255, 85],   [85, 255, 255],
        [255, 85, 85],   [255, 85, 255],  [255, 255, 85],  [255, 255, 255],
    ];

    pixels.resize(FB_W * FB_H * 4, 0);
    // Simple fallback: fill cells with background color, no glyph rendering
    // (full glyph rendering needs the CP437 font bitmap which is in vmmanager)
    for row in 0..ROWS {
        for col in 0..COLS {
            let idx = row * COLS + col;
            if idx >= text.len() { break; }
            let cell = text[idx];
            let bg = ((cell >> 12) & 0x07) as usize;
            let fg_color = PALETTE[((cell >> 8) & 0x0F) as usize];
            let bg_color = PALETTE[bg];
            let ch = (cell & 0xFF) as u8;
            for cy in 0..CHAR_H {
                for cx in 0..CHAR_W {
                    let px = col * CHAR_W + cx;
                    let py = row * CHAR_H + cy;
                    let di = (py * FB_W + px) * 4;
                    if di + 3 < pixels.len() {
                        // Without a font bitmap, show background only (or simple block for non-space)
                        let color = if ch != 0 && ch != 0x20 && cx < 8 {
                            &fg_color
                        } else {
                            &bg_color
                        };
                        pixels[di]     = color[0];
                        pixels[di + 1] = color[1];
                        pixels[di + 2] = color[2];
                        pixels[di + 3] = 255;
                    }
                }
            }
        }
    }
}
