//! GPU adapter configuration for virtual machines.
//!
//! Defines which virtual graphics card the VM exposes to the guest OS.

/// Virtual GPU adapter model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuModel {
    /// QEMU Standard VGA (Bochs VBE DISPI) — widely compatible, works with
    /// every guest OS that has a VBE or VESA driver. PCI ID 1234:1111.
    StdVga,
    /// VirtIO GPU — paravirtual GPU with 2D/3D acceleration support.
    /// Uses host Vulkan for rendering. Windows gets WHQL-signed drivers
    /// via Windows Update (viogpudo). PCI ID 1AF4:1050.
    VirtioGpu,
}

impl GpuModel {
    /// Human-readable name shown in the UI.
    pub fn label(&self) -> &'static str {
        match self {
            GpuModel::StdVga => "Standard VGA (Bochs VBE)",
            GpuModel::VirtioGpu => "VirtIO GPU (3D-accelerated)",
        }
    }

    /// All available GPU models (for UI combo boxes).
    pub const ALL: &'static [GpuModel] = &[
        GpuModel::StdVga,
        GpuModel::VirtioGpu,
    ];
}

impl Default for GpuModel {
    fn default() -> Self {
        GpuModel::StdVga
    }
}

/// Display configuration for a VM.
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    /// Which GPU adapter to emulate.
    pub gpu_model: GpuModel,
    /// Video RAM in MiB (clamped to 8..=256, 0 = default 16 MiB).
    pub vram_mb: u32,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            gpu_model: GpuModel::default(),
            vram_mb: 16,
        }
    }
}

impl DisplayConfig {
    /// Effective VRAM size in bytes.
    pub fn vram_bytes(&self) -> usize {
        let mb = if self.vram_mb == 0 { 16 } else { self.vram_mb.clamp(8, 256) };
        (mb as usize) * 1024 * 1024
    }
}
