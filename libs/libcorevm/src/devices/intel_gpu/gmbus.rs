//! GMBUS (I2C) controller for DDC/EDID readout.
//!
//! The i915 driver uses GMBUS to read the EDID from connected displays.
//! We emulate a minimal GMBUS controller that responds to DDC reads
//! (slave address 0x50) with our built-in EDID block.

use super::regs;
use super::edid;

const STATE_IDLE: u8 = 0;
const STATE_READING: u8 = 1;

/// GMBUS I2C controller state.
pub struct GmbusController {
    state: u8,
    /// Byte index into EDID data.
    index: usize,
    /// Bytes remaining in current transfer.
    remaining: u32,
    /// Target slave address.
    slave_addr: u8,
}

impl GmbusController {
    pub fn new() -> Self {
        Self {
            state: STATE_IDLE,
            index: 0,
            remaining: 0,
            slave_addr: 0,
        }
    }

    /// Read a GMBUS register.
    pub fn read(&self, regs_file: &[u32], offset: usize) -> u32 {
        match offset {
            regs::GMBUS0 => regs_file[offset / 4],
            regs::GMBUS1 => regs_file[offset / 4],
            regs::GMBUS2 => {
                let mut status = regs_file[offset / 4];
                // bit 14 = HW_RDY (always set — we're always ready)
                status |= 1 << 14;
                if self.state == STATE_READING && self.remaining > 0 {
                    // bit 11 = HW_RDY with data
                    status |= 1 << 11;
                } else {
                    // bit 11 = idle
                    status |= 1 << 11;
                }
                status
            }
            regs::GMBUS3 => {
                // Should not be called directly — use read_data()
                0
            }
            _ => regs_file.get(offset / 4).copied().unwrap_or(0),
        }
    }

    /// Read GMBUS3 data register (returns up to 4 bytes of EDID).
    pub fn read_data(&mut self) -> u32 {
        if self.state != STATE_READING { return 0; }

        let mut val: u32 = 0;
        for i in 0..4u32 {
            if self.remaining == 0 { break; }
            let byte = if self.index < edid::DEFAULT_EDID.len() {
                edid::DEFAULT_EDID[self.index]
            } else {
                0
            };
            val |= (byte as u32) << (i * 8);
            self.index += 1;
            self.remaining -= 1;
        }

        if self.remaining == 0 {
            self.state = STATE_IDLE;
        }

        val
    }

    /// Write a GMBUS register.
    pub fn write(&mut self, regs_file: &mut [u32], offset: usize, val: u32) {
        match offset {
            regs::GMBUS0 => {
                // Clock/port select — just store it
                regs_file[offset / 4] = val;
            }
            regs::GMBUS1 => {
                regs_file[offset / 4] = val;
                self.handle_command(regs_file, val);
            }
            regs::GMBUS2 => {
                // Status register — guest writes to clear bits (W1C for some)
                // bit 10 = NAK, bit 9 = timeout — clear on write
                regs_file[offset / 4] &= !(val & ((1 << 10) | (1 << 9)));
            }
            _ => {
                if let Some(reg) = regs_file.get_mut(offset / 4) {
                    *reg = val;
                }
            }
        }
    }

    fn handle_command(&mut self, regs_file: &mut [u32], val: u32) {
        // bit 30 = SW_RDY (initiate transfer)
        if val & (1 << 30) == 0 {
            // SW_CLR_INT or stop — reset
            if val & (1 << 31) != 0 {
                self.state = STATE_IDLE;
                // Clear NAK/timeout
                regs_file[regs::GMBUS2 / 4] = (1 << 14) | (1 << 11);
            }
            return;
        }

        let slave_addr = ((val >> 1) & 0x7F) as u8;
        let is_read = val & 1 != 0;
        let byte_count = (val >> 16) & 0x1FF;

        if slave_addr == 0x50 && is_read {
            // DDC EDID read
            self.slave_addr = slave_addr;
            self.state = STATE_READING;
            self.index = 0;
            self.remaining = byte_count;
            // Status: HW_RDY, no NAK
            regs_file[regs::GMBUS2 / 4] = (1 << 14) | (1 << 11);
        } else if slave_addr == 0x50 && !is_read {
            // DDC write (setting EDID offset) — just accept it
            self.index = 0; // Reset to start of EDID
            regs_file[regs::GMBUS2 / 4] = (1 << 14) | (1 << 11);
        } else {
            // Unknown slave → NAK
            regs_file[regs::GMBUS2 / 4] = (1 << 14) | (1 << 11) | (1 << 10);
            self.state = STATE_IDLE;
        }
    }
}

/// Check if an offset is a GMBUS register.
pub fn is_gmbus_reg(offset: usize) -> bool {
    offset >= regs::GMBUS_RANGE_START && offset < regs::GMBUS_RANGE_END
}
