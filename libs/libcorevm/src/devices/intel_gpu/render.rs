//! Render engine / Command Streamer stubs.
//!
//! Provides minimal register handling for the render and BLT ring buffers
//! so that the i915 driver doesn't error out during initialization.
//! Actual command execution (3D, BLT) is not yet implemented.

extern crate alloc;
use alloc::vec::Vec;
use super::regs;

/// Render engine state (stub).
pub struct RenderEngine {
    /// Fence registers (tiling metadata).
    pub fences: [u64; regs::NUM_FENCES],
}

impl RenderEngine {
    pub fn new() -> Self {
        Self {
            fences: [0u64; regs::NUM_FENCES],
        }
    }
}

/// Initialize render-related registers with defaults.
pub fn init_registers(regs_file: &mut Vec<u32>) {
    // Render ring: disabled (CTL = 0)
    regs_file[regs::RENDER_RING_CTL / 4] = 0;
    regs_file[regs::RENDER_RING_HEAD / 4] = 0;
    regs_file[regs::RENDER_RING_TAIL / 4] = 0;

    // BLT ring: disabled
    regs_file[regs::BLT_RING_CTL / 4] = 0;
    regs_file[regs::BLT_RING_HEAD / 4] = 0;
    regs_file[regs::BLT_RING_TAIL / 4] = 0;
}

/// Check if an offset is a render engine register.
pub fn is_render_reg(offset: usize) -> bool {
    regs::is_render_range(offset)
}

/// Read a render engine register.
pub fn reg_read(render: &mut RenderEngine, regs_file: &[u32], offset: usize) -> u32 {
    // Fence registers (64-bit, stored as pairs of u32)
    if offset >= regs::FENCE_REG_BASE && offset < regs::FENCE_REG_BASE + regs::NUM_FENCES * 8 {
        let fence_idx = (offset - regs::FENCE_REG_BASE) / 8;
        let is_high = ((offset - regs::FENCE_REG_BASE) % 8) >= 4;
        if fence_idx < regs::NUM_FENCES {
            return if is_high {
                (render.fences[fence_idx] >> 32) as u32
            } else {
                render.fences[fence_idx] as u32
            };
        }
    }

    // GTT entries
    if offset >= regs::GTT_BASE && offset < regs::GTT_BASE + regs::GTT_SIZE {
        // GTT reads are handled by the main IntelGpu::reg_read via gtt module
        return 0;
    }

    // Ring buffer registers: return stored values
    // For HEAD registers, mirror TAIL to simulate "commands consumed instantly"
    match offset {
        regs::RENDER_RING_HEAD => regs_file.get(regs::RENDER_RING_TAIL / 4).copied().unwrap_or(0),
        regs::BLT_RING_HEAD => regs_file.get(regs::BLT_RING_TAIL / 4).copied().unwrap_or(0),
        regs::RENDER_RING_CTL => {
            // Report ring as idle (bit 9 = ring not empty → 0 = empty/idle)
            regs_file.get(offset / 4).copied().unwrap_or(0) & !(1 << 9)
        }
        regs::BLT_RING_CTL => {
            regs_file.get(offset / 4).copied().unwrap_or(0) & !(1 << 9)
        }
        _ => regs_file.get(offset / 4).copied().unwrap_or(0),
    }
}

/// Write a render engine register.
pub fn reg_write(render: &mut RenderEngine, regs_file: &mut Vec<u32>, offset: usize, val: u32) {
    // Fence registers
    if offset >= regs::FENCE_REG_BASE && offset < regs::FENCE_REG_BASE + regs::NUM_FENCES * 8 {
        let fence_idx = (offset - regs::FENCE_REG_BASE) / 8;
        let is_high = ((offset - regs::FENCE_REG_BASE) % 8) >= 4;
        if fence_idx < regs::NUM_FENCES {
            if is_high {
                render.fences[fence_idx] = (render.fences[fence_idx] & 0xFFFF_FFFF) | ((val as u64) << 32);
            } else {
                render.fences[fence_idx] = (render.fences[fence_idx] & 0xFFFF_FFFF_0000_0000) | (val as u64);
            }
        }
        return;
    }

    // GTT entries
    if offset >= regs::GTT_BASE && offset < regs::GTT_BASE + regs::GTT_SIZE {
        // GTT writes are handled by the main IntelGpu::reg_write via gtt module
        return;
    }

    // Ring buffer control: handle enable/disable
    match offset {
        regs::RENDER_RING_CTL | regs::BLT_RING_CTL => {
            let idx = offset / 4;
            if idx < regs_file.len() {
                // Accept the write. When ring is enabled (bit 0),
                // HEAD = TAIL (all commands "executed instantly").
                regs_file[idx] = val;
            }
        }
        _ => {
            let idx = offset / 4;
            if idx < regs_file.len() {
                regs_file[idx] = val;
            }
        }
    }
}
