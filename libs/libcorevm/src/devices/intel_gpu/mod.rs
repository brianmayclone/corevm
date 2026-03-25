//! Intel HD Graphics emulation (Sandy Bridge / Gen6).
//!
//! Emulates an Intel HD Graphics 2000 (PCI ID 8086:0102) with enough
//! functionality for the Linux i915 driver and Windows igfx driver to
//! initialize the display engine and set up a framebuffer.
//!
//! # Module Structure
//!
//! - [`regs`]    — Register offset constants
//! - [`edid`]    — EDID block generator
//! - [`gmbus`]   — GMBUS/I2C controller for DDC (EDID readout)
//! - [`display`] — Display engine: pipes, planes, cursor
//! - [`gtt`]     — Graphics Translation Table (VRAM address mapping)
//! - [`render`]  — Command Streamer / BLT engine stubs

pub mod regs;
pub mod edid;
pub mod gmbus;
pub mod display;
pub mod gtt;
pub mod render;

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use crate::memory::mmio::MmioHandler;
use crate::error::Result;

/// Intel HD Graphics 2000 (Sandy Bridge GT1).
pub const VENDOR_ID: u16 = 0x8086;
pub const DEVICE_ID: u16 = 0x0102;

/// MMIO register space size (BAR0): 2 MB (Sandy Bridge uses up to 0x100000+ for GT registers).
pub const MMIO_SIZE: usize = 2 * 1024 * 1024;

/// Intel HD Graphics device.
pub struct IntelGpu {
    /// MMIO register file (BAR0), addressed as u32 array.
    pub(crate) regs: Vec<u32>,
    /// Graphics aperture / VRAM (BAR2).
    pub(crate) vram: Vec<u8>,
    /// VRAM size in bytes.
    pub(crate) vram_size: usize,
    /// Page-aligned framebuffer for host display output (BGRA32).
    pub(crate) framebuffer: Vec<u8>,

    /// Display engine state.
    pub(crate) display: display::DisplayEngine,
    /// GMBUS (I2C) state machine for EDID.
    pub(crate) gmbus: gmbus::GmbusController,
    /// GTT state.
    pub(crate) gtt: gtt::Gtt,
    /// Render engine state (stub).
    pub(crate) render: render::RenderEngine,
}

impl IntelGpu {
    /// Create a new Intel HD Graphics device.
    ///
    /// `vram_mb` is the graphics memory size in MiB (typically 64–256).
    pub fn new(vram_mb: u32) -> Self {
        let vram_mb = vram_mb.clamp(32, 512);
        let vram_size = (vram_mb as usize) * 1024 * 1024;
        let num_regs = MMIO_SIZE / 4;

        let mut regs = vec![0u32; num_regs];

        // Initialize key registers with defaults
        display::init_registers(&mut regs);
        render::init_registers(&mut regs);

        // Initial framebuffer: 1920x1080 BGRA32
        let fb_size = 1920 * 1080 * 4;
        let framebuffer = vec![0u8; fb_size];

        IntelGpu {
            regs,
            vram: vec![0u8; vram_size],
            vram_size,
            framebuffer,
            display: display::DisplayEngine::new(),
            gmbus: gmbus::GmbusController::new(),
            gtt: gtt::Gtt::new(vram_size),
            render: render::RenderEngine::new(),
        }
    }

    /// Get framebuffer pointer and length for host display.
    pub fn framebuffer_ptr(&self) -> (*const u8, usize) {
        (self.framebuffer.as_ptr(), self.framebuffer.len())
    }

    /// Get current display mode (width, height, bpp).
    pub fn display_mode(&self) -> (u32, u32, u32) {
        (self.display.width, self.display.height, self.display.bpp)
    }

    /// VRAM pointer for hypervisor memory region mapping.
    pub fn vram_mut_ptr(&mut self) -> (*mut u8, usize) {
        (self.vram.as_mut_ptr(), self.vram_size)
    }

    /// Read a 32-bit MMIO register (BAR0).
    pub fn reg_read(&mut self, offset: usize) -> u32 {
        // GMBUS3 data register: special — returns EDID bytes from state machine
        if offset == regs::GMBUS3 {
            return self.gmbus.read_data();
        }
        // Other GMBUS registers
        if gmbus::is_gmbus_reg(offset) {
            return self.gmbus.read(&self.regs, offset);
        }
        // GTT entries
        if offset >= regs::GTT_BASE && offset < regs::GTT_BASE + regs::GTT_SIZE {
            let entry_idx = (offset - regs::GTT_BASE) / 4;
            return self.gtt.read(entry_idx);
        }
        // Display engine
        if display::is_display_reg(offset) {
            return display::reg_read(&mut self.display, &mut self.regs, offset);
        }
        // Render engine
        if render::is_render_reg(offset) {
            return render::reg_read(&mut self.render, &self.regs, offset);
        }

        // Default: return raw register value
        let idx = offset / 4;
        if idx < self.regs.len() { self.regs[idx] } else { 0 }
    }

    /// Write a 32-bit MMIO register (BAR0).
    pub fn reg_write(&mut self, offset: usize, val: u32) {
        // GMBUS
        if gmbus::is_gmbus_reg(offset) {
            self.gmbus.write(&mut self.regs, offset, val);
            return;
        }
        // GTT entries
        if offset >= regs::GTT_BASE && offset < regs::GTT_BASE + regs::GTT_SIZE {
            let entry_idx = (offset - regs::GTT_BASE) / 4;
            self.gtt.write(entry_idx, val);
            return;
        }
        // Display engine
        if display::is_display_reg(offset) {
            display::reg_write(&mut self.display, &mut self.regs, &self.vram, &mut self.framebuffer, offset, val);
            return;
        }
        // Render engine
        if render::is_render_reg(offset) {
            render::reg_write(&mut self.render, &mut self.regs, offset, val);
            return;
        }

        // Default: store raw value
        let idx = offset / 4;
        if idx < self.regs.len() {
            self.regs[idx] = val;
        }
    }

    /// Copy the current display surface from VRAM to the host framebuffer.
    /// Called periodically from the VM loop.
    pub fn refresh_framebuffer(&mut self) {
        self.display.refresh_framebuffer(&self.vram, &mut self.framebuffer);
    }
}

// ── MMIO Handler for BAR0 (register space) ──────────────────────────────────

impl MmioHandler for IntelGpu {
    fn read(&mut self, addr: u64, size: u8) -> Result<u64> {
        let offset = (addr as usize) & (MMIO_SIZE - 1);
        let aligned = offset & !3;
        let val = self.reg_read(aligned);

        let shift = ((offset & 3) * 8) as u32;
        let result = (val >> shift) as u64;

        Ok(match size {
            1 => result & 0xFF,
            2 => result & 0xFFFF,
            4 => result & 0xFFFF_FFFF,
            8 => {
                let hi = self.reg_read(aligned + 4) as u64;
                (hi << 32) | (val as u64)
            }
            _ => result,
        })
    }

    fn write(&mut self, addr: u64, size: u8, val: u64) -> Result<()> {
        let offset = (addr as usize) & (MMIO_SIZE - 1);
        let aligned = offset & !3;

        match size {
            4 => self.reg_write(aligned, val as u32),
            8 => {
                self.reg_write(aligned, val as u32);
                self.reg_write(aligned + 4, (val >> 32) as u32);
            }
            2 => {
                let shift = ((offset & 3) * 8) as u32;
                let mask = 0xFFFFu32 << shift;
                let old = self.regs.get(aligned / 4).copied().unwrap_or(0);
                self.reg_write(aligned, (old & !mask) | (((val as u32) << shift) & mask));
            }
            1 => {
                let shift = ((offset & 3) * 8) as u32;
                let mask = 0xFFu32 << shift;
                let old = self.regs.get(aligned / 4).copied().unwrap_or(0);
                self.reg_write(aligned, (old & !mask) | (((val as u32) << shift) & mask));
            }
            _ => {}
        }
        Ok(())
    }
}

// ── VRAM Aperture MMIO Handler (BAR2) ───────────────────────────────────────

/// Handles guest reads/writes to the graphics aperture (GTT-mapped VRAM).
pub struct IntelGpuAperture(*mut IntelGpu);
unsafe impl Send for IntelGpuAperture {}

impl IntelGpuAperture {
    pub fn new(gpu: *mut IntelGpu) -> Self { Self(gpu) }
}

impl MmioHandler for IntelGpuAperture {
    fn read(&mut self, addr: u64, size: u8) -> Result<u64> {
        let gpu = unsafe { &mut *self.0 };
        let offset = addr as usize;
        if offset >= gpu.vram_size { return Ok(0); }

        Ok(match size {
            1 => gpu.vram.get(offset).copied().unwrap_or(0) as u64,
            2 if offset + 1 < gpu.vram_size => {
                u16::from_le_bytes([gpu.vram[offset], gpu.vram[offset + 1]]) as u64
            }
            4 if offset + 3 < gpu.vram_size => {
                u32::from_le_bytes([
                    gpu.vram[offset], gpu.vram[offset + 1],
                    gpu.vram[offset + 2], gpu.vram[offset + 3],
                ]) as u64
            }
            _ => 0,
        })
    }

    fn write(&mut self, addr: u64, size: u8, val: u64) -> Result<()> {
        let gpu = unsafe { &mut *self.0 };
        let offset = addr as usize;
        if offset >= gpu.vram_size { return Ok(()); }

        match size {
            1 => gpu.vram[offset] = val as u8,
            2 if offset + 1 < gpu.vram_size => {
                gpu.vram[offset..offset + 2].copy_from_slice(&(val as u16).to_le_bytes());
            }
            4 if offset + 3 < gpu.vram_size => {
                gpu.vram[offset..offset + 4].copy_from_slice(&(val as u32).to_le_bytes());
            }
            _ => {}
        }
        Ok(())
    }
}
