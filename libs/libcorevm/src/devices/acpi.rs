//! ACPI Power Management emulation (ICH9/Q35).
//!
//! Emulates the ACPI PM I/O registers at base address 0xB000 (SeaBIOS) or
//! 0x600 (OVMF/ICH9). Covers the full ICH9 PM register set:
//!
//! # I/O Ports (relative to PMBASE)
//!
//! | Offset | Size | Register | Access |
//! |--------|------|----------|--------|
//! | 0x00 | 2 | PM1a Status | R/W1C |
//! | 0x02 | 2 | PM1a Enable | R/W |
//! | 0x04 | 2 | PM1a Control | R/W |
//! | 0x08 | 4 | PM Timer | RO |
//! | 0x20 | 2 | GPE0 Status | R/W1C |
//! | 0x22 | 2 | GPE0 Enable | R/W |
//! | 0x60 | 2 | TCO_RLD | R/W |
//! | 0x62 | 2 | TCO_DAT_IN/OUT | R/W |
//! | 0x64 | 2 | TCO1_STS | R/W1C |
//! | 0x66 | 2 | TCO2_STS | R/W1C |
//! | 0x68 | 2 | TCO1_CNT | R/W |
//! | 0x6A | 2 | TCO2_CNT | R/W |
//! | 0x6C | 2 | TCO_MESSAGE | R/W |
//! | 0x70 | 1 | TCO_WDCNT | R/W |
//! | 0x72 | 2 | SW_IRQ_GEN | R/W |
//! | 0x74 | 2 | TCO_TMR | R/W |

use crate::error::Result;
use crate::io::IoHandler;

/// PM timer frequency: 3.579545 MHz.
const PM_TIMER_HZ: u64 = 3_579_545;

/// Approximate guest instructions per PM timer tick (host_test fallback).
const PM_TIMER_INSTS_PER_TICK: u64 = 10;

/// ACPI Power Management I/O device.
///
/// Covers the full ICH9 ACPI I/O range: PM1 event/control/timer,
/// GPE0 block, and TCO timer registers.
#[derive(Debug)]
pub struct AcpiPm {
    /// PM1a Status Register (offset 0x00).
    pm1_status: u16,
    /// PM1a Enable Register (offset 0x02).
    pm1_enable: u16,
    /// PM1a Control Register (offset 0x04).
    pm1_control: u16,
    /// PM Timer counter (offset 0x08, 32-bit read-only).
    timer_count: u32,
    /// Fractional guest instruction credits toward the next PM timer tick.
    instruction_credit: u64,
    /// Wall-clock epoch for std platforms.
    #[cfg(feature = "std")]
    epoch: std::time::Instant,
    /// Set when the guest writes SLP_EN with SLP_TYP=S5 (soft-off/shutdown).
    pub shutdown_requested: bool,

    // ── GPE0 Block (offset 0x20) ──
    /// GPE0 Status Register (offset 0x20, write-1-to-clear).
    gpe0_status: u16,
    /// GPE0 Enable Register (offset 0x22).
    gpe0_enable: u16,

    // ── TCO Timer (offset 0x60) ──
    /// TCO reload value (offset 0x60).
    tco_rld: u16,
    /// TCO data in/out (offset 0x62).
    tco_dat: u16,
    /// TCO1 Status (offset 0x64, write-1-to-clear).
    tco1_sts: u16,
    /// TCO2 Status (offset 0x66, write-1-to-clear).
    tco2_sts: u16,
    /// TCO1 Control (offset 0x68).
    tco1_cnt: u16,
    /// TCO2 Control (offset 0x6A).
    tco2_cnt: u16,
    /// TCO Message (offset 0x6C).
    tco_message: u16,
    /// TCO Watchdog count (offset 0x70).
    tco_wdcnt: u8,
    /// Software IRQ generation (offset 0x72).
    sw_irq_gen: u16,
    /// TCO Timer initial value (offset 0x74).
    tco_tmr: u16,
}

impl AcpiPm {
    /// Create a new ACPI PM device with all registers zeroed.
    pub fn new() -> Self {
        AcpiPm {
            pm1_status: 0,
            pm1_enable: 0,
            pm1_control: 1, // SCI_EN=1: ACPI mode always active (no SMI transition needed)
            timer_count: 0,
            instruction_credit: 0,
            #[cfg(feature = "std")]
            epoch: std::time::Instant::now(),
            shutdown_requested: false,
            // GPE0
            gpe0_status: 0,
            gpe0_enable: 0,
            // TCO — initialized per ICH9 defaults
            tco_rld: 0,
            tco_dat: 0,
            tco1_sts: 0,
            tco2_sts: 0,
            tco1_cnt: 0,
            tco2_cnt: 0x0008, // TCO2_CNT default: bit 3 = OS_POLICY (intruder detection disabled)
            tco_message: 0,
            tco_wdcnt: 0,
            sw_irq_gen: 0,
            tco_tmr: 0x0004, // TCO_TMR default: 4 ticks
        }
    }

    /// Advance the free-running PM timer by guest execution progress.
    /// Used in host_test mode.
    pub fn advance(&mut self, guest_instructions: u64) {
        self.instruction_credit = self.instruction_credit.saturating_add(guest_instructions);
        if self.instruction_credit < PM_TIMER_INSTS_PER_TICK {
            return;
        }

        let ticks = (self.instruction_credit / PM_TIMER_INSTS_PER_TICK) as u32;
        self.instruction_credit %= PM_TIMER_INSTS_PER_TICK;

        let prev = self.timer_count;
        self.timer_count = self.timer_count.wrapping_add(ticks);

        // PM1_STS.TMR_STS reflects a 24-bit PM timer overflow on ICH-style PM.
        let prev_24 = prev & 0x00FF_FFFF;
        let cur_24 = self.timer_count & 0x00FF_FFFF;
        if cur_24 < prev_24 {
            self.pm1_status |= 1;
        }
    }

    /// Get the current PM timer value. On std platforms, uses wall-clock time
    /// for accurate 3.579545 MHz free-running counter.
    fn timer_value(&self) -> u32 {
        #[cfg(feature = "std")]
        {
            let elapsed_us = self.epoch.elapsed().as_micros() as u64;
            // ticks = elapsed_us * 3579545 / 1000000
            let ticks = elapsed_us.wrapping_mul(PM_TIMER_HZ) / 1_000_000;
            ticks as u32
        }
        #[cfg(not(feature = "std"))]
        {
            self.timer_count
        }
    }
}

impl IoHandler for AcpiPm {
    fn read(&mut self, port: u16, size: u8) -> Result<u32> {
        #[cfg(feature = "std")]
        {
            static ACPI_RD_LOG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = ACPI_RD_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 30 {
                let tmr = self.timer_value();
                eprintln!("[acpi-pm] read port=0x{:04X} size={} tmr=0x{:08X} ({})", port, size, tmr, tmr);
            }
        }
        let offset = port & 0x7F;
        let val = match offset {
            // ── PM1 Block ──
            // PM1a Status Register.
            0x00 => self.pm1_status as u32,
            // PM1a Enable Register.
            0x02 => self.pm1_enable as u32,
            // PM1a Control Register.
            0x04 => self.pm1_control as u32,
            // PM Timer — free-running 32-bit counter with byte/word subaccesses.
            0x08..=0x0B => {
                let shift = ((offset - 0x08) * 8) as u32;
                self.timer_value() >> shift
            }

            // ── GPE0 Block ──
            0x20 => self.gpe0_status as u32,
            0x22 => self.gpe0_enable as u32,

            // ── TCO Timer ──
            0x60 => self.tco_rld as u32,
            0x62 => self.tco_dat as u32,
            0x64 => self.tco1_sts as u32,
            0x66 => self.tco2_sts as u32,
            0x68 => self.tco1_cnt as u32,
            0x6A => self.tco2_cnt as u32,
            0x6C => self.tco_message as u32,
            0x70 => self.tco_wdcnt as u32,
            0x72 => self.sw_irq_gen as u32,
            0x74 => self.tco_tmr as u32,

            _ => 0,
        };

        // Mask to requested access size.
        let masked = match size {
            1 => val & 0xFF,
            2 => val & 0xFFFF,
            _ => val,
        };
        Ok(masked)
    }

    /// Write to ACPI PM I/O registers.
    fn write(&mut self, port: u16, _size: u8, val: u32) -> Result<()> {
        let offset = port & 0x7F;
        match offset {
            // ── PM1 Block ──
            // PM1a Status: write-1-to-clear.
            0x00 => self.pm1_status &= !(val as u16),
            // PM1a Enable: writable.
            0x02 => self.pm1_enable = val as u16,
            // PM1a Control: writable (bit 13 SLP_EN triggers sleep).
            0x04 => {
                self.pm1_control = val as u16;
                // SLP_EN (bit 13) + SLP_TYP (bits 12:10) = S5 → shutdown.
                let slp_en = (val >> 13) & 1;
                let slp_typ = (val >> 10) & 0x7;
                if slp_en == 1 && slp_typ == 5 {
                    self.shutdown_requested = true;
                }
            }
            // PM Timer is read-only — writes are silently ignored.
            0x08..=0x0B => {}

            // ── GPE0 Block ──
            // GPE0 Status: write-1-to-clear.
            0x20 => self.gpe0_status &= !(val as u16),
            // GPE0 Enable: writable.
            0x22 => self.gpe0_enable = val as u16,

            // ── TCO Timer ──
            // TCO_RLD: reload the TCO timer (writing any value reloads).
            0x60 => self.tco_rld = val as u16,
            // TCO_DAT_IN/OUT: data register.
            0x62 => self.tco_dat = val as u16,
            // TCO1_STS: write-1-to-clear.
            0x64 => self.tco1_sts &= !(val as u16),
            // TCO2_STS: write-1-to-clear.
            0x66 => self.tco2_sts &= !(val as u16),
            // TCO1_CNT: writable (bit 11 = TCO_TMR_HLT halts the timer).
            0x68 => self.tco1_cnt = val as u16,
            // TCO2_CNT: writable.
            0x6A => self.tco2_cnt = val as u16,
            // TCO_MESSAGE: writable.
            0x6C => self.tco_message = val as u16,
            // TCO_WDCNT: watchdog count (byte).
            0x70 => self.tco_wdcnt = val as u8,
            // SW_IRQ_GEN: software IRQ generation.
            0x72 => self.sw_irq_gen = val as u16,
            // TCO_TMR: timer initial value.
            0x74 => self.tco_tmr = val as u16,

            _ => {}
        }
        Ok(())
    }
}
