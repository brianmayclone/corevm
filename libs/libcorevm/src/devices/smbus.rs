//! ICH9 SMBus controller stub.
//!
//! OVMF probes the ICH9 SMBus controller during platform initialization
//! and performs SMBus transactions (e.g., SPD reads for memory detection).
//!
//! The SMBus I/O registers (relative to SMBus Base Address = 0x0CC0):
//!
//! | Offset | Register           | Key Bits                              |
//! |--------|--------------------|---------------------------------------|
//! | 0x00   | Host Status (HST_STS) | bit 0: HOST_BUSY, bit 1: INTR (done) |
//! | 0x02   | Host Control       | bit 6: START                          |
//! | 0x03   | Host Command       | SMBus command byte                    |
//! | 0x04   | Transmit Slave Addr| Target device address                 |
//! | 0x05   | Data 0             | Data byte 0                           |
//! | 0x06   | Data 1             | Data byte 1                           |
//! | 0x07   | Block Data         | Block data byte                       |
//! | 0x20   | Auxiliary Status   |                                       |
//!
//! This stub immediately completes any transaction: when the guest writes
//! HOST_CONTROL with START bit, the status register shows INTR (complete).
//! Data reads return 0 (no SPD/device present).

use crate::error::Result;
use crate::io::IoHandler;

/// ICH9 SMBus I/O stub (ports 0x0CC0–0x0CFF, 64 bytes).
pub struct SmBus {
    /// Host Status Register — bit 1 (INTR) set when transfer completes.
    host_status: u8,
}

impl SmBus {
    pub fn new() -> Self {
        SmBus { host_status: 0 }
    }
}

impl IoHandler for SmBus {
    fn read(&mut self, port: u16, _size: u8) -> Result<u32> {
        let offset = (port & 0x3F) as u8;
        let val = match offset {
            // HST_STS: return current status (INTR=done, not busy)
            0x00 => self.host_status as u32,
            // Everything else: 0 (no data, no device)
            _ => 0,
        };
        Ok(val)
    }

    fn write(&mut self, port: u16, _size: u8, val: u32) -> Result<()> {
        let offset = (port & 0x3F) as u8;
        match offset {
            // HST_STS: write-1-to-clear semantics
            0x00 => {
                self.host_status &= !(val as u8);
            }
            // HST_CNT: if START bit (bit 6) is set, complete immediately
            0x02 => {
                if val & (1 << 6) != 0 {
                    // Set INTR (bit 1) = transfer completed, DEV_ERR (bit 2) = no device
                    self.host_status = 0x04;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
