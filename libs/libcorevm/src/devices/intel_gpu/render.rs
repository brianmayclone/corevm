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
    /// Monotonic timestamp counter (incremented on each read).
    pub timestamp: u32,
}

impl RenderEngine {
    pub fn new() -> Self {
        Self {
            fences: [0u64; regs::NUM_FENCES],
            forcewake_active: false,
            timestamp: 0,
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

    // FUSE_STRAP: GT configuration fuses
    // Bits 9:8 = GT type: 10 = GT2 (HD Graphics 530, Skylake)
    // Bit 25 = internal display available
    if regs::FUSE_STRAP / 4 < regs_file.len() {
        regs_file[regs::FUSE_STRAP / 4] = (2 << 8) | (1 << 25);
    }

    // GEN6_GT_THREAD_STATUS_REG: report all threads idle (0)
    if regs::GEN6_GT_THREAD_STATUS_REG / 4 < regs_file.len() {
        regs_file[regs::GEN6_GT_THREAD_STATUS_REG / 4] = 0;
    }

    // GEN6_MBCTL: set defaults (enable boot fetch)
    if regs::GEN6_MBCTL / 4 < regs_file.len() {
        regs_file[regs::GEN6_MBCTL / 4] = 0;
    }

    // GEN6_UCGCTL1: default clock gating (all units clocked)
    if regs::GEN6_UCGCTL1 / 4 < regs_file.len() {
        regs_file[regs::GEN6_UCGCTL1 / 4] = 0;
    }

    // MI_MODE: default
    if regs::MI_MODE / 4 < regs_file.len() {
        regs_file[regs::MI_MODE / 4] = 0;
    }

    // GEN6_GT_PERF_STATUS: report current frequency = RP1 (800 MHz)
    if regs::GEN6_GT_PERF_STATUS / 4 < regs_file.len() {
        regs_file[regs::GEN6_GT_PERF_STATUS / 4] = 0x10 << 8; // RP1 = 16 in 50MHz units
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

        // GPU Reset: return 0 = "reset completed" (all request bits cleared)
        regs::GEN6_GDRST => 0,

        // FUSE_STRAP: GT2 (Skylake), internal display
        regs::FUSE_STRAP => (2 << 8) | (1 << 25),

        // Timestamp: monotonically incrementing counter
        regs::TIMESTAMP => {
            render.timestamp = render.timestamp.wrapping_add(1000);
            render.timestamp
        }

        // GT Thread Status: all idle
        regs::GEN6_GT_THREAD_STATUS_REG => 0,

        // GT Perf Status: report current freq = RP1
        regs::GEN6_GT_PERF_STATUS => 0x10 << 8,

        // RP State Limits
        regs::GEN6_RP_STATE_LIMITS => 0x06_10_14,

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
        // GPU Reset: driver writes bits to request reset, then polls
        // until those bits are cleared (= reset complete).
        // We clear the bits immediately to simulate instant reset.
        regs::GEN6_GDRST => {
            // Reset requested — acknowledge by NOT storing the value.
            // When the driver reads GEN6_GDRST, it gets 0 (all clear),
            // which means "reset done". We also re-initialize ring state.
            if val & 1 != 0 {
                // Full GPU reset: clear all ring state
                regs_file[regs::RENDER_RING_CTL / 4] = 0;
                regs_file[regs::RENDER_RING_HEAD / 4] = 0;
                regs_file[regs::RENDER_RING_TAIL / 4] = 0;
                regs_file[regs::BLT_RING_CTL / 4] = 0;
                regs_file[regs::BLT_RING_HEAD / 4] = 0;
                regs_file[regs::BLT_RING_TAIL / 4] = 0;
                regs_file[regs::BSD_RING_CTL / 4] = 0;
                regs_file[regs::BSD_RING_HEAD / 4] = 0;
                regs_file[regs::BSD_RING_TAIL / 4] = 0;
            }
            // Do NOT store val — reading back 0 signals "reset done"
        }

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

        // Masked registers (MI_MODE, CACHE_MODE, GFX_MODE, INSTPM, etc.):
        // Upper 16 bits = mask, lower 16 bits = value.
        // Only bits with corresponding mask bit set are modified.
        regs::MI_MODE | regs::CACHE_MODE_0 | regs::CACHE_MODE_1 |
        regs::GFX_MODE | regs::INSTPM => {
            let idx = offset / 4;
            if idx < regs_file.len() {
                let mask = (val >> 16) & 0xFFFF;
                let bits = val & 0xFFFF;
                let old = regs_file[idx] & 0xFFFF;
                regs_file[idx] = (old & !mask) | (bits & mask);
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
