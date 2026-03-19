use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, Instant};

use crate::app::{VmEntry, FrameBufferData};
use crate::config::{BiosType, MacMode, NetMode};
use crate::engine::diagnostics::{DiagLog, DiagCategory};
use crate::ui::components::display;
use crate::engine::platform;
use crate::ui::components::sidebar::VmState;

use libcorevm::ffi::{
    corevm_create, corevm_create_vcpu, corevm_destroy,
    corevm_setup_standard_devices, corevm_setup_acpi_tables, corevm_setup_acpi_tables_with_hpet, corevm_setup_ahci, corevm_setup_e1000, corevm_setup_hpet, corevm_setup_ac97,
    corevm_setup_uhci,
    corevm_setup_virtio_gpu, corevm_virtio_gpu_get_framebuffer, corevm_virtio_gpu_get_mode, corevm_has_virtio_gpu, corevm_virtio_gpu_scanout_active,
    corevm_setup_virtio_input,
    corevm_get_vcpu_regs,
    corevm_get_vcpu_sregs,
    corevm_vga_get_framebuffer, corevm_vga_get_text_buffer, corevm_vga_get_mode, corevm_vga_get_fb_offset,
    corevm_cancel_vcpu, corevm_set_cpu_count,
    corevm_ahci_flush_caches,
    corevm_setup_net,
    corevm_setup_virtio_net,
    corevm_read_phys,
};
use libcorevm::setup;
use libcorevm::backend::{VcpuRegs, VcpuSregs};
use libcorevm::runtime::{VmRuntime, VmRuntimeConfig, VmEvent, EventHandler};

/// Callback for WHP debug output — routes messages to DiagLog's WHP tab.
#[cfg(target_os = "windows")]
extern "C" fn whp_debug_callback(ctx: *mut std::ffi::c_void, msg: *const u8, len: u32) {
    if ctx.is_null() || msg.is_null() { return; }
    let diag = unsafe { &*(ctx as *const DiagLog) };
    let text = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(msg, len as usize)) };
    diag.append_whp_text(text);
}

/// Retrieve the last error message from libcorevm.
pub fn get_last_error_public() -> Option<String> {
    setup::get_last_error()
}

/// Shared control flags for the VM thread
pub struct VmControl {
    pub stop: AtomicBool,
    pub pause: AtomicBool,
    pub exited: AtomicBool,
    pub exit_reason: Mutex<String>,
    /// Set when the guest requests a reboot (soft reset).
    pub reboot_requested: AtomicBool,
}

/// EventHandler for vmmanager — routes VM events to DiagLog, FrameBufferData,
/// and VmControl for the GUI to consume.
struct VmManagerEventHandler {
    fb: Arc<Mutex<FrameBufferData>>,
    diag: DiagLog,
    control: Arc<VmControl>,
    handle: u64,
    last_fb_update: Instant,
    fb_debug_count: u32,
}

// Safety: all fields are Send-safe (Arc, DiagLog uses Arc internally).
unsafe impl Send for VmManagerEventHandler {}

impl EventHandler for VmManagerEventHandler {
    fn on_event(&mut self, event: VmEvent) {
        match event {
            VmEvent::SerialOutput(data) => {
                for &ch in &data {
                    if ch >= 0x20 || ch == b'\n' || ch == b'\r' || ch == b'\t' {
                        eprint!("{}", ch as char);
                    }
                }
            }
            VmEvent::DebugOutput(data) => {
                if let Ok(s) = std::str::from_utf8(&data) {
                    self.diag.append_debug_text(s);
                }
            }
            VmEvent::Shutdown => {
                self.diag.log(DiagCategory::Info, "VM shutdown".into());
                if let Ok(mut r) = self.control.exit_reason.lock() {
                    *r = "Shutdown".into();
                }
                self.control.exited.store(true, Ordering::Relaxed);
                // Final framebuffer update
                update_framebuffer(self.handle, &self.fb, &self.diag, &mut self.fb_debug_count);
            }
            VmEvent::Error { message } => {
                self.diag.log(DiagCategory::Error, format!("VM error: {}", message));
                if let Ok(mut r) = self.control.exit_reason.lock() {
                    *r = format!("Error: {}", message);
                }
                self.control.exited.store(true, Ordering::Relaxed);
                update_framebuffer(self.handle, &self.fb, &self.diag, &mut self.fb_debug_count);
            }
            VmEvent::RebootRequested => {
                self.diag.log(DiagCategory::Info, "System reset requested by guest".into());
                self.control.reboot_requested.store(true, Ordering::Relaxed);
                self.control.exited.store(true, Ordering::Relaxed);
            }
            VmEvent::Diagnostic(msg) => {
                self.diag.log(DiagCategory::Info, msg);
            }
        }
    }

    fn on_tick(&mut self, handle: u64) {
        // Framebuffer update at ~60fps
        if self.last_fb_update.elapsed() >= Duration::from_millis(16) {
            update_framebuffer(handle, &self.fb, &self.diag, &mut self.fb_debug_count);
            self.last_fb_update = Instant::now();
        }
    }
}

/// Start a VM. Sets up libcorevm, spawns execution thread.
pub fn start_vm(entry: &mut VmEntry) -> Result<(), String> {
    let config = &entry.config;

    // Install SIGUSR1 handler so the cancel thread can interrupt KVM_RUN
    // without terminating the process (SIGUSR1 default action = terminate).
    #[cfg(target_os = "linux")]
    libcorevm::backend::kvm::install_sigusr1_handler();

    // Reset diagnostics log
    entry.diag_log.clear();
    if entry.config.diagnostics {
        entry.diag_log.log(DiagCategory::Info, format!("Starting VM '{}' RAM={}MB BIOS={:?}", config.name, config.ram_mb, config.bios_type));
    }

    // Create VM
    let handle = corevm_create(config.ram_mb);
    if handle == 0 {
        let msg = setup::get_last_error().unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to create VM: {}", msg));
    }

    // Set CPU count BEFORE creating vCPUs (needed for CPUID topology)
    let num_cpus = config.cpu_cores.max(1).min(32);
    corevm_set_cpu_count(handle, num_cpus);

    // Create vCPUs — libcorevm sets CPUID per-vCPU and APs to INIT_RECEIVED
    for cpu_id in 0..num_cpus {
        if corevm_create_vcpu(handle, cpu_id) != 0 {
            corevm_destroy(handle);
            return Err(format!("Failed to create vCPU {}", cpu_id));
        }
    }
    setup::set_vram_mb(handle, config.vram_mb);

    // Setup devices (includes PCI bus)
    corevm_setup_standard_devices(handle);

    // HPET — required for Windows guests
    if config.guest_os.is_windows() {
        corevm_setup_hpet(handle);
        entry.diag_log.log(DiagCategory::Info, "HPET enabled (Windows guest)".into());
    }

    // VGA LFB is already mapped by setup_standard_devices → setup_vga_lfb_mapping
    // at slot 2 (0xE0000000, 8MB) pointing to the SVGA device's internal framebuffer.
    // No additional mapping needed here.

    // Setup AHCI controller (replaces IDE)
    corevm_setup_ahci(handle, 6);

    // Network adapter — only if enabled in config
    if config.net_enabled {
        let mac = setup::resolve_mac(
            &config.uuid,
            config.mac_mode == MacMode::Static,
            &config.mac_address,
        );

        match config.nic_model {
            crate::config::NicModel::VirtioNet => {
                corevm_setup_virtio_net(handle, mac.as_ptr());
                entry.diag_log.log(DiagCategory::Info, format!(
                    "VirtIO-Net NIC enabled (mac={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                ));
                // VirtIO-Net doesn't need a PXE ROM — SeaBIOS doesn't support
                // VirtIO PXE boot directly (iPXE ROM needed separately if desired).
            }
            crate::config::NicModel::E1000 => {
                corevm_setup_e1000(handle, mac.as_ptr());
                entry.diag_log.log(DiagCategory::Info, format!(
                    "E1000 NIC enabled (mac={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                ));
                // Load E1000 PXE ROM — SeaBIOS needs this to initialize the NIC
                let extra_bios_paths = platform::bios_search_paths();
                match setup::load_e1000_rom(handle, &extra_bios_paths) {
                    Ok(()) => entry.diag_log.log(DiagCategory::Info, "E1000 PXE ROM loaded".into()),
                    Err(e) => entry.diag_log.log(DiagCategory::Info, format!("E1000 ROM warning: {}", e)),
                }
            }
        }

        // Set up network backend — shared between E1000 and VirtIO-Net
        let net_mode_id = match config.net_mode {
            NetMode::Disconnected => 0,
            NetMode::UserMode => 1,
            NetMode::Bridge => 2,
        };
        corevm_setup_net(handle, net_mode_id);
        entry.diag_log.log(DiagCategory::Info, format!("Network backend: {:?}", config.net_mode));
    }

    // UHCI USB Controller with tablet — if enabled in config
    if config.usb_tablet {
        corevm_setup_uhci(handle);
        entry.diag_log.log(DiagCategory::Info, "UHCI USB controller + tablet enabled".to_string());
    }

    // AC97 Audio Controller — only if enabled in config
    if config.audio_enabled {
        corevm_setup_ac97(handle);
        entry.diag_log.log(DiagCategory::Info, "AC97 audio controller enabled".to_string());
    }

    // VirtIO GPU — if selected in config
    if config.gpu_model == crate::config::GpuModel::VirtioGpu {
        let gpu_vram = config.vram_mb.max(64); // VirtIO GPU needs at least 64MB VRAM
        corevm_setup_virtio_gpu(handle, gpu_vram);
        entry.diag_log.log(DiagCategory::Info, format!("VirtIO GPU enabled (VRAM={}MB)", gpu_vram));

        // VirtIO Input devices — always enabled alongside VirtIO GPU.
        // The guest expects virtio-input when using virtio-gpu for input routing.
        corevm_setup_virtio_input(handle);
        entry.diag_log.log(DiagCategory::Info, "VirtIO Input (keyboard + tablet) enabled".into());
    }

    // ACPI tables — MUST be generated AFTER all PCI devices are set up,
    // because libcorevm auto-detects which devices are present for _PRT generation.
    let acpi_rc = if config.guest_os.is_windows() {
        corevm_setup_acpi_tables_with_hpet(handle)
    } else {
        corevm_setup_acpi_tables(handle)
    };
    entry.diag_log.log(DiagCategory::Info, format!("ACPI tables setup: rc={} (hpet={})", acpi_rc, config.guest_os.is_windows()));

    // Load BIOS
    let extra_bios_paths = platform::bios_search_paths();
    match config.bios_type {
        BiosType::SeaBios => setup::load_seabios(handle, &extra_bios_paths)?,
        BiosType::CoreVm => setup::load_corevm_bios(handle, &extra_bios_paths)?,
    }

    // Attach ISO — always on AHCI (port 1) for SeaBIOS boot.
    // For Windows guests, also attach on IDE so Windows Setup can use its
    // built-in ATAPI driver (Windows has no AHCI driver during Setup).
    if !config.iso_image.is_empty() {
        setup::attach_image_to_ahci(handle, &config.iso_image, 1, true)?;
        if config.guest_os.is_windows() {
            setup::attach_cdrom_to_ide(handle, &config.iso_image)?;
            entry.diag_log.log(DiagCategory::Info, "ISO attached via AHCI + IDE (Windows compat)".into());
        } else {
            entry.diag_log.log(DiagCategory::Info, "ISO attached via AHCI".into());
        }
    }

    // Attach disk images (port 0, 2, 3, 4, 5 — port 1 is reserved for CDROM)
    let disk_ports = [0u32, 2, 3, 4, 5];
    for (i, disk_path) in config.disk_images.iter().enumerate() {
        if !disk_path.is_empty() {
            if let Some(&port) = disk_ports.get(i) {
                setup::attach_image_to_ahci(handle, disk_path, port, false)?;
                // Configure disk cache
                let cache_mode = match config.disk_cache_mode {
                    crate::config::DiskCacheMode::WriteBack => 0u32,
                    crate::config::DiskCacheMode::WriteThrough => 1,
                    crate::config::DiskCacheMode::None => 2,
                };
                setup::configure_disk_cache(handle, port, config.disk_cache_mb, cache_mode);
            }
        }
    }

    // Set initial CPU state: CS:IP = F000:FFF0 (real-mode reset vector)
    let sregs_rc = setup::set_initial_cpu_state(handle);
    if entry.config.diagnostics {
        if let Err(ref e) = sregs_rc {
            entry.diag_log.log(DiagCategory::Error, format!("set_initial_cpu_state failed: {}", e));
        }
        // Dump actual VP state after setup
        let mut sregs = VcpuSregs::default();
        let mut regs = VcpuRegs::default();
        corevm_get_vcpu_sregs(handle, 0, &mut sregs);
        corevm_get_vcpu_regs(handle, 0, &mut regs);
        entry.diag_log.log(DiagCategory::CpuState, format!(
            "VP state: CS={:04X}:{:016X}(lim={:X} attr={:02X}/{}/{}) RIP={:X} RFLAGS={:X} CR0={:X}",
            sregs.cs.selector, sregs.cs.base, sregs.cs.limit,
            sregs.cs.type_, sregs.cs.s, sregs.cs.present,
            regs.rip, regs.rflags, sregs.cr0
        ));
        entry.diag_log.log(DiagCategory::CpuState, format!(
            "SS={:04X}:{:016X} DS={:04X}:{:016X} TR={:04X}:{:016X}(type={:02X} s={} p={})",
            sregs.ss.selector, sregs.ss.base,
            sregs.ds.selector, sregs.ds.base,
            sregs.tr.selector, sregs.tr.base,
            sregs.tr.type_, sregs.tr.s, sregs.tr.present
        ));
        entry.diag_log.log(DiagCategory::CpuState, format!(
            "GDT base={:X} lim={:X}  IDT base={:X} lim={:X}  CR4={:X} EFER={:X}",
            sregs.gdt.base, sregs.gdt.limit,
            sregs.idt.base, sregs.idt.limit,
            sregs.cr4, sregs.efer
        ));
    }

    // Write COM1 base address into BDA so SeaBIOS finds the serial port.
    setup::setup_bda_com1(handle);

    // Setup shared state
    let control = Arc::new(VmControl {
        stop: AtomicBool::new(false),
        pause: AtomicBool::new(false),
        exited: AtomicBool::new(false),
        exit_reason: Mutex::new(String::new()),
        reboot_requested: AtomicBool::new(false),
    });

    // Register WHP debug callback to route output to the diagnostics UI
    #[cfg(target_os = "windows")]
    {
        let diag_for_whp = Box::new(entry.diag_log.clone());
        let ctx = Box::into_raw(diag_for_whp) as *mut std::ffi::c_void;
        libcorevm::ffi::corevm_set_whp_debug_callback(Some(whp_debug_callback), ctx);
    }

    // Install SIGUSR1 handler so cancel_vcpu can interrupt KVM_RUN (Linux/KVM only)
    #[cfg(target_os = "linux")]
    libcorevm::backend::kvm::install_sigusr1_handler();

    let fb = entry.framebuffer.clone();
    let control_clone = control.clone();
    let diag = entry.diag_log.clone();
    let diag_enabled = entry.config.diagnostics;
    let num_cpus = entry.config.cpu_cores.max(1).min(32);

    let has_virtio_gpu = entry.config.gpu_model == crate::config::GpuModel::VirtioGpu;
    let has_virtio_input = has_virtio_gpu; // VirtIO Input is enabled alongside VirtIO GPU

    let rt_config = VmRuntimeConfig {
        handle,
        num_cpus,
        usb_tablet: entry.config.usb_tablet,
        audio_enabled: entry.config.audio_enabled,
        net_enabled: entry.config.net_enabled,
        virtio_gpu: has_virtio_gpu,
        virtio_input: has_virtio_input,
        diagnostics: diag_enabled,
        ..Default::default()
    };

    // EventHandler that routes events to DiagLog + FrameBufferData
    let handler = VmManagerEventHandler {
        fb: fb.clone(),
        diag: diag.clone(),
        control: control_clone.clone(),
        handle,
        last_fb_update: Instant::now(),
        fb_debug_count: 0,
    };

    // Spawn VM execution thread using VmRuntime
    let thread = thread::spawn(move || {
        let mut runtime = VmRuntime::new(rt_config, handler);
        runtime.start();
        runtime.wait();
        corevm_ahci_flush_caches(handle);
        corevm_destroy(handle);
    });

    entry.vm_handle = Some(handle);
    entry.control = Some(control);
    entry.vm_thread = Some(thread);
    entry.state = VmState::Running;

    Ok(())
}

/// Stop a running VM
pub fn stop_vm(entry: &mut VmEntry) {
    if let Some(ref control) = entry.control {
        control.stop.store(true, Ordering::Relaxed);
    }
    if let Some(thread) = entry.vm_thread.take() {
        let _ = thread.join();
    }
    entry.vm_handle = None;
    entry.control = None;
    entry.state = VmState::Stopped;
}

/// Pause a running VM
pub fn pause_vm(entry: &mut VmEntry) {
    if let Some(ref control) = entry.control {
        control.pause.store(true, Ordering::Relaxed);
    }
    entry.state = VmState::Paused;
}

/// Resume a paused VM
pub fn resume_vm(entry: &mut VmEntry) {
    if let Some(ref control) = entry.control {
        control.pause.store(false, Ordering::Relaxed);
    }
    entry.state = VmState::Running;
}

// The VM execution loop has been moved to `libcorevm::runtime::loop_core`.
// VmRuntime handles vCPU threads, timer advancement, device polling, and I/O dispatch.
// ~540 lines of duplicated VM loop code have been replaced by VmRuntime.

/// Read VGA state and update the shared framebuffer
fn update_framebuffer(handle: u64, fb: &Arc<Mutex<FrameBufferData>>, diag: &DiagLog, fb_debug_count: &mut u32) {
    // If VirtIO GPU scanout is active (guest driver loaded and configured),
    // read its framebuffer instead of VGA. Falls back to VGA during BIOS boot
    // and until the guest driver issues SET_SCANOUT with a valid resource.
    if corevm_virtio_gpu_scanout_active(handle) != 0 {
        let mut gpu_w: u32 = 0;
        let mut gpu_h: u32 = 0;
        let mut gpu_bpp: u8 = 0;
        if corevm_virtio_gpu_get_mode(handle, &mut gpu_w, &mut gpu_h, &mut gpu_bpp) == 0
            && gpu_w > 0 && gpu_h > 0
        {
            let mut fb_ptr: *const u8 = std::ptr::null();
            let mut fb_len: u32 = 0;
            if corevm_virtio_gpu_get_framebuffer(handle, &mut fb_ptr, &mut fb_len) == 0
                && !fb_ptr.is_null() && fb_len > 0
            {
                let fb_size = (gpu_w as usize) * (gpu_h as usize) * 4; // BGRA32
                if (fb_len as usize) >= fb_size {
                    let raw = unsafe { std::slice::from_raw_parts(fb_ptr, fb_size) };
                    if let Ok(mut fb_data) = fb.lock() {
                        fb_data.text_mode = false;
                        fb_data.width = gpu_w;
                        fb_data.height = gpu_h;
                        // VirtIO GPU framebuffer is already RGBA32 (blit converts).
                        // Just memcpy — no per-pixel conversion needed.
                        fb_data.pixels.resize(fb_size, 0);
                        fb_data.pixels.copy_from_slice(raw);
                        fb_data.dirty = true;
                    }
                    return;
                }
            }
        }
    }

    // Query VGA mode to get exact dimensions
    let mut vga_w: u32 = 0;
    let mut vga_h: u32 = 0;
    let mut vga_bpp: u8 = 0;
    let mode_ret = corevm_vga_get_mode(handle, &mut vga_w, &mut vga_h, &mut vga_bpp);

    if *fb_debug_count < 5 {
        *fb_debug_count += 1;
        diag.log(DiagCategory::Info, format!(
            "update_fb #{}: mode_ret={} {}x{}x{}",
            fb_debug_count, mode_ret, vga_w, vga_h, vga_bpp
        ));
    }

    if mode_ret == 1 {
        // Text mode — read text buffer
        let mut text_ptr: *const u16 = std::ptr::null();
        let mut text_len: u32 = 0;
        let ret = corevm_vga_get_text_buffer(handle, &mut text_ptr, &mut text_len);
        if ret == 0 && !text_ptr.is_null() && text_len > 0 {
            let text_cells = unsafe { std::slice::from_raw_parts(text_ptr, text_len as usize) };
            if let Ok(mut fb_data) = fb.lock() {
                fb_data.text_mode = true;
                fb_data.text_buffer = text_cells.to_vec();
                let buf = fb_data.text_buffer.clone();
                let (tw, th) = display::render_text_mode(&buf, &mut fb_data.pixels);
                fb_data.width = tw;
                fb_data.height = th;
                fb_data.dirty = true;
            }
        }
    } else if mode_ret == 0 && vga_w > 0 && vga_h > 0 && vga_bpp > 0 {
        // Graphics mode — read from the SVGA device's internal framebuffer.
        // With KVM, setup_vga_lfb_mapping maps 0xE0000000 directly to the
        // SVGA framebuffer buffer, so guest writes update it in-place.
        let bytes_per_pixel = (vga_bpp as usize + 7) / 8;
        let fb_size = vga_w as usize * vga_h as usize * bytes_per_pixel;

        // bochs-drm places its framebuffer at an offset within VRAM using
        // VBE_DISPI_INDEX_X/Y_OFFSET registers. Query the byte offset.
        let vram_offset = corevm_vga_get_fb_offset(handle);

        // Read LFB via corevm_read_phys (reads from KVM memory region = SVGA buffer)
        let mut raw_pixels = vec![0u8; fb_size];
        let read_addr = 0xE000_0000u64 + vram_offset;
        let phys_ret = corevm_read_phys(handle, read_addr, raw_pixels.as_mut_ptr(), fb_size as u32);
        let have_pixels = if phys_ret == 0 {
            // Check if LFB has actual data (guest may have switched to text mode).
            // Sample from multiple locations across the framebuffer — checking
            // only the first 64 bytes fails when the top-left corner is black
            // (e.g. a terminal with black background).
            let stride = vga_w as usize * bytes_per_pixel;
            let sample_rows = [0, vga_h as usize / 4, vga_h as usize / 2, vga_h as usize * 3 / 4];
            sample_rows.iter().any(|&row| {
                let off = row * stride;
                if off + 64 <= raw_pixels.len() {
                    raw_pixels[off..off + 64].iter().any(|&b| b != 0)
                } else {
                    false
                }
            })
        } else {
            // Fallback: read internal SVGA framebuffer directly
            let mut fb_ptr: *const u8 = std::ptr::null();
            let mut fb_len: u32 = 0;
            corevm_vga_get_framebuffer(handle, &mut fb_ptr, &mut fb_len);
            if !fb_ptr.is_null() && fb_len > 0 {
                let off = vram_offset as usize;
                let avail = (fb_len as usize).saturating_sub(off);
                let len = avail.min(fb_size);
                if len > 0 {
                    let raw = unsafe { std::slice::from_raw_parts(fb_ptr.add(off), len) };
                    raw_pixels[..len].copy_from_slice(raw);
                    // Sample from multiple rows, not just the first 64 bytes
                    let stride = vga_w as usize * bytes_per_pixel;
                    let sample_rows = [0, vga_h as usize / 4, vga_h as usize / 2, vga_h as usize * 3 / 4];
                    sample_rows.iter().any(|&row| {
                        let roff = row * stride;
                        if roff + 64 <= len {
                            raw_pixels[roff..roff + 64].iter().any(|&b| b != 0)
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            } else {
                false
            }
        };

        // Always render graphics mode when VGA reports it — even if the
        // framebuffer appears black (e.g. Windows loading screen with dark background).
        if let Ok(mut fb_data) = fb.lock() {
            fb_data.text_mode = false;
            fb_data.width = vga_w;
            fb_data.height = vga_h;
            display::render_graphics_mode(&raw_pixels, vga_w, vga_h, vga_bpp, &mut fb_data.pixels);
            fb_data.dirty = true;
        }
    }
}

/// Fallback: render text mode when graphics mode reports empty LFB.
/// This handles the case where the guest switches to text mode via VGA
/// register programming without disabling VBE.
fn render_text_fallback(handle: u64, fb: &Arc<Mutex<FrameBufferData>>) {
    let mut text_ptr: *const u16 = std::ptr::null();
    let mut text_len: u32 = 0;
    let ret = corevm_vga_get_text_buffer(handle, &mut text_ptr, &mut text_len);
    if ret == 0 && !text_ptr.is_null() && text_len > 0 {
        let text_cells = unsafe { std::slice::from_raw_parts(text_ptr, text_len as usize) };
        if let Ok(mut fb_data) = fb.lock() {
            fb_data.text_mode = true;
            fb_data.text_buffer = text_cells.to_vec();
            let buf = fb_data.text_buffer.clone();
            let (tw, th) = display::render_text_mode(&buf, &mut fb_data.pixels);
            fb_data.width = tw;
            fb_data.height = th;
            fb_data.dirty = true;
        }
    }
}

/// Guess framebuffer resolution from byte length.
/// Returns (width, height, bpp).
fn guess_resolution(len: usize) -> (u32, u32, u8) {
    // Try common resolutions at 32bpp first, then 24bpp, then 16bpp
    let common = [
        (1280, 1024), (1024, 768), (800, 600), (640, 480),
        (1920, 1080), (1600, 1200), (1280, 800), (1280, 720),
    ];
    for &(w, h) in &common {
        if len == (w * h * 4) as usize { return (w, h, 32); }
    }
    for &(w, h) in &common {
        if len == (w * h * 3) as usize { return (w, h, 24); }
    }
    for &(w, h) in &common {
        if len == (w * h * 2) as usize { return (w, h, 16); }
    }
    // Fallback: assume 32bpp, try to find a reasonable width
    let pixels = len / 4;
    if pixels > 0 {
        // Try 640-wide
        if pixels % 640 == 0 {
            return (640, (pixels / 640) as u32, 32);
        }
        if pixels % 800 == 0 {
            return (800, (pixels / 800) as u32, 32);
        }
    }
    (0, 0, 0)
}

