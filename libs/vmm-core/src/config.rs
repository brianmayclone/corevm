//! VM configuration data models with serde support.
//!
//! These types define the VM configuration schema shared between the
//! desktop GUI (vmmanager) and the web server (vmm-server).

use std::path::Path;
use serde::{Serialize, Deserialize};

// ── Enums ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BootOrder {
    #[serde(alias = "disk")]
    DiskFirst,
    #[serde(alias = "cd")]
    CdFirst,
    #[serde(alias = "floppy")]
    FloppyFirst,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BiosType {
    #[serde(alias = "corevm")]
    CoreVm,
    #[serde(alias = "seabios")]
    SeaBios,
    #[serde(alias = "uefi")]
    Uefi,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RamAlloc {
    Preallocate,
    #[serde(alias = "ondemand")]
    OnDemand,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetMode {
    Disconnected,
    #[serde(alias = "nat", alias = "usermode")]
    UserMode,
    Bridge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MacMode {
    Dynamic,
    Static,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskCacheMode {
    #[serde(alias = "writeback")]
    WriteBack,
    #[serde(alias = "writethrough")]
    WriteThrough,
    None,
}

/// Virtual GPU adapter model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuModel {
    #[serde(alias = "stdvga")]
    StdVga,
    #[serde(alias = "virtiogpu", alias = "virtio-gpu", alias = "virtio_gpu")]
    VirtioGpu,
}

/// Virtual network adapter model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NicModel {
    E1000,
    #[serde(alias = "virtionet", alias = "virtio-net", alias = "virtio_net")]
    VirtioNet,
}

/// Guest OS type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuestOs {
    Other,
    Windows7,
    Windows8,
    Windows10,
    Windows11,
    WindowsServer2016,
    WindowsServer2019,
    WindowsServer2022,
    Ubuntu,
    Debian,
    Fedora,
    OpenSuse,
    RedHat,
    Arch,
    LinuxOther,
    FreeBsd,
    DosFreeDos,
}

/// Guest architecture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuestArch {
    X86,
    X64,
}

// ── VmConfig ─────────────────────────────────────────────────────────────

/// Complete VM configuration — the single source of truth shared by
/// vmmanager (desktop GUI) and vmm-server (web backend).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmConfig {
    pub uuid: String,
    pub name: String,
    pub guest_os: GuestOs,
    pub guest_arch: GuestArch,
    pub ram_mb: u32,
    pub cpu_cores: u32,
    pub disk_images: Vec<String>,
    pub iso_image: String,
    pub boot_order: BootOrder,
    pub bios_type: BiosType,
    pub gpu_model: GpuModel,
    pub vram_mb: u32,
    pub nic_model: NicModel,
    pub net_enabled: bool,
    pub net_mode: NetMode,
    pub net_host_nic: String,
    pub mac_mode: MacMode,
    pub mac_address: String,
    pub audio_enabled: bool,
    pub usb_tablet: bool,
    pub ram_alloc: RamAlloc,
    pub diagnostics: bool,
    pub disk_cache_mb: u32,
    pub disk_cache_mode: DiskCacheMode,
}

impl VmConfig {
    /// First disk image path (for backwards compat / primary disk).
    pub fn primary_disk(&self) -> &str {
        self.disk_images.first().map(|s| s.as_str()).unwrap_or("")
    }

    /// Validate the VM configuration. Returns a list of error messages.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (i, disk) in self.disk_images.iter().enumerate() {
            if !disk.is_empty() && !Path::new(disk).exists() {
                let filename = Path::new(disk)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| disk.clone());
                errors.push(format!("Disk {}: \"{}\" not found", i, filename));
            }
        }
        if !self.iso_image.is_empty() && !Path::new(&self.iso_image).exists() {
            let filename = Path::new(&self.iso_image)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| self.iso_image.clone());
            errors.push(format!("ISO: \"{}\" not found", filename));
        }
        errors
    }

    /// Config file path for this VM in the given directory.
    pub fn config_path(&self, dir: &Path) -> std::path::PathBuf {
        dir.join(format!("{}.conf", self.uuid))
    }
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string().replace("-", ""),
            name: "New VM".into(),
            guest_os: GuestOs::Other,
            guest_arch: GuestArch::X64,
            ram_mb: 256,
            cpu_cores: 1,
            disk_images: Vec::new(),
            iso_image: String::new(),
            boot_order: BootOrder::CdFirst,
            bios_type: BiosType::SeaBios,
            gpu_model: GpuModel::StdVga,
            vram_mb: 16,
            nic_model: NicModel::E1000,
            net_enabled: false,
            net_mode: NetMode::UserMode,
            net_host_nic: String::new(),
            mac_mode: MacMode::Dynamic,
            mac_address: String::new(),
            audio_enabled: true,
            usb_tablet: false,
            ram_alloc: RamAlloc::OnDemand,
            diagnostics: false,
            disk_cache_mb: 0,
            disk_cache_mode: DiskCacheMode::None,
        }
    }
}

// ── Legacy key=value persistence (vmmanager .conf files) ─────────────────

impl VmConfig {
    /// Save config as key=value file (vmmanager compat format).
    pub fn save_legacy(&self, dir: &Path) -> std::io::Result<()> {
        let path = dir.join(format!("{}.conf", self.uuid));
        let boot = match self.boot_order {
            BootOrder::DiskFirst => "disk",
            BootOrder::CdFirst => "cd",
            BootOrder::FloppyFirst => "floppy",
        };
        let bios = match self.bios_type {
            BiosType::CoreVm => "corevm",
            BiosType::SeaBios => "seabios",
            BiosType::Uefi => "uefi",
        };
        let alloc = match self.ram_alloc {
            RamAlloc::Preallocate => "preallocate",
            RamAlloc::OnDemand => "ondemand",
        };
        let net_mode = match self.net_mode {
            NetMode::Disconnected => "disconnected",
            NetMode::UserMode => "usermode",
            NetMode::Bridge => "bridge",
        };
        let mac_mode = match self.mac_mode {
            MacMode::Dynamic => "dynamic",
            MacMode::Static => "static",
        };
        let guest_os_str = self.guest_os.to_config_str();
        let gpu_str = match self.gpu_model {
            GpuModel::StdVga => "stdvga",
            GpuModel::VirtioGpu => "virtiogpu",
        };
        let nic_str = match self.nic_model {
            NicModel::E1000 => "e1000",
            NicModel::VirtioNet => "virtionet",
        };
        let arch_str = match self.guest_arch {
            GuestArch::X86 => "x86",
            GuestArch::X64 => "x64",
        };
        let mut disk_lines = String::new();
        for (i, d) in self.disk_images.iter().enumerate() {
            if i == 0 {
                disk_lines.push_str(&format!("disk={}\n", d));
            } else {
                disk_lines.push_str(&format!("disk{}={}\n", i + 1, d));
            }
        }
        if self.disk_images.is_empty() {
            disk_lines.push_str("disk=\n");
        }
        let content = format!(
            "name={}\nguest_os={}\nguest_arch={}\nram={}\ncpu_cores={}\n{}iso={}\nboot={}\nbios={}\n\
             ram_alloc={}\ngpu={}\nvram_mb={}\nnic={}\nnet_enabled={}\nnet_mode={}\nnet_host_nic={}\n\
             mac_mode={}\nmac_address={}\naudio_enabled={}\nusb_tablet={}\ndiagnostics={}\n\
             disk_cache_mb={}\ndisk_cache_mode={}\n",
            self.name, guest_os_str, arch_str,
            self.ram_mb, self.cpu_cores, disk_lines, self.iso_image,
            boot, bios, alloc, gpu_str, self.vram_mb, nic_str,
            if self.net_enabled { "1" } else { "0" },
            net_mode, self.net_host_nic, mac_mode, self.mac_address,
            if self.audio_enabled { "1" } else { "0" },
            if self.usb_tablet { "1" } else { "0" },
            if self.diagnostics { "1" } else { "0" },
            self.disk_cache_mb,
            match self.disk_cache_mode {
                DiskCacheMode::WriteBack => "writeback",
                DiskCacheMode::WriteThrough => "writethrough",
                DiskCacheMode::None => "none",
            },
        );
        std::fs::write(&path, content)
    }

    /// Load config from legacy key=value file.
    pub fn load_legacy(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let uuid = path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut cfg = VmConfig { uuid, ..Default::default() };

        for line in content.lines() {
            let Some((key, val)) = line.split_once('=') else { continue };
            match key.trim() {
                "name" => cfg.name = val.to_string(),
                "guest_os" => cfg.guest_os = GuestOs::from_config_str(val),
                "guest_arch" => cfg.guest_arch = match val {
                    "x86" => GuestArch::X86,
                    _ => GuestArch::X64,
                },
                "ram" => cfg.ram_mb = val.parse().unwrap_or(256),
                "cpu_cores" => cfg.cpu_cores = val.parse().unwrap_or(1),
                "disk" => {
                    if !val.is_empty() {
                        if cfg.disk_images.is_empty() {
                            cfg.disk_images.push(val.to_string());
                        } else {
                            cfg.disk_images[0] = val.to_string();
                        }
                    }
                }
                k if k.starts_with("disk") && k.len() > 4 => {
                    if let Ok(idx) = k[4..].parse::<usize>() {
                        let pos = idx - 1;
                        if !val.is_empty() {
                            while cfg.disk_images.len() <= pos {
                                cfg.disk_images.push(String::new());
                            }
                            cfg.disk_images[pos] = val.to_string();
                        }
                    }
                }
                "iso" => cfg.iso_image = val.to_string(),
                "boot" => cfg.boot_order = match val {
                    "disk" => BootOrder::DiskFirst,
                    "floppy" => BootOrder::FloppyFirst,
                    _ => BootOrder::CdFirst,
                },
                "bios" => cfg.bios_type = match val {
                    "corevm" => BiosType::CoreVm,
                    "uefi" => BiosType::Uefi,
                    _ => BiosType::SeaBios,
                },
                "jit" => { /* ignored, legacy field */ },
                "ram_alloc" => cfg.ram_alloc = match val {
                    "preallocate" => RamAlloc::Preallocate,
                    _ => RamAlloc::OnDemand,
                },
                "gpu" => cfg.gpu_model = match val {
                    "virtiogpu" | "virtio-gpu" | "virtio_gpu" => GpuModel::VirtioGpu,
                    _ => GpuModel::StdVga,
                },
                "vram_mb" => cfg.vram_mb = val.parse().unwrap_or(16),
                "nic" => cfg.nic_model = match val {
                    "virtionet" | "virtio-net" | "virtio_net" => NicModel::VirtioNet,
                    _ => NicModel::E1000,
                },
                "net_enabled" => cfg.net_enabled = val == "1",
                "net_mode" => cfg.net_mode = match val {
                    "bridge" => NetMode::Bridge,
                    "disconnected" => NetMode::Disconnected,
                    _ => NetMode::UserMode,
                },
                "net_host_nic" => cfg.net_host_nic = val.to_string(),
                "mac_mode" => cfg.mac_mode = match val {
                    "static" => MacMode::Static,
                    _ => MacMode::Dynamic,
                },
                "mac_address" => cfg.mac_address = val.to_string(),
                "audio_enabled" => cfg.audio_enabled = val == "1",
                "usb_tablet" => cfg.usb_tablet = val == "1",
                "diagnostics" => cfg.diagnostics = val == "1",
                "disk_cache_mb" => cfg.disk_cache_mb = val.parse().unwrap_or(32),
                "disk_cache_mode" => cfg.disk_cache_mode = match val {
                    "writethrough" => DiskCacheMode::WriteThrough,
                    "none" => DiskCacheMode::None,
                    _ => DiskCacheMode::WriteBack,
                },
                _ => {}
            }
        }
        while cfg.disk_images.last().map_or(false, |s| s.is_empty()) {
            cfg.disk_images.pop();
        }
        Ok(cfg)
    }
}

// ── GuestOs helper methods ───────────────────────────────────────────────

impl GuestOs {
    /// All known guest OS variants.
    pub const ALL: &'static [GuestOs] = &[
        GuestOs::Other,
        GuestOs::Windows7, GuestOs::Windows8, GuestOs::Windows10, GuestOs::Windows11,
        GuestOs::WindowsServer2016, GuestOs::WindowsServer2019, GuestOs::WindowsServer2022,
        GuestOs::Ubuntu, GuestOs::Debian, GuestOs::Fedora, GuestOs::OpenSuse,
        GuestOs::RedHat, GuestOs::Arch, GuestOs::LinuxOther,
        GuestOs::FreeBsd, GuestOs::DosFreeDos,
    ];

    /// Convert to config file string.
    pub fn to_config_str(&self) -> &'static str {
        match self {
            GuestOs::Other => "other",
            GuestOs::Windows7 => "win7",
            GuestOs::Windows8 => "win8",
            GuestOs::Windows10 => "win10",
            GuestOs::Windows11 => "win11",
            GuestOs::WindowsServer2016 => "winserver2016",
            GuestOs::WindowsServer2019 => "winserver2019",
            GuestOs::WindowsServer2022 => "winserver2022",
            GuestOs::Ubuntu => "ubuntu",
            GuestOs::Debian => "debian",
            GuestOs::Fedora => "fedora",
            GuestOs::OpenSuse => "opensuse",
            GuestOs::RedHat => "redhat",
            GuestOs::Arch => "arch",
            GuestOs::LinuxOther => "linux",
            GuestOs::FreeBsd => "freebsd",
            GuestOs::DosFreeDos => "dos",
        }
    }

    /// Parse from config file string.
    pub fn from_config_str(s: &str) -> Self {
        match s {
            "win7" => GuestOs::Windows7,
            "win8" => GuestOs::Windows8,
            "win10" => GuestOs::Windows10,
            "win11" => GuestOs::Windows11,
            "winserver2016" => GuestOs::WindowsServer2016,
            "winserver2019" => GuestOs::WindowsServer2019,
            "winserver2022" => GuestOs::WindowsServer2022,
            "ubuntu" => GuestOs::Ubuntu,
            "debian" => GuestOs::Debian,
            "fedora" => GuestOs::Fedora,
            "opensuse" => GuestOs::OpenSuse,
            "redhat" => GuestOs::RedHat,
            "arch" => GuestOs::Arch,
            "linux" => GuestOs::LinuxOther,
            "freebsd" => GuestOs::FreeBsd,
            "dos" => GuestOs::DosFreeDos,
            _ => GuestOs::Other,
        }
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            GuestOs::Other => "Other",
            GuestOs::Windows7 => "Windows 7",
            GuestOs::Windows8 => "Windows 8/8.1",
            GuestOs::Windows10 => "Windows 10",
            GuestOs::Windows11 => "Windows 11",
            GuestOs::WindowsServer2016 => "Windows Server 2016",
            GuestOs::WindowsServer2019 => "Windows Server 2019",
            GuestOs::WindowsServer2022 => "Windows Server 2022",
            GuestOs::Ubuntu => "Ubuntu",
            GuestOs::Debian => "Debian",
            GuestOs::Fedora => "Fedora",
            GuestOs::OpenSuse => "openSUSE",
            GuestOs::RedHat => "Red Hat / CentOS",
            GuestOs::Arch => "Arch Linux",
            GuestOs::LinuxOther => "Linux (Other)",
            GuestOs::FreeBsd => "FreeBSD",
            GuestOs::DosFreeDos => "DOS / FreeDOS",
        }
    }
}
