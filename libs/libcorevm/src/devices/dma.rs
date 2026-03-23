//! Intel 8237A DMA Controller stub.
//!
//! OVMF probes the legacy ISA DMA controller during early initialization.
//! Without a handler, reads return 0xFF (bus float) which OVMF interprets
//! as "all channels active" and polls indefinitely.
//!
//! This stub returns 0x00 for status registers (no channels active, no
//! requests pending) and silently absorbs writes.
//!
//! # I/O Ports
//!
//! | Range       | Controller |
//! |-------------|------------|
//! | 0x00–0x0F   | DMA1 (8-bit channels 0–3) |
//! | 0x80–0x8F   | DMA page registers |
//! | 0xC0–0xDF   | DMA2 (16-bit channels 4–7) |

use crate::error::Result;
use crate::io::IoHandler;

/// DMA1 controller stub (ports 0x00–0x0F).
pub struct Dma1;

impl IoHandler for Dma1 {
    fn read(&mut self, _port: u16, _size: u8) -> Result<u32> {
        // Return 0 — no channels active, no requests pending.
        // Port 0x08 (status register) returning 0 tells OVMF that
        // no DMA transfers are in progress.
        Ok(0)
    }

    fn write(&mut self, _port: u16, _size: u8, _val: u32) -> Result<()> {
        Ok(())
    }
}

/// DMA2 controller stub (ports 0xC0–0xDF).
pub struct Dma2;

impl IoHandler for Dma2 {
    fn read(&mut self, _port: u16, _size: u8) -> Result<u32> {
        Ok(0)
    }

    fn write(&mut self, _port: u16, _size: u8, _val: u32) -> Result<()> {
        Ok(())
    }
}

/// DMA page register stub (ports 0x80–0x8F).
pub struct DmaPage;

impl IoHandler for DmaPage {
    fn read(&mut self, _port: u16, _size: u8) -> Result<u32> {
        Ok(0)
    }

    fn write(&mut self, _port: u16, _size: u8, _val: u32) -> Result<()> {
        Ok(())
    }
}
