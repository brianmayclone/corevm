//! VirtIO Network device emulation (VirtIO Spec v1.2, Section 5.1).
//!
//! Implements a paravirtual high-performance NIC. Much faster than emulated
//! E1000 because the guest driver knows it's running in a VM and uses
//! efficient ring-buffer based communication instead of emulating hardware
//! register access.
//!
//! Windows gets WHQL-signed drivers via Windows Update (netkvm from Red Hat
//! virtio-win). Linux has `virtio_net` built into the kernel.
//!
//! # PCI Identity
//!
//! - Vendor: 0x1AF4 (Red Hat / VirtIO)
//! - Device: 0x1041 (virtio-net, non-transitional)
//! - Subsystem: 0x1AF4:0x0001
//! - Class: 0x02 (Network Controller), Subclass: 0x00 (Ethernet)
//!
//! # MMIO Layout (BAR0, 16KB)
//!
//! | Offset   | Size  | Name            | Description                     |
//! |----------|-------|-----------------|---------------------------------|
//! | 0x0000   | 0x40  | Common Config   | VirtIO common configuration     |
//! | 0x1000   | 0x04  | Notify          | Queue notification doorbell     |
//! | 0x2000   | 0x04  | ISR Status      | Interrupt status                |
//! | 0x3000   | 0x10  | Device Config   | Net-specific (MAC + status)     |
//!
//! # Virtqueues
//!
//! - Queue 0: receiveq — host → guest packets
//! - Queue 1: transmitq — guest → host packets

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::Result;
use crate::memory::mmio::MmioHandler;

// ── VirtIO feature bits ──

const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_F_STATUS: u64 = 1 << 16;
const VIRTIO_NET_F_MRG_RXBUF: u64 = 1 << 15;

// ── VirtIO device status bits ──

const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

// ── PCI hole constants for GPA → host offset ──

const PCI_HOLE_START: u64 = 0xE000_0000;
const PCI_HOLE_END: u64 = 0x1_0000_0000;

// ── BAR0 MMIO region layout ──

const COMMON_CFG_OFFSET: u64 = 0x0000;
const COMMON_CFG_SIZE: u64 = 0x40;
const NOTIFY_OFFSET: u64 = 0x1000;
const NOTIFY_SIZE: u64 = 0x04;
const ISR_OFFSET: u64 = 0x2000;
const ISR_SIZE: u64 = 0x04;
const DEVICE_CFG_OFFSET: u64 = 0x3000;
const DEVICE_CFG_SIZE: u64 = 0x10;

/// BAR0 total size (16 KB).
pub const VIRTIO_NET_BAR0_SIZE: u32 = 0x4000;

/// Maximum number of virtqueue entries.
const VIRTQUEUE_MAX_SIZE: u16 = 256;

/// Number of virtqueues (receiveq + transmitq).
const NUM_QUEUES: usize = 2;

/// VirtIO net header size (non-mergeable, no hash).
const VIRTIO_NET_HDR_SIZE: usize = 12;

// ── Virtqueue descriptor flags ──

const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

/// A single virtqueue (split virtqueue layout).
#[derive(Debug)]
struct Virtqueue {
    max_size: u16,
    size: u16,
    ready: u16,
    desc_addr: u64,
    avail_addr: u64,
    used_addr: u64,
    last_avail_idx: u16,
}

impl Virtqueue {
    fn new() -> Self {
        Self {
            max_size: VIRTQUEUE_MAX_SIZE,
            size: VIRTQUEUE_MAX_SIZE,
            ready: 0,
            desc_addr: 0,
            avail_addr: 0,
            used_addr: 0,
            last_avail_idx: 0,
        }
    }
}

/// VirtIO Network device.
pub struct VirtioNet {
    // ── VirtIO transport state ──

    device_features: u64,
    driver_features: u64,
    device_feature_sel: u32,
    driver_feature_sel: u32,
    status: u8,
    queue_select: u16,
    pub isr_status: u32,
    config_generation: u8,

    // ── Virtqueues ──

    queues: [Virtqueue; NUM_QUEUES],

    // ── Network state ──

    /// MAC address (6 bytes).
    pub mac_address: [u8; 6],
    /// Link status: 1 = link up, 0 = link down.
    pub link_status: u16,

    /// Packets received from the network backend, waiting for guest to consume.
    pub rx_buffer: VecDeque<Vec<u8>>,
    /// Packets transmitted by the guest, waiting for the host/backend to send.
    pub tx_buffer: Vec<Vec<u8>>,

    // ── DMA ──

    pub guest_mem_ptr: *mut u8,
    pub guest_mem_len: usize,

    // ── MSI state ──

    pub msi_enabled: bool,
    pub msi_address: u64,
    pub msi_data: u32,

    /// Set when the device has pending data that requires an interrupt.
    pub irq_pending: bool,
    /// Optional I/O activity callback: called on TX/RX activity.
    pub io_activity_cb: Option<fn(ctx: *mut ())>,
    pub io_activity_ctx: *mut (),
}

unsafe impl Send for VirtioNet {}

impl VirtioNet {
    /// Create a new VirtIO-Net device with the specified MAC address.
    pub fn new(mac: [u8; 6]) -> Self {
        VirtioNet {
            device_features: VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS,
            driver_features: 0,
            device_feature_sel: 0,
            driver_feature_sel: 0,
            status: 0,
            queue_select: 0,
            isr_status: 0,
            config_generation: 0,

            queues: [Virtqueue::new(), Virtqueue::new()],

            mac_address: mac,
            link_status: 1, // Link up

            rx_buffer: VecDeque::new(),
            tx_buffer: Vec::new(),

            guest_mem_ptr: core::ptr::null_mut(),
            guest_mem_len: 0,

            msi_enabled: false,
            msi_address: 0,
            msi_data: 0,
            irq_pending: false,
            io_activity_cb: None,
            io_activity_ctx: core::ptr::null_mut(),
        }
    }

    fn notify_io(&self) {
        if let Some(cb) = self.io_activity_cb {
            cb(self.io_activity_ctx);
        }
    }

    /// Enqueue a packet received from the network for guest consumption.
    /// The packet should be a raw Ethernet frame (no virtio header).
    pub fn receive_packet(&mut self, data: &[u8]) {
        const RX_BUFFER_LIMIT: usize = 512;
        if self.rx_buffer.len() < RX_BUFFER_LIMIT {
            self.rx_buffer.push_back(data.to_vec());
        }
    }

    /// Drain and return all packets transmitted by the guest.
    pub fn take_tx_packets(&mut self) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        core::mem::swap(&mut packets, &mut self.tx_buffer);
        packets
    }

    /// Translate guest physical address to host pointer.
    #[inline]
    fn gpa_to_host(&self, gpa: u64) -> Option<*mut u8> {
        let offset = if gpa < PCI_HOLE_START {
            gpa as usize
        } else if gpa >= PCI_HOLE_END {
            (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize
        } else {
            return None;
        };
        if offset >= self.guest_mem_len || self.guest_mem_ptr.is_null() {
            return None;
        }
        Some(unsafe { self.guest_mem_ptr.add(offset) })
    }

    fn dma_read(&self, gpa: u64, buf: &mut [u8]) -> bool {
        let ptr = match self.gpa_to_host(gpa) {
            Some(p) => p,
            None => return false,
        };
        let offset = if gpa < PCI_HOLE_START { gpa as usize }
            else { (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize };
        if offset + buf.len() > self.guest_mem_len { return false; }
        unsafe { core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), buf.len()); }
        true
    }

    fn dma_write(&self, gpa: u64, data: &[u8]) -> bool {
        let ptr = match self.gpa_to_host(gpa) {
            Some(p) => p,
            None => return false,
        };
        let offset = if gpa < PCI_HOLE_START { gpa as usize }
            else { (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize };
        if offset + data.len() > self.guest_mem_len { return false; }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()); }
        true
    }

    /// Process the TX virtqueue — guest has submitted packets to transmit.
    /// Called when the guest writes to the notify doorbell for queue 1.
    fn process_transmitq(&mut self) {
        let q = &self.queues[1];
        if q.ready == 0 || q.size == 0 || q.desc_addr == 0 || q.avail_addr == 0 {
            return;
        }

        let desc_addr = q.desc_addr;
        let avail_addr = q.avail_addr;
        let used_addr = q.used_addr;
        let qsize = q.size;
        let mut last_avail = q.last_avail_idx;

        let mut avail_idx_buf = [0u8; 2];
        if !self.dma_read(avail_addr + 2, &mut avail_idx_buf) { return; }
        let avail_idx = u16::from_le_bytes(avail_idx_buf);

        let mut used_count = 0u16;

        while last_avail != avail_idx {
            let avail_offset = 4 + (last_avail % qsize) as u64 * 2;
            let mut head_buf = [0u8; 2];
            if !self.dma_read(avail_addr + avail_offset, &mut head_buf) { break; }
            let head_idx = u16::from_le_bytes(head_buf);

            // Walk descriptor chain, collect packet data.
            let mut pkt_data = Vec::new();
            let mut idx = head_idx;
            let mut chain_len = 0u32;
            let mut total_len = 0u32;

            loop {
                if chain_len > 64 { break; }
                chain_len += 1;

                let desc_off = (idx % qsize) as u64 * 16;
                let mut desc = [0u8; 16];
                if !self.dma_read(desc_addr + desc_off, &mut desc) { break; }

                let addr = u64::from_le_bytes([
                    desc[0], desc[1], desc[2], desc[3],
                    desc[4], desc[5], desc[6], desc[7],
                ]);
                let len = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]);
                let flags = u16::from_le_bytes([desc[12], desc[13]]);
                let next = u16::from_le_bytes([desc[14], desc[15]]);

                // Device-readable descriptor (TX data from guest).
                if flags & VRING_DESC_F_WRITE == 0 {
                    let mut buf = vec![0u8; len as usize];
                    if self.dma_read(addr, &mut buf) {
                        pkt_data.extend_from_slice(&buf);
                    }
                }
                total_len += len;

                if flags & VRING_DESC_F_NEXT != 0 {
                    idx = next;
                } else {
                    break;
                }
            }

            // Strip the virtio_net_hdr (12 bytes) from the front.
            if pkt_data.len() > VIRTIO_NET_HDR_SIZE {
                let frame = pkt_data[VIRTIO_NET_HDR_SIZE..].to_vec();
                if !frame.is_empty() {
                    self.tx_buffer.push(frame);
                }
            }

            // Write to used ring.
            let used_ring_idx_addr = used_addr + 2;
            let mut used_idx_buf = [0u8; 2];
            if !self.dma_read(used_ring_idx_addr, &mut used_idx_buf) { break; }
            let used_idx = u16::from_le_bytes(used_idx_buf);

            let used_elem_offset = 4 + (used_idx % qsize) as u64 * 8;
            let mut used_elem = [0u8; 8];
            used_elem[0..4].copy_from_slice(&(head_idx as u32).to_le_bytes());
            used_elem[4..8].copy_from_slice(&total_len.to_le_bytes());
            self.dma_write(used_addr + used_elem_offset, &used_elem);

            let new_used_idx = used_idx.wrapping_add(1);
            self.dma_write(used_ring_idx_addr, &new_used_idx.to_le_bytes());

            last_avail = last_avail.wrapping_add(1);
            used_count += 1;
        }

        self.queues[1].last_avail_idx = last_avail;

        if used_count > 0 {
            self.isr_status |= 1;
            self.irq_pending = true;
            self.notify_io();
        }
    }

    /// Deliver pending RX packets to the guest via the RX virtqueue (queue 0).
    /// Called from the VM poll loop.
    pub fn process_rx(&mut self) {
        if self.status & VIRTIO_STATUS_DRIVER_OK == 0 { return; }
        if self.rx_buffer.is_empty() { return; }

        let q = &self.queues[0];
        if q.ready == 0 || q.size == 0 || q.desc_addr == 0 || q.avail_addr == 0 {
            return;
        }

        let desc_addr = q.desc_addr;
        let avail_addr = q.avail_addr;
        let used_addr = q.used_addr;
        let qsize = q.size;
        let mut last_avail = q.last_avail_idx;

        let mut avail_idx_buf = [0u8; 2];
        if !self.dma_read(avail_addr + 2, &mut avail_idx_buf) { return; }
        let avail_idx = u16::from_le_bytes(avail_idx_buf);

        let mut delivered = 0u32;
        const MAX_RX_PER_POLL: u32 = 128;

        while !self.rx_buffer.is_empty() && delivered < MAX_RX_PER_POLL {
            if last_avail == avail_idx { break; } // No available descriptors

            let avail_offset = 4 + (last_avail % qsize) as u64 * 2;
            let mut head_buf = [0u8; 2];
            if !self.dma_read(avail_addr + avail_offset, &mut head_buf) { break; }
            let head_idx = u16::from_le_bytes(head_buf);

            // Collect device-writable descriptors for this chain.
            let mut write_bufs: Vec<(u64, u32)> = Vec::new();
            let mut idx = head_idx;
            let mut chain_len = 0u32;

            loop {
                if chain_len > 64 { break; }
                chain_len += 1;

                let desc_off = (idx % qsize) as u64 * 16;
                let mut desc = [0u8; 16];
                if !self.dma_read(desc_addr + desc_off, &mut desc) { break; }

                let addr = u64::from_le_bytes([
                    desc[0], desc[1], desc[2], desc[3],
                    desc[4], desc[5], desc[6], desc[7],
                ]);
                let len = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]);
                let flags = u16::from_le_bytes([desc[12], desc[13]]);
                let next = u16::from_le_bytes([desc[14], desc[15]]);

                if flags & VRING_DESC_F_WRITE != 0 {
                    write_bufs.push((addr, len));
                }

                if flags & VRING_DESC_F_NEXT != 0 {
                    idx = next;
                } else {
                    break;
                }
            }

            if write_bufs.is_empty() { break; }

            // Build the packet: virtio_net_hdr (12 bytes, all zeros) + raw Ethernet frame.
            let pkt = self.rx_buffer.pop_front().unwrap();
            let mut frame = vec![0u8; VIRTIO_NET_HDR_SIZE + pkt.len()];
            // virtio_net_hdr is all zeros (no offloading, no GSO).
            frame[VIRTIO_NET_HDR_SIZE..].copy_from_slice(&pkt);

            // Write frame into device-writable descriptors.
            let mut written = 0usize;
            for (addr, len) in &write_bufs {
                if written >= frame.len() { break; }
                let chunk = (*len as usize).min(frame.len() - written);
                self.dma_write(*addr, &frame[written..written + chunk]);
                written += chunk;
            }

            // Write to used ring.
            let used_ring_idx_addr = used_addr + 2;
            let mut used_idx_buf = [0u8; 2];
            if !self.dma_read(used_ring_idx_addr, &mut used_idx_buf) { break; }
            let used_idx = u16::from_le_bytes(used_idx_buf);

            let used_elem_offset = 4 + (used_idx % qsize) as u64 * 8;
            let mut used_elem = [0u8; 8];
            used_elem[0..4].copy_from_slice(&(head_idx as u32).to_le_bytes());
            used_elem[4..8].copy_from_slice(&(written as u32).to_le_bytes());
            self.dma_write(used_addr + used_elem_offset, &used_elem);

            let new_used_idx = used_idx.wrapping_add(1);
            self.dma_write(used_ring_idx_addr, &new_used_idx.to_le_bytes());

            last_avail = last_avail.wrapping_add(1);
            delivered += 1;
        }

        self.queues[0].last_avail_idx = last_avail;

        if delivered > 0 {
            self.isr_status |= 1;
            self.irq_pending = true;
            self.notify_io();
        }
    }

    // ── Common config register access ──

    fn read_common_cfg(&self, offset: u64, size: u8) -> u64 {
        let val = match offset {
            0x00 => self.device_feature_sel as u64,
            0x04 => {
                if self.device_feature_sel == 0 {
                    (self.device_features & 0xFFFF_FFFF) as u64
                } else {
                    ((self.device_features >> 32) & 0xFFFF_FFFF) as u64
                }
            }
            0x08 => self.driver_feature_sel as u64,
            0x0C => {
                if self.driver_feature_sel == 0 {
                    (self.driver_features & 0xFFFF_FFFF) as u64
                } else {
                    ((self.driver_features >> 32) & 0xFFFF_FFFF) as u64
                }
            }
            0x10 => 0xFFFF, // msix_config (no MSI-X)
            0x12 => NUM_QUEUES as u64,
            0x14 => self.status as u64,
            0x15 => self.config_generation as u64,
            0x16 => self.queue_select as u64,
            0x18 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].size as u64 } else { 0 }
            }
            0x1A => 0xFFFF, // queue_msix_vector
            0x1C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].ready as u64 } else { 0 }
            }
            0x1E => self.queue_select as u64, // queue_notify_off
            0x20 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].desc_addr & 0xFFFF_FFFF } else { 0 }
            }
            0x24 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].desc_addr >> 32 } else { 0 }
            }
            0x28 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].avail_addr & 0xFFFF_FFFF } else { 0 }
            }
            0x2C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].avail_addr >> 32 } else { 0 }
            }
            0x30 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].used_addr & 0xFFFF_FFFF } else { 0 }
            }
            0x34 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].used_addr >> 32 } else { 0 }
            }
            _ => 0,
        };
        let mask = match size { 1 => 0xFF, 2 => 0xFFFF, _ => 0xFFFF_FFFF };
        val & mask
    }

    fn write_common_cfg(&mut self, offset: u64, _size: u8, val: u64) {
        match offset {
            0x00 => self.device_feature_sel = val as u32,
            0x08 => self.driver_feature_sel = val as u32,
            0x0C => {
                if self.driver_feature_sel == 0 {
                    self.driver_features = (self.driver_features & 0xFFFF_FFFF_0000_0000) | (val & 0xFFFF_FFFF);
                } else {
                    self.driver_features = (self.driver_features & 0xFFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32);
                }
            }
            0x14 => {
                let new_status = val as u8;
                if new_status == 0 {
                    self.status = 0;
                    self.driver_features = 0;
                    for q in &mut self.queues { q.ready = 0; q.last_avail_idx = 0; }
                    self.isr_status = 0;
                    self.rx_buffer.clear();
                    self.tx_buffer.clear();
                    return;
                }
                self.status = new_status;
            }
            0x16 => self.queue_select = val as u16,
            0x18 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].size = (val as u16).min(VIRTQUEUE_MAX_SIZE); }
            }
            0x1C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].ready = val as u16; }
            }
            0x20 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].desc_addr = (self.queues[qi].desc_addr & !0xFFFF_FFFF) | (val & 0xFFFF_FFFF); } }
            0x24 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].desc_addr = (self.queues[qi].desc_addr & 0xFFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32); } }
            0x28 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].avail_addr = (self.queues[qi].avail_addr & !0xFFFF_FFFF) | (val & 0xFFFF_FFFF); } }
            0x2C => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].avail_addr = (self.queues[qi].avail_addr & 0xFFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32); } }
            0x30 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].used_addr = (self.queues[qi].used_addr & !0xFFFF_FFFF) | (val & 0xFFFF_FFFF); } }
            0x34 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].used_addr = (self.queues[qi].used_addr & 0xFFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32); } }
            _ => {}
        }
    }

    /// Read net device-specific config: MAC address (6 bytes) + status (2 bytes).
    fn read_device_cfg(&self, offset: u64, _size: u8) -> u64 {
        match offset {
            0..=5 => self.mac_address[offset as usize] as u64,
            6 => (self.link_status & 0xFF) as u64,
            7 => ((self.link_status >> 8) & 0xFF) as u64,
            _ => 0,
        }
    }
}

impl MmioHandler for VirtioNet {
    fn read(&mut self, offset: u64, size: u8) -> Result<u64> {
        if offset >= COMMON_CFG_OFFSET && offset < COMMON_CFG_OFFSET + COMMON_CFG_SIZE {
            return Ok(self.read_common_cfg(offset - COMMON_CFG_OFFSET, size));
        }
        if offset >= ISR_OFFSET && offset < ISR_OFFSET + ISR_SIZE {
            let val = self.isr_status as u64;
            self.isr_status = 0;
            return Ok(val);
        }
        if offset >= DEVICE_CFG_OFFSET && offset < DEVICE_CFG_OFFSET + DEVICE_CFG_SIZE {
            return Ok(self.read_device_cfg(offset - DEVICE_CFG_OFFSET, size));
        }
        if offset >= NOTIFY_OFFSET && offset < NOTIFY_OFFSET + NOTIFY_SIZE {
            return Ok(0);
        }
        Ok(0xFFFF_FFFF)
    }

    fn write(&mut self, offset: u64, size: u8, val: u64) -> Result<()> {
        if offset >= COMMON_CFG_OFFSET && offset < COMMON_CFG_OFFSET + COMMON_CFG_SIZE {
            self.write_common_cfg(offset - COMMON_CFG_OFFSET, size, val);
            return Ok(());
        }
        if offset >= NOTIFY_OFFSET && offset < NOTIFY_OFFSET + NOTIFY_SIZE {
            // Queue notification doorbell.
            // The value written identifies which queue, but we process both.
            self.process_transmitq();
            return Ok(());
        }
        if offset >= ISR_OFFSET && offset < ISR_OFFSET + ISR_SIZE {
            self.isr_status &= !(val as u32);
            return Ok(());
        }
        Ok(())
    }
}
