//! Intel Graphics OpRegion (ACPI IGD OpRegion).
//!
//! The Intel igfx driver on Windows (and i915 on Linux) reads the OpRegion
//! address from PCI config register ASLS (offset 0xFC).  That address points
//! to a structure in guest physical memory containing:
//!
//!   - Header (256 bytes) — signature "IntelGraphicsMem", version, size, mailbox flags
//!   - Mailbox 1 (ACPI, 256 bytes) — public ACPI fields
//!   - Mailbox 2 (SWSCI, 256 bytes) — software SCI interface
//!   - Mailbox 3 (ASLE, 256 bytes) — ASLE (backlight, ALS, etc.)
//!   - VBT (Video BIOS Table, ~6 KB) — panel/output configuration
//!
//! The total OpRegion is 8 KB (0x2000 bytes).  We build a minimal but valid
//! OpRegion that satisfies the Windows igfx driver initialization checks.

/// Total OpRegion size: 8 KB.
pub const OPREGION_SIZE: usize = 0x2000;

/// Build a minimal Intel Graphics OpRegion (8 KB) for Sandy Bridge.
///
/// Returns a fixed-size byte array that can be written into guest RAM.
pub fn build_opregion() -> [u8; OPREGION_SIZE] {
    let mut op = [0u8; OPREGION_SIZE];

    // ═══════════════════════════════════════════════════════════════
    // Header (offset 0x000 – 0x0FF, 256 bytes)
    // ═══════════════════════════════════════════════════════════════

    // 0x00: Signature — "IntelGraphicsMem" (16 bytes)
    let sig = b"IntelGraphicsMem";
    op[0x00..0x10].copy_from_slice(sig);

    // 0x10: Size of OpRegion in KB (u32 LE) — 8 KB
    write_u32(&mut op, 0x10, 8);

    // 0x14: OpRegion version (u32 LE)
    // Version 2.1 (major=2, minor=1) — Skylake era
    // Format: (major << 16) | minor
    write_u32(&mut op, 0x14, 0x0002_0001);

    // 0x18: System BIOS Build Version (null-terminated string, 32 bytes)
    let bios_ver = b"CoreVM VBIOS 1.0";
    op[0x18..0x18 + bios_ver.len()].copy_from_slice(bios_ver);

    // 0x38: Video BIOS Build Version (null-terminated string, 16 bytes)
    let vbios_ver = b"SKL.VBIOS.1052";
    op[0x38..0x38 + vbios_ver.len()].copy_from_slice(vbios_ver);

    // 0x48: Graphics BIOS Build Version (null-terminated string, 16 bytes)
    let gfx_ver = b"GFX.1000";
    op[0x48..0x48 + gfx_ver.len()].copy_from_slice(gfx_ver);

    // 0x58: Supported Mailboxes (u32 LE)
    // Bit 0 = Mailbox 1 (ACPI) supported
    // Bit 1 = Mailbox 2 (SWSCI) supported
    // Bit 2 = Mailbox 3 (ASLE) supported
    // Bit 3 = Mailbox 4 (VBT) supported
    write_u32(&mut op, 0x58, 0x0F); // All four mailboxes supported

    // 0x5C: Driver Notifications (DMOD — u32 LE)
    // Bit 0 = driver loaded notification supported
    write_u32(&mut op, 0x5C, 0x01);

    // 0x60: PCIe Configuration Base Address (PCON — u32 LE)
    // MMCFG base — 0 = not used (use PCI CF8/CFC)
    write_u32(&mut op, 0x60, 0);

    // ═══════════════════════════════════════════════════════════════
    // Mailbox 1 — ACPI (offset 0x100 – 0x1FF, 256 bytes)
    // ═══════════════════════════════════════════════════════════════

    // 0x100: DRDY — Driver Readiness (u32 LE)
    // Set to 1 = driver is ready to receive notifications
    write_u32(&mut op, 0x100, 0x01);

    // 0x104: CSTS — Completion Status (u32 LE)
    // 0 = no pending completion
    write_u32(&mut op, 0x104, 0x00);

    // 0x108: CEVT — Current Event (u32 LE)
    write_u32(&mut op, 0x108, 0x00);

    // 0x110: DIDL — Supported Display Devices List (8 × u32)
    // Entry format: port type | port number
    // Device 0: Integrated LVDS/eDP (type 0x400 = internal flat panel)
    write_u32(&mut op, 0x110, 0x80010400);
    // Device 1: HDMI-B (type 0x100 = CRT, but we use 0x300 = DFP for HDMI)
    write_u32(&mut op, 0x114, 0x80010300);

    // 0x130: CPDL — Currently present device list (8 × u32)
    write_u32(&mut op, 0x130, 0x80010400); // eDP present
    write_u32(&mut op, 0x134, 0x80010300); // HDMI present

    // 0x150: CADL — Currently active device list (8 × u32)
    write_u32(&mut op, 0x150, 0x80010400); // eDP active

    // 0x170: NADL — Next active device list (8 × u32)
    write_u32(&mut op, 0x170, 0x80010400);

    // 0x190: APTS — Active Panel Technology Setting (u32)
    write_u32(&mut op, 0x190, 0x00);

    // ═══════════════════════════════════════════════════════════════
    // Mailbox 2 — SWSCI (offset 0x200 – 0x2FF, 256 bytes)
    // ═══════════════════════════════════════════════════════════════

    // 0x200: SCIC — SCI command/status (u32)
    // Bit 0 = SCI trigger (driver sets, BIOS clears)
    // We leave it at 0 (no pending SCI).
    write_u32(&mut op, 0x200, 0x00);

    // 0x204: PARM — SCI command parameter (u32)
    write_u32(&mut op, 0x204, 0x00);

    // 0x208: DSLP — Driver Sleep Timeout (u32, in ms)
    // How long driver waits for BIOS to handle SCI
    write_u32(&mut op, 0x208, 1500); // 1.5 seconds (typical)

    // ═══════════════════════════════════════════════════════════════
    // Mailbox 3 — ASLE (offset 0x300 – 0x3FF, 256 bytes)
    // ═══════════════════════════════════════════════════════════════

    // 0x300: ARDY — ASLE Ready (u32)
    // Bit 0 = ASLE ready to accept requests
    write_u32(&mut op, 0x300, 0x01);

    // 0x304: APTS — ASLE result (u32)
    write_u32(&mut op, 0x304, 0x00);

    // 0x308: PFMB — PWM Freq and Min Brightness (u32)
    // Bits 15:0 = min brightness (0), bits 31:16 = PWM freq divisor
    write_u32(&mut op, 0x308, 0x0000_0000);

    // 0x30C: CBLV — Current Backlight Value (u32)
    // Bit 31 = valid, bits 7:0 = brightness level (100%)
    write_u32(&mut op, 0x30C, 0x8000_0064); // valid, 100% brightness

    // 0x310: BCLM — Backlight Brightness Levels Table (20 × u16, 40 bytes)
    // Provide a simple linear ramp
    for i in 0u16..20 {
        let level = ((i + 1) * 5) as u16; // 5%, 10%, ... 100%
        let offset = 0x310 + (i as usize) * 2;
        op[offset] = (level & 0xFF) as u8;
        op[offset + 1] = ((level >> 8) & 0xFF) as u8;
    }

    // 0x338: CPFM — Current Panel Fitting Mode (u32)
    write_u32(&mut op, 0x338, 0x00);

    // 0x33C: EPFM — Enabled Panel Fitting Mode (u32)
    write_u32(&mut op, 0x33C, 0x00);

    // 0x340: PLUT — Panel LUT Identifier (u32)
    write_u32(&mut op, 0x340, 0x00);

    // ═══════════════════════════════════════════════════════════════
    // Mailbox 4 — VBT (offset 0x400 – 0x1FFF)
    // ═══════════════════════════════════════════════════════════════
    // Video BIOS Table — minimal valid VBT for Sandy Bridge
    build_minimal_vbt(&mut op);

    op
}

/// Build a minimal Video BIOS Table (VBT) at OpRegion offset 0x400.
///
/// The VBT contains the BDB (BIOS Data Block) which describes
/// panel type, display outputs, and timing parameters.
fn build_minimal_vbt(op: &mut [u8; OPREGION_SIZE]) {
    let base = 0x400;

    // ── VBT Header (offset 0x400, 32 bytes) ──
    // Signature: "$VBT" (20 bytes, null-padded)
    op[base] = b'$';
    op[base + 1] = b'V';
    op[base + 2] = b'B';
    op[base + 3] = b'T';

    // VBT version: 1.0 (u16 LE at offset+4)
    write_u16(op, base + 4, 0x0100);

    // Header size: 32 bytes (u16 LE at offset+6)
    write_u16(op, base + 6, 32);

    // VBT size: total VBT data size (u16 LE at offset+8)
    // We'll use a small VBT: 512 bytes total
    let vbt_size: u16 = 512;
    write_u16(op, base + 8, vbt_size);

    // VBT checksum (offset+10): we compute it after filling data
    // offset+11: reserved
    // offset+12: BDB offset from VBT start (u32 LE)
    write_u32(op, base + 12, 32); // BDB starts right after VBT header

    // offset+16..31: reserved / AIMS (all zero is fine)

    // ── BDB Header (offset 0x420, 16 bytes) ──
    let bdb_base = base + 32;

    // BDB Signature: "BIOS_DATA_BLOCK " (16 bytes)
    let bdb_sig = b"BIOS_DATA_BLOCK ";
    op[bdb_base..bdb_base + 16].copy_from_slice(bdb_sig);

    // BDB version: 209 (u16 LE at bdb_base+16) — Skylake era
    write_u16(op, bdb_base + 16, 209);

    // BDB header size: 22 bytes (u16 LE at bdb_base+18)
    write_u16(op, bdb_base + 18, 22);

    // BDB size including header: 480 bytes (= vbt_size - vbt_header(32))
    write_u16(op, bdb_base + 20, vbt_size - 32);

    // ── BDB Block 1: General Features (ID=1) ──
    let blk1 = bdb_base + 22; // after BDB header
    op[blk1] = 1;  // Block ID = 1 (General Features)
    // Block size (u16 LE): 12 bytes of payload
    write_u16(op, blk1 + 1, 12);
    // General Features payload:
    // Byte 0: boot display type = 0 (LFP)
    op[blk1 + 3] = 0x00;
    // Byte 1: panel fitting, int TV, etc.
    op[blk1 + 4] = 0x00;
    // Byte 2-3: legacy CRT/DVO flags
    op[blk1 + 5] = 0x00;
    op[blk1 + 6] = 0x00;
    // Byte 4: display pipe/port for LFP
    op[blk1 + 7] = 0x00;
    // Byte 5: misc bits
    op[blk1 + 8] = 0x04; // DVO hot plug, connected standby
    // Bytes 6-11: reserved
}

/// Write a little-endian u32 into a byte slice.
fn write_u32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset] = (val & 0xFF) as u8;
    buf[offset + 1] = ((val >> 8) & 0xFF) as u8;
    buf[offset + 2] = ((val >> 16) & 0xFF) as u8;
    buf[offset + 3] = ((val >> 24) & 0xFF) as u8;
}

/// Write a little-endian u16 into a byte slice.
fn write_u16(buf: &mut [u8], offset: usize, val: u16) {
    buf[offset] = (val & 0xFF) as u8;
    buf[offset + 1] = ((val >> 8) & 0xFF) as u8;
}
