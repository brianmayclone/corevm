//! Q35 Memory Controller Hub (MCH) emulation.
//!
//! Minimal implementation for SeaBIOS Q35 detection and MMCONFIG discovery.
//! The MCH is a standard PCI device at 00:00.0 with device ID 8086:29C0.
//!
//! Key registers:
//! - PCIEXBAR (0x60): PCIe extended config space base address
//! - PAM0-PAM6 (0x90-0x96): Programmable Attribute Map (ROM shadowing)
//! - SMRAM (0x9D): System Management RAM control
//! - ESMRAMC (0x9E): Extended SMRAM control
//! - TSEG (0xA8-0xAB): TSEG memory base

use crate::devices::bus::PciDevice;

/// PCIEXBAR register offset in MCH config space.
const PCIEXBAR_OFFSET: usize = 0x60;

/// PAM (Programmable Attribute Map) register offsets.
const PAM0_OFFSET: usize = 0x90;

/// SMRAM register offset.
const SMRAM_OFFSET: usize = 0x9D;

/// Create a Q35 MCH PCI device with the correct config space layout.
///
/// SeaBIOS detects Q35 by reading vendor/device ID (8086:29C0) at 00:00.0
/// and then reads PCIEXBAR to discover the MMCONFIG region.
pub fn create_q35_mch(mmconfig_base: u64) -> PciDevice {
    let mut mch = PciDevice::new(
        0x8086, // Intel
        0x29C0, // Q35 MCH
        0x06,   // Bridge
        0x00,   // Host bridge
        0x00,   // prog-if
    );
    mch.device = 0;
    mch.function = 0;

    // Revision ID
    mch.config_space[0x08] = 0x02;

    // Header type 0 (single-function for MCH itself)
    mch.config_space[0x0E] = 0x00;

    // PCIEXBAR (offset 0x60, 8 bytes): encodes MMCONFIG base address.
    // Bits 63:28 = base address (256MB aligned)
    // Bits 2:1 = length (00=256MB, 01=128MB, 10=64MB)
    // Bit 0 = enable
    let pciexbar_val: u64 = (mmconfig_base & 0xFFFF_FFFF_F000_0000) | 0x01; // enabled, 256MB
    mch.config_space[PCIEXBAR_OFFSET]     = (pciexbar_val & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 1] = ((pciexbar_val >> 8) & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 2] = ((pciexbar_val >> 16) & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 3] = ((pciexbar_val >> 24) & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 4] = ((pciexbar_val >> 32) & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 5] = ((pciexbar_val >> 40) & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 6] = ((pciexbar_val >> 48) & 0xFF) as u8;
    mch.config_space[PCIEXBAR_OFFSET + 7] = ((pciexbar_val >> 56) & 0xFF) as u8;

    // PAM registers (0x90-0x96): Programmable Attribute Map.
    // Controls ROM shadowing for 0xC0000-0xFFFFF regions.
    // Default: all read-write (0x33 = read from DRAM, write to DRAM).
    for i in 0..7 {
        mch.config_space[PAM0_OFFSET + i] = 0x33;
    }

    // SMRAM (0x9D): System Management RAM.
    // Bit 4 (D_OPEN) = 0 (SMRAM not open to non-SMM code)
    // Bit 3 (D_CLS) = 0 (SMRAM not closed/locked)
    // Bit 1 (G_SMRAME) = 0 (global SMRAM not enabled)
    mch.config_space[SMRAM_OFFSET] = 0x02; // D_LCK=0, D_CLS=0, D_OPEN=0, G_SMRAME=1

    // ESMRAMC (0x9E): Extended SMRAM.
    mch.config_space[0x9E] = 0x38; // T_EN=1, TSEG_SZ=11 (8MB), H_SMRAME=1

    // TOLUD (Top of Low Usable DRAM, offset 0xB0) — set by firmware.
    // Default to 0 (firmware will configure).

    mch
}
