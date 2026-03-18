//! Network adapter configuration for virtual machines.
//!
//! Defines which virtual NIC the VM exposes to the guest OS.

/// Virtual network adapter model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicModel {
    /// Intel E1000 (82540EM) — legacy Gigabit Ethernet, widely compatible.
    /// Works with all guest OSes out of the box. PCI ID 8086:100E.
    E1000,
    /// VirtIO-Net — paravirtual high-performance NIC.
    /// Much faster than E1000 due to paravirtualization (no hardware emulation
    /// overhead). Windows gets WHQL-signed drivers via Windows Update (netkvm).
    /// Linux has the virtio_net driver built into the kernel.
    /// PCI ID 1AF4:1041.
    VirtioNet,
}

impl NicModel {
    /// Human-readable name shown in the UI.
    pub fn label(&self) -> &'static str {
        match self {
            NicModel::E1000 => "Intel E1000 (legacy)",
            NicModel::VirtioNet => "VirtIO-Net (high-performance)",
        }
    }

    /// All available NIC models (for UI combo boxes).
    pub const ALL: &'static [NicModel] = &[
        NicModel::E1000,
        NicModel::VirtioNet,
    ];
}

impl Default for NicModel {
    fn default() -> Self {
        NicModel::E1000
    }
}
