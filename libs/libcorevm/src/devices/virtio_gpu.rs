//! VirtIO GPU device emulation (VirtIO Spec v1.2, Section 5.7).
//!
//! Implements a paravirtual GPU device with 2D acceleration support.
//! Windows guests use the WHQL-signed viogpudo driver (available via
//! Windows Update) for automatic plug-and-play operation.
//!
//! # PCI Identity
//!
//! - Vendor: 0x1AF4 (Red Hat / VirtIO)
//! - Device: 0x1050 (virtio-gpu, non-transitional)
//! - Subsystem: 0x1AF4:0x1100
//! - Class: 0x03 (Display Controller), Subclass: 0x00 (VGA compatible)
//!
//! # MMIO Layout (BAR0, 16KB)
//!
//! | Offset   | Size  | Name            | Description                     |
//! |----------|-------|-----------------|---------------------------------|
//! | 0x0000   | 0x40  | Common Config   | VirtIO common configuration     |
//! | 0x1000   | 0x04  | Notify          | Queue notification doorbell     |
//! | 0x2000   | 0x04  | ISR Status      | Interrupt status                |
//! | 0x3000   | 0x40  | Device Config   | GPU-specific configuration      |
//!
//! # Virtqueues
//!
//! - Queue 0: controlq — 2D/3D GPU commands
//! - Queue 1: cursorq — cursor updates

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::Result;
use crate::memory::mmio::MmioHandler;

// ── VirtIO GPU command types ──

const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
const VIRTIO_GPU_CMD_GET_CAPSET_INFO: u32 = 0x0108;
const VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0109;
const VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010A;

// Response types
const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
const VIRTIO_GPU_RESP_OK_CAPSET_INFO: u32 = 0x1102;
const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1103;
const VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
const VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1202;
const VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1203;

// Resource formats
const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;
const VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM: u32 = 3;
const VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM: u32 = 4;
const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;
const VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM: u32 = 68;
const VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM: u32 = 121;
const VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM: u32 = 134;

// ── VirtIO feature bits ──

const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_GPU_F_EDID: u64 = 1 << 1;

// ── VirtIO device status bits ──

const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
const VIRTIO_STATUS_FAILED: u8 = 128;

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
const DEVICE_CFG_SIZE: u64 = 0x40;

/// BAR0 total size (16 KB).
pub const VIRTIO_GPU_BAR0_SIZE: u32 = 0x4000;

/// Maximum number of virtqueue entries.
const VIRTQUEUE_MAX_SIZE: u16 = 256;

/// Number of virtqueues (controlq + cursorq).
const NUM_QUEUES: usize = 2;

/// Maximum number of scanouts.
const MAX_SCANOUTS: usize = 1;

/// Default scanout resolution.
const DEFAULT_WIDTH: u32 = 1920;
const DEFAULT_HEIGHT: u32 = 1080;

// ── Virtqueue descriptor flags ──

const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

/// A single virtqueue (split virtqueue layout per VirtIO spec).
#[derive(Debug)]
struct Virtqueue {
    /// Maximum number of descriptors.
    max_size: u16,
    /// Current queue size (set by driver, must be power of 2).
    size: u16,
    /// Set to 1 when the queue is enabled by the driver.
    ready: u16,
    /// Guest physical address of the descriptor table.
    desc_addr: u64,
    /// Guest physical address of the available ring.
    avail_addr: u64,
    /// Guest physical address of the used ring.
    used_addr: u64,
    /// Last seen available ring index.
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

/// A 2D resource owned by the guest.
struct Resource2D {
    width: u32,
    height: u32,
    format: u32,
    /// Host-side pixel buffer (BGRA32).
    pixels: Vec<u8>,
    /// Guest memory backing pages (GPA, length).
    backing: Vec<(u64, u32)>,
}

/// Scanout configuration.
struct Scanout {
    enabled: bool,
    resource_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// VirtIO GPU device.
pub struct VirtioGpu {
    // ── VirtIO transport state ──

    /// Device features offered to the driver.
    device_features: u64,
    /// Features selected by the driver.
    driver_features: u64,
    /// Feature selection page (0 = low 32 bits, 1 = high 32 bits).
    device_feature_sel: u32,
    driver_feature_sel: u32,
    /// Device status register.
    status: u8,
    /// Currently selected queue index for configuration.
    queue_select: u16,
    /// ISR status (bit 0 = used buffer notification, bit 1 = config change).
    pub isr_status: u32,
    /// Configuration generation counter.
    config_generation: u8,

    // ── Virtqueues ──

    queues: [Virtqueue; NUM_QUEUES],

    // ── GPU state ──

    /// 2D resources, keyed by resource_id.
    resources: BTreeMap<u32, Resource2D>,
    /// Scanout configurations.
    scanouts: [Scanout; MAX_SCANOUTS],

    /// Scanout framebuffer (BGRA32, page-aligned for hypervisor mapping).
    pub framebuffer: Vec<u8>,
    /// Current framebuffer width.
    pub fb_width: u32,
    /// Current framebuffer height.
    pub fb_height: u32,
    /// True when the guest driver has configured a scanout with a valid resource.
    /// Used by the display pipeline to decide whether to show the VirtIO GPU
    /// framebuffer or fall back to VGA.
    pub scanout_active: bool,

    // ── DMA ──

    /// Guest memory pointer for DMA access.
    pub guest_mem_ptr: *mut u8,
    /// Guest memory length in bytes.
    pub guest_mem_len: usize,

    // ── MSI state ──

    pub msi_enabled: bool,
    pub msi_address: u64,
    pub msi_data: u32,

    /// Set when the device has pending data that requires an interrupt.
    pub irq_pending: bool,

    /// Callback to immediately signal an interrupt and kick the vCPU.
    /// Called after writing to the used ring (like QEMU's virtio_notify).
    /// Arguments: none. The callback should pulse the IRQ line and kick the vCPU.
    #[cfg(feature = "std")]
    pub notify_callback: Option<alloc::boxed::Box<dyn FnMut() + Send>>,
}

unsafe impl Send for VirtioGpu {}

impl VirtioGpu {
    /// Create a new VirtIO GPU device with the specified VRAM size.
    pub fn new(vram_mb: u32) -> Self {
        let vram_mb = if vram_mb == 0 { 256 } else { vram_mb.clamp(16, 512) };
        let vram_bytes = (vram_mb as usize) * 1024 * 1024;

        // Allocate page-aligned framebuffer for hypervisor memory mapping.
        let fb_size = (DEFAULT_WIDTH as usize) * (DEFAULT_HEIGHT as usize) * 4;
        let framebuffer = alloc_page_aligned(fb_size.max(vram_bytes));

        VirtioGpu {
            device_features: VIRTIO_F_VERSION_1 | VIRTIO_GPU_F_EDID,
            driver_features: 0,
            device_feature_sel: 0,
            driver_feature_sel: 0,
            status: 0,
            queue_select: 0,
            isr_status: 0,
            config_generation: 0,

            queues: [Virtqueue::new(), Virtqueue::new()],

            resources: BTreeMap::new(),
            scanouts: [Scanout {
                enabled: true,
                resource_id: 0,
                x: 0,
                y: 0,
                width: DEFAULT_WIDTH,
                height: DEFAULT_HEIGHT,
            }],

            framebuffer,
            fb_width: DEFAULT_WIDTH,
            fb_height: DEFAULT_HEIGHT,
            scanout_active: false,

            guest_mem_ptr: core::ptr::null_mut(),
            guest_mem_len: 0,

            msi_enabled: false,
            msi_address: 0,
            msi_data: 0,
            irq_pending: false,
            #[cfg(feature = "std")]
            notify_callback: None,
        }
    }

    /// Fire the notify callback (IRQ pulse + vCPU kick).
    fn virtio_notify(&mut self) {
        self.isr_status |= 1;
        self.irq_pending = true;
        #[cfg(feature = "std")]
        if let Some(ref mut cb) = self.notify_callback {
            cb();
        }
    }

    /// Get pointer to the framebuffer for hypervisor memory mapping.
    pub fn framebuffer_mut_ptr(&mut self) -> *mut u8 {
        self.framebuffer.as_mut_ptr()
    }

    /// Current VRAM size in bytes.
    pub fn vram_size(&self) -> usize {
        self.framebuffer.len()
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

    /// Read bytes from guest physical memory.
    fn dma_read(&self, gpa: u64, buf: &mut [u8]) -> bool {
        let ptr = match self.gpa_to_host(gpa) {
            Some(p) => p,
            None => return false,
        };
        let offset = if gpa < PCI_HOLE_START {
            gpa as usize
        } else {
            (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize
        };
        if offset + buf.len() > self.guest_mem_len {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), buf.len()); }
        true
    }

    /// Write bytes to guest physical memory.
    fn dma_write(&self, gpa: u64, data: &[u8]) -> bool {
        let ptr = match self.gpa_to_host(gpa) {
            Some(p) => p,
            None => return false,
        };
        let offset = if gpa < PCI_HOLE_START {
            gpa as usize
        } else {
            (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize
        };
        if offset + data.len() > self.guest_mem_len {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()); }
        true
    }

    /// Process pending commands on all virtqueues.
    /// Called periodically from the VM run loop.
    pub fn process(&mut self) {
        if self.status & VIRTIO_STATUS_DRIVER_OK == 0 {
            return;
        }
        self.process_controlq();
    }

    /// Process the control virtqueue (queue 0).
    fn process_controlq(&mut self) {
        let q = &self.queues[0];
        if q.ready == 0 || q.size == 0 || q.desc_addr == 0 || q.avail_addr == 0 {
            return;
        }

        let desc_addr = q.desc_addr;
        let avail_addr = q.avail_addr;
        let used_addr = q.used_addr;
        let qsize = q.size;
        let mut last_avail = q.last_avail_idx;

        // Read the available ring index.
        let mut avail_idx_buf = [0u8; 2];
        if !self.dma_read(avail_addr + 2, &mut avail_idx_buf) {
            return;
        }
        let avail_idx = u16::from_le_bytes(avail_idx_buf);

        let mut used_count = 0u16;

        while last_avail != avail_idx {
            // Read the descriptor chain head index from avail ring.
            let avail_offset = 4 + (last_avail % qsize) as u64 * 2;
            let mut head_buf = [0u8; 2];
            if !self.dma_read(avail_addr + avail_offset, &mut head_buf) {
                break;
            }
            let head_idx = u16::from_le_bytes(head_buf);

            // Walk the descriptor chain, collecting read and write buffers.
            let total_written = self.process_descriptor_chain(desc_addr, head_idx, qsize);

            // Write to used ring.
            let used_ring_idx_addr = used_addr + 2;
            let mut used_idx_buf = [0u8; 2];
            if !self.dma_read(used_ring_idx_addr, &mut used_idx_buf) {
                break;
            }
            let used_idx = u16::from_le_bytes(used_idx_buf);

            let used_elem_offset = 4 + (used_idx % qsize) as u64 * 8;
            let used_elem_addr = used_addr + used_elem_offset;
            let mut used_elem = [0u8; 8];
            used_elem[0..4].copy_from_slice(&(head_idx as u32).to_le_bytes());
            used_elem[4..8].copy_from_slice(&total_written.to_le_bytes());
            self.dma_write(used_elem_addr, &used_elem);

            // Advance used ring index.
            let new_used_idx = used_idx.wrapping_add(1);
            self.dma_write(used_ring_idx_addr, &new_used_idx.to_le_bytes());

            last_avail = last_avail.wrapping_add(1);
            used_count += 1;
        }

        self.queues[0].last_avail_idx = last_avail;

        if used_count > 0 {
            self.virtio_notify();
        }
    }

    /// Process a single descriptor chain. Returns total bytes written to device-writable descriptors.
    fn process_descriptor_chain(&mut self, desc_table: u64, head: u16, qsize: u16) -> u32 {
        // Collect read (device-readable) and write (device-writable) buffers.
        let mut read_bufs: Vec<(u64, u32)> = Vec::new();
        let mut write_bufs: Vec<(u64, u32)> = Vec::new();

        let mut idx = head;
        let mut chain_len = 0u32;

        loop {
            if chain_len > 256 {
                break; // Safety limit
            }
            chain_len += 1;

            let desc_offset = (idx % qsize) as u64 * 16;
            let mut desc = [0u8; 16];
            if !self.dma_read(desc_table + desc_offset, &mut desc) {
                break;
            }

            let addr = u64::from_le_bytes([
                desc[0], desc[1], desc[2], desc[3],
                desc[4], desc[5], desc[6], desc[7],
            ]);
            let len = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]);
            let flags = u16::from_le_bytes([desc[12], desc[13]]);
            let next = u16::from_le_bytes([desc[14], desc[15]]);

            if flags & VRING_DESC_F_WRITE != 0 {
                write_bufs.push((addr, len));
            } else {
                read_bufs.push((addr, len));
            }

            if flags & VRING_DESC_F_NEXT != 0 {
                idx = next;
            } else {
                break;
            }
        }

        // Read the command header from device-readable buffers.
        let mut cmd_data = Vec::new();
        for (addr, len) in &read_bufs {
            let mut buf = vec![0u8; *len as usize];
            if self.dma_read(*addr, &mut buf) {
                cmd_data.extend_from_slice(&buf);
            }
        }

        if cmd_data.len() < 24 {
            // Minimum header size is 24 bytes (virtio_gpu_ctrl_hdr).
            return self.write_response(&write_bufs, VIRTIO_GPU_RESP_ERR_UNSPEC, &[]);
        }

        let cmd_type = u32::from_le_bytes([cmd_data[0], cmd_data[1], cmd_data[2], cmd_data[3]]);

        #[cfg(feature = "std")]
        eprintln!("[virtio-gpu] cmd=0x{:04X} len={}", cmd_type, cmd_data.len());

        match cmd_type {
            VIRTIO_GPU_CMD_GET_DISPLAY_INFO => {
                self.cmd_get_display_info(&write_bufs)
            }
            VIRTIO_GPU_CMD_RESOURCE_CREATE_2D => {
                self.cmd_resource_create_2d(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_RESOURCE_UNREF => {
                self.cmd_resource_unref(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_SET_SCANOUT => {
                self.cmd_set_scanout(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_RESOURCE_FLUSH => {
                self.cmd_resource_flush(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D => {
                self.cmd_transfer_to_host_2d(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING => {
                self.cmd_resource_attach_backing(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING => {
                self.cmd_resource_detach_backing(&cmd_data, &write_bufs)
            }
            VIRTIO_GPU_CMD_GET_CAPSET_INFO => {
                // No capability sets supported yet — return zeroed response.
                let resp = [0u8; 16]; // capset_info response body (after header)
                self.write_response(&write_bufs, VIRTIO_GPU_RESP_OK_CAPSET_INFO, &resp)
            }
            VIRTIO_GPU_CMD_GET_CAPSET => {
                self.write_response(&write_bufs, VIRTIO_GPU_RESP_OK_CAPSET, &[])
            }
            VIRTIO_GPU_CMD_GET_EDID => {
                self.cmd_get_edid(&cmd_data, &write_bufs)
            }
            _ => {
                #[cfg(feature = "std")]
                eprintln!("[virtio-gpu] unknown command: 0x{:04X}", cmd_type);
                self.write_response(&write_bufs, VIRTIO_GPU_RESP_ERR_UNSPEC, &[])
            }
        }
    }

    /// Write a response header + optional body to device-writable descriptors.
    /// Returns total bytes written.
    fn write_response(&self, write_bufs: &[(u64, u32)], resp_type: u32, body: &[u8]) -> u32 {
        // Build response header (24 bytes).
        let mut hdr = [0u8; 24];
        hdr[0..4].copy_from_slice(&resp_type.to_le_bytes());
        // flags, fence_id, ctx_id, ring_idx all zero.

        let total = 24 + body.len();
        let mut response = Vec::with_capacity(total);
        response.extend_from_slice(&hdr);
        response.extend_from_slice(body);

        let mut written = 0u32;
        let mut resp_offset = 0usize;

        for (addr, len) in write_bufs {
            if resp_offset >= response.len() {
                break;
            }
            let chunk = (*len as usize).min(response.len() - resp_offset);
            self.dma_write(*addr, &response[resp_offset..resp_offset + chunk]);
            resp_offset += chunk;
            written += chunk as u32;
        }

        written
    }

    // ── GPU command handlers ──

    fn cmd_get_display_info(&self, write_bufs: &[(u64, u32)]) -> u32 {
        // Response: header (24 bytes) + 16 * display_one (24 bytes each) = 408 bytes.
        // We have 1 scanout, rest are zeroed.
        let mut body = vec![0u8; MAX_SCANOUTS * 24 + (16 - MAX_SCANOUTS) * 24];

        // Ensure we always produce exactly 16 entries (16 * 24 = 384 bytes).
        let body_len = 16 * 24;
        body.resize(body_len, 0);

        // Fill scanout 0.
        let s = &self.scanouts[0];
        if s.enabled {
            // rect: x, y, width, height (each u32)
            body[0..4].copy_from_slice(&s.x.to_le_bytes());
            body[4..8].copy_from_slice(&s.y.to_le_bytes());
            body[8..12].copy_from_slice(&s.width.to_le_bytes());
            body[12..16].copy_from_slice(&s.height.to_le_bytes());
            // enabled flag
            body[16..20].copy_from_slice(&1u32.to_le_bytes());
            // flags
            body[20..24].copy_from_slice(&0u32.to_le_bytes());
        }

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_DISPLAY_INFO, &body)
    }

    fn cmd_resource_create_2d(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 24 + 16 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let resource_id = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);
        let format = u32::from_le_bytes([cmd[28], cmd[29], cmd[30], cmd[31]]);
        let width = u32::from_le_bytes([cmd[32], cmd[33], cmd[34], cmd[35]]);
        let height = u32::from_le_bytes([cmd[36], cmd[37], cmd[38], cmd[39]]);

        if resource_id == 0 || width == 0 || height == 0 || width > 8192 || height > 8192 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let bpp = format_bpp(format);
        let pixel_size = (width as usize) * (height as usize) * (bpp as usize);

        let resource = Resource2D {
            width,
            height,
            format,
            pixels: vec![0u8; pixel_size],
            backing: Vec::new(),
        };

        self.resources.insert(resource_id, resource);

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
    }

    fn cmd_resource_unref(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 28 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let resource_id = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);

        if self.resources.remove(&resource_id).is_none() {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID, &[]);
        }

        // If any scanout references this resource, detach it.
        for scanout in &mut self.scanouts {
            if scanout.resource_id == resource_id {
                scanout.resource_id = 0;
            }
        }

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
    }

    fn cmd_set_scanout(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 24 + 24 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        // struct virtio_gpu_set_scanout: hdr(24) + rect(x,y,w,h @ 24,28,32,36) + scanout_id(40) + resource_id(44)
        let r_x = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);
        let r_y = u32::from_le_bytes([cmd[28], cmd[29], cmd[30], cmd[31]]);
        let r_w = u32::from_le_bytes([cmd[32], cmd[33], cmd[34], cmd[35]]);
        let r_h = u32::from_le_bytes([cmd[36], cmd[37], cmd[38], cmd[39]]);
        let scanout_id = u32::from_le_bytes([cmd[40], cmd[41], cmd[42], cmd[43]]);
        let resource_id = u32::from_le_bytes([cmd[44], cmd[45], cmd[46], cmd[47]]);

        if scanout_id as usize >= MAX_SCANOUTS {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        // resource_id == 0 means disable scanout.
        if resource_id == 0 {
            self.scanouts[scanout_id as usize].resource_id = 0;
            if scanout_id == 0 { self.scanout_active = false; }
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[]);
        }

        if !self.resources.contains_key(&resource_id) {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID, &[]);
        }

        let scanout = &mut self.scanouts[scanout_id as usize];
        scanout.resource_id = resource_id;
        scanout.x = r_x;
        scanout.y = r_y;
        if r_w > 0 && r_h > 0 {
            scanout.width = r_w;
            scanout.height = r_h;
        }

        // Mark scanout as active — the guest driver has configured display output.
        if scanout_id == 0 { self.scanout_active = true; }

        #[cfg(feature = "std")]
        eprintln!("[virtio-gpu] SET_SCANOUT scanout={} res={} rect={}x{}+{}+{}", scanout_id, resource_id, r_w, r_h, r_x, r_y);

        // Update framebuffer dimensions if scanout size changed.
        if scanout_id == 0 && r_w > 0 && r_h > 0 {
            self.fb_width = r_w;
            self.fb_height = r_h;
            let needed = (r_w as usize) * (r_h as usize) * 4;
            if self.framebuffer.len() < needed {
                self.framebuffer.resize(needed, 0);
            }
        }

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
    }

    fn cmd_resource_flush(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 24 + 24 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        // struct virtio_gpu_resource_flush: hdr(24) + rect(x,y,w,h @ 24,28,32,36) + resource_id(40)
        let _r_x = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);
        let _r_y = u32::from_le_bytes([cmd[28], cmd[29], cmd[30], cmd[31]]);
        let _r_w = u32::from_le_bytes([cmd[32], cmd[33], cmd[34], cmd[35]]);
        let _r_h = u32::from_le_bytes([cmd[36], cmd[37], cmd[38], cmd[39]]);
        let resource_id = u32::from_le_bytes([cmd[40], cmd[41], cmd[42], cmd[43]]);

        #[cfg(feature = "std")]
        eprintln!("[virtio-gpu] FLUSH res={} rect={}x{}+{}+{}", resource_id, _r_w, _r_h, _r_x, _r_y);

        // Find scanout(s) displaying this resource and copy to framebuffer.
        // We need to temporarily remove the resource to avoid borrow conflicts
        // with self.framebuffer during blit.
        for i in 0..MAX_SCANOUTS {
            if self.scanouts[i].resource_id == resource_id {
                if let Some(resource) = self.resources.remove(&resource_id) {
                    #[cfg(feature = "std")]
                    {
                        let nonzero = resource.pixels.iter().filter(|&&b| b != 0).count();
                        eprintln!("[virtio-gpu] BLIT res={} {}x{} fmt={} pixels_nonzero={}/{}",
                            resource_id, resource.width, resource.height, resource.format,
                            nonzero, resource.pixels.len());
                    }
                    self.blit_resource_to_framebuffer(&resource, i);
                    self.resources.insert(resource_id, resource);
                }
            }
        }

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
    }

    fn cmd_transfer_to_host_2d(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 24 + 28 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let r_x = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);
        let r_y = u32::from_le_bytes([cmd[28], cmd[29], cmd[30], cmd[31]]);
        let r_w = u32::from_le_bytes([cmd[32], cmd[33], cmd[34], cmd[35]]);
        let r_h = u32::from_le_bytes([cmd[36], cmd[37], cmd[38], cmd[39]]);
        let offset = u64::from_le_bytes([
            cmd[40], cmd[41], cmd[42], cmd[43],
            cmd[44], cmd[45], cmd[46], cmd[47],
        ]);
        let resource_id = u32::from_le_bytes([cmd[48], cmd[49], cmd[50], cmd[51]]);

        // Extract DMA pointers before borrowing resources mutably.
        let mem_ptr = self.guest_mem_ptr;
        let mem_len = self.guest_mem_len;

        let resource = match self.resources.get_mut(&resource_id) {
            Some(r) => r,
            None => return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID, &[]),
        };

        if resource.backing.is_empty() {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_UNSPEC, &[]);
        }

        let bpp = format_bpp(resource.format) as u32;
        let src_stride = resource.width * bpp;

        // First, read all backing pages into a contiguous host buffer.
        // This is much faster than pixel-by-pixel DMA reads and avoids
        // offset calculation bugs with scatter-gather backing.
        let total_backing_size: u64 = resource.backing.iter().map(|(_, len)| *len as u64).sum();
        let mut backing_buf = vec![0u8; total_backing_size as usize];
        let mut buf_offset = 0usize;
        let mut dma_fail_count = 0u32;
        let mut dma_ok_count = 0u32;
        for (addr, len) in &resource.backing {
            let chunk_len = *len as usize;
            if buf_offset + chunk_len <= backing_buf.len() {
                let ok = dma_read_raw(mem_ptr, mem_len, *addr, &mut backing_buf[buf_offset..buf_offset + chunk_len]);
                if ok { dma_ok_count += 1; } else { dma_fail_count += 1; }
            }
            buf_offset += chunk_len;
        }

        #[cfg(feature = "std")]
        {
            static mut BACKING_LOG: u32 = 0;
            unsafe { BACKING_LOG += 1; if BACKING_LOG <= 5 {
                let first_addr = resource.backing.first().map(|(a,_)| *a).unwrap_or(0);
                let backing_nonzero = backing_buf.iter().filter(|&&b| b != 0).count();
                eprintln!("[virtio-gpu] TRANSFER backing: {} entries, first_gpa=0x{:X} total={} dma_ok={} dma_fail={} backing_nonzero={}/{}",
                    resource.backing.len(), first_addr, total_backing_size, dma_ok_count, dma_fail_count, backing_nonzero, backing_buf.len());
            }}
        }

        // Now copy the requested rectangle from the backing buffer into the resource pixels.
        // The backing buffer layout matches the resource layout (stride = width * bpp).
        let offset = offset as usize;
        for y in r_y..(r_y + r_h).min(resource.height) {
            let dst_row_start = (y * src_stride + r_x * bpp) as usize;
            // Source offset: the `offset` parameter is the starting byte offset
            // within the backing, then we advance by row stride.
            let src_row_start = offset + ((y - r_y) as usize) * (r_w * bpp) as usize;
            let row_bytes = ((r_w.min(resource.width - r_x)) * bpp) as usize;

            if src_row_start + row_bytes <= backing_buf.len()
                && dst_row_start + row_bytes <= resource.pixels.len()
            {
                resource.pixels[dst_row_start..dst_row_start + row_bytes]
                    .copy_from_slice(&backing_buf[src_row_start..src_row_start + row_bytes]);
            }
        }

        #[cfg(feature = "std")]
        {
            let nonzero = resource.pixels.iter().filter(|&&b| b != 0).count();
            static mut XFER_LOG_COUNT: u32 = 0;
            unsafe {
                XFER_LOG_COUNT += 1;
                if XFER_LOG_COUNT <= 10 || XFER_LOG_COUNT % 100 == 0 {
                    eprintln!("[virtio-gpu] TRANSFER_2D res={} rect={}x{}+{}+{} off={} backing={} nonzero={}/{}",
                        resource_id, r_w, r_h, r_x, r_y, offset,
                        total_backing_size, nonzero, resource.pixels.len());
                }
            }
        }

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
    }

    fn cmd_resource_attach_backing(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 24 + 8 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let resource_id = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);
        let nr_entries = u32::from_le_bytes([cmd[28], cmd[29], cmd[30], cmd[31]]);

        let resource = match self.resources.get_mut(&resource_id) {
            Some(r) => r,
            None => return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID, &[]),
        };

        // Read memory entries (addr: u64, length: u32, padding: u32 = 16 bytes each).
        resource.backing.clear();
        let entries_offset = 32; // After header (24) + resource_id (4) + nr_entries (4)
        for i in 0..nr_entries as usize {
            let base = entries_offset + i * 16;
            if base + 16 > cmd.len() {
                break;
            }
            let addr = u64::from_le_bytes([
                cmd[base], cmd[base + 1], cmd[base + 2], cmd[base + 3],
                cmd[base + 4], cmd[base + 5], cmd[base + 6], cmd[base + 7],
            ]);
            let length = u32::from_le_bytes([
                cmd[base + 8], cmd[base + 9], cmd[base + 10], cmd[base + 11],
            ]);
            resource.backing.push((addr, length));
        }

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
    }

    fn cmd_resource_detach_backing(&mut self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 28 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let resource_id = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);

        match self.resources.get_mut(&resource_id) {
            Some(r) => {
                r.backing.clear();
                self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_NODATA, &[])
            }
            None => self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID, &[]),
        }
    }

    fn cmd_get_edid(&self, cmd: &[u8], write_bufs: &[(u64, u32)]) -> u32 {
        if cmd.len() < 28 {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let scanout_id = u32::from_le_bytes([cmd[24], cmd[25], cmd[26], cmd[27]]);
        if scanout_id as usize >= MAX_SCANOUTS {
            return self.write_response(write_bufs, VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER, &[]);
        }

        let scanout = &self.scanouts[scanout_id as usize];

        // Generate a minimal EDID block (128 bytes).
        let edid = generate_edid(scanout.width, scanout.height);

        // Response body: size (u32) + padding (u32) + 1024 bytes EDID data.
        let mut body = vec![0u8; 8 + 1024];
        let edid_len = edid.len() as u32;
        body[0..4].copy_from_slice(&edid_len.to_le_bytes());
        // padding at [4..8]
        body[8..8 + edid.len()].copy_from_slice(&edid);

        self.write_response(write_bufs, VIRTIO_GPU_RESP_OK_EDID, &body)
    }

    /// Resolve a byte offset within the backing memory entries to a guest physical address.
    fn backing_gpa_at(backing: &[(u64, u32)], offset: u64) -> Option<u64> {
        let mut current = 0u64;
        for (addr, len) in backing {
            let end = current + *len as u64;
            if offset >= current && offset < end {
                return Some(*addr + (offset - current));
            }
            current = end;
        }
        None
    }

    /// Blit a resource's pixel data into the scanout framebuffer.
    fn blit_resource_to_framebuffer(&mut self, resource: &Resource2D, scanout_idx: usize) {
        if scanout_idx >= MAX_SCANOUTS {
            return;
        }
        let scanout = &self.scanouts[scanout_idx];
        let src_bpp = format_bpp(resource.format) as u32;
        let src_stride = resource.width * src_bpp;
        let dst_stride = self.fb_width * 4; // Framebuffer is always BGRA32

        let copy_w = resource.width.min(self.fb_width);
        let copy_h = resource.height.min(self.fb_height);

        for y in 0..copy_h {
            for x in 0..copy_w {
                let src_off = (y * src_stride + x * src_bpp) as usize;
                let dst_off = (y * dst_stride + x * 4) as usize;

                if src_off + src_bpp as usize > resource.pixels.len() {
                    continue;
                }
                if dst_off + 4 > self.framebuffer.len() {
                    continue;
                }

                // Convert from source format to BGRA32.
                let (b, g, r, a) = pixel_to_bgra(
                    resource.format,
                    &resource.pixels[src_off..src_off + src_bpp as usize],
                );
                self.framebuffer[dst_off] = b;
                self.framebuffer[dst_off + 1] = g;
                self.framebuffer[dst_off + 2] = r;
                self.framebuffer[dst_off + 3] = a;
            }
        }
    }

    // ── Common config register access ──

    fn read_common_cfg(&self, offset: u64, size: u8) -> u64 {
        let val = match offset {
            // device_feature_sel
            0x00 => self.device_feature_sel as u64,
            // device_feature (returns low or high 32 bits based on sel)
            0x04 => {
                if self.device_feature_sel == 0 {
                    (self.device_features & 0xFFFF_FFFF) as u64
                } else {
                    ((self.device_features >> 32) & 0xFFFF_FFFF) as u64
                }
            }
            // driver_feature_sel
            0x08 => self.driver_feature_sel as u64,
            // driver_feature
            0x0C => {
                if self.driver_feature_sel == 0 {
                    (self.driver_features & 0xFFFF_FFFF) as u64
                } else {
                    ((self.driver_features >> 32) & 0xFFFF_FFFF) as u64
                }
            }
            // msix_config
            0x10 => 0xFFFF, // No MSI-X table
            // num_queues
            0x12 => NUM_QUEUES as u64,
            // device_status
            0x14 => self.status as u64,
            // config_generation
            0x15 => self.config_generation as u64,
            // queue_select
            0x16 => self.queue_select as u64,
            // queue_size
            0x18 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].size as u64 } else { 0 }
            }
            // queue_msix_vector
            0x1A => 0xFFFF,
            // queue_enable
            0x1C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].ready as u64 } else { 0 }
            }
            // queue_notify_off
            0x1E => self.queue_select as u64,
            // queue_desc (lo)
            0x20 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].desc_addr & 0xFFFF_FFFF } else { 0 }
            }
            // queue_desc (hi)
            0x24 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].desc_addr >> 32 } else { 0 }
            }
            // queue_avail (lo)
            0x28 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].avail_addr & 0xFFFF_FFFF } else { 0 }
            }
            // queue_avail (hi)
            0x2C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].avail_addr >> 32 } else { 0 }
            }
            // queue_used (lo)
            0x30 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].used_addr & 0xFFFF_FFFF } else { 0 }
            }
            // queue_used (hi)
            0x34 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES { self.queues[qi].used_addr >> 32 } else { 0 }
            }
            _ => 0,
        };

        // Handle sub-dword reads.
        let mask = match size {
            1 => 0xFF,
            2 => 0xFFFF,
            _ => 0xFFFF_FFFF,
        };
        val & mask
    }

    fn write_common_cfg(&mut self, offset: u64, _size: u8, val: u64) {
        match offset {
            0x00 => self.device_feature_sel = val as u32,
            0x08 => self.driver_feature_sel = val as u32,
            0x0C => {
                if self.driver_feature_sel == 0 {
                    self.driver_features = (self.driver_features & 0xFFFF_FFFF_0000_0000)
                        | (val & 0xFFFF_FFFF);
                } else {
                    self.driver_features = (self.driver_features & 0xFFFF_FFFF)
                        | ((val & 0xFFFF_FFFF) << 32);
                }
            }
            0x14 => {
                // device_status
                let new_status = val as u8;
                if new_status == 0 {
                    // Device reset.
                    self.status = 0;
                    self.driver_features = 0;
                    for q in &mut self.queues {
                        q.ready = 0;
                        q.last_avail_idx = 0;
                    }
                    self.isr_status = 0;
                    return;
                }
                self.status = new_status;
            }
            0x16 => self.queue_select = val as u16,
            0x18 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    let new_size = (val as u16).min(VIRTQUEUE_MAX_SIZE);
                    self.queues[qi].size = new_size;
                }
            }
            0x1C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].ready = val as u16;
                }
            }
            0x20 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].desc_addr = (self.queues[qi].desc_addr & 0xFFFF_FFFF_0000_0000)
                        | (val & 0xFFFF_FFFF);
                }
            }
            0x24 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].desc_addr = (self.queues[qi].desc_addr & 0xFFFF_FFFF)
                        | ((val & 0xFFFF_FFFF) << 32);
                }
            }
            0x28 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].avail_addr = (self.queues[qi].avail_addr & 0xFFFF_FFFF_0000_0000)
                        | (val & 0xFFFF_FFFF);
                }
            }
            0x2C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].avail_addr = (self.queues[qi].avail_addr & 0xFFFF_FFFF)
                        | ((val & 0xFFFF_FFFF) << 32);
                }
            }
            0x30 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].used_addr = (self.queues[qi].used_addr & 0xFFFF_FFFF_0000_0000)
                        | (val & 0xFFFF_FFFF);
                }
            }
            0x34 => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].used_addr = (self.queues[qi].used_addr & 0xFFFF_FFFF)
                        | ((val & 0xFFFF_FFFF) << 32);
                }
            }
            _ => {}
        }
    }

    /// Read GPU device-specific config (events_read, events_clear, num_scanouts, num_capsets).
    fn read_device_cfg(&self, offset: u64, _size: u8) -> u64 {
        match offset {
            0x00 => 0,                   // events_read
            0x04 => 0,                   // events_clear
            0x08 => MAX_SCANOUTS as u64, // num_scanouts
            0x0C => 0,                   // num_capsets (Phase 2: Virgl3D)
            _ => 0,
        }
    }
}

impl MmioHandler for VirtioGpu {
    fn read(&mut self, offset: u64, size: u8) -> Result<u64> {
        if offset >= COMMON_CFG_OFFSET && offset < COMMON_CFG_OFFSET + COMMON_CFG_SIZE {
            return Ok(self.read_common_cfg(offset - COMMON_CFG_OFFSET, size));
        }
        if offset >= ISR_OFFSET && offset < ISR_OFFSET + ISR_SIZE {
            // Reading ISR clears it.
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
            // Queue notification doorbell — process pending commands.
            self.process();
            return Ok(());
        }
        if offset >= ISR_OFFSET && offset < ISR_OFFSET + ISR_SIZE {
            // Write to ISR (acknowledge).
            self.isr_status &= !(val as u32);
            return Ok(());
        }
        if offset >= DEVICE_CFG_OFFSET && offset < DEVICE_CFG_OFFSET + DEVICE_CFG_SIZE {
            let dev_off = offset - DEVICE_CFG_OFFSET;
            if dev_off == 0x04 {
                // events_clear — acknowledge events.
            }
            return Ok(());
        }
        Ok(())
    }
}

// ── Helper functions ──

/// Bytes per pixel for a given VirtIO GPU format.
fn format_bpp(format: u32) -> u8 {
    match format {
        VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM
        | VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM
        | VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM
        | VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM
        | VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM
        | VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM
        | VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM
        | VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM => 4,
        _ => 4, // Default to 4 bpp for unknown formats.
    }
}

/// Convert a pixel from source format to BGRA components.
fn pixel_to_bgra(format: u32, src: &[u8]) -> (u8, u8, u8, u8) {
    if src.len() < 4 {
        return (0, 0, 0, 255);
    }
    match format {
        VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM => (src[0], src[1], src[2], src[3]),
        VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM => (src[0], src[1], src[2], 255),
        VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM => (src[2], src[1], src[0], src[3]),
        VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM => (src[2], src[1], src[0], 255),
        VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM => (src[3], src[2], src[1], src[0]),
        VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM => (src[3], src[2], src[1], 255),
        VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM => (src[1], src[2], src[3], src[0]),
        VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM => (src[1], src[2], src[3], 255),
        _ => (src[0], src[1], src[2], if src.len() > 3 { src[3] } else { 255 }),
    }
}

/// Standalone DMA read that doesn't borrow `self`.
/// Used to avoid borrow conflicts when resources are borrowed mutably.
fn dma_read_raw(mem_ptr: *mut u8, mem_len: usize, gpa: u64, buf: &mut [u8]) -> bool {
    if mem_ptr.is_null() { return false; }
    let offset = if gpa < PCI_HOLE_START {
        gpa as usize
    } else if gpa >= PCI_HOLE_END {
        (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize
    } else {
        return false;
    };
    if offset + buf.len() > mem_len {
        return false;
    }
    unsafe { core::ptr::copy_nonoverlapping(mem_ptr.add(offset), buf.as_mut_ptr(), buf.len()); }
    true
}

/// Allocate a page-aligned Vec<u8> of the given size.
fn alloc_page_aligned(size: usize) -> Vec<u8> {
    // Round up to page boundary for hypervisor mapping.
    let page_size = 4096;
    let aligned_size = (size + page_size - 1) & !(page_size - 1);

    // Allocate with proper alignment.
    let layout = core::alloc::Layout::from_size_align(aligned_size, page_size).unwrap();
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        // Fallback to regular allocation.
        return vec![0u8; aligned_size];
    }
    unsafe { Vec::from_raw_parts(ptr, aligned_size, aligned_size) }
}

/// Generate a minimal 128-byte EDID block for the given resolution.
fn generate_edid(width: u32, height: u32) -> Vec<u8> {
    let mut edid = vec![0u8; 128];

    // Header.
    edid[0] = 0x00;
    edid[1] = 0xFF;
    edid[2] = 0xFF;
    edid[3] = 0xFF;
    edid[4] = 0xFF;
    edid[5] = 0xFF;
    edid[6] = 0xFF;
    edid[7] = 0x00;

    // Manufacturer ID: "CVM" (CoreVM) — encoded as 3x5-bit chars.
    // C=3, V=22, M=13 → 00011 10110 01101 = 0x0ED5
    // Wait, EDID encodes as big-endian: byte[8] = high, byte[9] = low
    // C=3 (00011), V=22 (10110), M=13 (01101)
    // bits: 0_00011_10110_01101 = 0x0ED5
    edid[8] = 0x0E;
    edid[9] = 0xD5;

    // Product code.
    edid[10] = 0x01;
    edid[11] = 0x00;

    // Serial number.
    edid[12] = 0x01;
    edid[13] = 0x00;
    edid[14] = 0x00;
    edid[15] = 0x00;

    // Week/year of manufacture (week 1, 2024).
    edid[16] = 1;
    edid[17] = 34; // 2024 - 1990

    // EDID version 1.4.
    edid[18] = 1;
    edid[19] = 4;

    // Video input: digital, 8-bit color depth, DisplayPort.
    edid[20] = 0xA5;

    // Screen size (cm): approximate from pixels (assume 96 DPI).
    let w_cm = ((width as f32 / 96.0) * 2.54) as u8;
    let h_cm = ((height as f32 / 96.0) * 2.54) as u8;
    edid[21] = w_cm.max(1);
    edid[22] = h_cm.max(1);

    // Gamma (2.2 = 120 in EDID encoding: (gamma - 1) * 100).
    edid[23] = 120;

    // Supported features: RGB color, preferred timing in DTD 1.
    edid[24] = 0x0A;

    // Chromaticity coordinates (sRGB standard).
    edid[25] = 0xEE;
    edid[26] = 0x91;
    edid[27] = 0xA3;
    edid[28] = 0x54;
    edid[29] = 0x4C;
    edid[30] = 0x99;
    edid[31] = 0x26;
    edid[32] = 0x0F;
    edid[33] = 0x50;
    edid[34] = 0x54;

    // Established timings (640x480, 800x600, 1024x768).
    edid[35] = 0x21;
    edid[36] = 0x08;
    edid[37] = 0x00;

    // Standard timings (all unused).
    for i in 38..54 {
        edid[i] = 0x01;
        if i % 2 == 1 {
            edid[i] = 0x01;
        }
    }

    // Detailed Timing Descriptor #1 (18 bytes at offset 54).
    // Use a simple timing for the native resolution.
    let pixel_clock_khz: u32 = if width == 1920 && height == 1080 {
        148500 // Standard 1080p @ 60Hz
    } else if width == 1280 && height == 720 {
        74250
    } else {
        // Approximate: width * height * 60 * 1.3 (blanking overhead) / 1000
        ((width as u64 * height as u64 * 78) / 1000) as u32
    };
    let pc_10khz = (pixel_clock_khz / 10) as u16;
    edid[54] = (pc_10khz & 0xFF) as u8;
    edid[55] = ((pc_10khz >> 8) & 0xFF) as u8;

    // Horizontal addressable pixels (lower 8 bits).
    let h_active = width as u16;
    let h_blank: u16 = 160; // Simplified blanking.
    edid[56] = (h_active & 0xFF) as u8;
    edid[57] = (h_blank & 0xFF) as u8;
    edid[58] = (((h_active >> 8) & 0x0F) << 4 | ((h_blank >> 8) & 0x0F)) as u8;

    // Vertical addressable lines.
    let v_active = height as u16;
    let v_blank: u16 = 45; // Simplified blanking.
    edid[59] = (v_active & 0xFF) as u8;
    edid[60] = (v_blank & 0xFF) as u8;
    edid[61] = (((v_active >> 8) & 0x0F) << 4 | ((v_blank >> 8) & 0x0F)) as u8;

    // Sync (simplified).
    edid[62] = 44; // H front porch
    edid[63] = 88; // H sync pulse
    edid[64] = 0x54; // V front porch + V sync (4 lines front, 5 lines sync)
    edid[65] = 0x00;

    // Image size in mm.
    let w_mm = (w_cm as u16) * 10;
    let h_mm = (h_cm as u16) * 10;
    edid[66] = (w_mm & 0xFF) as u8;
    edid[67] = (h_mm & 0xFF) as u8;
    edid[68] = (((w_mm >> 8) & 0x0F) << 4 | ((h_mm >> 8) & 0x0F)) as u8;

    // No border.
    edid[69] = 0;
    edid[70] = 0;

    // Features: non-interlaced, normal display, digital separate sync.
    edid[71] = 0x1E;

    // Descriptor #2: Monitor name.
    edid[72..90].copy_from_slice(&[
        0x00, 0x00, 0x00, 0xFC, 0x00, // Monitor Name tag
        b'C', b'o', b'r', b'e', b'V', b'M', b' ', b'G', b'P', b'U',
        0x0A, 0x20, 0x20,
    ]);

    // Descriptor #3: Monitor range limits (dummy).
    edid[90..108].copy_from_slice(&[
        0x00, 0x00, 0x00, 0xFD, 0x00, // Range Limits tag
        0x32, 0x4C, 0x1E, 0x51, 0x11, // 50-76Hz V, 30-81kHz H
        0x00, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
    ]);

    // Descriptor #4: Dummy.
    edid[108..126].copy_from_slice(&[
        0x00, 0x00, 0x00, 0x10, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ]);

    // Extension count.
    edid[126] = 0;

    // Checksum: sum of all 128 bytes must be 0 mod 256.
    let sum: u8 = edid[0..127].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    edid[127] = 0u8.wrapping_sub(sum);

    edid
}
