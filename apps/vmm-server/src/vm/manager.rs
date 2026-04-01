//! VM lifecycle manager — start, stop, pause, resume.
//!
//! Mirrors the setup sequence from vmmanager/engine/vm.rs but without GUI deps.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use tokio::sync::broadcast;

use libcorevm::ffi::*;
use libcorevm::setup;
use libcorevm::runtime::{VmRuntime, VmRuntimeConfig, VmControlHandle};

use vmm_core::config::*;
use crate::vm::event_handler::{FrameBufferData, ServerEventHandler};

/// Everything needed to control a running VM.
pub struct RunningVm {
    pub handle: u64,
    pub control: VmControlHandle,
    pub framebuffer: Arc<Mutex<FrameBufferData>>,
    pub serial_tx: broadcast::Sender<Vec<u8>>,
    pub thread: JoinHandle<()>,
}

/// Start a VM from config. Returns RunningVm on success.
pub fn start_vm(config: &VmConfig, bios_paths: &[std::path::PathBuf]) -> Result<RunningVm, String> {
    // Validate disk and ISO paths before allocating VM resources
    for (i, path) in config.disk_images.iter().enumerate() {
        if !path.is_empty() && !std::path::Path::new(path).exists() {
            return Err(format!("Disk {}: '{}' not found", i, path));
        }
    }
    if !config.iso_image.is_empty() && !std::path::Path::new(&config.iso_image).exists() {
        return Err(format!("ISO image '{}' not found", config.iso_image));
    }

    // Install SIGUSR1 handler (Linux/KVM — allows cancel_vcpu to interrupt KVM_RUN)
    #[cfg(target_os = "linux")]
    libcorevm::backend::kvm::install_sigusr1_handler();

    // Create VM
    let handle = corevm_create(config.ram_mb);
    if handle == 0 {
        return Err(format!("Failed to create VM: {}",
            setup::get_last_error().unwrap_or_else(|| "Unknown error".into())));
    }

    let num_cpus = config.cpu_cores.max(1).min(32);
    corevm_set_cpu_count(handle, num_cpus);

    for cpu_id in 0..num_cpus {
        if corevm_create_vcpu(handle, cpu_id) != 0 {
            corevm_destroy(handle);
            return Err(format!("Failed to create vCPU {}", cpu_id));
        }
    }
    setup::set_vram_mb(handle, config.vram_mb);

    // Set UEFI boot flag BEFORE device setup so setup_vga_lfb_mapping() skips
    // the VGA LFB KVM slot at 0xE0000000 — OVMF relocates PCIEXBAR there.
    if config.bios_type == BiosType::Uefi {
        libcorevm::ffi::corevm_set_uefi_boot(handle);
    }

    // Standard devices (PCI bus, PIC, IOAPIC, LAPIC, serial, PS/2, PIT, CMOS)
    corevm_setup_standard_devices(handle);

    // HPET for Windows guests
    let is_windows = matches!(config.guest_os,
        GuestOs::Windows7 | GuestOs::Windows8 | GuestOs::Windows10 | GuestOs::Windows11 |
        GuestOs::WindowsServer2016 | GuestOs::WindowsServer2019 | GuestOs::WindowsServer2022);
    if is_windows {
        corevm_setup_hpet(handle);
    }

    // AHCI controller
    corevm_setup_ahci(handle, 6);

    // Networking
    if config.net_enabled {
        let mac = setup::resolve_mac(
            &config.uuid,
            config.mac_mode == MacMode::Static,
            &config.mac_address,
        );
        match config.nic_model {
            NicModel::VirtioNet => {
                corevm_setup_virtio_net(handle, mac.as_ptr());
            }
            NicModel::E1000 => {
                corevm_setup_e1000(handle, mac.as_ptr());
                let _ = setup::load_e1000_rom(handle, bios_paths);
            }
        }
        // Use SDN config if provided (from cluster), otherwise default SLIRP
        if config.net_mode == NetMode::UserMode {
            if let Some(ref sdn) = config.sdn_config {
                // Cluster-managed SDN network — use custom SLIRP parameters
                let slirp_cfg = libcorevm::devices::slirp::SlirpConfig {
                    net_prefix: sdn.net_prefix,
                    gateway_ip: sdn.gateway_ip,
                    dns_ip: sdn.dns_ip,
                    guest_ip: sdn.guest_ip,
                    netmask: sdn.netmask,
                    gw_mac: [0x52, 0x55, sdn.net_prefix[0], sdn.net_prefix[1], sdn.net_prefix[2], sdn.gateway_ip[3]],
                    custom_dns: if sdn.upstream_dns.is_empty() { None } else {
                        sdn.upstream_dns.first()
                            .and_then(|s| s.parse::<std::net::SocketAddr>().ok()
                                .or_else(|| format!("{}:53", s).parse().ok()))
                    },
                    pxe_boot_file: sdn.pxe_boot_file.as_bytes().to_vec(),
                    pxe_next_server: sdn.pxe_next_server,
                };
                libcorevm::ffi::corevm_setup_net_sdn(handle, &slirp_cfg);
            } else {
                // Standard SLIRP with defaults
                corevm_setup_net(handle, 1);
            }
        } else {
            match config.net_mode {
                NetMode::Disconnected => {
                    corevm_setup_net(handle, 0);
                }
                NetMode::UserMode => unreachable!(),
                NetMode::Bridge => {
                    // Generate a TAP name from the VM UUID (first 8 chars)
                    let tap_name = format!("vm{}", &config.uuid[..8.min(config.uuid.len())]);
                    let bridge_name = &config.net_host_nic;
                    let ret = corevm_setup_net_tap(
                        handle,
                        tap_name.as_ptr(),
                        tap_name.len() as u32,
                        bridge_name.as_ptr(),
                        bridge_name.len() as u32,
                    );
                    if ret != 0 {
                        eprintln!("[vm] WARNING: TAP setup failed (tap={}, bridge={}), falling back to disconnected",
                            tap_name, bridge_name);
                        corevm_setup_net(handle, 0);
                    }
                }
            }
        }
    }

    // USB tablet
    if config.usb_tablet {
        corevm_setup_uhci(handle);
    }

    // Audio
    if config.audio_enabled {
        corevm_setup_ac97(handle);
    }

    // GPU setup
    let has_virtio_gpu = config.gpu_model == GpuModel::VirtioGpu;
    let has_intel_gpu = config.gpu_model == GpuModel::IntelHD;
    if has_virtio_gpu {
        corevm_setup_virtio_gpu(handle, config.vram_mb.max(64));
        // VirtIO Input disabled — PS/2 keyboard/mouse works fine with VirtIO GPU.
        // Re-enable once VirtIO Input driver issues are resolved.
        // corevm_setup_virtio_input(handle);
    } else if has_intel_gpu {
        corevm_setup_intel_gpu(handle, config.vram_mb);
    }

    // Load firmware BEFORE ACPI tables — load_ovmf() sets vm.uefi_boot which
    // controls PM base address (0x600 vs 0xB000) and MCFG table generation.
    let bios_paths_str: Vec<&std::path::Path> = bios_paths.iter().map(|p| p.as_path()).collect();
    match config.bios_type {
        BiosType::SeaBios => setup::load_seabios(handle, bios_paths)?,
        BiosType::CoreVm => setup::load_corevm_bios(handle, bios_paths)?,
        BiosType::Uefi => {
            let vars_path = std::env::temp_dir().join(format!("{}_ovmf.fd", config.uuid));
            setup::load_ovmf(handle, bios_paths, &vars_path)?;
        }
    }

    // ACPI tables (AFTER all PCI devices AND firmware load)
    if is_windows {
        corevm_setup_acpi_tables_with_hpet(handle);
    } else {
        corevm_setup_acpi_tables(handle);
    }

    // Attach ISO (port 1 = CDROM)
    if !config.iso_image.is_empty() {
        if config.iso_image.starts_with("/vmm/san/") {
            // SAN ISO — open a read-only UDS connection for reliable I/O (bypasses FUSE)
            let parts: Vec<&str> = config.iso_image.strip_prefix("/vmm/san/").unwrap().splitn(2, '/').collect();
            let iso_via_uds = if parts.len() == 2 {
                let volume_name = parts[0];
                let rel_path = parts[1];
                resolve_san_volume_id(volume_name).and_then(|vid| {
                    libcorevm::san_disk::SanDiskConnection::open(&vid, rel_path).ok()
                })
            } else { None };

            if let Some(conn) = iso_via_uds {
                let size = conn.disk_size;
                tracing::info!("SAN ISO attached: {} ({}B) via UDS on port 1", config.iso_image, size);
                setup::attach_san_cdrom_to_ahci(handle, 1, size, Box::new(conn))?;
            } else {
                // Fallback to FUSE fd
                tracing::warn!("SAN ISO {} — UDS failed, falling back to FUSE fd", config.iso_image);
                setup::attach_image_to_ahci(handle, &config.iso_image, 1, true)?;
            }
        } else {
            setup::attach_image_to_ahci(handle, &config.iso_image, 1, true)?;
        }
        if is_windows {
            let _ = setup::attach_cdrom_to_ide(handle, &config.iso_image);
        }
    }

    // Attach disk images (ports 0, 2, 3, 4, 5)
    // SAN disks (path starts with /vmm/san/) use direct UDS connection to vmm-san.
    // Local disks use traditional fd-based I/O.
    let disk_ports = [0u32, 2, 3, 4, 5];
    for (i, disk_path) in config.disk_images.iter().enumerate() {
        if !disk_path.is_empty() {
            if let Some(&port) = disk_ports.get(i) {
                if disk_path.starts_with("/vmm/san/") {
                    // SAN disk — connect directly to vmm-san via UDS (bypasses FUSE)
                    // Parse: /vmm/san/<volume_name>/<rel_path>
                    let parts: Vec<&str> = disk_path.strip_prefix("/vmm/san/").unwrap().splitn(2, '/').collect();
                    if parts.len() == 2 {
                        let volume_name = parts[0];
                        let rel_path = parts[1];
                        // Resolve volume_id from volume_name via SAN API
                        let volume_id = resolve_san_volume_id(volume_name);
                        if let Some(vid) = volume_id {
                            match libcorevm::san_disk::SanDiskConnection::open(&vid, rel_path) {
                                Ok(conn) => {
                                    let size = conn.disk_size;
                                    tracing::info!("SAN disk attached: {} ({}B) via UDS on port {}", disk_path, size, port);
                                    setup::attach_san_disk_to_ahci(handle, port, size, Box::new(conn))?;
                                }
                                Err(e) => {
                                    tracing::warn!("SAN disk {} failed, falling back to FUSE: {}", disk_path, e);
                                    setup::attach_image_to_ahci(handle, disk_path, port, false)?;
                                }
                            }
                        } else {
                            tracing::warn!("Cannot resolve SAN volume '{}', falling back to FUSE", volume_name);
                            setup::attach_image_to_ahci(handle, disk_path, port, false)?;
                        }
                    } else {
                        setup::attach_image_to_ahci(handle, disk_path, port, false)?;
                    }
                } else {
                    // Local disk — standard fd-based I/O (unchanged)
                    setup::attach_image_to_ahci(handle, disk_path, port, false)?;
                }
                let cache_mode = match config.disk_cache_mode {
                    DiskCacheMode::WriteBack => 0u32,
                    DiskCacheMode::WriteThrough => 1,
                    DiskCacheMode::None => 2,
                };
                setup::configure_disk_cache(handle, port, config.disk_cache_mb, cache_mode);
            }
        }
    }

    // Initial CPU state
    if config.bios_type == BiosType::Uefi {
        setup::set_initial_cpu_state_uefi(handle)?;
    } else {
        setup::set_initial_cpu_state(handle)?;
    }
    setup::setup_bda_com1(handle);

    // Create shared state
    let fb = Arc::new(Mutex::new(FrameBufferData::default()));
    let (serial_tx, _) = broadcast::channel(256);

    // Build runtime
    let rt_config = VmRuntimeConfig {
        handle,
        num_cpus,
        usb_tablet: config.usb_tablet,
        audio_enabled: config.audio_enabled,
        net_enabled: config.net_enabled,
        virtio_gpu: has_virtio_gpu,
        virtio_input: has_virtio_gpu,
        intel_gpu: has_intel_gpu,
        diagnostics: false,
        ..Default::default()
    };

    let mut runtime = VmRuntime::new(rt_config, libcorevm::runtime::NullEventHandler);
    let control = runtime.control_handle();

    let handler = ServerEventHandler::new(
        fb.clone(), serial_tx.clone(), control.clone(), handle,
    );
    runtime.set_handler(handler);

    // Spawn VM execution thread
    let vm_thread = thread::spawn(move || {
        runtime.start();
        runtime.wait();
        corevm_ahci_flush_caches(handle);
        corevm_destroy(handle);
    });

    Ok(RunningVm {
        handle,
        control,
        framebuffer: fb,
        serial_tx,
        thread: vm_thread,
    })
}

/// Resolve a SAN volume name to its UUID by querying the local vmm-san daemon.
fn resolve_san_volume_id(volume_name: &str) -> Option<String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect("127.0.0.1:7443").ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok();
    let req = "GET /api/volumes HTTP/1.0\r\nHost: 127.0.0.1:7443\r\n\r\n";
    stream.write_all(req.as_bytes()).ok()?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok();
    let response = String::from_utf8_lossy(&buf);
    let body = response.split("\r\n\r\n").nth(1)?;

    let volumes: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    volumes.as_array()?.iter().find_map(|v| {
        if v["name"].as_str() == Some(volume_name) {
            v["id"].as_str().map(|s| s.to_string())
        } else {
            None
        }
    })
}
