//! Render engine / Command Streamer / Power Management stubs.
//!
//! Provides register handling for render, BLT, and BSD ring buffers,
//! forcewake protocol, and power management so the i915/igfx driver
//! can initialize without errors. No actual command execution yet.

extern crate alloc;
use alloc::vec::Vec;
use super::regs;

/// Render engine state.
pub struct RenderEngine {
    /// Fence registers (tiling metadata).
    pub fences: [u64; regs::NUM_FENCES],
    /// Forcewake is active (driver claimed GT access).
    pub forcewake_active: bool,
}

impl RenderEngine {
    pub fn new() -> Self {
        Self {
            fences: [0u64; regs::NUM_FENCES],
            forcewake_active: false,
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

    // BSD ring: disabled
    regs_file[regs::BSD_RING_CTL / 4] = 0;
    regs_file[regs::BSD_RING_HEAD / 4] = 0;
    regs_file[regs::BSD_RING_TAIL / 4] = 0;

    // GT FIFO: report plenty of free entries (driver polls this before register writes)
    regs_file[regs::GT_FIFO_FREE_ENTRIES / 4] = 0x3F; // 63 free entries (max)

    // Forcewake ACK: report acknowledged (so driver thinks GT is awake)
    if regs::FORCEWAKE_ACK / 4 < regs_file.len() {
        regs_file[regs::FORCEWAKE_ACK / 4] = 1; // bit 0 = forcewake acknowledged
    }

    // RP STATE CAP: report min/max/RP1 GPU frequencies
    // Bits 7:0 = RP0 (max), 15:8 = RP1 (normal), 23:16 = RPn (min)
    // Values in 50 MHz units: RP0=20(1GHz), RP1=16(800MHz), RPn=6(300MHz)
    if regs::GEN6_RP_STATE_CAP / 4 < regs_file.len() {
        regs_file[regs::GEN6_RP_STATE_CAP / 4] = 0x06_10_14;
    }
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

    // GTT entries — handled by main IntelGpu via gtt module
    if offset >= regs::GTT_BASE && offset < regs::GTT_BASE + regs::GTT_SIZE {
        return 0;
    }

    match offset {
        // Ring HEAD registers: mirror TAIL to simulate "commands consumed instantly"
        regs::RENDER_RING_HEAD => regs_file.get(regs::RENDER_RING_TAIL / 4).copied().unwrap_or(0),
        regs::BLT_RING_HEAD => regs_file.get(regs::BLT_RING_TAIL / 4).copied().unwrap_or(0),
        regs::BSD_RING_HEAD => regs_file.get(regs::BSD_RING_TAIL / 4).copied().unwrap_or(0),

        // Ring CTL: report ring idle (clear bit 9 = "ring empty")
        regs::RENDER_RING_CTL | regs::BLT_RING_CTL | regs::BSD_RING_CTL => {
            regs_file.get(offset / 4).copied().unwrap_or(0) & !(1 << 9)
        }

        // Forcewake ACK: always report acknowledged
        regs::FORCEWAKE_ACK => {
            if render.forcewake_active { 1 } else { 0 }
        }

        // GT FIFO free entries: always report plenty of room
        regs::GT_FIFO_FREE_ENTRIES => 0x3F,

        // RP STATE CAP: GPU frequency capabilities
        regs::GEN6_RP_STATE_CAP => 0x06_10_14,

        // Default: return stored value (bounds-checked)
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

    // GTT entries — handled by main IntelGpu via gtt module
    if offset >= regs::GTT_BASE && offset < regs::GTT_BASE + regs::GTT_SIZE {
        return;
    }

    match offset {
        // Forcewake: driver writes to claim/release GT access
        regs::FORCEWAKE | regs::FORCEWAKE_MT => {
            // Bit 0 = set forcewake, bit 16 = mask bit for bit 0
            if val & (1 << 16) != 0 {
                render.forcewake_active = val & 1 != 0;
            } else {
                render.forcewake_active = val & 1 != 0;
            }
            // Immediately acknowledge
            if regs::FORCEWAKE_ACK / 4 < regs_file.len() {
                regs_file[regs::FORCEWAKE_ACK / 4] = if render.forcewake_active { 1 } else { 0 };
            }
        }

        // Ring buffer control: accept the write
        regs::RENDER_RING_CTL | regs::BLT_RING_CTL | regs::BSD_RING_CTL => {
            let idx = offset / 4;
            if idx < regs_file.len() {
                regs_file[idx] = val;
            }
        }

        // Power management: accept writes silently
        regs::GEN6_RP_CONTROL | regs::GEN6_RPNSWREQ |
        regs::GEN6_RP_UP_THRESHOLD | regs::GEN6_RP_DOWN_THRESHOLD |
        regs::GEN6_PMINTRMSK => {
            let idx = offset / 4;
            if idx < regs_file.len() {
                regs_file[idx] = val;
            }
        }

        // Default: store the value (bounds-checked)
        _ => {
            let idx = offset / 4;
            if idx < regs_file.len() {
                regs_file[idx] = val;
            }
        }
    }
}
