//! Top-level VM struct that ties backend, memory, I/O dispatch, and devices together.

use alloc::boxed::Box;
use crate::backend::{VmBackend, VmExitReason, VmError};
use crate::backend::types::*;
use crate::io::IoDispatch;
use crate::memory::{GuestMemory, MemoryBus};

use core::sync::atomic::AtomicBool;

use crate::devices::serial::Serial;
use crate::devices::ps2::Ps2Controller;
use crate::devices::svga::Svga;
use crate::devices::ahci::Ahci;
use crate::devices::e1000::E1000;
use crate::devices::ac97::Ac97;
use crate::devices::hpet::Hpet;
use crate::devices::virtio_gpu::VirtioGpu;
use crate::devices::virtio_net::VirtioNet;
use crate::devices::virtio_input::VirtioInput;

/// The virtual machine instance.
///
/// Owns the hardware backend, guest memory, and I/O dispatch tables.
/// Device models are registered into `io` (port I/O) and `memory` (MMIO)
/// during setup. Typed device pointers are retained for device-specific
/// FFI functions that need direct access (e.g., serial input/output,
/// PS/2 key injection, VGA framebuffer).
pub struct Vm {
    #[cfg(feature = "linux")]
    pub backend: crate::backend::kvm::KvmBackend,
    #[cfg(all(feature = "anyos", not(feature = "linux")))]
    pub backend: crate::backend::anyos::AnyOsBackend,

    pub memory: GuestMemory,
    pub io: IoDispatch,

    /// Chipset configuration (Q35 or i440FX). Determines PCI layout,
    /// IRQ routing, MMIO addresses, and device slot assignments.
    pub chipset: &'static crate::devices::chipset::ChipsetConfig,

    // Typed device pointers (set during setup_standard_devices / setup_ahci).
    // These point into the Box<dyn IoHandler/MmioHandler> owned by IoDispatch
    // or GuestMemory. Valid for the lifetime of the Vm.
    pub serial_ptr: *mut Serial,
    pub ps2_ptr: *mut Ps2Controller,
    pub svga_ptr: *mut Svga,
    pub ahci_ptr: *mut Ahci,
    pub pit_ptr: *mut crate::devices::pit::Pit,
    pub pic_ptr: *mut crate::devices::pic::PicPair,
    pub debug_port_ptr: *mut crate::devices::debug_port::DebugPort,
    pub pci_bus_ptr: *mut crate::devices::bus::PciBus,
    pub fw_cfg_ptr: *mut crate::devices::fw_cfg::FwCfg,
    pub cmos_ptr: *mut crate::devices::cmos::Cmos,
    pub hpet_ptr: *mut Hpet,
    pub ide_ptr: *mut crate::devices::ide::Ide,
    #[cfg(feature = "std")]
    pub e1000: Option<alloc::sync::Arc<std::sync::Mutex<E1000>>>,
    #[cfg(not(feature = "std"))]
    pub e1000_ptr: *mut E1000,
    pub ac97_ptr: *mut Ac97,
    pub uhci_ptr: *mut crate::devices::uhci::Uhci,
    pub virtio_gpu_ptr: *mut VirtioGpu,
    pub intel_gpu_ptr: *mut crate::devices::intel_gpu::IntelGpu,
    pub virtio_net_ptr: *mut VirtioNet,
    pub virtio_kbd_ptr: *mut VirtioInput,
    pub virtio_tablet_ptr: *mut VirtioInput,

    /// Pointer to the PCI MMIO router (lives inside the MMIO dispatch regions).
    /// Used by `corevm_setup_e1000()` to add E1000 to the router after AHCI setup.
    pub pci_mmio_router_ptr: *mut crate::ffi::PciMmioRouter,

    /// True if booting via UEFI/OVMF (skips legacy VBE LFB mapping at 0xE0000000).
    pub uefi_boot: bool,
    pub pci_io_router_ptr: *mut crate::ffi::PciIoRouter,

    /// Tracks whether AHCI IRQ 11 is currently asserted on the in-kernel irqchip.
    /// Used for level-triggered interrupt semantics: only call set_irq_line when
    /// the state changes.
    /// AtomicBool for SMP safety — accessed from BSP and AP threads concurrently.
    pub ahci_irq_asserted: AtomicBool,

    /// Tracks whether E1000 IRQ 11 is currently asserted (level-triggered).
    pub e1000_irq_asserted: AtomicBool,

    /// Tracks whether VirtIO GPU IRQ is currently asserted (level-triggered).
    pub virtio_gpu_irq_asserted: AtomicBool,

    /// Tracks whether VirtIO Net IRQ is currently asserted (level-triggered).
    pub virtio_net_irq_asserted: AtomicBool,

    /// Set when the guest writes to port 0xCF9 requesting a system reset.
    pub cf9_reset_pending: AtomicBool,

    /// Pointer to ACPI PM device for shutdown detection.
    pub acpi_pm_ptr: *mut crate::devices::acpi::AcpiPm,

    /// VRAM size in MiB (0 = default 16). Set before setup_standard_devices().
    pub vram_mb: u32,

    /// Number of CPU cores. Set before setup_acpi_tables().
    pub cpu_count: u32,

    /// Network backend (user-mode NAT, TAP, etc.). Set via corevm_setup_net().
    #[cfg(feature = "std")]
    pub net_backend: Option<alloc::boxed::Box<dyn crate::devices::net::NetBackend>>,

    /// Thread-safe queue for mouse events from external threads (e.g., UI or
    /// injection threads).  The FFI function `corevm_ps2_mouse_move` pushes
    /// events here instead of calling `ps2.mouse_move()` directly, avoiding
    /// data races on the PS/2 controller.  The VM loop drains this queue in
    /// `corevm_poll_irqs` (single-threaded context).
    #[cfg(feature = "std")]
    pub pending_mouse: std::sync::Mutex<alloc::vec::Vec<(i16, i16, u8, i8)>>,

    /// VMware backdoor device — provides absolute pointer and VMware detection.
    /// Thread-safe (uses atomics) so input threads can update the cursor position.
    pub vmware_backdoor: crate::devices::vmware::VmwareBackdoor,
}

impl Vm {
    /// Create a new VM with `ram_size_mb` megabytes of guest RAM.
    /// Uses Q35 chipset by default. Call `new_with_chipset` for a specific chipset.
    pub fn new(ram_size_mb: u32) -> Result<Self, VmError> {
        Self::new_with_chipset(ram_size_mb, &crate::devices::chipset::Q35_CONFIG)
    }

    /// Create a new VM with a specific chipset configuration.
    pub fn new_with_chipset(ram_size_mb: u32, chipset: &'static crate::devices::chipset::ChipsetConfig) -> Result<Self, VmError> {
        let ram_bytes = (ram_size_mb as usize) * 1024 * 1024;

        #[cfg(feature = "linux")]
        let mut backend = crate::backend::kvm::KvmBackend::new()?;
        #[cfg(all(feature = "anyos", not(feature = "linux")))]
        let mut backend = crate::backend::anyos::AnyOsBackend::new(ram_bytes)?;

        let mut memory = GuestMemory::new(ram_bytes);
        let io = IoDispatch::new();

        // Register guest RAM with the backend.
        //
        // Two reserved regions must be excluded from guest RAM:
        //
        // 1. PCI MMCONFIG hole (Q35 only): 0xB0000000–0xBFFFFFFF (256MB)
        //    Used for PCIe extended config space. SeaBIOS discovers this
        //    via the MCH PCIEXBAR register.
        //
        // 2. PCI device hole: 0xE0000000–0xFFFFFFFF (512MB)
        //    VGA LFB, PCI BARs, IOAPIC, LAPIC, BIOS ROM.
        //    Guest RAM that would fall here is relocated above 4 GB.
        //
        // Memory layout for Q35 with >2.75GB RAM:
        //   Slot 0: 0x00000000 – 0xAFFFFFFF (below MMCONFIG)
        //   Slot 5: 0xC0000000 – 0xDFFFFFFF (between MMCONFIG and PCI hole)
        //   Slot 3: 0x100000000+ (above 4GB)
        //
        const PCI_HOLE_START: u64 = 0xE000_0000;
        const PCI_HOLE_END: u64   = 0x1_0000_0000;
        let mmcfg_base = chipset.mmio.pci_mmconfig_base;
        let mmcfg_size: u64 = 0x1000_0000; // 256MB
        let mmcfg_end = mmcfg_base + mmcfg_size;

        let (ptr, total_size) = memory.ram_mut_ptr();
        let total = total_size as u64;

        if total <= mmcfg_base {
            // RAM fits entirely below MMCONFIG — single slot.
            backend.set_memory_region(0, 0, total, ptr)?;
        } else if total <= PCI_HOLE_START {
            // RAM spans MMCONFIG but not PCI hole.
            // Slot 0: below MMCONFIG
            backend.set_memory_region(0, 0, mmcfg_base, ptr)?;
            // Slot 5: between MMCONFIG end and RAM end (or PCI hole)
            let above_mmcfg = total - mmcfg_base;
            let gap_in_ram = mmcfg_size.min(above_mmcfg);
            let after_mmcfg_ram = above_mmcfg.saturating_sub(mmcfg_size);
            if after_mmcfg_ram > 0 {
                let after_ptr = unsafe { ptr.add((mmcfg_base + gap_in_ram) as usize) };
                backend.set_memory_region(5, mmcfg_end, after_mmcfg_ram, after_ptr)?;
            }
        } else {
            // RAM spans both MMCONFIG and PCI hole.
            // Slot 0: below MMCONFIG
            backend.set_memory_region(0, 0, mmcfg_base, ptr)?;
            // Slot 5: between MMCONFIG end and PCI hole
            let between = PCI_HOLE_START - mmcfg_end;
            if between > 0 {
                let between_ptr = unsafe { ptr.add(mmcfg_end as usize) };
                backend.set_memory_region(5, mmcfg_end, between, between_ptr)?;
            }
            // Slot 3: above 4GB
            let above_4g = total - PCI_HOLE_START;
            let above_ptr = unsafe { ptr.add(PCI_HOLE_START as usize) };
            backend.set_memory_region(3, PCI_HOLE_END, above_4g, above_ptr)?;
        }

        Ok(Self {
            backend,
            memory,
            io,
            chipset,
            serial_ptr: core::ptr::null_mut(),
            ps2_ptr: core::ptr::null_mut(),
            svga_ptr: core::ptr::null_mut(),
            ahci_ptr: core::ptr::null_mut(),
            pit_ptr: core::ptr::null_mut(),
            pic_ptr: core::ptr::null_mut(),
            debug_port_ptr: core::ptr::null_mut(),
            pci_bus_ptr: core::ptr::null_mut(),
            fw_cfg_ptr: core::ptr::null_mut(),
            cmos_ptr: core::ptr::null_mut(),
            hpet_ptr: core::ptr::null_mut(),
            ide_ptr: core::ptr::null_mut(),
            #[cfg(feature = "std")]
            e1000: None,
            #[cfg(not(feature = "std"))]
            e1000_ptr: core::ptr::null_mut(),
            ac97_ptr: core::ptr::null_mut(),
            uhci_ptr: core::ptr::null_mut(),
            virtio_gpu_ptr: core::ptr::null_mut(),
            intel_gpu_ptr: core::ptr::null_mut(),
            virtio_net_ptr: core::ptr::null_mut(),
            virtio_kbd_ptr: core::ptr::null_mut(),
            virtio_tablet_ptr: core::ptr::null_mut(),
            pci_mmio_router_ptr: core::ptr::null_mut(),
            pci_io_router_ptr: core::ptr::null_mut(),
            ahci_irq_asserted: AtomicBool::new(false),
            e1000_irq_asserted: AtomicBool::new(false),
            virtio_gpu_irq_asserted: AtomicBool::new(false),
            virtio_net_irq_asserted: AtomicBool::new(false),
            cf9_reset_pending: AtomicBool::new(false),
            acpi_pm_ptr: core::ptr::null_mut(),
            vram_mb: 0,
            cpu_count: 1,
            #[cfg(feature = "std")]
            net_backend: None,
            #[cfg(feature = "std")]
            pending_mouse: std::sync::Mutex::new(alloc::vec::Vec::new()),
            vmware_backdoor: crate::devices::vmware::VmwareBackdoor::new(),
            uefi_boot: false,
        })
    }

    /// Register all standard chipset devices into I/O and MMIO dispatch.
    pub fn setup_standard_devices(&mut self) {
        use crate::devices::pic::PicPair;
        use crate::devices::pit::Pit;
        use crate::devices::cmos::Cmos;
        use crate::devices::debug_port::DebugPort;
        use crate::devices::acpi::AcpiPm;
        use crate::devices::apm::ApmControl;
        use crate::devices::fw_cfg::FwCfg;
        use crate::devices::bus::{PciBus, PciDevice, PciMmcfgHandler};
        use crate::devices::ioapic::IoApic;
        use crate::devices::lapic::Lapic;
        use crate::devices::port61::Port61;
        use crate::devices::ide::Ide;

        let ram_size = self.memory.ram_size();

        // PIC is registered at the end with a wide range (0x20, count=0x82)
        // covering both master (0x20-0x21) and slave (0xA0-0xA1).
        // All specific devices below are registered first for priority.

        // PIT (0x40-0x43)
        let pit = Box::new(Pit::new());
        let pit_ptr = &*pit as *const Pit as *mut Pit;
        self.io.register(0x40, 4, pit);
        self.pit_ptr = pit_ptr;

        // Port 61 (speaker gate, needs PIT pointer)
        let mut port61 = Port61::new(pit_ptr);
        // On Linux/KVM, sync PIT channel 2 gate to the in-kernel PIT
        // and read channel 2 output from the in-kernel PIT (not the userspace one).
        #[cfg(feature = "linux")]
        {
            port61.set_gate_sync(crate::backend::kvm::kvm_sync_pit_ch2_gate);
            port61.set_pit_output(crate::backend::kvm::kvm_pit_ch2_output);
        }
        self.io.register(0x61, 1, Box::new(port61));

        // CMOS (0x70-0x71)
        let cmos = Box::new(Cmos::new(ram_size));
        let cmos_ptr = &*cmos as *const Cmos as *mut Cmos;
        self.io.register(0x70, 2, cmos);
        self.cmos_ptr = cmos_ptr;

        // PS/2 controller (0x60 and 0x64 — register as 0x60 count=5)
        let ps2 = Box::new(Ps2Controller::new());
        self.ps2_ptr = &*ps2 as *const Ps2Controller as *mut Ps2Controller;
        self.io.register(0x60, 5, ps2);

        // Serial COM1 (0x3F8-0x3FF)
        let serial = Box::new(Serial::new());
        self.serial_ptr = &*serial as *const Serial as *mut Serial;
        self.io.register(0x3F8, 8, serial);

        // COM2-COM4 stubs — Linux probes these during boot.
        // Return 0xFF (no UART present) so detection fails cleanly.
        self.io.register(0x2F8, 8, Box::new(EmptyUartStub));  // COM2
        self.io.register(0x3E8, 8, Box::new(EmptyUartStub));  // COM3
        self.io.register(0x2E8, 8, Box::new(EmptyUartStub));  // COM4

        // Floppy disk controller (0x3F0-0x3F5, 0x3F7) — no floppy emulated.
        // Linux writes DOR=0x00 to reset/disable. Absorb silently.
        self.io.register(0x3F0, 6, Box::new(EmptyUartStub));  // reuse stub (0xFF = no FDC)

        // VGA I/O ports (0x3C0-0x3DA) and MMIO framebuffer
        let svga = Box::new(Svga::new_with_vram(1024, 768, self.vram_mb));
        self.svga_ptr = &*svga as *const Svga as *mut Svga;
        self.io.register(0x3C0, 0x1B, svga);
        // Bochs VBE ports (0x1CE-0x1CF) — same Svga device, accessed via svga_ptr
        self.io.register(0x1CE, 2, Box::new(SvgaVbePortProxy(self.svga_ptr)));
        // VGA legacy framebuffer MMIO at 0xA0000 (128KB)
        self.memory.add_mmio(0xA0000, 0x20000, Box::new(SvgaMmioProxy(self.svga_ptr)));
        // VGA linear framebuffer MMIO at VBE default and PCI BAR0.
        let lfb_size = unsafe { &*self.svga_ptr }.vram_size() as u64;
        self.memory.add_mmio(self.chipset.mmio.vbe_lfb, lfb_size, Box::new(SvgaLfbProxy(self.svga_ptr)));
        // BAR0 proxy: cap at 16 MB to avoid colliding with other PCI regions.
        let bar0_proxy_size = lfb_size.min(0x0100_0000);
        self.memory.add_mmio(self.chipset.mmio.vga_bar0, bar0_proxy_size, Box::new(SvgaLfbProxy(self.svga_ptr)));
        // Bochs dispi register MMIO at the PCI BAR2 default (0xFEBE0000).
        // SeaBIOS may remap BAR2 — the PCI MMIO router handles that dynamically.
        // Do NOT register 0xFE002000 here — it collides with E1000 BAR0 offsets
        // when SeaBIOS places the E1000 at 0xFE000000.
        self.memory.add_mmio(self.chipset.mmio.vga_bar2, 0x1000, Box::new(SvgaDispiMmioProxy(self.svga_ptr)));

        // Port 0x92: Fast A20 Gate + System Reset
        // Required by Windows 10 bootmgr to enable A20 and check system state.
        self.io.register(0x92, 1, Box::new(Port92::new()));

        // Debug port (0x402)
        let dbg = Box::new(DebugPort::new());
        self.debug_port_ptr = &*dbg as *const DebugPort as *mut DebugPort;
        self.io.register(0x402, 1, dbg);

        // ACPI PM at PMBASE 0xB000 (matches FADT PM1a_EVT_BLK in SeaBIOS ACPI tables)
        let acpi_pm = Box::new(AcpiPm::new());
        self.acpi_pm_ptr = &*acpi_pm as *const AcpiPm as *mut AcpiPm;
        self.io.register(0xB000, 0x80, acpi_pm);
        // ACPI PM also at 0x600 — OVMF/UEFI sets ICH9 LPC PMBASE to 0x600.
        // Uses a proxy to the SAME AcpiPm instance so shutdown detection and
        // timer state are shared between SeaBIOS (0xB000) and UEFI (0x600).
        // Range 0x80 covers PM1, PM Timer, GPE0, and TCO registers.
        self.io.register(0x600, 0x80, Box::new(AcpiPmProxy(self.acpi_pm_ptr)));

        // APM (0xB2-0xB3)
        self.io.register(0xB2, 2, Box::new(ApmControl::new()));

        // VMware backdoor port (0x5658) — guest OSes probe this to detect VMware.
        // Return 0xFFFFFFFF to signal "not VMware" and suppress unhandled I/O logs.
        self.io.register(0x5658, 2, Box::new(VmwareBackdoorStub));

        // ICH9 LPC I/O registers (0x700-0x71F) — NMI control, GEN_PMCON, etc.
        // Linux and Windows probe these for NMI configuration and power management.
        self.io.register(0x700, 0x20, Box::new(Ich9LpcIo::new()));

        // fw_cfg (0x510-0x51B: selector, data, DMA ports)
        let mut fw_cfg = Box::new(FwCfg::new(ram_size as u64));
        // Give fw_cfg access to guest RAM for DMA operations.
        let (ram_ptr, ram_len) = self.memory.ram_mut_ptr();
        fw_cfg.set_ram(ram_ptr, ram_len);
        self.fw_cfg_ptr = &*fw_cfg as *const FwCfg as *mut FwCfg;
        self.io.register(0x510, 12, fw_cfg);

        // PCI bus (0xCF8-0xCFF)
        // Add standard PCI devices before registering the bus with I/O dispatcher.
        let mut pci_bus = Box::new(PciBus::new());
        let cs = self.chipset;

        // Host Bridge — chipset-dependent
        {
            use crate::devices::chipset::ChipsetType;
            match cs.chipset_type {
                ChipsetType::Q35 => {
                    // Q35 MCH with PCIEXBAR pointing to MMCONFIG
                    let mch = crate::devices::q35_mch::create_q35_mch(cs.mmio.pci_mmconfig_base);
                    pci_bus.add_device(mch);
                }
                ChipsetType::I440FX => {
                    let mut host = PciDevice::new(cs.host_bridge_vendor, cs.host_bridge_device, 0x06, 0x00, 0x00);
                    host.device = cs.slots.host_bridge;
                    pci_bus.add_device(host);
                }
            }
        }

        // ISA/LPC Bridge — provides PCI IRQ routing via PIRQ registers
        {
            let mut bridge = PciDevice::new(cs.isa_bridge_vendor, cs.isa_bridge_device, 0x06, 0x01, 0x00);
            bridge.device = cs.slots.isa_lpc_bridge;
            bridge.function = 0;
            // Header type 0x80 = multi-function device (required by SeaBIOS)
            bridge.config_space[0x0E] = 0x80;
            // PIRQ routing registers (offsets 0x60-0x63): map PIRQA-D to IRQs.
            // Both PIIX3 and ICH9 LPC use the same register offsets for PIRQA-D.
            bridge.config_space[0x60] = cs.pirq_route[0]; // PIRQA
            bridge.config_space[0x61] = cs.pirq_route[1]; // PIRQB
            bridge.config_space[0x62] = cs.pirq_route[2]; // PIRQC
            bridge.config_space[0x63] = cs.pirq_route[3]; // PIRQD
            // ICH9 has 4 additional PIRQ routes (E-H) at 0x68-0x6B — disable them
            if cs.chipset_type == crate::devices::chipset::ChipsetType::Q35 {
                bridge.config_space[0x68] = 0x80; // PIRQE disabled
                bridge.config_space[0x69] = 0x80; // PIRQF disabled
                bridge.config_space[0x6A] = 0x80; // PIRQG disabled
                bridge.config_space[0x6B] = 0x80; // PIRQH disabled

                // PMBASE (0x40): ACPI PM I/O base address = 0x600 (ICH9 default)
                bridge.config_space[0x40] = 0x01; // bit 0 = I/O space, base = 0x600
                bridge.config_space[0x41] = 0x06;

                // ACPI_CNTL (0x44): ACPI enable bit
                bridge.config_space[0x44] = 0x80; // bit 7 = ACPI enabled

                // RCBA (0xF0): Root Complex Base Address = 0xFED1C000 (ICH9 standard)
                bridge.config_space[0xF0] = 0x01; // bit 0 = enable
                bridge.config_space[0xF1] = 0xC0;
                bridge.config_space[0xF2] = 0xD1;
                bridge.config_space[0xF3] = 0xFE;
            }
            pci_bus.add_device(bridge);
        }

        // ICH9 SMBus controller (00:1F.3) — OVMF's PCI enumeration expects this.
        // Without it, the SMBus DXE driver may misbehave during platform init.
        // I/O BAR at 0x0CC0 (32 bytes), matching the SmBus I/O stub registered above.
        if cs.chipset_type == crate::devices::chipset::ChipsetType::Q35 {
            let mut smb = PciDevice::new(0x8086, 0x2930, 0x0C, 0x05, 0x00); // SMBus class
            smb.device = cs.slots.isa_lpc_bridge; // device 31 (same as LPC)
            smb.function = 3;
            smb.config_space[0x08] = 0x02; // revision
            // I/O BAR at offset 0x20: base = 0x0CC0, size = 32
            smb.set_bar(4, 0x0CC0, 0x20, false); // I/O space BAR
            // Host Configuration register (0x40): HST_EN bit 0 = enabled
            smb.config_space[0x40] = 0x01;
            pci_bus.add_device(smb);
        }

        // VGA (QEMU stdvga) — SeaBIOS needs a PCI VGA for option ROM.
        // If VirtIO GPU is later set up, setup_virtio_gpu() removes this device.
        {
            let mut vga = PciDevice::new(0x1234, 0x1111, 0x03, 0x00, 0x00);
            vga.device = cs.slots.vga;
            let bar0_size = unsafe { &*self.svga_ptr }.vram_size() as u32;
            vga.set_bar_64bit_prefetchable(0, cs.mmio.vga_bar0 as u32, bar0_size);
            vga.set_bar(2, cs.mmio.vga_bar2 as u32, 0x1000, true);
            vga.set_interrupt(cs.irqs.vga, 1);
            vga.add_pm_capability(0x40, 0x00);
            vga.config_space[0x30] = 0x00;
            vga.config_space[0x31] = 0x00;
            vga.config_space[0x32] = 0x00;
            vga.config_space[0x33] = 0x00;
            pci_bus.add_device(vga);
        }

        // Set memory pointer so PCIEXBAR writes can relocate MMCONFIG MMIO
        pci_bus.memory_ptr = &self.memory as *const crate::memory::GuestMemory;
        let pci_bus_ptr = &*pci_bus as *const PciBus as *mut PciBus;
        self.pci_bus_ptr = pci_bus_ptr;
        self.io.register(0xCF8, 8, pci_bus);

        // PCI MMCONFIG MMIO (0xB0000000, 256MB)
        self.memory.add_mmio(0xB000_0000, 0x1000_0000, Box::new(PciMmcfgHandler::new(pci_bus_ptr)));

        // ICH9 RCRB (Root Complex Register Block) at 0xFED1C000 (16KB)
        // OVMF reads this via RCBA in LPC config space. Return zeros for unimplemented registers.
        self.memory.add_mmio(0xFED1_C000, 0x4000, Box::new(RcrbMmioHandler));

        // IDE (0x1F0-0x1F7, 0x3F6, 0x170-0x177, 0x376)
        let ide = Box::new(Ide::new());
        self.ide_ptr = &*ide as *const Ide as *mut Ide;
        self.io.register(0x1F0, 8, ide);

        // Note: HPET is optional — call setup_hpet() separately if needed
        // (required for Windows guests).

        // I/O APIC and Local APIC MMIO.
        // On Linux/KVM with KVM_CREATE_IRQCHIP, these are handled in-kernel.
        // These standalone handlers are only needed for anyOS.
        #[cfg(not(feature = "linux"))]
        {
            self.memory.add_mmio(0xFEC0_0000, 0x1000, Box::new(IoApic::new()));
            self.memory.add_mmio(0xFEE0_0000, 0x1000, Box::new(Lapic::new()));
        }

        // ISA DMA controller stubs — OVMF polls port 0x0008 (DMA1 status)
        // during early init. Without a handler it returns 0xFF (bus float)
        // which OVMF interprets as "channels busy" and spins forever.
        {
            use crate::devices::dma::{Dma1, Dma2, DmaPage};
            self.io.register(0x00, 0x10, Box::new(Dma1));     // DMA1: 0x00-0x0F
            self.io.register(0xC0, 0x20, Box::new(Dma2));     // DMA2: 0xC0-0xDF
            self.io.register(0x80, 0x10, Box::new(DmaPage));   // Page regs: 0x80-0x8F
        }

        // ICH9 SMBus stub — OVMF probes the SMBus host controller registers
        // in the 0x0CC0-0x0CFF range during platform init.
        {
            use crate::devices::smbus::SmBus;
            self.io.register(0x0CC0, 64, Box::new(SmBus::new()));
        }

        // Register PIC pair covering both master (0x20-0x21) and slave (0xA0-0xA1).
        //
        // IMPORTANT: The PIC is registered as a wide range (0x20, count=0x82) and
        // MUST be registered LAST. IoDispatch uses first-match lookup, so all
        // devices registered before the PIC take priority. PicPair's IoHandler
        // returns 0xFF for any port outside 0x20/0x21/0xA0/0xA1.
        //
        // If a new device with ports in 0x22-0x9F is added, it must be registered
        // BEFORE this point.
        let pic = Box::new(PicPair::new());
        self.pic_ptr = &*pic as *const PicPair as *mut PicPair;
        self.io.register(0x20, 0x82, pic);
    }

    /// Map the VGA linear framebuffer (8 MiB at GPA 0xE0000000) as a
    /// hypervisor memory region for fast direct guest access.
    ///
    /// Without this, every guest write to the VGA LFB generates an MMIO
    /// exit which is extremely slow for framebuffer-intensive operations.
    /// With this mapping, guest writes go directly to the SVGA device's
    /// framebuffer buffer and the display update reads from it.
    ///
    /// Must be called AFTER `setup_standard_devices()` (which creates the
    /// SVGA device).
    /// Map the VGA linear framebuffer as a hypervisor memory region for fast
    /// direct guest access. Without this, every guest write to the VGA LFB
    /// generates an MMIO exit which is extremely slow.
    ///
    /// Must be called AFTER `setup_standard_devices()` (which creates the
    /// SVGA device).
    #[cfg(feature = "linux")]
    pub fn setup_vga_lfb_mapping(&mut self) -> Result<(), VmError> {
        if self.svga_ptr.is_null() {
            return Ok(());
        }
        let svga = unsafe { &mut *self.svga_ptr };
        let fb_ptr = svga.framebuffer_mut_ptr();
        let fb_size = svga.vram_size() as u64;
        // Slot 2: VGA LFB at 0xE0000000 — VBE dispi default address.
        // SeaVGABIOS always uses 0xE0000000 for the LFB, so this is the
        // primary mapping. The PCI BAR0 region (0xC0000000) is NOT mapped
        // as a separate KVM slot because for VRAM > 8 MB it would collide
        // with other PCI device regions above 0xFE000000. The PCI BAR0
        // address is handled by the MMIO fallback proxy if any guest
        // driver tries to use it instead of the VBE address.
        // (Slot 0 = RAM below PCI hole, slot 1 = reserved for BIOS ROM, slot 3 = RAM above 4GB)
        //
        // Cap the LFB mapping to avoid overlapping with PCI device MMIO regions.
        // The PCI MMIO catchall starts at 0xF200_0000 (PCI_MMIO_CATCHALL_BASE in ffi.rs),
        // so the LFB must not extend past 0xF1FF_FFFF. Maximum safe size = 0x1200_0000 (288MB).
        // IOAPIC at 0xFEC00000 and LAPIC at 0xFEE00000 are even harder limits.
        // Skip legacy VBE LFB mapping at 0xE0000000 for UEFI boot — OVMF
        // relocates PCIEXBAR to 0xE0000000 and needs it for PCI MMCONFIG.
        if !self.uefi_boot {
            let max_lfb_size: u64 = 0x1200_0000; // 288 MB, up to 0xF1FFFFFF
            let lfb_size = fb_size.min(max_lfb_size);
            self.backend.set_memory_region(2, 0xE000_0000, lfb_size, fb_ptr)?;
        }

        // Slot 4: VGA LFB at PCI BAR0 address.
        // Linux bochs-drm uses the PCI BAR0 address, not the VBE default.
        let bar0_size = fb_size.min(0x0100_0000); // max 16 MB
        self.backend.set_memory_region(4, self.chipset.mmio.vga_bar0, bar0_size, fb_ptr)
    }

    // ── VmBackend delegations ──

    pub fn create_vcpu(&mut self, id: u32) -> Result<(), VmError> {
        self.backend.create_vcpu(id)
    }

    pub fn destroy_vcpu(&mut self, id: u32) -> Result<(), VmError> {
        self.backend.destroy_vcpu(id)
    }

    pub fn run_vcpu(&mut self, id: u32) -> Result<VmExitReason, VmError> {
        self.backend.run_vcpu(id)
    }

    pub fn get_vcpu_regs(&self, id: u32) -> Result<VcpuRegs, VmError> {
        self.backend.get_vcpu_regs(id)
    }

    pub fn set_vcpu_regs(&mut self, id: u32, regs: &VcpuRegs) -> Result<(), VmError> {
        self.backend.set_vcpu_regs(id, regs)
    }

    pub fn get_vcpu_sregs(&self, id: u32) -> Result<VcpuSregs, VmError> {
        self.backend.get_vcpu_sregs(id)
    }

    pub fn set_vcpu_sregs(&mut self, id: u32, sregs: &VcpuSregs) -> Result<(), VmError> {
        self.backend.set_vcpu_sregs(id, sregs)
    }

    pub fn inject_interrupt(&mut self, id: u32, vector: u8) -> Result<(), VmError> {
        self.backend.inject_interrupt(id, vector)
    }

    pub fn inject_exception(&mut self, id: u32, vector: u8, error_code: Option<u32>) -> Result<(), VmError> {
        self.backend.inject_exception(id, vector, error_code)
    }

    pub fn inject_nmi(&mut self, id: u32) -> Result<(), VmError> {
        self.backend.inject_nmi(id)
    }

    pub fn request_interrupt_window(&mut self, id: u32, enable: bool) -> Result<(), VmError> {
        self.backend.request_interrupt_window(id, enable)
    }

    pub fn set_cpuid(&mut self, entries: &[CpuidEntry]) -> Result<(), VmError> {
        self.backend.set_cpuid(entries)
    }

    pub fn set_memory_region(&mut self, slot: u32, guest_phys: u64, size: u64, host_ptr: *mut u8) -> Result<(), VmError> {
        self.backend.set_memory_region(slot, guest_phys, size, host_ptr)
    }

    pub fn reset(&mut self) -> Result<(), VmError> {
        self.backend.reset()
    }

    pub fn destroy_backend(&mut self) {
        self.backend.destroy();
    }

    // ── I/O exit dispatch ──

    /// Route a port I/O exit to the registered device handler.
    pub fn handle_io(&mut self, port: u16, is_write: bool, size: u8, data: &mut [u8]) {
        if is_write {
            let val = match size {
                1 => data[0] as u32,
                2 => u16::from_le_bytes([data[0], data[1]]) as u32,
                4 => u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
                _ => 0,
            };
            let _ = self.io.port_out(port, size, val);
        } else {
            let val = self.io.port_in(port, size).unwrap_or(0xFFFF_FFFF);
            let bytes = val.to_le_bytes();
            for i in 0..size as usize {
                if i < data.len() {
                    data[i] = bytes[i];
                }
            }
        }
    }

    /// Handle a bulk string I/O transfer (REP INS/OUTS).
    ///
    /// Performs `count` port I/O operations of `access_size` bytes each,
    /// reading from or writing to guest physical memory starting at `gpa`,
    /// advancing by `step` (±access_size) each time.
    /// Updates guest registers (RCX, RDI/RSI, RIP) after completion.
    pub fn handle_string_io(
        &mut self, vcpu_id: u32, port: u16, is_write: bool, count: u64, gpa: u64,
        step: i64, instr_len: u64, addr_size: u8, access_size: u8,
    ) {
        let mut current_gpa = gpa;
        for _ in 0..count {
            if is_write {
                // OUTS: read from guest memory, write to port
                let val = match access_size {
                    1 => self.memory.read_u8(current_gpa).unwrap_or(0) as u32,
                    2 => self.memory.read_u16(current_gpa).unwrap_or(0) as u32,
                    4 => self.memory.read_u32(current_gpa).unwrap_or(0),
                    _ => 0,
                };
                let _ = self.io.port_out(port, access_size, val);
            } else {
                // INS: read from port, write to guest memory
                let val = self.io.port_in(port, access_size).unwrap_or(0xFFFF_FFFF);
                match access_size {
                    1 => { let _ = self.memory.write_u8(current_gpa, val as u8); }
                    2 => { let _ = self.memory.write_u16(current_gpa, val as u16); }
                    4 => { let _ = self.memory.write_u32(current_gpa, val); }
                    _ => {}
                }
            }
            current_gpa = (current_gpa as i64 + step) as u64;
        }

        // Update guest registers
        if let Ok(mut regs) = self.get_vcpu_regs(vcpu_id) {
            // Update pointer register
            let total_delta = step * count as i64;
            if is_write {
                regs.rsi = match addr_size {
                    2 => { let v = (regs.rsi as u16).wrapping_add(total_delta as u16); (regs.rsi & !0xFFFF) | v as u64 }
                    4 => (regs.rsi as u32).wrapping_add(total_delta as u32) as u64,
                    _ => regs.rsi.wrapping_add(total_delta as u64),
                };
            } else {
                regs.rdi = match addr_size {
                    2 => { let v = (regs.rdi as u16).wrapping_add(total_delta as u16); (regs.rdi & !0xFFFF) | v as u64 }
                    4 => (regs.rdi as u32).wrapping_add(total_delta as u32) as u64,
                    _ => regs.rdi.wrapping_add(total_delta as u64),
                };
            }
            // Update counter
            regs.rcx = match addr_size {
                2 => { let v = (regs.rcx as u16).wrapping_sub(count as u16); (regs.rcx & !0xFFFF) | v as u64 }
                4 => (regs.rcx as u32).wrapping_sub(count as u32) as u64,
                _ => regs.rcx.wrapping_sub(count),
            };
            // Advance RIP
            regs.rip += instr_len;
            let _ = self.set_vcpu_regs(vcpu_id, &regs);
        }
    }

    /// Route an MMIO exit to the registered device handler.
    ///
    /// If no registered MMIO region matches, checks whether the address falls
    /// within the VGA BAR2 region (Bochs VBE DISPI registers). SeaBIOS may
    /// remap BAR2 to an address different from our statically registered ones,
    /// so this fallback ensures the bochs-drm driver always reaches the VBE
    /// registers regardless of where the firmware places BAR2.
    pub fn handle_mmio(&mut self, addr: u64, is_write: bool, size: u8, data: &mut [u8]) {
        if is_write {
            let val = match size {
                1 => data[0] as u64,
                2 => u16::from_le_bytes([data[0], data[1]]) as u64,
                4 => u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as u64,
                8 => u64::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ]),
                _ => 0,
            };
            if !self.memory.dispatch_mmio_write(addr, size, val) {
                // Fallback: check VGA BAR2 for VBE DISPI registers
                if let Some(offset) = self.vga_bar2_offset(addr) {
                    self.svga_dispi_write(offset, size, val);
                } else {
                    #[cfg(feature = "std")]
                    {
                        static UNH_MMIO_WR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                        let n = UNH_MMIO_WR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if n < 30 {
                            eprintln!("[mmio] unhandled write addr=0x{:X} size={} val=0x{:X}", addr, size, val);
                        }
                    }
                }
            }
        } else {
            match self.memory.dispatch_mmio_read(addr, size) {
                Some(val) => {
                    let bytes = val.to_le_bytes();
                    for i in 0..size as usize {
                        if i < data.len() {
                            data[i] = bytes[i];
                        }
                    }
                }
                None => {
                    // Fallback: check VGA BAR2 for VBE DISPI registers
                    let val = if let Some(offset) = self.vga_bar2_offset(addr) {
                        self.svga_dispi_read(offset, size)
                    } else {
                        #[cfg(feature = "std")]
                        {
                            static UNH_MMIO_RD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                            let n = UNH_MMIO_RD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            if n < 30 {
                                eprintln!("[mmio] unhandled read addr=0x{:X} size={}", addr, size);
                            }
                        }
                        0xFFFF_FFFF_FFFF_FFFF
                    };
                    let bytes = val.to_le_bytes();
                    for i in 0..size as usize {
                        if i < data.len() {
                            data[i] = bytes[i];
                        }
                    }
                }
            }
        }
    }

    /// Check if `addr` falls within the current VGA BAR2 region (4KB).
    /// Returns the offset within the BAR2 region, or None.
    fn vga_bar2_offset(&self, addr: u64) -> Option<u64> {
        if self.pci_bus_ptr.is_null() { return None; }
        let pci_bus = unsafe { &*self.pci_bus_ptr };
        let vga_slot = self.chipset.slots.vga;
        if let Some(vga_dev) = pci_bus.devices.iter().find(|d| d.device == vga_slot && d.function == 0) {
            let bar2 = u32::from_le_bytes([
                vga_dev.config_space[0x18],
                vga_dev.config_space[0x19],
                vga_dev.config_space[0x1A],
                vga_dev.config_space[0x1B],
            ]) & 0xFFFFF000; // Mask out type bits
            if bar2 != 0 {
                let bar2_base = bar2 as u64;
                if addr >= bar2_base && addr < bar2_base + 0x1000 {
                    return Some(addr - bar2_base);
                }
            }
        }
        None
    }

    /// Read from VBE DISPI MMIO registers via the SVGA device.
    fn svga_dispi_read(&mut self, offset: u64, size: u8) -> u64 {
        if self.svga_ptr.is_null() { return 0xFFFF_FFFF; }
        let svga = unsafe { &mut *self.svga_ptr };
        let mut proxy = SvgaDispiMmioProxy(svga as *mut _);
        use crate::memory::mmio::MmioHandler;
        proxy.read(offset, size).unwrap_or(0xFFFF_FFFF)
    }

    /// Write to VBE DISPI MMIO registers via the SVGA device.
    fn svga_dispi_write(&mut self, offset: u64, size: u8, val: u64) {
        if self.svga_ptr.is_null() { return; }
        let svga = unsafe { &mut *self.svga_ptr };
        let mut proxy = SvgaDispiMmioProxy(svga as *mut _);
        use crate::memory::mmio::MmioHandler;
        let _ = proxy.write(offset, size, val);
    }

    // ── KVM-specific response writing ──

    #[cfg(feature = "linux")]
    pub fn set_io_response(&mut self, vcpu_id: u32, data: &[u8]) {
        self.backend.set_io_response(vcpu_id, data);
    }

    #[cfg(feature = "linux")]
    pub fn set_mmio_response(&mut self, vcpu_id: u32, data: &[u8]) {
        self.backend.set_mmio_response(vcpu_id, data);
    }

    /// Handle KVM string I/O IN (REP INSB): loop count times, calling port_in
    /// for each iteration and writing results to the kvm_run data buffer.
    #[cfg(feature = "linux")]
    pub fn complete_string_io_in(&mut self, vcpu_id: u32, port: u16, size: u8, count: u32) {
        for i in 0..count {
            let val = self.io.port_in(port, size).unwrap_or(0xFFFF_FFFF);
            let bytes = val.to_le_bytes();
            self.backend.set_io_response_at(vcpu_id, i, &bytes[..size as usize]);
        }
    }

    /// Handle KVM string I/O OUT (REP OUTSB): loop count times, reading data
    /// from the kvm_run data buffer and calling port_out for each iteration.
    #[cfg(feature = "linux")]
    pub fn complete_string_io_out(&mut self, vcpu_id: u32, port: u16, size: u8, count: u32) {
        for i in 0..count {
            let val = self.backend.get_io_data_at(vcpu_id, i);
            let _ = self.io.port_out(port, size, val);
        }
    }

    // ── Physical memory access ──

    pub fn read_phys(&self, addr: u64, buf: &mut [u8]) -> Result<(), VmError> {
        self.backend.read_phys(addr, buf)
    }

    pub fn write_phys(&mut self, addr: u64, buf: &[u8]) -> Result<(), VmError> {
        self.backend.write_phys(addr, buf)
    }

    pub fn load_binary(&mut self, guest_phys: u64, data: &[u8]) -> Result<(), VmError> {
        self.backend.write_phys(guest_phys, data)
    }

    // ── Typed device access ──

    /// Get a mutable reference to the serial port, if set up.
    ///
    /// # Safety
    /// The pointer is valid for the lifetime of the Vm (points into a Box
    /// owned by IoDispatch). Single-threaded access only.
    pub fn serial(&mut self) -> Option<&mut Serial> {
        if self.serial_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.serial_ptr })
        }
    }

    /// Get a mutable reference to the PS/2 controller, if set up.
    pub fn ps2(&mut self) -> Option<&mut Ps2Controller> {
        if self.ps2_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.ps2_ptr })
        }
    }

    /// Get a reference to the VGA adapter, if set up.
    pub fn svga(&self) -> Option<&Svga> {
        if self.svga_ptr.is_null() {
            None
        } else {
            Some(unsafe { &*self.svga_ptr })
        }
    }

    /// Get a mutable reference to the VGA adapter, if set up.
    pub fn svga_mut(&mut self) -> Option<&mut Svga> {
        if self.svga_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.svga_ptr })
        }
    }

    /// Get a mutable reference to the AHCI controller, if set up.
    pub fn ahci(&mut self) -> Option<&mut Ahci> {
        if self.ahci_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.ahci_ptr })
        }
    }

    /// Get a mutable reference to the PIT timer, if set up.
    pub fn pit_mut(&mut self) -> Option<&mut crate::devices::pit::Pit> {
        if self.pit_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.pit_ptr })
        }
    }

    pub fn cmos_mut(&mut self) -> Option<&mut crate::devices::cmos::Cmos> {
        if self.cmos_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.cmos_ptr })
        }
    }

    /// Enable the HPET (High Precision Event Timer) device.
    /// Required for Windows guests. Call after setup_standard_devices().
    pub fn setup_hpet(&mut self) {
        if !self.hpet_ptr.is_null() {
            return; // already set up
        }
        use crate::devices::hpet::{HPET_BASE, HPET_SIZE};
        let hpet = Box::new(Hpet::new());
        self.hpet_ptr = &*hpet as *const Hpet as *mut Hpet;
        self.memory.add_mmio(HPET_BASE, HPET_SIZE, hpet);
    }

    pub fn hpet_mut(&mut self) -> Option<&mut Hpet> {
        if self.hpet_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.hpet_ptr })
        }
    }

    /// Get a reference to the E1000 Arc<Mutex>, if set up.
    #[cfg(feature = "std")]
    pub fn e1000_arc(&self) -> Option<&alloc::sync::Arc<std::sync::Mutex<E1000>>> {
        self.e1000.as_ref()
    }

    /// Get a mutable reference to the E1000 NIC, if set up. (no_std only)
    #[cfg(not(feature = "std"))]
    pub fn e1000(&mut self) -> Option<&mut E1000> {
        if self.e1000_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.e1000_ptr })
        }
    }

    /// Get a mutable reference to the UHCI USB controller, if set up.
    pub fn uhci(&mut self) -> Option<&mut crate::devices::uhci::Uhci> {
        if self.uhci_ptr.is_null() { None }
        else { Some(unsafe { &mut *self.uhci_ptr }) }
    }

    /// Get a mutable reference to the AC97 audio controller, if set up.
    pub fn ac97(&mut self) -> Option<&mut Ac97> {
        if self.ac97_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.ac97_ptr })
        }
    }

    /// Get a mutable reference to the VirtIO GPU device, if set up.
    pub fn virtio_gpu(&mut self) -> Option<&mut VirtioGpu> {
        if self.virtio_gpu_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.virtio_gpu_ptr })
        }
    }

    /// Get an immutable reference to the VirtIO GPU device, if set up.
    pub fn virtio_gpu_ref(&self) -> Option<&VirtioGpu> {
        if self.virtio_gpu_ptr.is_null() {
            None
        } else {
            Some(unsafe { &*self.virtio_gpu_ptr })
        }
    }

    /// Get a reference to the Intel GPU device, if set up.
    pub fn intel_gpu(&mut self) -> Option<&mut crate::devices::intel_gpu::IntelGpu> {
        if self.intel_gpu_ptr.is_null() { None }
        else { Some(unsafe { &mut *self.intel_gpu_ptr }) }
    }

    /// Get a const reference to the Intel GPU device.
    pub fn intel_gpu_ref(&self) -> Option<&crate::devices::intel_gpu::IntelGpu> {
        if self.intel_gpu_ptr.is_null() { None }
        else { Some(unsafe { &*self.intel_gpu_ptr }) }
    }

    /// Set up the Intel HD Graphics device, replacing the standard VGA adapter.
    /// Call after `setup_standard_devices()`.
    pub fn setup_intel_gpu(&mut self, vram_mb: u32) {
        if !self.intel_gpu_ptr.is_null() { return; }

        use crate::devices::intel_gpu::{self, IntelGpu, IntelGpuAperture, MMIO_SIZE};
        use crate::devices::bus::PciDevice;

        let vram_mb = vram_mb.clamp(16, 512);
        let vram_size = (vram_mb as usize) * 1024 * 1024;

        let gpu = Box::new(IntelGpu::new(vram_mb));
        let gpu_ptr = Box::into_raw(gpu);
        self.intel_gpu_ptr = gpu_ptr;

        // No direct MMIO registration — all BAR0 accesses are routed through
        // the PCI MMIO Router which reads the current BAR address dynamically.
        // SeaBIOS relocates BARs unpredictably based on device sizes.

        // Default BAR addresses for PCI config space (SeaBIOS will override):
        let bar0_addr: u64 = 0xFE00_0000; // BAR0: 4 MB MMIO registers
        let bar2_addr: u64 = 0xFC00_0000; // BAR2: VRAM aperture

        // Remove standard VGA PCI device and add Intel HD Graphics
        if !self.pci_bus_ptr.is_null() {
            let pci_bus = unsafe { &mut *self.pci_bus_ptr };
            pci_bus.remove_device(self.chipset.slots.vga);

            let mut pci_dev = PciDevice::new(
                intel_gpu::VENDOR_ID,  // 0x8086
                intel_gpu::DEVICE_ID,  // 0x0102 (HD Graphics 2000)
                0x03,   // Display controller
                0x00,   // VGA compatible
                0x00,   // prog-if
            );
            pci_dev.device = self.chipset.slots.vga;

            // Revision ID: Skylake GT2 stepping 06
            pci_dev.config_space[0x08] = 0x06;

            // Subsystem: Intel HD Graphics 530
            pci_dev.config_space[0x2C] = 0x86; // Subsystem vendor low (0x8086)
            pci_dev.config_space[0x2D] = 0x80;
            pci_dev.config_space[0x2E] = 0x12; // Subsystem device low (0x1912)
            pci_dev.config_space[0x2F] = 0x19;

            // BAR0: 16 MB MMIO (Skylake GTTMMADR)
            pci_dev.set_bar(0, bar0_addr as u32, MMIO_SIZE as u32, true);

            // BAR2: VRAM aperture (prefetchable, 64-bit)
            // For PCI, set BAR2 as 32-bit for simplicity
            pci_dev.set_bar(2, bar2_addr as u32, vram_size as u32, true);

            // Interrupt: same as VGA slot
            pci_dev.set_interrupt(self.chipset.irqs.vga, 1);

            // PCI Capabilities: PM (0x40) → MSI (0x50 would collide with GGC)
            // Intel SNB uses PM at 0x40 chained to MSI at 0x48
            // But GGC lives at 0x50, so MSI goes at 0x60.
            pci_dev.add_pm_capability(0x40, 0x60);   // PM cap at 0x40, next → 0x60
            pci_dev.add_msi_capability(0x60);          // MSI cap at 0x60

            // GMCH Graphics Control (PCI config 0x50-0x51):
            // Report stolen memory size and GTT size
            // Bits 7:4 = GMS (Graphics Memory Size): 0101 = 32MB stolen
            // Bits 9:8 = GGMS (GTT size): 01 = 1 MB GTT
            pci_dev.config_space[0x50] = 0x50; // 32 MB stolen, 1 MB GTT
            pci_dev.config_space[0x51] = 0x01;

            // MCHBAR / BSM (Base of Stolen Memory) at config 0x5C
            // Point to end of VRAM aperture
            let bsm = bar2_addr as u32 + vram_size as u32;
            pci_dev.config_space[0x5C] = (bsm & 0xFF) as u8;
            pci_dev.config_space[0x5D] = ((bsm >> 8) & 0xFF) as u8;
            pci_dev.config_space[0x5E] = ((bsm >> 16) & 0xFF) as u8;
            pci_dev.config_space[0x5F] = ((bsm >> 24) & 0xFF) as u8;

            // ── OpRegion: write into guest RAM and set ASLS ──
            // Reserve 64 KB at the top of below-4G RAM for the OpRegion.
            // CMOS reports RAM in 64 KB units, so we must reduce by at least
            // 64 KB to ensure SeaBIOS marks the region as unavailable.
            // We reduce both CMOS and FwCfg so the E820 map excludes this area.
            const OPREGION_RESERVE: u64 = 0x10000; // 64 KB (CMOS granularity)
            if !self.fw_cfg_ptr.is_null() {
                let fw_cfg = unsafe { &mut *self.fw_cfg_ptr };
                fw_cfg.reduce_ram_size(OPREGION_RESERVE);
            }
            // Also reduce CMOS extended memory above 16 MB (register 0x34/0x35).
            if !self.cmos_ptr.is_null() {
                let cmos = unsafe { &mut *self.cmos_ptr };
                let cur = (cmos.data[0x34] as u16) | ((cmos.data[0x35] as u16) << 8);
                if cur > 0 {
                    let new_val = cur - 1; // subtract one 64 KB unit
                    cmos.data[0x34] = new_val as u8;
                    cmos.data[0x35] = (new_val >> 8) as u8;
                }
            }
            let (ram_ptr, ram_len) = self.memory.ram_mut_ptr();
            let below_4g_top = ram_len.min(0xE000_0000) as u32;
            // Place OpRegion at the start of the reserved 64 KB block
            let opregion_addr: u32 = below_4g_top - OPREGION_RESERVE as u32;
            let opregion_data = intel_gpu::opregion::build_opregion();
            if (opregion_addr as usize) + intel_gpu::opregion::OPREGION_SIZE <= ram_len {
                unsafe {
                    let dst = ram_ptr.add(opregion_addr as usize);
                    core::ptr::copy_nonoverlapping(
                        opregion_data.as_ptr(),
                        dst,
                        intel_gpu::opregion::OPREGION_SIZE,
                    );
                }
            }

            // Verify OpRegion was written correctly
            #[cfg(feature = "std")]
            if (opregion_addr as usize) + 128 <= ram_len {
                let src = unsafe { core::slice::from_raw_parts(ram_ptr.add(opregion_addr as usize), 128) };
                eprintln!("[intel-gpu] OpRegion verify @ 0x{:08X}:", opregion_addr);
                for row in 0..8 {
                    let off = row * 16;
                    let hex: alloc::vec::Vec<alloc::string::String> = (0..16).map(|i| {
                        alloc::format!("{:02X}", src[off + i])
                    }).collect();
                    let ascii: alloc::string::String = (0..16).map(|i| {
                        let b = src[off + i];
                        if b >= 0x20 && b < 0x7F { b as char } else { '.' }
                    }).collect();
                    eprintln!("  {:04X}: {} |{}|", off, hex.join(" "), ascii);
                }
            }

            // ASLS (Address of System Loaded Software) at PCI config 0xFC
            // Points the igfx driver to our OpRegion in guest RAM
            pci_dev.config_space[0xFC] = (opregion_addr & 0xFF) as u8;
            pci_dev.config_space[0xFD] = ((opregion_addr >> 8) & 0xFF) as u8;
            pci_dev.config_space[0xFE] = ((opregion_addr >> 16) & 0xFF) as u8;
            pci_dev.config_space[0xFF] = ((opregion_addr >> 24) & 0xFF) as u8;

            pci_bus.add_device(pci_dev);
        }

        eprintln!("[intel-gpu] Intel HD Graphics 530 (8086:1912) initialized");
        eprintln!("[intel-gpu] BAR0 (MMIO): 0x{:08X} (4 MB)", bar0_addr);
        eprintln!("[intel-gpu] BAR2 (VRAM): 0x{:08X} ({} MB)", bar2_addr, vram_mb);
        {
            let below_4g = self.memory.ram_mut_ptr().1.min(0xE000_0000) as u32;
            let op_addr = below_4g - 0x10000;
            eprintln!("[intel-gpu] OpRegion: 0x{:08X} (8 KB in 64 KB reserved block)", op_addr);
        }
    }

    /// Set up the VirtIO GPU device at PCI slot 00:07.0.
    /// Call after `setup_standard_devices()`.
    pub fn setup_virtio_gpu(&mut self, vram_mb: u32) {
        if !self.virtio_gpu_ptr.is_null() {
            return; // already set up
        }

        use crate::devices::virtio_gpu::VIRTIO_GPU_BAR0_SIZE;
        use crate::devices::bus::PciDevice;

        let gpu = Box::new(VirtioGpu::new(vram_mb));
        let gpu_ptr = &*gpu as *const VirtioGpu as *mut VirtioGpu;
        self.virtio_gpu_ptr = gpu_ptr;

        // Give VirtIO GPU access to guest RAM for DMA.
        let (ram_ptr, ram_len) = self.memory.ram_mut_ptr();
        unsafe {
            (*gpu_ptr).guest_mem_ptr = ram_ptr;
            (*gpu_ptr).guest_mem_len = ram_len;
        }

        // Register BAR0 MMIO (VirtIO config space) at a default address.
        // SeaBIOS may remap this — the PCI MMIO router handles dynamic routing.
        let bar0_addr: u64 = self.chipset.mmio.virtio_gpu_bar0;
        self.memory.add_mmio(bar0_addr, VIRTIO_GPU_BAR0_SIZE as u64, gpu);

        // Register PCI device and remove VGA from PCI bus.
        // VGA I/O ports and SVGA device remain (needed for BIOS boot),
        // but removing the PCI device prevents Linux from loading bochs-drm
        // which would compete with the virtio-gpu driver.
        if !self.pci_bus_ptr.is_null() {
            let pci_bus = unsafe { &mut *self.pci_bus_ptr };
            pci_bus.remove_device(self.chipset.slots.vga);
            let mut pci_dev = PciDevice::new(
                0x1AF4, // VirtIO vendor
                0x1050, // VirtIO GPU (non-transitional)
                0x03,   // Display controller
                0x00,   // VGA compatible controller
                0x00,   // prog-if
            );
            pci_dev.device = self.chipset.slots.virtio_gpu;

            // Subsystem IDs.
            pci_dev.config_space[0x2C] = 0xF4; // Subsystem vendor ID low (0x1AF4)
            pci_dev.config_space[0x2D] = 0x1A;
            pci_dev.config_space[0x2E] = 0x00; // Subsystem device ID low (0x1100)
            pci_dev.config_space[0x2F] = 0x11;

            // BAR0: MMIO config space (16 KB).
            pci_dev.set_bar(0, bar0_addr as u32, VIRTIO_GPU_BAR0_SIZE, true);

            // Interrupt: INTA, IRQ from chipset config (dedicated, no sharing).
            pci_dev.set_interrupt(self.chipset.irqs.virtio_gpu, 1);

            // VirtIO PCI capability list pointer.
            // Set capabilities pointer (offset 0x34) and status bit.
            pci_dev.config_space[0x34] = 0x40; // Cap list starts at 0x40
            pci_dev.config_space[0x06] |= 0x10; // Status: capabilities list bit

            // VirtIO PCI capabilities at 0x40+.
            // Cap 1: Common configuration (type 1).
            let cap_base = 0x40usize;
            pci_dev.config_space[cap_base] = 0x09;     // VirtIO cap ID
            pci_dev.config_space[cap_base + 1] = 0x54;  // Next cap pointer
            pci_dev.config_space[cap_base + 2] = 0; // Cap length
            pci_dev.config_space[cap_base + 3] = 1;     // cfg_type = COMMON_CFG
            pci_dev.config_space[cap_base + 4] = 0;     // BAR 0
            // Offset within BAR (little-endian u32 at +8).
            pci_dev.config_space[cap_base + 8] = 0x00;  // offset = 0x0000
            pci_dev.config_space[cap_base + 9] = 0x00;
            pci_dev.config_space[cap_base + 10] = 0x00;
            pci_dev.config_space[cap_base + 11] = 0x00;
            // Length (u32 at +12).
            pci_dev.config_space[cap_base + 12] = 0x40; // 64 bytes
            pci_dev.config_space[cap_base + 13] = 0x00;
            pci_dev.config_space[cap_base + 14] = 0x00;
            pci_dev.config_space[cap_base + 15] = 0x00;

            // Cap 2: Notifications (type 2).
            let cap2 = 0x54usize;
            pci_dev.config_space[cap2] = 0x09;
            pci_dev.config_space[cap2 + 1] = 0x6C; // Next cap
            pci_dev.config_space[cap2 + 2] = 0;
            pci_dev.config_space[cap2 + 3] = 2;    // cfg_type = NOTIFY_CFG
            pci_dev.config_space[cap2 + 4] = 0;    // BAR 0
            pci_dev.config_space[cap2 + 8] = 0x00; // offset = 0x1000
            pci_dev.config_space[cap2 + 9] = 0x10;
            pci_dev.config_space[cap2 + 10] = 0x00;
            pci_dev.config_space[cap2 + 11] = 0x00;
            pci_dev.config_space[cap2 + 12] = 0x04; // length = 4
            pci_dev.config_space[cap2 + 13] = 0x00;
            pci_dev.config_space[cap2 + 14] = 0x00;
            pci_dev.config_space[cap2 + 15] = 0x00;
            // Notify offset multiplier (u32 at +16).
            pci_dev.config_space[cap2 + 16] = 0x00;
            pci_dev.config_space[cap2 + 17] = 0x00;
            pci_dev.config_space[cap2 + 18] = 0x00;
            pci_dev.config_space[cap2 + 19] = 0x00;

            // Cap 3: ISR status (type 3).
            let cap3 = 0x6Cusize;
            pci_dev.config_space[cap3] = 0x09;
            pci_dev.config_space[cap3 + 1] = 0x80; // Next cap
            pci_dev.config_space[cap3 + 2] = 0;
            pci_dev.config_space[cap3 + 3] = 3;    // cfg_type = ISR_CFG
            pci_dev.config_space[cap3 + 4] = 0;    // BAR 0
            pci_dev.config_space[cap3 + 8] = 0x00; // offset = 0x2000
            pci_dev.config_space[cap3 + 9] = 0x20;
            pci_dev.config_space[cap3 + 10] = 0x00;
            pci_dev.config_space[cap3 + 11] = 0x00;
            pci_dev.config_space[cap3 + 12] = 0x04; // length = 4
            pci_dev.config_space[cap3 + 13] = 0x00;
            pci_dev.config_space[cap3 + 14] = 0x00;
            pci_dev.config_space[cap3 + 15] = 0x00;

            // Cap 4: Device-specific config (type 4).
            let cap4 = 0x80usize;
            pci_dev.config_space[cap4] = 0x09;
            pci_dev.config_space[cap4 + 1] = 0x00; // No next cap
            pci_dev.config_space[cap4 + 2] = 0;
            pci_dev.config_space[cap4 + 3] = 4;    // cfg_type = DEVICE_CFG
            pci_dev.config_space[cap4 + 4] = 0;    // BAR 0
            pci_dev.config_space[cap4 + 8] = 0x00; // offset = 0x3000
            pci_dev.config_space[cap4 + 9] = 0x30;
            pci_dev.config_space[cap4 + 10] = 0x00;
            pci_dev.config_space[cap4 + 11] = 0x00;
            pci_dev.config_space[cap4 + 12] = 0x40; // length = 64
            pci_dev.config_space[cap4 + 13] = 0x00;
            pci_dev.config_space[cap4 + 14] = 0x00;
            pci_dev.config_space[cap4 + 15] = 0x00;

            // Revision ID (VirtIO 1.0+).
            pci_dev.config_space[0x08] = 0x01;

            pci_bus.add_device(pci_dev);
        }
    }

    /// Map the VirtIO GPU VRAM as a hypervisor memory region for fast
    /// direct guest access. Must be called after `setup_virtio_gpu()`.
    #[cfg(feature = "linux")]
    pub fn setup_virtio_gpu_vram_mapping(&mut self) -> core::result::Result<(), VmError> {
        if self.virtio_gpu_ptr.is_null() {
            return Ok(());
        }
        // Slot 5: VirtIO GPU VRAM — separate from VGA slots (2, 4).
        // Not mapped yet because VirtIO GPU uses DMA-based transfers,
        // not direct framebuffer writes. The host reads the scanout
        // framebuffer via the device struct directly.
        // Future: if performance requires it, map VRAM BAR as a
        // hypervisor memory region for zero-copy guest writes.
        Ok(())
    }

    /// Get a mutable reference to the VirtIO-Net device, if set up.
    pub fn virtio_net(&mut self) -> Option<&mut VirtioNet> {
        if self.virtio_net_ptr.is_null() { None }
        else { Some(unsafe { &mut *self.virtio_net_ptr }) }
    }

    /// Set up the VirtIO-Net device at PCI slot 00:08.0.
    /// Call after `setup_standard_devices()`.
    pub fn setup_virtio_net(&mut self, mac: [u8; 6]) {
        if !self.virtio_net_ptr.is_null() {
            return; // already set up
        }

        use crate::devices::virtio_net::VIRTIO_NET_BAR0_SIZE;
        use crate::devices::bus::PciDevice;

        let net = Box::new(VirtioNet::new(mac));
        let net_ptr = &*net as *const VirtioNet as *mut VirtioNet;
        self.virtio_net_ptr = net_ptr;

        // Give VirtIO-Net access to guest RAM for DMA.
        let (ram_ptr, ram_len) = self.memory.ram_mut_ptr();
        unsafe {
            (*net_ptr).guest_mem_ptr = ram_ptr;
            (*net_ptr).guest_mem_len = ram_len;
        }

        // Register BAR0 MMIO at a default address.
        let bar0_addr: u64 = self.chipset.mmio.virtio_net_bar0;
        self.memory.add_mmio(bar0_addr, VIRTIO_NET_BAR0_SIZE as u64, net);

        // Register PCI device at slot 8 (00:08.0).
        if !self.pci_bus_ptr.is_null() {
            let pci_bus = unsafe { &mut *self.pci_bus_ptr };
            let mut pci_dev = PciDevice::new(
                0x1AF4, // VirtIO vendor
                0x1041, // VirtIO Net (non-transitional)
                0x02,   // Network controller
                0x00,   // Ethernet controller
                0x00,   // prog-if
            );
            pci_dev.device = self.chipset.slots.virtio_net;

            // Subsystem IDs.
            pci_dev.config_space[0x2C] = 0xF4;
            pci_dev.config_space[0x2D] = 0x1A;
            pci_dev.config_space[0x2E] = 0x01; // Subsystem device ID (0x0001)
            pci_dev.config_space[0x2F] = 0x00;

            // BAR0: MMIO config space (16 KB).
            pci_dev.set_bar(0, bar0_addr as u32, VIRTIO_NET_BAR0_SIZE, true);

            // Interrupt: INTA → PIRQ routing.
            // Device 8, INTA: PIRQ = (8+0)%4 = 0 → PIRQA → IRQ 11.
            pci_dev.set_interrupt(11, 1);

            // VirtIO PCI capability list.
            pci_dev.config_space[0x34] = 0x40;
            pci_dev.config_space[0x06] |= 0x10;

            // Cap 1: Common configuration (type 1).
            let cap_base = 0x40usize;
            pci_dev.config_space[cap_base] = 0x09;
            pci_dev.config_space[cap_base + 1] = 0x54;
            pci_dev.config_space[cap_base + 2] = 0;
            pci_dev.config_space[cap_base + 3] = 1;    // COMMON_CFG
            pci_dev.config_space[cap_base + 4] = 0;    // BAR 0
            pci_dev.config_space[cap_base + 8] = 0x00; // offset = 0x0000
            pci_dev.config_space[cap_base + 9] = 0x00;
            pci_dev.config_space[cap_base + 10] = 0x00;
            pci_dev.config_space[cap_base + 11] = 0x00;
            pci_dev.config_space[cap_base + 12] = 0x40;
            pci_dev.config_space[cap_base + 13] = 0x00;
            pci_dev.config_space[cap_base + 14] = 0x00;
            pci_dev.config_space[cap_base + 15] = 0x00;

            // Cap 2: Notifications (type 2).
            let cap2 = 0x54usize;
            pci_dev.config_space[cap2] = 0x09;
            pci_dev.config_space[cap2 + 1] = 0x6C;
            pci_dev.config_space[cap2 + 2] = 0;
            pci_dev.config_space[cap2 + 3] = 2;    // NOTIFY_CFG
            pci_dev.config_space[cap2 + 4] = 0;
            pci_dev.config_space[cap2 + 8] = 0x00;
            pci_dev.config_space[cap2 + 9] = 0x10; // offset = 0x1000
            pci_dev.config_space[cap2 + 10] = 0x00;
            pci_dev.config_space[cap2 + 11] = 0x00;
            pci_dev.config_space[cap2 + 12] = 0x04;
            pci_dev.config_space[cap2 + 13] = 0x00;
            pci_dev.config_space[cap2 + 14] = 0x00;
            pci_dev.config_space[cap2 + 15] = 0x00;
            pci_dev.config_space[cap2 + 16] = 0x00;
            pci_dev.config_space[cap2 + 17] = 0x00;
            pci_dev.config_space[cap2 + 18] = 0x00;
            pci_dev.config_space[cap2 + 19] = 0x00;

            // Cap 3: ISR status (type 3).
            let cap3 = 0x6Cusize;
            pci_dev.config_space[cap3] = 0x09;
            pci_dev.config_space[cap3 + 1] = 0x80;
            pci_dev.config_space[cap3 + 2] = 0;
            pci_dev.config_space[cap3 + 3] = 3;    // ISR_CFG
            pci_dev.config_space[cap3 + 4] = 0;
            pci_dev.config_space[cap3 + 8] = 0x00;
            pci_dev.config_space[cap3 + 9] = 0x20; // offset = 0x2000
            pci_dev.config_space[cap3 + 10] = 0x00;
            pci_dev.config_space[cap3 + 11] = 0x00;
            pci_dev.config_space[cap3 + 12] = 0x04;
            pci_dev.config_space[cap3 + 13] = 0x00;
            pci_dev.config_space[cap3 + 14] = 0x00;
            pci_dev.config_space[cap3 + 15] = 0x00;

            // Cap 4: Device-specific config (type 4).
            let cap4 = 0x80usize;
            pci_dev.config_space[cap4] = 0x09;
            pci_dev.config_space[cap4 + 1] = 0x00; // No next cap
            pci_dev.config_space[cap4 + 2] = 0;
            pci_dev.config_space[cap4 + 3] = 4;    // DEVICE_CFG
            pci_dev.config_space[cap4 + 4] = 0;
            pci_dev.config_space[cap4 + 8] = 0x00;
            pci_dev.config_space[cap4 + 9] = 0x30; // offset = 0x3000
            pci_dev.config_space[cap4 + 10] = 0x00;
            pci_dev.config_space[cap4 + 11] = 0x00;
            pci_dev.config_space[cap4 + 12] = 0x10; // length = 16 bytes (MAC + status)
            pci_dev.config_space[cap4 + 13] = 0x00;
            pci_dev.config_space[cap4 + 14] = 0x00;
            pci_dev.config_space[cap4 + 15] = 0x00;

            // Revision ID (VirtIO 1.0+).
            pci_dev.config_space[0x08] = 0x01;

            pci_bus.add_device(pci_dev);
        }
    }

    /// Get a mutable reference to the VirtIO keyboard, if set up.
    pub fn virtio_kbd(&mut self) -> Option<&mut VirtioInput> {
        if self.virtio_kbd_ptr.is_null() { None }
        else { Some(unsafe { &mut *self.virtio_kbd_ptr }) }
    }

    /// Get a mutable reference to the VirtIO tablet, if set up.
    pub fn virtio_tablet(&mut self) -> Option<&mut VirtioInput> {
        if self.virtio_tablet_ptr.is_null() { None }
        else { Some(unsafe { &mut *self.virtio_tablet_ptr }) }
    }

    /// Set up VirtIO Input devices (keyboard at 00:09.0, tablet at 00:0A.0).
    /// Call after `setup_standard_devices()`.
    pub fn setup_virtio_input(&mut self) {
        use crate::devices::virtio_input::{VIRTIO_INPUT_BAR0_SIZE, InputDeviceType};
        use crate::devices::bus::PciDevice;

        if !self.virtio_kbd_ptr.is_null() { return; } // already set up

        let (ram_ptr, ram_len) = self.memory.ram_mut_ptr();

        // ── Keyboard at PCI 00:09.0, BAR0 at 0xFE90_0000 ──
        let kbd = Box::new(VirtioInput::new(InputDeviceType::Keyboard));
        let kbd_ptr = &*kbd as *const VirtioInput as *mut VirtioInput;
        self.virtio_kbd_ptr = kbd_ptr;
        unsafe { (*kbd_ptr).guest_mem_ptr = ram_ptr; (*kbd_ptr).guest_mem_len = ram_len; }
        let kbd_bar0: u64 = self.chipset.mmio.virtio_kbd_bar0;
        self.memory.add_mmio(kbd_bar0, VIRTIO_INPUT_BAR0_SIZE as u64, kbd);

        if !self.pci_bus_ptr.is_null() {
            let pci_bus = unsafe { &mut *self.pci_bus_ptr };
            let mut pci_dev = PciDevice::new(0x1AF4, 0x1052, 0x09, 0x00, 0x00);
            pci_dev.device = self.chipset.slots.virtio_kbd;
            pci_dev.config_space[0x2C] = 0xF4; pci_dev.config_space[0x2D] = 0x1A;
            pci_dev.config_space[0x2E] = 0x01; pci_dev.config_space[0x2F] = 0x00;
            pci_dev.set_bar(0, kbd_bar0 as u32, VIRTIO_INPUT_BAR0_SIZE, true);
            pci_dev.set_interrupt(10, 1); // IRQ 10 (PIRQ = (9+0)%4=1 → PIRQB → IRQ 10)
            pci_dev.config_space[0x08] = 0x01; // revision
            setup_virtio_pci_caps(&mut pci_dev);
            pci_bus.add_device(pci_dev);
        }

        // ── Tablet at PCI 00:0A.0, BAR0 at 0xFE80_0000 ──
        let tablet = Box::new(VirtioInput::new(InputDeviceType::Tablet));
        let tablet_ptr = &*tablet as *const VirtioInput as *mut VirtioInput;
        self.virtio_tablet_ptr = tablet_ptr;
        unsafe { (*tablet_ptr).guest_mem_ptr = ram_ptr; (*tablet_ptr).guest_mem_len = ram_len; }
        let tablet_bar0: u64 = self.chipset.mmio.virtio_tablet_bar0;
        self.memory.add_mmio(tablet_bar0, VIRTIO_INPUT_BAR0_SIZE as u64, tablet);

        if !self.pci_bus_ptr.is_null() {
            let pci_bus = unsafe { &mut *self.pci_bus_ptr };
            let mut pci_dev = PciDevice::new(0x1AF4, 0x1052, 0x09, 0x00, 0x00);
            pci_dev.device = self.chipset.slots.virtio_tablet;
            pci_dev.config_space[0x2C] = 0xF4; pci_dev.config_space[0x2D] = 0x1A;
            pci_dev.config_space[0x2E] = 0x02; pci_dev.config_space[0x2F] = 0x00;
            pci_dev.set_bar(0, tablet_bar0 as u32, VIRTIO_INPUT_BAR0_SIZE, true);
            pci_dev.set_interrupt(10, 2); // IRQ 10, INTB (PIRQ = (10+1)%4=3 → PIRQD)
            pci_dev.config_space[0x08] = 0x01;
            setup_virtio_pci_caps(&mut pci_dev);
            pci_bus.add_device(pci_dev);
        }
    }
}

/// Set up standard VirtIO PCI capability structures in config space.
/// Shared helper for VirtIO GPU, Net, and Input devices.
fn setup_virtio_pci_caps(pci_dev: &mut crate::devices::bus::PciDevice) {
    pci_dev.config_space[0x34] = 0x40;
    pci_dev.config_space[0x06] |= 0x10;

    // Cap 1: Common Config (type 1)
    let c1 = 0x40usize;
    pci_dev.config_space[c1] = 0x09; pci_dev.config_space[c1+1] = 0x54; pci_dev.config_space[c1+3] = 1; pci_dev.config_space[c1+4] = 0;
    pci_dev.config_space[c1+8] = 0x00; pci_dev.config_space[c1+9] = 0x00; // offset 0x0000
    pci_dev.config_space[c1+12] = 0x40; // length 64

    // Cap 2: Notifications (type 2)
    let c2 = 0x54usize;
    pci_dev.config_space[c2] = 0x09; pci_dev.config_space[c2+1] = 0x6C; pci_dev.config_space[c2+3] = 2; pci_dev.config_space[c2+4] = 0;
    pci_dev.config_space[c2+8] = 0x00; pci_dev.config_space[c2+9] = 0x10; // offset 0x1000
    pci_dev.config_space[c2+12] = 0x04; // length 4

    // Cap 3: ISR (type 3)
    let c3 = 0x6Cusize;
    pci_dev.config_space[c3] = 0x09; pci_dev.config_space[c3+1] = 0x80; pci_dev.config_space[c3+3] = 3; pci_dev.config_space[c3+4] = 0;
    pci_dev.config_space[c3+8] = 0x00; pci_dev.config_space[c3+9] = 0x20; // offset 0x2000
    pci_dev.config_space[c3+12] = 0x04;

    // Cap 4: Device Config (type 4)
    let c4 = 0x80usize;
    pci_dev.config_space[c4] = 0x09; pci_dev.config_space[c4+1] = 0x00; pci_dev.config_space[c4+3] = 4; pci_dev.config_space[c4+4] = 0;
    pci_dev.config_space[c4+8] = 0x00; pci_dev.config_space[c4+9] = 0x30; // offset 0x3000
    pci_dev.config_space[c4+12] = 0x00; pci_dev.config_space[c4+13] = 0x01; // length 256
}

// ── ACPI PM proxy ──
// The canonical AcpiPm instance is registered at 0xB000 (SeaBIOS PMBASE).
// OVMF uses PMBASE=0x600. This proxy delegates to the same instance so
// shutdown detection, PM timer, and all register state is shared.

/// Port I/O proxy that delegates to the canonical AcpiPm instance.
struct AcpiPmProxy(*mut crate::devices::acpi::AcpiPm);
unsafe impl Send for AcpiPmProxy {}

impl crate::io::IoHandler for AcpiPmProxy {
    fn read(&mut self, port: u16, size: u8) -> crate::error::Result<u32> {
        unsafe { &mut *self.0 }.read(port, size)
    }
    fn write(&mut self, port: u16, size: u8, val: u32) -> crate::error::Result<()> {
        unsafe { &mut *self.0 }.write(port, size, val)
    }
}

// ── Empty UART stub (COM2-COM4) ──
// Returns 0xFF on all reads (= no UART chip present), silently absorbs writes.
// This makes Linux/Windows UART detection fail cleanly without log spam.

struct EmptyUartStub;

impl crate::io::IoHandler for EmptyUartStub {
    fn read(&mut self, _port: u16, _size: u8) -> crate::error::Result<u32> {
        Ok(0xFF)
    }
    fn write(&mut self, _port: u16, _size: u8, _val: u32) -> crate::error::Result<()> {
        Ok(())
    }
}

// ── ICH9 LPC I/O registers (0x700-0x71F) ──
// Covers NMI control, GEN_PMCON, and related ICH9 registers that
// Linux and Windows probe during boot.

struct Ich9LpcIo {
    regs: [u8; 0x20],
}

impl Ich9LpcIo {
    fn new() -> Self {
        let mut regs = [0u8; 0x20];
        // NMI_SC (offset 0x02, port 0x702): NMI status/control.
        // Bit 7: SERR# NMI source status (read-only, 0 = no NMI).
        // Bit 3: IOCHK# NMI enable (0 = disabled). Default safe.
        regs[0x02] = 0x00;
        // Port 0x70D (offset 0x0D): often mirrors port 0x61 behavior.
        // Bit 5: Timer Counter 2 output (speaker timer). Default 0.
        regs[0x0D] = 0x00;
        // GEN_PMCON (offset 0x10-0x11, ports 0x710-0x711):
        // General PM configuration. Default: all features disabled.
        regs[0x10] = 0x00;
        regs[0x11] = 0x00;
        Self { regs }
    }
}

impl crate::io::IoHandler for Ich9LpcIo {
    fn read(&mut self, port: u16, _size: u8) -> crate::error::Result<u32> {
        let offset = (port & 0x1F) as usize;
        Ok(self.regs[offset] as u32)
    }
    fn write(&mut self, port: u16, _size: u8, val: u32) -> crate::error::Result<()> {
        let offset = (port & 0x1F) as usize;
        self.regs[offset] = val as u8;
        Ok(())
    }
}

// ── VMware backdoor stub ──
// Guest OSes (Linux, Windows) probe port 0x5658 to detect VMware.
// Returning 0xFFFFFFFF signals "not VMware" and silences the log.

struct VmwareBackdoorStub;

impl crate::io::IoHandler for VmwareBackdoorStub {
    fn read(&mut self, _port: u16, _size: u8) -> crate::error::Result<u32> {
        Ok(0xFFFFFFFF)
    }
    fn write(&mut self, _port: u16, _size: u8, _val: u32) -> crate::error::Result<()> {
        Ok(())
    }
}

// ── Proxy wrappers for VGA device ──
// The Svga instance is owned by IoDispatch (ports 0x3C0-0x3DA).
// These proxies delegate to the same instance via raw pointer for
// additional port ranges and MMIO regions.

/// Proxy for Bochs VBE I/O ports 0x1CE-0x1CF.
struct SvgaVbePortProxy(*mut Svga);
unsafe impl Send for SvgaVbePortProxy {}

impl crate::io::IoHandler for SvgaVbePortProxy {
    fn read(&mut self, port: u16, size: u8) -> crate::error::Result<u32> {
        unsafe { &mut *self.0 }.read(port, size)
    }
    fn write(&mut self, port: u16, size: u8, val: u32) -> crate::error::Result<()> {
        unsafe { &mut *self.0 }.write(port, size, val)
    }
}

/// Proxy for Bochs VBE dispi MMIO registers (PCI BAR2).
/// The bochs DRM driver reads VBE registers via MMIO at offset = index * 2.
struct SvgaDispiMmioProxy(*mut Svga);
unsafe impl Send for SvgaDispiMmioProxy {}

impl crate::memory::mmio::MmioHandler for SvgaDispiMmioProxy {
    fn read(&mut self, offset: u64, size: u8) -> crate::error::Result<u64> {
        let svga = unsafe { &mut *self.0 };
        // QEMU Bochs VBE MMIO layout in BAR2:
        //   0x000-0x3FF: VGA I/O ports (0x3C0-0x3DF) mapped at offset = port - 0x3C0
        //   0x400-0x4FF: VGA I/O ports aliased
        //   0x500-0x5FF: VBE dispi registers at offset 0x500 + index * 2
        //   0x600+:      QEMU extended registers
        if offset >= 0x500 && offset < 0x600 {
            // VBE dispi registers
            let idx = ((offset - 0x500) / 2) as usize;
            if idx < svga.vbe_regs.len() {
                let val = svga.vbe_regs[idx] as u64;
                return Ok(match size {
                    1 => if offset & 1 == 0 { val & 0xFF } else { (val >> 8) & 0xFF },
                    2 => val,
                    4 => {
                        let hi = if idx + 1 < svga.vbe_regs.len() { svga.vbe_regs[idx + 1] as u64 } else { 0 };
                        val | (hi << 16)
                    }
                    _ => val,
                });
            }
            return Ok(0);
        } else if offset < 0x400 {
            // VGA I/O port emulation via MMIO
            let port = 0x3C0 + offset as u16;
            let result = <Svga as crate::io::IoHandler>::read(svga, port, size);
            return result.map(|v| v as u64);
        }
        Ok(0xFFFF_FFFF)
    }
    fn write(&mut self, offset: u64, _size: u8, val: u64) -> crate::error::Result<()> {
        let svga = unsafe { &mut *self.0 };
        if offset >= 0x500 && offset < 0x600 {
            // VBE dispi registers
            let idx = ((offset - 0x500) / 2) as usize;
            let v = val as u16;
            if idx < svga.vbe_regs.len() {
                svga.vbe_regs[idx] = v;
                // VBE_DISPI_INDEX_ENABLE (4): mode switch.
                if idx == 4 && (v & 0x01) != 0 {
                    let w = svga.vbe_regs[1] as u32;
                    let h = svga.vbe_regs[2] as u32;
                    let bpp = svga.vbe_regs[3] as u8;
                    if w > 0 && h > 0 && bpp > 0 {
                        svga.set_mode(crate::devices::svga::VgaMode::LinearFramebuffer { width: w, height: h, bpp });
                    }
                } else if idx == 4 && (v & 0x01) == 0 {
                    svga.set_mode(crate::devices::svga::VgaMode::Text80x25);
                }
            }
            return Ok(());
        } else if offset < 0x400 {
            // VGA I/O port emulation via MMIO
            let port = 0x3C0 + offset as u16;
            return <Svga as crate::io::IoHandler>::write(svga, port, _size, val as u32);
        }
        Ok(())
    }
}

/// Proxy for VGA legacy framebuffer MMIO at 0xA0000 (128KB).
struct SvgaMmioProxy(*mut Svga);
unsafe impl Send for SvgaMmioProxy {}

impl crate::memory::mmio::MmioHandler for SvgaMmioProxy {
    fn read(&mut self, offset: u64, size: u8) -> crate::error::Result<u64> {
        unsafe { &mut *self.0 }.read(offset, size)
    }
    fn write(&mut self, offset: u64, size: u8, val: u64) -> crate::error::Result<()> {
        unsafe { &mut *self.0 }.write(offset, size, val)
    }
}

/// Port 0x92: System Control Port A (Fast A20 Gate).
/// Bit 0: Fast Reset (write 1 = system reset, always reads 0)
/// Bit 1: A20 Gate Enable (1 = enabled, default = enabled in KVM)
/// Bits 2-7: reserved, read as 0
struct Port92 {
    value: u8,
}

impl Port92 {
    fn new() -> Self { Port92 { value: 0x02 } } // A20 enabled by default
}

impl crate::io::IoHandler for Port92 {
    fn read(&mut self, _port: u16, _size: u8) -> crate::error::Result<u32> {
        Ok(self.value as u32)
    }
    fn write(&mut self, _port: u16, _size: u8, val: u32) -> crate::error::Result<()> {
        let v = val as u8;
        // Bit 0: Fast Reset — if set, request system reset
        // (handled by the VM loop checking cf9_reset_pending)
        // Bit 1: A20 Gate — store it (KVM handles A20 internally)
        self.value = v & 0x02; // only store A20 bit, clear reset bit
        Ok(())
    }
}

/// ICH9 Root Complex Register Block (RCRB) MMIO handler.
/// Returns zeros for all reads — stub for OVMF compatibility.
struct RcrbMmioHandler;
unsafe impl Send for RcrbMmioHandler {}

impl crate::memory::mmio::MmioHandler for RcrbMmioHandler {
    fn read(&mut self, _offset: u64, _size: u8) -> crate::error::Result<u64> {
        Ok(0)
    }
    fn write(&mut self, _offset: u64, _size: u8, _val: u64) -> crate::error::Result<()> {
        Ok(())
    }
}

/// Proxy for VGA linear framebuffer MMIO at PCI BAR0 (0xC0000000, 16MB).
/// Guest LFB writes go directly to the Svga framebuffer.
struct SvgaLfbProxy(*mut Svga);
unsafe impl Send for SvgaLfbProxy {}

impl crate::memory::mmio::MmioHandler for SvgaLfbProxy {
    fn read(&mut self, offset: u64, size: u8) -> crate::error::Result<u64> {
        let svga = unsafe { &mut *self.0 };
        let off = offset as usize;
        if off >= svga.framebuffer.len() {
            return Ok(0);
        }
        let mut val: u64 = 0;
        let end = (off + size as usize).min(svga.framebuffer.len());
        for i in off..end {
            val |= (svga.framebuffer[i] as u64) << ((i - off) * 8);
        }
        Ok(val)
    }
    fn write(&mut self, offset: u64, size: u8, val: u64) -> crate::error::Result<()> {
        let svga = unsafe { &mut *self.0 };
        let off = offset as usize;
        for i in 0..(size as usize) {
            let idx = off + i;
            if idx < svga.framebuffer.len() {
                svga.framebuffer[idx] = ((val >> (i * 8)) & 0xFF) as u8;
            }
        }
        Ok(())
    }
}

