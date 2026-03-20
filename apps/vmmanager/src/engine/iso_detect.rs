//! ISO 9660 image detection — identifies the operating system from an ISO file.
//!
//! Reads the Primary Volume Descriptor (PVD) at sector 16 (offset 0x8000) and
//! the El Torito Boot Record at sector 17 to extract volume label, publisher,
//! and application ID. These strings are matched against known patterns to
//! identify Windows versions, common Linux distributions, and other OSes.

use std::path::Path;

/// Detected operating system from an ISO image.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectedOs {
    Windows7,
    Windows8,
    Windows81,
    Windows10,
    Windows11,
    WindowsServer,
    WindowsUnknown,
    Ubuntu,
    Debian,
    Fedora,
    CentOS,
    Arch,
    OpenSuse,
    Mint,
    LinuxGeneric,
    FreeBsd,
    Unknown,
}

impl DetectedOs {
    /// Human-readable label for the detected OS.
    pub fn label(&self) -> &'static str {
        match self {
            DetectedOs::Windows7 => "Windows 7",
            DetectedOs::Windows8 => "Windows 8",
            DetectedOs::Windows81 => "Windows 8.1",
            DetectedOs::Windows10 => "Windows 10",
            DetectedOs::Windows11 => "Windows 11",
            DetectedOs::WindowsServer => "Windows Server",
            DetectedOs::WindowsUnknown => "Windows (unknown version)",
            DetectedOs::Ubuntu => "Ubuntu Linux",
            DetectedOs::Debian => "Debian Linux",
            DetectedOs::Fedora => "Fedora Linux",
            DetectedOs::CentOS => "CentOS Linux",
            DetectedOs::Arch => "Arch Linux",
            DetectedOs::OpenSuse => "openSUSE Linux",
            DetectedOs::Mint => "Linux Mint",
            DetectedOs::LinuxGeneric => "Linux",
            DetectedOs::FreeBsd => "FreeBSD",
            DetectedOs::Unknown => "Unknown OS",
        }
    }

    /// Whether this is a Windows variant.
    pub fn is_windows(&self) -> bool {
        matches!(self,
            DetectedOs::Windows7 | DetectedOs::Windows8 | DetectedOs::Windows81 |
            DetectedOs::Windows10 | DetectedOs::Windows11 | DetectedOs::WindowsServer |
            DetectedOs::WindowsUnknown
        )
    }

    /// Whether this is a Linux variant.
    pub fn is_linux(&self) -> bool {
        matches!(self,
            DetectedOs::Ubuntu | DetectedOs::Debian | DetectedOs::Fedora |
            DetectedOs::CentOS | DetectedOs::Arch | DetectedOs::OpenSuse |
            DetectedOs::Mint | DetectedOs::LinuxGeneric
        )
    }

    /// Suggested GuestOs value for VmConfig.
    pub fn to_guest_os(&self) -> libcorevm::setup::GuestOs {
        use libcorevm::setup::GuestOs;
        match self {
            DetectedOs::Windows7 => GuestOs::Windows7,
            DetectedOs::Windows8 | DetectedOs::Windows81 => GuestOs::Windows8,
            DetectedOs::Windows10 => GuestOs::Windows10,
            DetectedOs::Windows11 => GuestOs::Windows11,
            DetectedOs::WindowsServer => GuestOs::WindowsServer2022,
            DetectedOs::WindowsUnknown => GuestOs::Windows10,
            DetectedOs::Ubuntu => GuestOs::Ubuntu,
            DetectedOs::Debian => GuestOs::Debian,
            DetectedOs::Fedora => GuestOs::Fedora,
            DetectedOs::CentOS => GuestOs::RedHat,
            DetectedOs::Arch => GuestOs::Arch,
            DetectedOs::OpenSuse => GuestOs::OpenSuse,
            DetectedOs::Mint => GuestOs::Ubuntu,
            DetectedOs::LinuxGeneric => GuestOs::LinuxOther,
            DetectedOs::FreeBsd => GuestOs::FreeBsd,
            DetectedOs::Unknown => GuestOs::Other,
        }
    }

    /// Suggested RAM in MB for this OS.
    pub fn suggested_ram_mb(&self) -> u32 {
        match self {
            DetectedOs::Windows11 => 4096,
            DetectedOs::Windows10 => 4096,
            DetectedOs::Windows8 | DetectedOs::Windows81 => 2048,
            DetectedOs::Windows7 => 2048,
            DetectedOs::WindowsServer => 4096,
            DetectedOs::WindowsUnknown => 2048,
            DetectedOs::Ubuntu | DetectedOs::Fedora | DetectedOs::Mint => 2048,
            DetectedOs::Debian | DetectedOs::CentOS | DetectedOs::Arch | DetectedOs::OpenSuse => 1024,
            DetectedOs::LinuxGeneric => 1024,
            DetectedOs::FreeBsd => 1024,
            DetectedOs::Unknown => 512,
        }
    }

    /// Suggested disk size in MB for this OS.
    pub fn suggested_disk_mb(&self) -> u64 {
        match self {
            DetectedOs::Windows11 => 65536,      // 64 GB
            DetectedOs::Windows10 => 65536,
            DetectedOs::Windows8 | DetectedOs::Windows81 => 40960,  // 40 GB
            DetectedOs::Windows7 => 32768,       // 32 GB
            DetectedOs::WindowsServer => 65536,
            DetectedOs::WindowsUnknown => 40960,
            DetectedOs::Ubuntu | DetectedOs::Fedora | DetectedOs::Mint => 25600, // 25 GB
            DetectedOs::Debian | DetectedOs::CentOS | DetectedOs::Arch | DetectedOs::OpenSuse => 20480, // 20 GB
            DetectedOs::LinuxGeneric => 16384,   // 16 GB
            DetectedOs::FreeBsd => 16384,
            DetectedOs::Unknown => 8192,         // 8 GB
        }
    }

    /// Suggested CPU cores.
    pub fn suggested_cpus(&self) -> u32 {
        match self {
            DetectedOs::Windows11 | DetectedOs::Windows10 | DetectedOs::WindowsServer => 2,
            _ => 1,
        }
    }

    /// Whether UEFI boot is recommended.
    pub fn suggest_uefi(&self) -> bool {
        matches!(self, DetectedOs::Windows11)
    }
}

/// Result of ISO detection.
#[derive(Debug, Clone)]
pub struct IsoInfo {
    pub os: DetectedOs,
    pub volume_id: String,
    pub publisher: String,
    pub application: String,
}

/// Detect the operating system from an ISO 9660 image file.
///
/// Reads the Primary Volume Descriptor at offset 0x8000 (sector 16) and
/// extracts identifying strings. Returns `None` if the file cannot be read
/// or is not a valid ISO 9660 image.
pub fn detect_iso(path: &Path) -> Option<IsoInfo> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;

    // ISO 9660 Primary Volume Descriptor is at sector 16 (0x8000)
    file.seek(SeekFrom::Start(0x8000)).ok()?;

    let mut pvd = [0u8; 2048];
    file.read_exact(&mut pvd).ok()?;

    // Check PVD signature: type=1, "CD001"
    if pvd[0] != 0x01 || &pvd[1..6] != b"CD001" {
        return None;
    }

    // Extract strings from PVD (all are padded with spaces)
    let volume_id = read_strA(&pvd[40..72]);        // Volume Identifier (32 bytes)
    let publisher = read_strA(&pvd[318..446]);       // Publisher Identifier (128 bytes)
    let application = read_strA(&pvd[574..702]);     // Application Identifier (128 bytes)
    let system_id = read_strA(&pvd[8..40]);          // System Identifier (32 bytes)

    // Also try to read El Torito boot record at sector 17 for additional hints
    let mut boot_id = String::new();
    file.seek(SeekFrom::Start(0x8800)).ok();
    let mut brvd = [0u8; 2048];
    if file.read_exact(&mut brvd).is_ok() {
        if &brvd[1..6] == b"CD001" && brvd[0] == 0x00 {
            boot_id = read_strA(&brvd[7..39]);
        }
    }

    // Combine all strings for matching
    let all = format!("{} {} {} {} {}", volume_id, publisher, application, system_id, boot_id).to_uppercase();

    let os = detect_os_from_strings(&volume_id.to_uppercase(), &all);

    Some(IsoInfo {
        os,
        volume_id,
        publisher,
        application,
    })
}

/// Read an ISO 9660 "strA" field: trim trailing spaces and NULs.
fn read_strA(data: &[u8]) -> String {
    let s = String::from_utf8_lossy(data);
    s.trim_end_matches(|c: char| c == ' ' || c == '\0').to_string()
}

/// Match OS from volume ID and combined info strings.
fn detect_os_from_strings(volume_id: &str, all: &str) -> DetectedOs {
    // ── Windows detection ──
    // Windows 11 ISOs typically have volume labels like:
    // "CCCOMA_X64FRE_EN-US_DV9" or contain "WINDOWS 11"
    if all.contains("WINDOWS 11") || all.contains("WIN11") {
        return DetectedOs::Windows11;
    }

    // Windows 10 ISOs
    if all.contains("WINDOWS 10") || all.contains("WIN10") {
        return DetectedOs::Windows10;
    }

    // Windows 8.1
    if all.contains("WINDOWS 8.1") || all.contains("WIN8.1") || all.contains("WINBLUE") {
        return DetectedOs::Windows81;
    }

    // Windows 8
    if all.contains("WINDOWS 8") || all.contains("WIN8") {
        return DetectedOs::Windows8;
    }

    // Windows 7
    if all.contains("WINDOWS 7") || all.contains("WIN7")
        || all.contains("GRMCULFRE") || all.contains("GRMCULXFRE")
        || all.contains("GRMSXFRE") || all.contains("GRMCHPXFRE")
    {
        return DetectedOs::Windows7;
    }

    // Windows Server
    if all.contains("SERVER") && (all.contains("WINDOWS") || all.contains("WIN")) {
        return DetectedOs::WindowsServer;
    }

    // Generic Windows detection (MICROSOFT, MSDN, etc.)
    // Windows ISOs typically have volume IDs like "CCCOMA_X64FRE_..." or "GSP1..."
    if all.contains("MICROSOFT") || all.contains("MSDN")
        || all.contains("WINDOWS") || all.contains("WINPE")
        || volume_id.contains("CCCOMA") || volume_id.contains("GSP1")
        || volume_id.starts_with("SW_DVD") || volume_id.starts_with("SSS_X")
        || volume_id.starts_with("X1") || volume_id.starts_with("J_CCSA")
        || (all.contains("X64FRE") || all.contains("X86FRE"))
    {
        // Try to distinguish version from volume ID patterns
        if volume_id.contains("CCCOMA") || volume_id.contains("_DV9") {
            // Modern Windows 10/11 pattern
            return DetectedOs::Windows10;
        }
        return DetectedOs::WindowsUnknown;
    }

    // ── Linux distribution detection ──
    if all.contains("UBUNTU") {
        return DetectedOs::Ubuntu;
    }
    if all.contains("DEBIAN") {
        return DetectedOs::Debian;
    }
    if all.contains("FEDORA") {
        return DetectedOs::Fedora;
    }
    if all.contains("CENTOS") || all.contains("CENT OS") {
        return DetectedOs::CentOS;
    }
    if all.contains("ARCH") && all.contains("LINUX") {
        return DetectedOs::Arch;
    }
    if all.contains("ARCHLINUX") || volume_id.contains("ARCH_") {
        return DetectedOs::Arch;
    }
    if all.contains("OPENSUSE") || all.contains("SUSE") {
        return DetectedOs::OpenSuse;
    }
    if all.contains("LINUX MINT") || all.contains("LINUXMINT") {
        return DetectedOs::Mint;
    }
    if all.contains("FREEBSD") {
        return DetectedOs::FreeBsd;
    }

    // Generic Linux (broad match)
    if all.contains("LINUX") || all.contains("ISOLINUX") || all.contains("GRUB")
        || all.contains("VMLINUZ") || all.contains("CASPER") || all.contains("LIVE")
    {
        return DetectedOs::LinuxGeneric;
    }

    DetectedOs::Unknown
}
