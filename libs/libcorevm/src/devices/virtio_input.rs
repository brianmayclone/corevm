//! VirtIO Input device emulation (VirtIO Spec v1.2, Section 5.8).
//!
//! Provides paravirtual keyboard and mouse/tablet input devices.
//! Linux has the `virtio_input` driver built into the kernel.
//! Windows gets drivers via Windows Update (vioinput from Red Hat virtio-win).
//!
//! Two separate PCI devices are created:
//! - Keyboard: PCI 1AF4:1052 at slot 00:09.0
//! - Tablet (absolute pointer): PCI 1AF4:1052 at slot 00:0A.0
//!
//! # Virtqueues
//!
//! - Queue 0: eventq — device → driver (input events)
//! - Queue 1: statusq — driver → device (LED status, etc.)

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::Result;
use crate::memory::mmio::MmioHandler;

// ── VirtIO Input config select values ──

const VIRTIO_INPUT_CFG_UNSET: u8 = 0x00;
const VIRTIO_INPUT_CFG_ID_NAME: u8 = 0x01;
const VIRTIO_INPUT_CFG_ID_SERIAL: u8 = 0x02;
const VIRTIO_INPUT_CFG_ID_DEVIDS: u8 = 0x03;
const VIRTIO_INPUT_CFG_PROP_BITS: u8 = 0x10;
const VIRTIO_INPUT_CFG_EV_BITS: u8 = 0x11;
const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

// ── Linux input event types ──

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;
const EV_LED: u16 = 0x11;
const EV_REP: u16 = 0x14;

// ── Linux input codes ──

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_TOUCH: u16 = 0x14A;
const BTN_TOOL_FINGER: u16 = 0x145;

// ── VirtIO feature bits ──

const VIRTIO_F_VERSION_1: u64 = 1 << 32;

// ── VirtIO device status bits ──

const VIRTIO_STATUS_DRIVER_OK: u8 = 4;

// ── PCI hole constants ──

const PCI_HOLE_START: u64 = 0xE000_0000;
const PCI_HOLE_END: u64 = 0x1_0000_0000;

// ── BAR0 MMIO layout ──

const COMMON_CFG_OFFSET: u64 = 0x0000;
const COMMON_CFG_SIZE: u64 = 0x40;
const NOTIFY_OFFSET: u64 = 0x1000;
const ISR_OFFSET: u64 = 0x2000;
const DEVICE_CFG_OFFSET: u64 = 0x3000;
const DEVICE_CFG_SIZE: u64 = 0x100;

/// BAR0 total size (16 KB).
pub const VIRTIO_INPUT_BAR0_SIZE: u32 = 0x4000;

const VIRTQUEUE_MAX_SIZE: u16 = 64;
const NUM_QUEUES: usize = 2;

const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

/// Input device subtype.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputDeviceType {
    Keyboard,
    Tablet, // Absolute pointer (like USB tablet)
}

/// A single virtqueue.
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

/// A single Linux input_event: type(u16) + code(u16) + value(u32) = 8 bytes.
#[derive(Clone, Copy)]
struct InputEvent {
    ev_type: u16,
    code: u16,
    value: u32,
}

/// VirtIO Input device.
pub struct VirtioInput {
    // ── VirtIO transport ──
    device_features: u64,
    driver_features: u64,
    device_feature_sel: u32,
    driver_feature_sel: u32,
    status: u8,
    queue_select: u16,
    pub isr_status: u32,
    config_generation: u8,
    queues: [Virtqueue; NUM_QUEUES],

    // ── Device config ──
    pub device_type: InputDeviceType,
    /// Current config select/subsel for device-specific config reads.
    cfg_select: u8,
    cfg_subsel: u8,

    // ── Event queue ──
    /// Pending input events to deliver to the guest.
    pub event_queue: VecDeque<InputEvent>,

    // ── DMA ──
    pub guest_mem_ptr: *mut u8,
    pub guest_mem_len: usize,

    pub irq_pending: bool,

    /// Callback to immediately signal an interrupt and kick the vCPU.
    #[cfg(feature = "std")]
    pub notify_callback: Option<alloc::boxed::Box<dyn FnMut() + Send>>,
}

unsafe impl Send for VirtioInput {}

impl VirtioInput {
    pub fn new(device_type: InputDeviceType) -> Self {
        VirtioInput {
            device_features: VIRTIO_F_VERSION_1,
            driver_features: 0,
            device_feature_sel: 0,
            driver_feature_sel: 0,
            status: 0,
            queue_select: 0,
            isr_status: 0,
            config_generation: 0,
            queues: [Virtqueue::new(), Virtqueue::new()],
            device_type,
            cfg_select: 0,
            cfg_subsel: 0,
            event_queue: VecDeque::new(),
            guest_mem_ptr: core::ptr::null_mut(),
            guest_mem_len: 0,
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

    /// Drop events if the queue is backing up (driver not consuming fast enough).
    fn trim_queue(&mut self) {
        const MAX_PENDING: usize = 256;
        if self.event_queue.len() > MAX_PENDING {
            // Drop oldest events, keep the newest ones.
            let drop_count = self.event_queue.len() - MAX_PENDING;
            self.event_queue.drain(..drop_count);
        }
    }

    /// Don't queue events if driver hasn't loaded yet.
    fn driver_ready(&self) -> bool {
        self.status & VIRTIO_STATUS_DRIVER_OK != 0
    }

    /// Inject a keyboard scancode. `pressed`: true=make, false=break.
    /// `key`: Linux KEY_* code (not PS/2 scancode!).
    pub fn inject_key(&mut self, key: u16, pressed: bool) {
        if !self.driver_ready() {
            #[cfg(feature = "std")]
            {
                static mut KBD_NOT_RDY: u32 = 0;
                unsafe { KBD_NOT_RDY += 1; if KBD_NOT_RDY <= 5 {
                    eprintln!("[virtio-input:kbd] inject_key(key={}, pressed={}) but driver not ready (status=0x{:02X})",
                        key, pressed, self.status);
                }}
            }
            return;
        }
        #[cfg(feature = "std")]
        {
            static mut KBD_INJ: u32 = 0;
            unsafe { KBD_INJ += 1; if KBD_INJ <= 20 {
                eprintln!("[virtio-input:kbd] inject_key(key={}, pressed={}) queue_len={}",
                    key, pressed, self.event_queue.len());
            }}
        }
        self.trim_queue();
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY,
            code: key,
            value: if pressed { 1 } else { 0 },
        });
        // SYN_REPORT
        self.event_queue.push_back(InputEvent {
            ev_type: EV_SYN,
            code: 0,
            value: 0,
        });
    }

    /// Inject relative mouse movement.
    pub fn inject_rel_mouse(&mut self, dx: i32, dy: i32, buttons: u8) {
        if !self.driver_ready() { return; }
        self.trim_queue();
        if dx != 0 {
            self.event_queue.push_back(InputEvent {
                ev_type: EV_REL, code: REL_X, value: dx as u32,
            });
        }
        if dy != 0 {
            self.event_queue.push_back(InputEvent {
                ev_type: EV_REL, code: REL_Y, value: dy as u32,
            });
        }
        // Button events
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY, code: BTN_LEFT, value: if buttons & 1 != 0 { 1 } else { 0 },
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY, code: BTN_RIGHT, value: if buttons & 2 != 0 { 1 } else { 0 },
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY, code: BTN_MIDDLE, value: if buttons & 4 != 0 { 1 } else { 0 },
        });
        // SYN_REPORT
        self.event_queue.push_back(InputEvent {
            ev_type: EV_SYN, code: 0, value: 0,
        });
    }

    /// Inject absolute tablet position (0..32767 for x and y).
    pub fn inject_abs_tablet(&mut self, x: u32, y: u32, buttons: u8) {
        if !self.driver_ready() { return; }
        self.trim_queue();
        self.event_queue.push_back(InputEvent {
            ev_type: EV_ABS, code: ABS_X, value: x,
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_ABS, code: ABS_Y, value: y,
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY, code: BTN_LEFT, value: if buttons & 1 != 0 { 1 } else { 0 },
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY, code: BTN_RIGHT, value: if buttons & 2 != 0 { 1 } else { 0 },
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_KEY, code: BTN_MIDDLE, value: if buttons & 4 != 0 { 1 } else { 0 },
        });
        self.event_queue.push_back(InputEvent {
            ev_type: EV_SYN, code: 0, value: 0,
        });
    }

    /// Deliver pending events to the guest via eventq (queue 0).
    pub fn process_eventq(&mut self) {
        if self.status & VIRTIO_STATUS_DRIVER_OK == 0 {
            #[cfg(feature = "std")]
            if !self.event_queue.is_empty() {
                static mut NOT_READY_LOG: u32 = 0;
                unsafe { NOT_READY_LOG += 1; if NOT_READY_LOG <= 5 {
                    let name = match self.device_type { InputDeviceType::Keyboard => "kbd", InputDeviceType::Tablet => "tablet" };
                    eprintln!("[virtio-input:{}] driver not ready (status=0x{:02X}), {} events pending", name, self.status, self.event_queue.len());
                }}
            }
            return;
        }
        if self.event_queue.is_empty() { return; }

        let q = &self.queues[0];
        if q.ready == 0 || q.size == 0 || q.desc_addr == 0 || q.avail_addr == 0 {
            #[cfg(feature = "std")]
            {
                static mut Q_NOT_READY_LOG: u32 = 0;
                unsafe { Q_NOT_READY_LOG += 1; if Q_NOT_READY_LOG <= 5 {
                    let name = match self.device_type { InputDeviceType::Keyboard => "kbd", InputDeviceType::Tablet => "tablet" };
                    eprintln!("[virtio-input:{}] eventq not ready (ready={} size={} desc=0x{:X} avail=0x{:X})", name, q.ready, q.size, q.desc_addr, q.avail_addr);
                }}
            }
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

        #[cfg(feature = "std")]
        {
            // Use device_type to disambiguate log counters between kbd and tablet
            let is_kbd = self.device_type == InputDeviceType::Keyboard;
            static mut DELIVER_LOG_KBD: u32 = 0;
            static mut DELIVER_LOG_TAB: u32 = 0;
            let log_count = unsafe { if is_kbd { DELIVER_LOG_KBD += 1; DELIVER_LOG_KBD } else { DELIVER_LOG_TAB += 1; DELIVER_LOG_TAB } };
            if log_count <= 30 || log_count % 500 == 0 {
                let name = if is_kbd { "kbd" } else { "tablet" };
                eprintln!("[virtio-input:{}] delivering: pending={} last_avail={} avail_idx={} qsize={}",
                    name, self.event_queue.len(), last_avail, avail_idx, qsize);
            }
        }

        let mut delivered = 0u32;

        while !self.event_queue.is_empty() {
            if last_avail == avail_idx { break; }

            let avail_offset = 4 + (last_avail % qsize) as u64 * 2;
            let mut head_buf = [0u8; 2];
            if !self.dma_read(avail_addr + avail_offset, &mut head_buf) { break; }
            let head_idx = u16::from_le_bytes(head_buf);

            // Read descriptor — eventq buffers are device-writable (WRITE flag).
            let desc_off = (head_idx % qsize) as u64 * 16;
            let mut desc = [0u8; 16];
            if !self.dma_read(desc_addr + desc_off, &mut desc) { break; }

            let buf_addr = u64::from_le_bytes([
                desc[0], desc[1], desc[2], desc[3],
                desc[4], desc[5], desc[6], desc[7],
            ]);
            let buf_len = u32::from_le_bytes([desc[8], desc[9], desc[10], desc[11]]);

            // Write one event (8 bytes) into the guest buffer.
            // VirtIO input event: type(u16LE) + code(u16LE) + value(u32LE)
            if buf_len >= 8 {
                let ev = self.event_queue.pop_front().unwrap();
                let mut event_data = [0u8; 8];
                event_data[0..2].copy_from_slice(&ev.ev_type.to_le_bytes());
                event_data[2..4].copy_from_slice(&ev.code.to_le_bytes());
                event_data[4..8].copy_from_slice(&ev.value.to_le_bytes());
                self.dma_write(buf_addr, &event_data);
            } else {
                // Buffer too small — skip event.
                self.event_queue.pop_front();
            }

            // Write to used ring.
            let used_ring_idx_addr = used_addr + 2;
            let mut used_idx_buf = [0u8; 2];
            if !self.dma_read(used_ring_idx_addr, &mut used_idx_buf) { break; }
            let used_idx = u16::from_le_bytes(used_idx_buf);

            let used_elem_offset = 4 + (used_idx % qsize) as u64 * 8;
            let mut used_elem = [0u8; 8];
            used_elem[0..4].copy_from_slice(&(head_idx as u32).to_le_bytes());
            used_elem[4..8].copy_from_slice(&8u32.to_le_bytes()); // written length
            self.dma_write(used_addr + used_elem_offset, &used_elem);

            let new_used_idx = used_idx.wrapping_add(1);
            self.dma_write(used_ring_idx_addr, &new_used_idx.to_le_bytes());

            last_avail = last_avail.wrapping_add(1);
            delivered += 1;
        }

        self.queues[0].last_avail_idx = last_avail;

        #[cfg(feature = "std")]
        if self.device_type == InputDeviceType::Keyboard {
            static mut KBD_RESULT_LOG: u32 = 0;
            unsafe { KBD_RESULT_LOG += 1; if KBD_RESULT_LOG <= 30 {
                eprintln!("[virtio-input:kbd] delivered={} remaining={}", delivered, self.event_queue.len());
            }}
        }

        if delivered > 0 {
            self.virtio_notify();
        }
    }

    fn dma_read(&self, gpa: u64, buf: &mut [u8]) -> bool {
        if self.guest_mem_ptr.is_null() { return false; }
        let offset = if gpa < PCI_HOLE_START { gpa as usize }
            else if gpa >= PCI_HOLE_END { (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize }
            else { return false; };
        if offset + buf.len() > self.guest_mem_len { return false; }
        unsafe { core::ptr::copy_nonoverlapping(self.guest_mem_ptr.add(offset), buf.as_mut_ptr(), buf.len()); }
        true
    }

    fn dma_write(&self, gpa: u64, data: &[u8]) -> bool {
        if self.guest_mem_ptr.is_null() { return false; }
        let offset = if gpa < PCI_HOLE_START { gpa as usize }
            else if gpa >= PCI_HOLE_END { (PCI_HOLE_START + (gpa - PCI_HOLE_END)) as usize }
            else { return false; };
        if offset + data.len() > self.guest_mem_len { return false; }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.guest_mem_ptr.add(offset), data.len()); }
        true
    }

    // ── Common config ──

    fn read_common_cfg(&self, offset: u64, size: u8) -> u64 {
        let val = match offset {
            0x00 => self.device_feature_sel as u64,
            0x04 => {
                if self.device_feature_sel == 0 { (self.device_features & 0xFFFF_FFFF) as u64 }
                else { ((self.device_features >> 32) & 0xFFFF_FFFF) as u64 }
            }
            0x08 => self.driver_feature_sel as u64,
            0x0C => {
                if self.driver_feature_sel == 0 { (self.driver_features & 0xFFFF_FFFF) as u64 }
                else { ((self.driver_features >> 32) & 0xFFFF_FFFF) as u64 }
            }
            0x10 => 0xFFFF,
            0x12 => NUM_QUEUES as u64,
            0x14 => self.status as u64,
            0x15 => self.config_generation as u64,
            0x16 => self.queue_select as u64,
            0x18 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].size as u64 } else { 0 } }
            0x1A => 0xFFFF,
            0x1C => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].ready as u64 } else { 0 } }
            0x1E => self.queue_select as u64,
            0x20 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].desc_addr & 0xFFFF_FFFF } else { 0 } }
            0x24 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].desc_addr >> 32 } else { 0 } }
            0x28 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].avail_addr & 0xFFFF_FFFF } else { 0 } }
            0x2C => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].avail_addr >> 32 } else { 0 } }
            0x30 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].used_addr & 0xFFFF_FFFF } else { 0 } }
            0x34 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].used_addr >> 32 } else { 0 } }
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
                if self.driver_feature_sel == 0 { self.driver_features = (self.driver_features & !0xFFFF_FFFF) | (val & 0xFFFF_FFFF); }
                else { self.driver_features = (self.driver_features & 0xFFFF_FFFF) | ((val & 0xFFFF_FFFF) << 32); }
            }
            0x14 => {
                if val as u8 == 0 {
                    self.status = 0; self.driver_features = 0;
                    for q in &mut self.queues { q.ready = 0; q.last_avail_idx = 0; }
                    self.isr_status = 0; self.event_queue.clear();
                    return;
                }
                self.status = val as u8;
                #[cfg(feature = "std")]
                {
                    let name = match self.device_type { InputDeviceType::Keyboard => "kbd", InputDeviceType::Tablet => "tablet" };
                    eprintln!("[virtio-input:{}] status = 0x{:02X}{}", name, self.status,
                        if self.status & VIRTIO_STATUS_DRIVER_OK != 0 { " (DRIVER_OK)" } else { "" });
                }
            }
            0x16 => self.queue_select = val as u16,
            0x18 => { let qi = self.queue_select as usize; if qi < NUM_QUEUES { self.queues[qi].size = (val as u16).min(VIRTQUEUE_MAX_SIZE); } }
            0x1C => {
                let qi = self.queue_select as usize;
                if qi < NUM_QUEUES {
                    self.queues[qi].ready = val as u16;
                    #[cfg(feature = "std")]
                    if val as u16 != 0 {
                        let name = match self.device_type { InputDeviceType::Keyboard => "kbd", InputDeviceType::Tablet => "tablet" };
                        eprintln!("[virtio-input:{}] queue {} enabled (desc=0x{:X} avail=0x{:X} used=0x{:X} size={})",
                            name, qi, self.queues[qi].desc_addr, self.queues[qi].avail_addr, self.queues[qi].used_addr, self.queues[qi].size);
                    }
                }
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

    /// Read device-specific config (virtio_input_config).
    /// Layout: select(u8@0) + subsel(u8@1) + size(u8@2) + reserved[5]@3 + data[128]@8
    fn read_device_cfg(&self, offset: u64, _size: u8) -> u64 {
        match offset {
            0 => self.cfg_select as u64,
            1 => self.cfg_subsel as u64,
            2 => {
                // Return size of config data for current select/subsel.
                self.config_data_size() as u64
            }
            3..=7 => 0, // reserved
            8..=135 => {
                // Config data
                let data = self.config_data();
                let idx = (offset - 8) as usize;
                if idx < data.len() { data[idx] as u64 } else { 0 }
            }
            _ => 0,
        }
    }

    /// Write device-specific config (select + subsel).
    fn write_device_cfg(&mut self, offset: u64, _size: u8, val: u64) {
        match offset {
            0 => {
                self.cfg_select = val as u8;
                #[cfg(feature = "std")]
                {
                    static mut CFG_LOG: u32 = 0;
                    unsafe { CFG_LOG += 1; if CFG_LOG <= 30 {
                        let name = match self.device_type { InputDeviceType::Keyboard => "kbd", InputDeviceType::Tablet => "tablet" };
                        let data = self.config_data();
                        eprintln!("[virtio-input:{}] cfg_select=0x{:02X} subsel=0x{:02X} → size={} data={:?}",
                            name, self.cfg_select, self.cfg_subsel, data.len(), &data[..data.len().min(16)]);
                    }}
                }
            }
            1 => self.cfg_subsel = val as u8,
            _ => {}
        }
    }

    fn config_data_size(&self) -> u8 {
        self.config_data().len() as u8
    }

    /// Generate config data based on current select/subsel.
    fn config_data(&self) -> Vec<u8> {
        match self.cfg_select {
            VIRTIO_INPUT_CFG_ID_NAME => {
                let name = match self.device_type {
                    InputDeviceType::Keyboard => b"CoreVM VirtIO Keyboard".to_vec(),
                    InputDeviceType::Tablet => b"CoreVM VirtIO Tablet".to_vec(),
                };
                name
            }
            VIRTIO_INPUT_CFG_ID_SERIAL => {
                b"corevm-input-0".to_vec()
            }
            VIRTIO_INPUT_CFG_ID_DEVIDS => {
                // struct virtio_input_devids: bustype(u16) + vendor(u16) + product(u16) + version(u16)
                let mut data = vec![0u8; 8];
                // BUS_VIRTUAL = 0x06
                data[0] = 0x06; data[1] = 0x00;
                // vendor = 0x1AF4 (Red Hat)
                data[2] = 0xF4; data[3] = 0x1A;
                // product
                match self.device_type {
                    InputDeviceType::Keyboard => { data[4] = 0x01; data[5] = 0x00; }
                    InputDeviceType::Tablet => { data[4] = 0x02; data[5] = 0x00; }
                }
                // version
                data[6] = 0x01; data[7] = 0x00;
                data
            }
            VIRTIO_INPUT_CFG_EV_BITS => {
                // subsel=0 (EV_SYN) → bitmap of supported event types.
                // subsel=N (N>0) → bitmap of supported codes for event type N.
                if self.cfg_subsel == 0 {
                    // Return bitmap of supported EV_* types.
                    // Bit N = device supports event type N.
                    let mut bitmap = vec![0u8; 4]; // up to 32 event types
                    match self.device_type {
                        InputDeviceType::Keyboard => {
                            // EV_SYN(0), EV_KEY(1), EV_LED(0x11), EV_REP(0x14)
                            bitmap[(EV_SYN / 8) as usize] |= 1 << (EV_SYN % 8);
                            bitmap[(EV_KEY / 8) as usize] |= 1 << (EV_KEY % 8);
                            bitmap[(EV_REP / 8) as usize] |= 1 << (EV_REP % 8);
                            bitmap[(EV_LED / 8) as usize] |= 1 << (EV_LED % 8);
                        }
                        InputDeviceType::Tablet => {
                            // EV_SYN(0), EV_KEY(1), EV_ABS(3)
                            bitmap[(EV_SYN / 8) as usize] |= 1 << (EV_SYN % 8);
                            bitmap[(EV_KEY / 8) as usize] |= 1 << (EV_KEY % 8);
                            bitmap[(EV_ABS / 8) as usize] |= 1 << (EV_ABS % 8);
                        }
                    }
                    bitmap
                } else {
                    match self.device_type {
                        InputDeviceType::Keyboard => {
                            match self.cfg_subsel as u16 {
                                EV_KEY => {
                                    // Bitmap of supported KEY_* codes.
                                    // Need at least 96 bytes to cover KEY_* up to 0x2FF.
                                    let mut bitmap = vec![0u8; 96]; // 768 bits
                                    // Standard keyboard keys (1-127 = main keyboard area).
                                    for i in 1u16..128 {
                                        bitmap[(i / 8) as usize] |= 1 << (i % 8);
                                    }
                                    bitmap
                                }
                                EV_REP => vec![1],
                                EV_LED => {
                                    let mut bitmap = vec![0u8; 2];
                                    bitmap[0] = 0x07; // NUM_LOCK(0), CAPS_LOCK(1), SCROLL_LOCK(2)
                                    bitmap
                                }
                                _ => Vec::new(),
                            }
                        }
                        InputDeviceType::Tablet => {
                            match self.cfg_subsel as u16 {
                                EV_KEY => {
                                    // Support BTN_LEFT (0x110), BTN_RIGHT (0x111), BTN_MIDDLE (0x112),
                                    // BTN_TOUCH (0x14A), BTN_TOOL_FINGER (0x145)
                                    let max_btn = 0x150u16;
                                    let bytes = ((max_btn as usize) + 7) / 8;
                                    let mut bitmap = vec![0u8; bytes];
                                    for btn in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_TOUCH, BTN_TOOL_FINGER] {
                                        bitmap[(btn / 8) as usize] |= 1 << (btn % 8);
                                    }
                                    bitmap
                                }
                                EV_ABS => {
                                    // Support ABS_X (0) and ABS_Y (1)
                                    vec![0x03] // bits 0 and 1
                                }
                                _ => Vec::new(),
                            }
                        }
                    }
                }
            }
            VIRTIO_INPUT_CFG_ABS_INFO => {
                if self.device_type == InputDeviceType::Tablet {
                    match self.cfg_subsel {
                        0 | 1 => {
                            // struct virtio_input_absinfo: min(u32) + max(u32) + fuzz(u32) + flat(u32) + res(u32)
                            let mut data = vec![0u8; 20];
                            // min = 0
                            data[0..4].copy_from_slice(&0u32.to_le_bytes());
                            // max = 32767
                            data[4..8].copy_from_slice(&32767u32.to_le_bytes());
                            // fuzz = 0, flat = 0, res = 0
                            data
                        }
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                }
            }
            VIRTIO_INPUT_CFG_PROP_BITS => {
                if self.device_type == InputDeviceType::Tablet {
                    // INPUT_PROP_DIRECT = bit 0
                    vec![0x01]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }
}

impl MmioHandler for VirtioInput {
    fn read(&mut self, offset: u64, size: u8) -> Result<u64> {
        if offset >= COMMON_CFG_OFFSET && offset < COMMON_CFG_OFFSET + COMMON_CFG_SIZE {
            return Ok(self.read_common_cfg(offset - COMMON_CFG_OFFSET, size));
        }
        if offset >= ISR_OFFSET && offset < ISR_OFFSET + 4 {
            let val = self.isr_status as u64;
            self.isr_status = 0;
            return Ok(val);
        }
        if offset >= DEVICE_CFG_OFFSET && offset < DEVICE_CFG_OFFSET + DEVICE_CFG_SIZE {
            return Ok(self.read_device_cfg(offset - DEVICE_CFG_OFFSET, size));
        }
        if offset >= NOTIFY_OFFSET && offset < NOTIFY_OFFSET + 4 {
            return Ok(0);
        }
        Ok(0xFFFF_FFFF)
    }

    fn write(&mut self, offset: u64, size: u8, val: u64) -> Result<()> {
        if offset >= COMMON_CFG_OFFSET && offset < COMMON_CFG_OFFSET + COMMON_CFG_SIZE {
            self.write_common_cfg(offset - COMMON_CFG_OFFSET, size, val);
            return Ok(());
        }
        if offset >= NOTIFY_OFFSET && offset < NOTIFY_OFFSET + 4 {
            // Notification doorbell — process event delivery.
            self.process_eventq();
            return Ok(());
        }
        if offset >= ISR_OFFSET && offset < ISR_OFFSET + 4 {
            self.isr_status &= !(val as u32);
            return Ok(());
        }
        if offset >= DEVICE_CFG_OFFSET && offset < DEVICE_CFG_OFFSET + DEVICE_CFG_SIZE {
            self.write_device_cfg(offset - DEVICE_CFG_OFFSET, size, val);
            return Ok(());
        }
        Ok(())
    }
}

// ── PS/2 scancode to Linux KEY_* code conversion table ──

/// Convert a PS/2 Set 1 scancode to a Linux KEY_* code.
/// Returns None for unknown/extended scancodes.
pub fn ps2_to_linux_key(scancode: u8) -> Option<u16> {
    // Standard AT keyboard Set 1 → Linux input.h KEY_* codes
    static MAP: [u16; 128] = [
        0,      // 0x00: (none)
        1,      // 0x01: KEY_ESC
        2,      // 0x02: KEY_1
        3,      // 0x03: KEY_2
        4,      // 0x04: KEY_3
        5,      // 0x05: KEY_4
        6,      // 0x06: KEY_5
        7,      // 0x07: KEY_6
        8,      // 0x08: KEY_7
        9,      // 0x09: KEY_8
        10,     // 0x0A: KEY_9
        11,     // 0x0B: KEY_0
        12,     // 0x0C: KEY_MINUS
        13,     // 0x0D: KEY_EQUAL
        14,     // 0x0E: KEY_BACKSPACE
        15,     // 0x0F: KEY_TAB
        16,     // 0x10: KEY_Q
        17,     // 0x11: KEY_W
        18,     // 0x12: KEY_E
        19,     // 0x13: KEY_R
        20,     // 0x14: KEY_T
        21,     // 0x15: KEY_Y
        22,     // 0x16: KEY_U
        23,     // 0x17: KEY_I
        24,     // 0x18: KEY_O
        25,     // 0x19: KEY_P
        26,     // 0x1A: KEY_LEFTBRACE
        27,     // 0x1B: KEY_RIGHTBRACE
        28,     // 0x1C: KEY_ENTER
        29,     // 0x1D: KEY_LEFTCTRL
        30,     // 0x1E: KEY_A
        31,     // 0x1F: KEY_S
        32,     // 0x20: KEY_D
        33,     // 0x21: KEY_F
        34,     // 0x22: KEY_G
        35,     // 0x23: KEY_H
        36,     // 0x24: KEY_J
        37,     // 0x25: KEY_K
        38,     // 0x26: KEY_L
        39,     // 0x27: KEY_SEMICOLON
        40,     // 0x28: KEY_APOSTROPHE
        41,     // 0x29: KEY_GRAVE
        42,     // 0x2A: KEY_LEFTSHIFT
        43,     // 0x2B: KEY_BACKSLASH
        44,     // 0x2C: KEY_Z
        45,     // 0x2D: KEY_X
        46,     // 0x2E: KEY_C
        47,     // 0x2F: KEY_V
        48,     // 0x30: KEY_B
        49,     // 0x31: KEY_N
        50,     // 0x32: KEY_M
        51,     // 0x33: KEY_COMMA
        52,     // 0x34: KEY_DOT
        53,     // 0x35: KEY_SLASH
        54,     // 0x36: KEY_RIGHTSHIFT
        55,     // 0x37: KEY_KPASTERISK
        56,     // 0x38: KEY_LEFTALT
        57,     // 0x39: KEY_SPACE
        58,     // 0x3A: KEY_CAPSLOCK
        59,     // 0x3B: KEY_F1
        60,     // 0x3C: KEY_F2
        61,     // 0x3D: KEY_F3
        62,     // 0x3E: KEY_F4
        63,     // 0x3F: KEY_F5
        64,     // 0x40: KEY_F6
        65,     // 0x41: KEY_F7
        66,     // 0x42: KEY_F8
        67,     // 0x43: KEY_F9
        68,     // 0x44: KEY_F10
        69,     // 0x45: KEY_NUMLOCK
        70,     // 0x46: KEY_SCROLLLOCK
        71,     // 0x47: KEY_KP7
        72,     // 0x48: KEY_KP8
        73,     // 0x49: KEY_KP9
        74,     // 0x4A: KEY_KPMINUS
        75,     // 0x4B: KEY_KP4
        76,     // 0x4C: KEY_KP5
        77,     // 0x4D: KEY_KP6
        78,     // 0x4E: KEY_KPPLUS
        79,     // 0x4F: KEY_KP1
        80,     // 0x50: KEY_KP2
        81,     // 0x51: KEY_KP3
        82,     // 0x52: KEY_KP0
        83,     // 0x53: KEY_KPDOT
        0, 0, 0, // 0x54-0x56: (unused)
        87,     // 0x57: KEY_F11
        88,     // 0x58: KEY_F12
        0, 0, 0, 0, 0, 0, 0, // 0x59-0x5F
        0, 0, 0, 0, 0, 0, 0, 0, // 0x60-0x67
        0, 0, 0, 0, 0, 0, 0, 0, // 0x68-0x6F
        0, 0, 0, 0, 0, 0, 0, 0, // 0x70-0x77
        0, 0, 0, 0, 0, 0, 0, 0, // 0x78-0x7F
    ];

    let idx = (scancode & 0x7F) as usize;
    let key = MAP[idx];
    if key != 0 { Some(key) } else { None }
}
