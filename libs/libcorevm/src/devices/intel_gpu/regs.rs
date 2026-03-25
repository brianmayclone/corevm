//! Intel HD Graphics register offset constants (Sandy Bridge / Gen6).
//!
//! Register offsets are relative to BAR0 MMIO base.
//! Organized by functional block as in the Intel PRM (Programmer's Reference Manual).

// ── General / Clocking ──────────────────────────────────────────────────────

pub const FUSE_STRAP: usize = 0x42014;

// ── GMBUS (I2C / DDC for EDID) ─────────────────────────────────────────────

pub const GMBUS0: usize = 0x5100;  // Clock/Port select
pub const GMBUS1: usize = 0x5104;  // Command/Status
pub const GMBUS2: usize = 0x5108;  // Status
pub const GMBUS3: usize = 0x510C;  // Data buffer
pub const GMBUS4: usize = 0x5110;  // Interrupt mask
pub const GMBUS5: usize = 0x5120;  // 2-byte index

pub const GMBUS_RANGE_START: usize = GMBUS0;
pub const GMBUS_RANGE_END: usize = GMBUS5 + 4;

// ── Display PLLs ────────────────────────────────────────────────────────────

pub const DPLL_A_CTRL: usize = 0x06014;
pub const DPLL_B_CTRL: usize = 0x06018;
pub const FPA0: usize = 0x06040;
pub const FPA1: usize = 0x06044;
pub const FPB0: usize = 0x06048;
pub const FPB1: usize = 0x0604C;
pub const DPLL_A_MD: usize = 0x0601C;  // DPLL A multiplier/divisor

// ── HTOTAL / VTOTAL (Pipe timing) ───────────────────────────────────────────

pub const HTOTAL_A: usize = 0x60000;
pub const HBLANK_A: usize = 0x60004;
pub const HSYNC_A: usize = 0x60008;
pub const VTOTAL_A: usize = 0x6000C;
pub const VBLANK_A: usize = 0x60010;
pub const VSYNC_A: usize = 0x60014;
pub const PIPEASRC: usize = 0x6001C;  // Pipe A source image size

pub const HTOTAL_B: usize = 0x61000;
pub const PIPEBSRC: usize = 0x6101C;

// ── Pipe Configuration ──────────────────────────────────────────────────────

pub const PIPEACONF: usize = 0x70008;
pub const PIPEASTAT: usize = 0x70024;  // Pipe A status (vblank, etc.)
pub const PIPEBCONF: usize = 0x71008;

// ── Display Plane A (Primary) ───────────────────────────────────────────────

pub const DSPACNTR: usize = 0x70180;   // Control
pub const DSPALINOFF: usize = 0x70184; // Linear offset
pub const DSPASTRIDE: usize = 0x70188; // Stride (bytes per scanline)
pub const DSPASURF: usize = 0x7019C;   // Surface base address (GTT offset)
pub const DSPATILEOFF: usize = 0x701A4; // Tile offset
pub const DSPASIZE: usize = 0x70190;   // Size (used by some drivers)
pub const DSPACNTR_END: usize = 0x701B0;

// ── Display Plane B (Primary) ───────────────────────────────────────────────

pub const DSPBCNTR: usize = 0x71180;
pub const DSPBLINOFF: usize = 0x71184;
pub const DSPBSTRIDE: usize = 0x71188;
pub const DSPBSURF: usize = 0x7119C;

// ── Cursor A ────────────────────────────────────────────────────────────────

pub const CURACNTR: usize = 0x70080;
pub const CURABASE: usize = 0x70084;
pub const CURAPOS: usize = 0x70088;

// ── Cursor B ────────────────────────────────────────────────────────────────

pub const CURBCNTR: usize = 0x700C0;
pub const CURBBASE: usize = 0x700C4;
pub const CURBPOS: usize = 0x700C8;

// ── Transcoder / Port ───────────────────────────────────────────────────────

pub const HDMIB: usize = 0xE1140;
pub const HDMIC: usize = 0xE1150;
pub const HDMID: usize = 0xE1160;
pub const DP_A: usize = 0x64000;  // eDP port A
pub const PCH_DP_B: usize = 0xE4100;
pub const LVDS: usize = 0xE1180;  // LVDS port

// ── Interrupt Registers ─────────────────────────────────────────────────────

pub const DEIMR: usize = 0x44004;  // DE interrupt mask
pub const DEIIR: usize = 0x44008;  // DE interrupt identity (W1C)
pub const DEIER: usize = 0x4400C;  // DE interrupt enable
pub const GTIMR: usize = 0x44014;  // GT interrupt mask
pub const GTIIR: usize = 0x44018;  // GT interrupt identity (W1C)
pub const GTIER: usize = 0x4401C;  // GT interrupt enable
pub const SDEIMR: usize = 0xC4004; // South DE interrupt mask
pub const SDEIIR: usize = 0xC4008; // South DE interrupt identity
pub const SDEIER: usize = 0xC400C; // South DE interrupt enable

// ── Render Engine (GT) ──────────────────────────────────────────────────────

pub const RENDER_RING_BASE: usize = 0x02000;  // Ring buffer base
pub const RENDER_RING_CTL: usize = 0x0203C;   // Ring buffer control
pub const RENDER_RING_HEAD: usize = 0x02034;   // Ring head pointer
pub const RENDER_RING_TAIL: usize = 0x02030;   // Ring tail pointer
pub const RENDER_HWS_PGA: usize = 0x02080;    // HW status page address

pub const BLT_RING_BASE: usize = 0x22000;     // BLT ring buffer base
pub const BLT_RING_CTL: usize = 0x2203C;      // BLT ring buffer control
pub const BLT_RING_HEAD: usize = 0x22034;
pub const BLT_RING_TAIL: usize = 0x22030;
pub const BLT_HWS_PGA: usize = 0x22080;

// ── GFX_MODE / MI registers ─────────────────────────────────────────────────

pub const GFX_MODE: usize = 0x02520;
pub const INSTPM: usize = 0x020C0;
pub const HWS_PGA: usize = 0x02080;

// ── Fence Registers (tiling) ────────────────────────────────────────────────

pub const FENCE_REG_BASE: usize = 0x100000;
pub const NUM_FENCES: usize = 16;

// ── GTT (Graphics Translation Table) ────────────────────────────────────────

/// GTT entries start at BAR0 + 0x80000 on Sandy Bridge.
/// Each entry is 4 bytes: physical address >> 12 | flags.
pub const GTT_BASE: usize = 0x80000;
pub const GTT_SIZE: usize = 0x80000; // 512 KB = 128K entries × 4 bytes = 512 MB addressable

// ── Display register range checks ──────────────────────────────────────────

/// Check if offset is in the display engine register space.
pub fn is_display_range(offset: usize) -> bool {
    // Pipe config, timing, plane, cursor
    (0x60000..0x72000).contains(&offset)
        // PLLs
        || (0x06000..0x06100).contains(&offset)
        // Interrupt (north + south display)
        || (0x44000..0x44030).contains(&offset)
        || (0xC4000..0xC4010).contains(&offset)
        // Transcoder/port (south display engine)
        || (0xE1100..0xE1200).contains(&offset)
        || (0x64000..0x64100).contains(&offset)
        // LVDS, PCH ports
        || (0xE4000..0xE4200).contains(&offset)
        // Pipe A stat
        || offset == PIPEASTAT
}

/// Check if offset is in the render engine register space.
pub fn is_render_range(offset: usize) -> bool {
    (0x02000..0x02600).contains(&offset)
        || (0x020C0..0x020D0).contains(&offset)
        || (0x22000..0x22100).contains(&offset)
        || (offset >= FENCE_REG_BASE && offset < FENCE_REG_BASE + NUM_FENCES * 8)
        || (offset >= GTT_BASE && offset < GTT_BASE + GTT_SIZE)
}
