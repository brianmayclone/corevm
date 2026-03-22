//! Chipset configuration for virtual machines.
//!
//! Defines the hardware platform (i440FX or Q35) and all chipset-specific
//! parameters: PCI device IDs, slot assignments, IRQ routing, MMIO addresses.
//!
//! All platform-specific values are centralised here so that vm.rs, ffi.rs,
//! and acpi_tables.rs read from a single source of truth.

/// Chipset / machine type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipsetType {
    /// Intel 440FX + PIIX3 (1996). Legacy PCI, limited MMIO window.
    I440FX,
    /// Intel Q35 + ICH9 (2007). PCIe-capable, larger MMIO window, modern.
    Q35,
}

/// PCI slot (device number) assignments for each device role.
#[derive(Debug, Clone, Copy)]
pub struct PciSlotMap {
    pub host_bridge: u8,
    pub isa_lpc_bridge: u8,
    pub vga: u8,
    pub ahci: u8,
    pub e1000: u8,
    pub ac97: u8,
    pub uhci: u8,
    pub virtio_gpu: u8,
    pub virtio_net: u8,
    pub virtio_kbd: u8,
    pub virtio_tablet: u8,
}

/// IRQ assignments for PCI devices.
#[derive(Debug, Clone, Copy)]
pub struct IrqMap {
    pub vga: u8,
    pub ahci: u8,
    pub e1000: u8,
    pub ac97: u8,
    pub uhci: u8,
    pub virtio_gpu: u8,
    pub virtio_net: u8,
    pub virtio_input: u8,
}

/// PCI MMIO address layout.
#[derive(Debug, Clone, Copy)]
pub struct MmioMap {
    /// VGA PCI BAR0 (linear framebuffer).
    pub vga_bar0: u64,
    /// VGA PCI BAR2 (Bochs VBE DISPI registers).
    pub vga_bar2: u64,
    /// VBE LFB default address (used by SeaVGABIOS).
    pub vbe_lfb: u64,
    /// E1000 BAR0 (MMIO register space).
    pub e1000_bar0: u64,
    /// VirtIO GPU BAR0.
    pub virtio_gpu_bar0: u64,
    /// VirtIO Net BAR0.
    pub virtio_net_bar0: u64,
    /// VirtIO Keyboard BAR0.
    pub virtio_kbd_bar0: u64,
    /// VirtIO Tablet BAR0.
    pub virtio_tablet_bar0: u64,
    /// PCI MMIO window start (for ACPI _CRS).
    pub pci_mmio_start: u64,
    /// PCI MMIO window end (for ACPI _CRS, inclusive).
    pub pci_mmio_end: u64,
    /// PCI MMCONFIG base (PCIe extended config space).
    pub pci_mmconfig_base: u64,
}

/// Complete chipset configuration.
#[derive(Debug, Clone, Copy)]
pub struct ChipsetConfig {
    pub chipset_type: ChipsetType,
    /// PCI vendor/device IDs for the host bridge.
    pub host_bridge_vendor: u16,
    pub host_bridge_device: u16,
    /// PCI vendor/device IDs for the ISA/LPC bridge.
    pub isa_bridge_vendor: u16,
    pub isa_bridge_device: u16,
    /// PCI slot assignments.
    pub slots: PciSlotMap,
    /// IRQ assignments.
    pub irqs: IrqMap,
    /// MMIO address map.
    pub mmio: MmioMap,
    /// PIRQ routing: PIRQ A/B/C/D → GSI number.
    pub pirq_route: [u8; 4],
}

/// Intel 440FX + PIIX3 configuration (legacy, for backwards compatibility).
pub static I440FX_CONFIG: ChipsetConfig = ChipsetConfig {
    chipset_type: ChipsetType::I440FX,
    host_bridge_vendor: 0x8086,
    host_bridge_device: 0x1237, // i440FX
    isa_bridge_vendor: 0x8086,
    isa_bridge_device: 0x7000,  // PIIX3
    slots: PciSlotMap {
        host_bridge: 0,
        isa_lpc_bridge: 1,
        vga: 2,
        ahci: 3,
        e1000: 4,
        ac97: 5,
        uhci: 6,
        virtio_gpu: 7,
        virtio_net: 8,
        virtio_kbd: 9,
        virtio_tablet: 10,
    },
    irqs: IrqMap {
        vga: 11,
        ahci: 11,
        e1000: 10,
        ac97: 5,
        uhci: 5,
        virtio_gpu: 9,
        virtio_net: 11,
        virtio_input: 10,
    },
    mmio: MmioMap {
        vga_bar0: 0xFD00_0000,
        vga_bar2: 0xFEBE_0000,
        vbe_lfb: 0xE000_0000,
        e1000_bar0: 0xF000_0000,
        virtio_gpu_bar0: 0xFEB0_0000,
        virtio_net_bar0: 0xFEA0_0000,
        virtio_kbd_bar0: 0xFE90_0000,
        virtio_tablet_bar0: 0xFE80_0000,
        pci_mmio_start: 0xE000_0000,
        pci_mmio_end: 0xFEBF_FFFF,
        pci_mmconfig_base: 0xB000_0000,
    },
    pirq_route: [11, 5, 11, 11], // A=11, B=5, C=11, D=11
};

/// Intel Q35 + ICH9 configuration (modern, default).
pub static Q35_CONFIG: ChipsetConfig = ChipsetConfig {
    chipset_type: ChipsetType::Q35,
    host_bridge_vendor: 0x8086,
    host_bridge_device: 0x29C0, // Q35 MCH
    isa_bridge_vendor: 0x8086,
    isa_bridge_device: 0x2918,  // ICH9 LPC
    slots: PciSlotMap {
        host_bridge: 0,
        isa_lpc_bridge: 31,     // 00:1F.0 — Q35 standard
        vga: 1,
        ahci: 2,
        e1000: 3,
        ac97: 4,
        uhci: 5,
        virtio_gpu: 6,
        virtio_net: 7,
        virtio_kbd: 8,
        virtio_tablet: 9,
    },
    irqs: IrqMap {
        vga: 11,
        ahci: 11,
        e1000: 10,
        ac97: 5,
        uhci: 5,
        virtio_gpu: 9,
        virtio_net: 11,
        virtio_input: 10,
    },
    mmio: MmioMap {
        vga_bar0: 0xFD00_0000,
        vga_bar2: 0xFEBE_0000,
        vbe_lfb: 0xE000_0000,
        e1000_bar0: 0xF000_0000,
        virtio_gpu_bar0: 0xFEB0_0000,
        virtio_net_bar0: 0xFEA0_0000,
        virtio_kbd_bar0: 0xFE90_0000,
        virtio_tablet_bar0: 0xFE80_0000,
        pci_mmio_start: 0xE000_0000,
        pci_mmio_end: 0xFEBF_FFFF,
        pci_mmconfig_base: 0xB000_0000,
    },
    pirq_route: [11, 5, 11, 11],
};

impl Default for ChipsetConfig {
    fn default() -> Self {
        Q35_CONFIG
    }
}
