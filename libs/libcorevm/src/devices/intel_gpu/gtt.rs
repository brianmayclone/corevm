//! Graphics Translation Table (GTT).
//!
//! The GTT maps guest graphics addresses to physical VRAM pages.
//! Each GTT entry is 4 bytes: (physical_address >> 12) | flags.
//!
//! For our emulation, the GTT is a simple identity map: GTT entry N
//! maps to VRAM offset N × 4096. The guest writes GTT entries to
//! configure display surfaces and command buffers.

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use super::regs;

/// Graphics Translation Table state.
pub struct Gtt {
    /// GTT entries (physical page address >> 12, with flags).
    entries: Vec<u32>,
    /// Total number of GTT entries.
    num_entries: usize,
}

impl Gtt {
    pub fn new(vram_size: usize) -> Self {
        // Each GTT entry maps one 4 KB page.
        // Max entries = GTT_SIZE / 4 = 128K entries = 512 MB addressable.
        let max_entries = regs::GTT_SIZE / 4;
        let vram_pages = vram_size / 4096;
        let num_entries = vram_pages.min(max_entries);

        // Initialize with identity mapping: entry[i] = (i << 12) | 1 (valid)
        let mut entries = vec![0u32; max_entries];
        for i in 0..num_entries {
            entries[i] = ((i as u32) << 12) | 1; // bit 0 = valid
        }

        Self { entries, num_entries }
    }

    /// Read a GTT entry.
    pub fn read(&self, entry_index: usize) -> u32 {
        self.entries.get(entry_index).copied().unwrap_or(0)
    }

    /// Write a GTT entry.
    pub fn write(&mut self, entry_index: usize, val: u32) {
        if entry_index < self.entries.len() {
            self.entries[entry_index] = val;
        }
    }

    /// Translate a graphics address (GTT offset) to a VRAM byte offset.
    /// Returns None if the entry is invalid or out of range.
    pub fn translate(&self, gtt_offset: usize) -> Option<usize> {
        let page_index = gtt_offset / 4096;
        let page_offset = gtt_offset & 0xFFF;
        let entry = self.entries.get(page_index)?;
        if entry & 1 == 0 { return None; } // Not valid
        let phys_page = (entry >> 12) as usize;
        Some(phys_page * 4096 + page_offset)
    }
}
