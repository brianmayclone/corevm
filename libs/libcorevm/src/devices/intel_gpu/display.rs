//! Display engine — pipes, planes, cursor.
//!
//! Manages the display pipeline state: pipe timing configuration,
//! primary plane (framebuffer surface), and cursor overlay.

extern crate alloc;
use alloc::vec::Vec;
use super::regs;

/// Display engine state.
pub struct DisplayEngine {
    /// Current display width.
    pub width: u32,
    /// Current display height.
    pub height: u32,
    /// Stride in bytes per scanline.
    pub stride: u32,
    /// Bits per pixel (8, 16, 32).
    pub bpp: u32,
    /// Pipe A enabled.
    pub pipe_enabled: bool,
    /// Plane A enabled.
    pub plane_enabled: bool,
    /// Surface base address (GTT offset into VRAM).
    pub surface_base: u32,
}

impl DisplayEngine {
    pub fn new() -> Self {
        Self {
            width: 1920,
            height: 1080,
            stride: 1920 * 4,
            bpp: 32,
            pipe_enabled: true,
            plane_enabled: true,
            surface_base: 0,
        }
    }

    /// Copy the display surface from VRAM to the host framebuffer.
    pub fn refresh_framebuffer(&self, vram: &[u8], framebuffer: &mut Vec<u8>) {
        let base = self.surface_base as usize;
        let stride = self.stride as usize;
        let height = self.height as usize;
        let total = stride * height;

        if base + total > vram.len() { return; }
        if framebuffer.len() < total {
            framebuffer.resize(total, 0);
        }

        framebuffer[..total].copy_from_slice(&vram[base..base + total]);
    }
}

/// Initialize display-related registers with sane defaults.
pub fn init_registers(regs: &mut Vec<u32>) {
    // DPLL A: enabled, high speed
    regs[regs::DPLL_A_CTRL / 4] = 0x8000_0000; // bit 31 = PLL enable

    // Pipe A: enabled, 8bpc
    regs[regs::PIPEACONF / 4] = 0x8000_0000; // bit 31 = pipe enable

    // Pipe A source size: 1920×1080 → (1079 << 16) | 1919
    regs[regs::PIPEASRC / 4] = ((1080 - 1) << 16) | (1920 - 1);

    // Pipe A timing: simplified (total = active + blanking)
    // HTOTAL: (total-1) << 16 | (active-1) = (2199 << 16) | 1919
    regs[regs::HTOTAL_A / 4] = (2199 << 16) | 1919;
    // VTOTAL: (total-1) << 16 | (active-1) = (1124 << 16) | 1079
    regs[regs::VTOTAL_A / 4] = (1124 << 16) | 1079;

    // Display Plane A: enabled, XRGB 8:8:8:8 (format code 0x6)
    // bit 31 = enable, bits 29:26 = 0110 = 32-bit XRGB
    regs[regs::DSPACNTR / 4] = 0x8400_0000 | (0x6 << 26);

    // Stride: 1920 × 4 = 7680
    regs[regs::DSPASTRIDE / 4] = 1920 * 4;

    // Surface base: 0 (start of VRAM)
    regs[regs::DSPASURF / 4] = 0;

    // Interrupt: all masked initially
    regs[regs::DEIMR / 4] = 0xFFFF_FFFF;
    regs[regs::GTIMR / 4] = 0xFFFF_FFFF;

    // HDMI port B: present but disabled
    regs[regs::HDMIB / 4] = 0x0000_0000;

    // PIPE A STAT: report vblank capability (bit 1)
    regs[regs::PIPEASTAT / 4] = 0;
}

/// Check if an offset is a display engine register.
pub fn is_display_reg(offset: usize) -> bool {
    regs::is_display_range(offset)
}

/// Read a display register with special handling.
pub fn reg_read(display: &mut DisplayEngine, regs_file: &mut Vec<u32>, offset: usize) -> u32 {
    match offset {
        regs::PIPEASTAT => {
            // Simulate vblank: toggle bit 1 (vblank) on each read
            // This is a simple heuristic — real hardware uses timers
            let val = regs_file[offset / 4];
            // Toggle vblank status bit (bit 1) to simulate periodic vblanks
            regs_file[offset / 4] = val ^ (1 << 1);
            val
        }
        regs::DEIIR => {
            // Read-and-clear
            let val = regs_file[offset / 4];
            regs_file[offset / 4] = 0;
            val
        }
        _ => regs_file.get(offset / 4).copied().unwrap_or(0),
    }
}

/// Write a display register with special handling.
pub fn reg_write(
    display: &mut DisplayEngine,
    regs_file: &mut Vec<u32>,
    vram: &[u8],
    framebuffer: &mut Vec<u8>,
    offset: usize,
    val: u32,
) {
    let idx = offset / 4;
    if idx >= regs_file.len() { return; }

    match offset {
        regs::PIPEACONF => {
            regs_file[idx] = val;
            display.pipe_enabled = val & 0x8000_0000 != 0;
        }

        regs::PIPEASRC => {
            regs_file[idx] = val;
            let w = (val & 0xFFFF) + 1;
            let h = ((val >> 16) & 0xFFFF) + 1;
            if w > 0 && w <= 8192 && h > 0 && h <= 8192 {
                display.width = w;
                display.height = h;
                display.stride = w * (display.bpp / 8);
                regs_file[regs::DSPASTRIDE / 4] = display.stride;
                let fb_size = (display.stride * display.height) as usize;
                if fb_size > framebuffer.len() {
                    framebuffer.resize(fb_size, 0);
                }
            }
        }

        regs::DSPACNTR => {
            regs_file[idx] = val;
            display.plane_enabled = val & 0x8000_0000 != 0;
            // Decode pixel format from bits 29:26
            let fmt = (val >> 26) & 0xF;
            display.bpp = match fmt {
                0b0010 => 8,
                0b0101 => 16,
                0b0110 | 0b0111 => 32,
                0b1010 => 32,
                _ => 32,
            };
        }

        regs::DSPASTRIDE => {
            regs_file[idx] = val;
            display.stride = val;
        }

        regs::DSPASURF => {
            regs_file[idx] = val;
            display.surface_base = val;
            // Surface changed — update framebuffer
            display.refresh_framebuffer(vram, framebuffer);
        }

        // Interrupt registers: W1C
        regs::DEIIR | regs::SDEIIR => {
            regs_file[idx] &= !val;
        }

        // Pipe A timing registers — store and potentially update mode
        regs::HTOTAL_A => {
            regs_file[idx] = val;
            let active = (val & 0xFFFF) + 1;
            if active > 0 && active <= 8192 {
                display.width = active;
                display.stride = active * (display.bpp / 8);
                regs_file[regs::DSPASTRIDE / 4] = display.stride;
            }
        }

        regs::VTOTAL_A => {
            regs_file[idx] = val;
            let active = (val & 0xFFFF) + 1;
            if active > 0 && active <= 8192 {
                display.height = active;
                let fb_size = (display.stride * display.height) as usize;
                if fb_size > framebuffer.len() {
                    framebuffer.resize(fb_size, 0);
                }
            }
        }

        _ => {
            regs_file[idx] = val;
        }
    }
}
