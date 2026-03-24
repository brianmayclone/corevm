# CoreVM Appliance ISO — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a turnkey Debian 12 appliance ISO with a Rust/ratatui installer wizard and permanent DCUI console for CoreVM.

**Architecture:** A single Rust binary (`vmm-appliance`) serves both the installer wizard (`--mode installer`) and the DCUI (`--mode dcui`). The ISO is built via `debootstrap` + `squashfs-tools` + `xorriso` as a hybrid UEFI/BIOS image. Shared TUI widgets and system interaction code live in `common/`, with `installer/` and `dcui/` modules for each mode.

**Tech Stack:** Rust, ratatui, crossterm, sysinfo, nix, serde/toml, debootstrap, squashfs-tools, xorriso, isolinux, GRUB

**Spec:** `docs/superpowers/specs/2026-03-24-appliance-iso-design.md`

---

## File Structure

```
apps/vmm-appliance/
├── Cargo.toml
└── src/
    ├── main.rs              # CLI arg parsing (--mode installer|dcui), terminal setup
    ├── installer/
    │   ├── mod.rs           # InstallerApp: screen state machine, navigation
    │   ├── welcome.rs       # Screen 1: logo, role selection, language
    │   ├── disk.rs          # Screen 2: disk detection, selection, partition preview
    │   ├── network.rs       # Screen 3: interface list, DHCP/static, hostname
    │   ├── timezone.rs      # Screen 4: timezone picker, NTP toggle + server
    │   ├── users.rs         # Screen 5: root password, standard user
    │   ├── ports.rs         # Screen 6: service port configuration
    │   ├── certs.rs         # Screen 7: self-signed or import certs
    │   ├── summary.rs       # Screen 8: review all settings
    │   └── progress.rs      # Screen 9+10: execute installation, completion
    ├── dcui/
    │   ├── mod.rs           # DcuiApp: main loop, F-key dispatch, layout
    │   ├── status.rs        # Status bar: hostname, IP, uptime, CPU/RAM, service status
    │   ├── network.rs       # F1: network config dialog
    │   ├── passwords.rs     # F2: password change dialog
    │   ├── ports.rs         # F3: port edit dialog
    │   ├── certs.rs         # F4: certificate management dialog
    │   ├── services.rs      # F5: service control dialog
    │   ├── time.rs          # F6: timezone + NTP dialog
    │   ├── logs.rs          # F7: log viewer
    │   ├── update.rs        # F8: update package dialog
    │   ├── shell.rs         # F9: shell escape
    │   ├── reboot.rs        # F10: reboot/shutdown dialog
    │   ├── diagnostics.rs   # F11: system diagnostics
    │   └── reset.rs         # F12: factory reset
    └── common/
        ├── mod.rs           # Re-exports
        ├── widgets.rs       # Reusable TUI components: text input, password input, select list, confirm dialog, progress bar, table
        ├── config.rs        # Read/write /etc/vmm/*.toml, appliance config (/etc/vmm/appliance.toml)
        ├── system.rs        # Disk ops (parted, mkfs), mount, chroot, grub-install, user management
        ├── network.rs       # systemd-networkd config read/write, networkctl, interface detection
        ├── firewall.rs      # nftables rule generation and application
        └── certs.rs         # OpenSSL cert generation, cert file management
tools/
├── build-iso.sh             # ISO build script (debootstrap, squashfs, xorriso)
├── iso/
│   ├── grub.cfg             # GRUB config for ISO boot (UEFI)
│   ├── isolinux.cfg         # isolinux config for ISO boot (BIOS)
│   ├── grub-installed.cfg   # GRUB config template for installed system
│   └── nftables.conf        # Default nftables ruleset template
```

---

## Task 1: Scaffold vmm-appliance Crate

**Files:**
- Create: `apps/vmm-appliance/Cargo.toml`
- Create: `apps/vmm-appliance/src/main.rs`
- Create: `apps/vmm-appliance/src/common/mod.rs`
- Create: `apps/vmm-appliance/src/installer/mod.rs`
- Create: `apps/vmm-appliance/src/dcui/mod.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create Cargo.toml for vmm-appliance**

```toml
[package]
name = "vmm-appliance"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "vmm-appliance"
path = "src/main.rs"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
sysinfo = "0.33"
nix = { version = "0.29", features = ["mount", "reboot", "user", "fs", "process", "signal"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
clap = { version = "4", features = ["derive"] }
chrono = "0.4"
anyhow = "1"
```

- [ ] **Step 2: Create main.rs with CLI arg parsing and mode dispatch**

```rust
use clap::Parser;

mod common;
mod dcui;
mod installer;

#[derive(Parser)]
#[command(name = "vmm-appliance", about = "CoreVM Appliance Installer & DCUI")]
struct Cli {
    #[arg(long, value_enum)]
    mode: Mode,
}

#[derive(Clone, clap::ValueEnum)]
enum Mode {
    Installer,
    Dcui,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.mode {
        Mode::Installer => installer::run()?,
        Mode::Dcui => dcui::run()?,
    }
    Ok(())
}
```

- [ ] **Step 3: Create stub modules**

`src/common/mod.rs`:
```rust
pub mod widgets;
pub mod config;
pub mod system;
pub mod network;
pub mod firewall;
pub mod certs;
```

Create empty files for each submodule (`widgets.rs`, `config.rs`, `system.rs`, `network.rs`, `firewall.rs`, `certs.rs`).

`src/installer/mod.rs`:
```rust
pub fn run() -> anyhow::Result<()> {
    todo!("installer not yet implemented")
}
```

`src/dcui/mod.rs`:
```rust
pub fn run() -> anyhow::Result<()> {
    todo!("dcui not yet implemented")
}
```

- [ ] **Step 4: Add vmm-appliance to workspace**

In root `Cargo.toml`, add `"apps/vmm-appliance"` to the `members` list.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p vmm-appliance`
Expected: Compiles with no errors (warnings about unused/todo are OK).

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-appliance/ Cargo.toml
git commit -m "feat(appliance): scaffold vmm-appliance crate with CLI arg parsing"
```

---

## Task 2: Common TUI Widgets

**Files:**
- Create: `apps/vmm-appliance/src/common/widgets.rs`

Reusable components used by both installer and DCUI: text input, password input, select list, confirm dialog, progress bar. These are ratatui stateful widgets.

- [ ] **Step 1: Implement TextInput widget**

```rust
use ratatui::prelude::*;
use ratatui::widgets::*;
use crossterm::event::KeyEvent;

pub struct TextInput {
    pub label: String,
    pub value: String,
    pub cursor: usize,
    pub focused: bool,
}

impl TextInput {
    pub fn new(label: &str) -> Self { /* ... */ }
    pub fn handle_key(&mut self, key: KeyEvent) { /* handle char input, backspace, delete, arrows */ }
    pub fn render(&self, area: Rect, buf: &mut Buffer) { /* render label + input box with cursor */ }
}
```

- [ ] **Step 2: Implement PasswordInput widget**

Same as TextInput but renders `*` instead of actual characters. Internally stores the real value.

```rust
pub struct PasswordInput {
    inner: TextInput,
}

impl PasswordInput {
    pub fn new(label: &str) -> Self { /* ... */ }
    pub fn handle_key(&mut self, key: KeyEvent) { self.inner.handle_key(key); }
    pub fn value(&self) -> &str { &self.inner.value }
    pub fn render(&self, area: Rect, buf: &mut Buffer) { /* render with masked chars */ }
}
```

- [ ] **Step 3: Implement SelectList widget**

```rust
pub struct SelectList {
    pub label: String,
    pub items: Vec<String>,
    pub selected: usize,
    pub focused: bool,
}

impl SelectList {
    pub fn new(label: &str, items: Vec<String>) -> Self { /* ... */ }
    pub fn handle_key(&mut self, key: KeyEvent) { /* Up/Down to navigate, Enter to select */ }
    pub fn selected_item(&self) -> Option<&str> { /* ... */ }
    pub fn render(&self, area: Rect, buf: &mut Buffer) { /* render highlighted list */ }
}
```

- [ ] **Step 4: Implement ConfirmDialog widget**

```rust
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub confirmed: Option<bool>,
    selected_yes: bool,
}

impl ConfirmDialog {
    pub fn new(title: &str, message: &str) -> Self { /* ... */ }
    pub fn handle_key(&mut self, key: KeyEvent) { /* Left/Right to toggle, Enter to confirm */ }
    pub fn render(&self, area: Rect, buf: &mut Buffer) { /* centered popup with Yes/No */ }
}
```

- [ ] **Step 5: Implement ProgressBar widget**

```rust
pub struct ProgressDisplay {
    pub label: String,
    pub progress: f64, // 0.0 .. 1.0
    pub status_text: String,
}

impl ProgressDisplay {
    pub fn new(label: &str) -> Self { /* ... */ }
    pub fn set_progress(&mut self, pct: f64, text: &str) { /* ... */ }
    pub fn render(&self, area: Rect, buf: &mut Buffer) { /* render gauge + status text */ }
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 7: Commit**

```bash
git add apps/vmm-appliance/src/common/widgets.rs
git commit -m "feat(appliance): add reusable TUI widgets (text input, select, confirm, progress)"
```

---

## Task 3: Common System Interaction Layer

**Files:**
- Create: `apps/vmm-appliance/src/common/system.rs`
- Create: `apps/vmm-appliance/src/common/network.rs`
- Create: `apps/vmm-appliance/src/common/firewall.rs`
- Create: `apps/vmm-appliance/src/common/certs.rs`
- Create: `apps/vmm-appliance/src/common/config.rs`

These modules wrap system commands and file operations. They are the only modules that shell out or write to `/etc/`.

- [ ] **Step 1: Implement system.rs — disk operations**

```rust
use std::process::Command;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub path: PathBuf,    // e.g. /dev/sda
    pub size_bytes: u64,
    pub model: String,
}

pub fn detect_disks() -> anyhow::Result<Vec<DiskInfo>> {
    // Parse `lsblk -Jb -o NAME,SIZE,MODEL,TYPE` for TYPE=disk
}

pub fn partition_disk(disk: &Path, efi: bool) -> anyhow::Result<()> {
    // Run parted to create GPT table with:
    // - EFI partition (256MB, fat32) if efi=true
    // - /boot (512MB, ext4)
    // - swap (min(RAM, 8GB))
    // - / (50GB, ext4)
    // - /var/lib/vmm (remainder, ext4)
}

pub fn format_partitions(disk: &Path, efi: bool) -> anyhow::Result<()> {
    // mkfs.vfat, mkfs.ext4, mkswap for each partition
}

pub fn mount_target(disk: &Path, target: &Path, efi: bool) -> anyhow::Result<()> {
    // Mount / to target, then /boot, /boot/efi, /var/lib/vmm under target
}

pub fn unmount_target(target: &Path) -> anyhow::Result<()> {
    // Unmount in reverse order
}

pub fn extract_rootfs(tarball: &Path, target: &Path) -> anyhow::Result<()> {
    // tar xzf rootfs.tar.gz -C target
}

pub fn install_grub(target: &Path, disk: &Path, efi: bool) -> anyhow::Result<()> {
    // chroot into target, run grub-install + grub-mkconfig
}

pub fn configure_fstab(target: &Path, disk: &Path, efi: bool) -> anyhow::Result<()> {
    // Write /etc/fstab with UUIDs from blkid
}

pub fn create_user(target: &Path, username: &str, password: &str, sudo: bool) -> anyhow::Result<()> {
    // chroot useradd + chpasswd
}

pub fn set_root_password(target: &Path, password: &str) -> anyhow::Result<()> {
    // chroot chpasswd for root
}

pub fn set_hostname(target: &Path, hostname: &str) -> anyhow::Result<()> {
    // Write /etc/hostname
}

pub fn set_locale(target: &Path, locale: &str) -> anyhow::Result<()> {
    // Write /etc/default/locale, run locale-gen in chroot
}

pub fn configure_chrony(target: &Path, ntp_enabled: bool, ntp_server: &str) -> anyhow::Result<()> {
    // Write /etc/chrony/chrony.conf
}

pub fn set_timezone(target: &Path, timezone: &str) -> anyhow::Result<()> {
    // ln -sf /usr/share/zoneinfo/{tz} /etc/localtime in chroot, write /etc/timezone
}

pub fn is_efi_booted() -> bool {
    Path::new("/sys/firmware/efi").exists()
}

pub fn reboot() -> anyhow::Result<()> {
    // nix::sys::reboot::reboot(RebootMode::RB_AUTOBOOT)
}

pub fn get_ram_bytes() -> u64 {
    // sysinfo::System to get total memory
}
```

- [ ] **Step 2: Implement network.rs — systemd-networkd interaction**

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct NetworkInterface {
    pub name: String,
    pub mac: String,
    pub has_link: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkConfig {
    pub interface: String,
    pub dhcp: bool,
    pub address: Option<String>,    // e.g. "192.168.1.50/24"
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub hostname: String,
}

pub fn detect_interfaces() -> anyhow::Result<Vec<NetworkInterface>> {
    // Read /sys/class/net/*, filter out lo, read address from /sys/class/net/{name}/address
    // Check carrier for link state
}

pub fn write_networkd_config(target: &Path, config: &NetworkConfig) -> anyhow::Result<()> {
    // Write /etc/systemd/network/10-management.network
    // [Match] Name={interface}
    // [Network] DHCP=yes | Address=... Gateway=... DNS=...
}

pub fn apply_networkd_config() -> anyhow::Result<()> {
    // networkctl reload
}

pub fn read_current_ip(interface: &str) -> anyhow::Result<Option<String>> {
    // Parse `ip -j addr show {interface}` for first IPv4 address
}
```

- [ ] **Step 3: Implement firewall.rs — nftables**

```rust
pub struct FirewallConfig {
    pub ssh_port: u16,
    pub vmm_server_port: Option<u16>,
    pub vmm_cluster_port: Option<u16>,
}

pub fn write_nftables_config(target: &Path, config: &FirewallConfig) -> anyhow::Result<()> {
    // Write /etc/nftables.conf with:
    // table inet filter { chain input { ... accept ssh, vmm ports, icmp; drop } }
}

pub fn apply_nftables() -> anyhow::Result<()> {
    // systemctl restart nftables
}
```

- [ ] **Step 4: Implement certs.rs — TLS certificate management**

```rust
pub fn generate_self_signed(target: &Path, cn: &str) -> anyhow::Result<(PathBuf, PathBuf)> {
    // openssl req -x509 -newkey rsa:4096 -sha256 -days 3650 -nodes
    //   -keyout /etc/vmm/tls/server.key -out /etc/vmm/tls/server.crt
    //   -subj "/CN={cn}"
    // Returns (cert_path, key_path)
}

pub fn import_certificates(target: &Path, cert_src: &Path, key_src: &Path) -> anyhow::Result<()> {
    // Copy cert and key to /etc/vmm/tls/
}
```

- [ ] **Step 5: Implement config.rs — appliance config read/write**

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApplianceConfig {
    pub role: ApplianceRole,
    pub language: String,      // "en" or "de"
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApplianceRole {
    Server,
    Cluster,
}

impl ApplianceConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> { /* read + toml::from_str */ }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> { /* toml::to_string_pretty + write */ }
}

/// Generate vmm-server.toml with given parameters
pub fn write_vmm_server_config(
    target: &Path, port: u16, data_dir: &str, log_file: &str,
) -> anyhow::Result<()> { /* ... */ }

/// Generate vmm-cluster.toml with given parameters
pub fn write_vmm_cluster_config(
    target: &Path, port: u16, data_dir: &str, log_file: &str,
) -> anyhow::Result<()> { /* ... */ }

/// Generate default vmm-server.toml or vmm-cluster.toml (for factory reset)
pub fn write_default_config(target: &Path, role: &ApplianceRole) -> anyhow::Result<()> { /* ... */ }
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 7: Commit**

```bash
git add apps/vmm-appliance/src/common/
git commit -m "feat(appliance): add system interaction layer (disk, network, firewall, certs, config)"
```

---

## Task 4: Installer — Screen State Machine & Welcome Screen

**Files:**
- Modify: `apps/vmm-appliance/src/installer/mod.rs`
- Create: `apps/vmm-appliance/src/installer/welcome.rs`

- [ ] **Step 1: Implement the installer state machine in mod.rs**

```rust
use crossterm::event::{self, Event, KeyCode};
use ratatui::prelude::*;
use anyhow::Result;

mod welcome;
mod disk;
mod network;
mod timezone;
mod users;
mod ports;
mod certs;
mod summary;
mod progress;

use crate::common::config::ApplianceRole;

/// All settings collected during installation
#[derive(Debug, Default, Clone)]
pub struct InstallConfig {
    pub role: Option<ApplianceRole>,
    pub language: String,             // "en" or "de"
    pub disk: Option<std::path::PathBuf>,
    pub network: Option<crate::common::network::NetworkConfig>,
    pub timezone: String,
    pub ntp_enabled: bool,
    pub ntp_server: String,
    pub root_password: String,
    pub username: String,
    pub user_password: String,
    pub server_port: u16,
    pub cluster_port: u16,
    pub self_signed_cert: bool,
    pub cert_path: Option<std::path::PathBuf>,
    pub key_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Screen {
    Welcome,
    Disk,
    Network,
    Timezone,
    Users,
    Ports,
    Certs,
    Summary,
    Progress,
}

impl Screen {
    fn next(&self) -> Option<Screen> {
        match self {
            Screen::Welcome => Some(Screen::Disk),
            Screen::Disk => Some(Screen::Network),
            Screen::Network => Some(Screen::Timezone),
            Screen::Timezone => Some(Screen::Users),
            Screen::Users => Some(Screen::Ports),
            Screen::Ports => Some(Screen::Certs),
            Screen::Certs => Some(Screen::Summary),
            Screen::Summary => Some(Screen::Progress),
            Screen::Progress => None,
        }
    }
    fn prev(&self) -> Option<Screen> {
        match self {
            Screen::Welcome => None,
            Screen::Disk => Some(Screen::Welcome),
            Screen::Network => Some(Screen::Disk),
            Screen::Timezone => Some(Screen::Network),
            Screen::Users => Some(Screen::Timezone),
            Screen::Ports => Some(Screen::Users),
            Screen::Certs => Some(Screen::Ports),
            Screen::Summary => Some(Screen::Certs),
            Screen::Progress => None, // can't go back during installation
        }
    }
}

pub fn run() -> Result<()> {
    let mut terminal = ratatui::init(); // ratatui::init() already enables raw mode

    let mut screen = Screen::Welcome;
    let mut config = InstallConfig::default();
    config.server_port = 8443;
    config.cluster_port = 9443;
    config.ntp_server = "pool.ntp.org".into();
    config.self_signed_cert = true;
    config.language = "en".into();

    // Per-screen state objects
    let mut welcome_state = welcome::WelcomeState::new();
    // ... other screen states initialized lazily

    loop {
        terminal.draw(|frame| {
            match screen {
                Screen::Welcome => welcome_state.render(frame, &config),
                // ... other screens (each screen state has a render method)
                _ => {}
            }
        })?;

        // Poll with timeout so progress screen can update without keypresses
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                // On Progress screen, also poll rx before handling key
                let result = match screen {
                    Screen::Welcome => welcome_state.handle_key(key, &mut config),
                    // ... other screens
                    _ => ScreenResult::Continue,
                };
                match result {
                    ScreenResult::Continue => {}
                    ScreenResult::Next => {
                        if let Some(next) = screen.next() { screen = next; }
                    }
                    ScreenResult::Prev => {
                        if let Some(prev) = screen.prev() { screen = prev; }
                    }
                    ScreenResult::Quit => break,
                }
            }
        } else {
            // Timeout — let progress screen poll its channel
            // (handle_key is also called with a synthetic tick for progress updates)
        }
    }

    ratatui::restore();
    Ok(())
}

pub enum ScreenResult {
    Continue,
    Next,
    Prev,
    Quit,
}
```

- [ ] **Step 2: Implement welcome.rs — Screen 1**

```rust
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;
use crate::common::config::ApplianceRole;
use crate::common::widgets::SelectList;
use super::{InstallConfig, ScreenResult};

pub struct WelcomeState {
    role_select: SelectList,
    lang_select: SelectList,
    focus: WelcomeFocus,
}

enum WelcomeFocus {
    Role,
    Language,
}

impl WelcomeState {
    pub fn new() -> Self {
        Self {
            role_select: SelectList::new("Role", vec![
                "Standalone Server".into(),
                "Cluster Controller".into(),
            ]),
            lang_select: SelectList::new("Language", vec![
                "English".into(),
                "Deutsch".into(),
            ]),
            focus: WelcomeFocus::Role,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match key.code {
            KeyCode::Esc => ScreenResult::Quit,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    WelcomeFocus::Role => WelcomeFocus::Language,
                    WelcomeFocus::Language => WelcomeFocus::Role,
                };
                ScreenResult::Continue
            }
            KeyCode::Enter => {
                config.role = Some(match self.role_select.selected {
                    0 => ApplianceRole::Server,
                    _ => ApplianceRole::Cluster,
                });
                config.language = match self.lang_select.selected {
                    0 => "en".into(),
                    _ => "de".into(),
                };
                ScreenResult::Next
            }
            _ => {
                match self.focus {
                    WelcomeFocus::Role => self.role_select.handle_key(key),
                    WelcomeFocus::Language => self.lang_select.handle_key(key),
                }
                ScreenResult::Continue
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(8),  // logo
            Constraint::Length(3),  // spacer
            Constraint::Min(10),   // selections
            Constraint::Length(2), // help line
        ]).split(area);

        // Logo banner
        let logo = Paragraph::new(vec![
            Line::from("╔═══════════════════════════════════════╗"),
            Line::from("║         CoreVM Appliance v0.1         ║"),
            Line::from("║     Hypervisor Management System      ║"),
            Line::from("╚═══════════════════════════════════════╝"),
        ]).alignment(Alignment::Center);
        frame.render_widget(logo, chunks[0]);

        // Role + Language side by side
        let cols = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ]).split(chunks[2]);

        self.role_select.render(cols[0], frame.buffer_mut());
        self.lang_select.render(cols[1], frame.buffer_mut());

        // Help line
        let help = Paragraph::new("[Tab] Switch field  [↑↓] Select  [Enter] Continue  [Esc] Quit")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[3]);
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-appliance/src/installer/
git commit -m "feat(appliance): installer state machine and welcome screen"
```

---

## Task 5: Installer — Disk Selection Screen

**Files:**
- Create: `apps/vmm-appliance/src/installer/disk.rs`

- [ ] **Step 1: Implement disk.rs — Screen 2**

```rust
use crate::common::system::{detect_disks, DiskInfo, is_efi_booted, get_ram_bytes};
use crate::common::widgets::{SelectList, ConfirmDialog};
use super::{InstallConfig, ScreenResult};

pub struct DiskState {
    disks: Vec<DiskInfo>,
    disk_select: SelectList,
    confirm: Option<ConfirmDialog>,
    error: Option<String>,
}

impl DiskState {
    pub fn new() -> Self {
        let disks = detect_disks().unwrap_or_default();
        let items: Vec<String> = disks.iter().map(|d| {
            let gb = d.size_bytes / 1_073_741_824;
            format!("{} — {} GB — {}", d.path.display(), gb, d.model)
        }).collect();
        Self {
            disk_select: SelectList::new("Select target disk", items),
            disks,
            confirm: None,
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        // If confirm dialog is active, handle it
        if let Some(ref mut dlg) = self.confirm {
            dlg.handle_key(key);
            if let Some(true) = dlg.confirmed {
                config.disk = Some(self.disks[self.disk_select.selected].path.clone());
                self.confirm = None;
                return ScreenResult::Next;
            } else if let Some(false) = dlg.confirmed {
                self.confirm = None;
                return ScreenResult::Continue;
            }
            return ScreenResult::Continue;
        }

        match key.code {
            KeyCode::Esc => ScreenResult::Prev,
            KeyCode::Enter => {
                if self.disks.is_empty() {
                    self.error = Some("No disks detected".into());
                    return ScreenResult::Continue;
                }
                let disk = &self.disks[self.disk_select.selected];
                let gb = disk.size_bytes / 1_073_741_824;
                let efi = is_efi_booted();
                let ram_gb = get_ram_bytes() / 1_073_741_824;
                let swap_gb = ram_gb.min(8);

                // Show partition preview in confirm message
                let mut msg = format!(
                    "ALL DATA on {} will be erased!\n\nPartition layout:\n",
                    disk.path.display()
                );
                if efi {
                    msg.push_str("  /boot/efi  — 256 MB (FAT32)\n");
                }
                msg.push_str(&format!("  /boot      — 512 MB (ext4)\n"));
                msg.push_str(&format!("  swap       — {} GB\n", swap_gb));
                msg.push_str(&format!("  /          — 50 GB (ext4)\n"));
                let data_gb = gb.saturating_sub(if efi { 1 } else { 0 } + 1 + swap_gb + 50);
                msg.push_str(&format!("  /var/lib/vmm — {} GB (ext4)\n", data_gb));

                self.confirm = Some(ConfirmDialog::new("Confirm", &msg));
                ScreenResult::Continue
            }
            _ => {
                self.disk_select.handle_key(key);
                ScreenResult::Continue
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        // Render disk list + optional confirm overlay + optional error
    }
}
```

- [ ] **Step 2: Wire into installer mod.rs** (add DiskState to the state machine loop)

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-appliance/src/installer/disk.rs apps/vmm-appliance/src/installer/mod.rs
git commit -m "feat(appliance): installer disk selection screen with partition preview"
```

---

## Task 6: Installer — Network, Timezone, Users Screens

**Files:**
- Create: `apps/vmm-appliance/src/installer/network.rs`
- Create: `apps/vmm-appliance/src/installer/timezone.rs`
- Create: `apps/vmm-appliance/src/installer/users.rs`

- [ ] **Step 1: Implement network.rs — Screen 3**

Uses `SelectList` for interface, `SelectList` for DHCP/Static, `TextInput` for IP/gateway/DNS/hostname. Tab to switch fields, Esc to go back, Enter to advance.

- [ ] **Step 2: Implement timezone.rs — Screen 4**

Two `SelectList`s: continent first, then city. A toggle for NTP enable, `TextInput` for NTP server. Timezone data from `/usr/share/zoneinfo/` directory listing.

- [ ] **Step 3: Implement users.rs — Screen 5**

`PasswordInput` x2 for root (password + confirm). `TextInput` for username, `PasswordInput` x2 for user password. Validation: passwords must match, username non-empty.

- [ ] **Step 4: Wire all three into mod.rs**

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-appliance/src/installer/
git commit -m "feat(appliance): installer network, timezone, and users screens"
```

---

## Task 7: Installer — Ports, Certs, Summary Screens

**Files:**
- Create: `apps/vmm-appliance/src/installer/ports.rs`
- Create: `apps/vmm-appliance/src/installer/certs.rs`
- Create: `apps/vmm-appliance/src/installer/summary.rs`

- [ ] **Step 1: Implement ports.rs — Screen 6**

`TextInput` for server port (default 8443). If role=Cluster, also `TextInput` for cluster port (default 9443). Validate: numeric, 1-65535, not already taken by other field.

- [ ] **Step 2: Implement certs.rs — Screen 7**

`SelectList` with two options: "Generate self-signed" / "Import custom". If import: two `TextInput` for cert path and key path. Validate files exist (in live env).

- [ ] **Step 3: Implement summary.rs — Screen 8**

Read-only display of all `InstallConfig` fields in a table. Two buttons: "Start Installation" (Enter) / "Go Back" (Esc). Show all values clearly formatted.

- [ ] **Step 4: Wire into mod.rs**

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-appliance/src/installer/
git commit -m "feat(appliance): installer ports, certs, and summary screens"
```

---

## Task 8: Installer — Progress Screen (Actual Installation)

**Files:**
- Create: `apps/vmm-appliance/src/installer/progress.rs`

This is the core — it actually partitions, formats, extracts, configures, and installs.

- [ ] **Step 1: Implement progress.rs — Screen 9+10**

```rust
use std::sync::mpsc;
use std::thread;
use crate::common::{system, network, firewall, certs, config};
use super::{InstallConfig, ScreenResult};

pub struct ProgressState {
    progress: f64,
    status: String,
    done: bool,
    error: Option<String>,
    web_url: Option<String>,
    rx: Option<mpsc::Receiver<ProgressMsg>>,
}

enum ProgressMsg {
    Update(f64, String),
    Done(String), // web_url
    Error(String),
}

impl ProgressState {
    pub fn start(config: &InstallConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let cfg = config.clone(); // InstallConfig needs Clone

        thread::spawn(move || {
            if let Err(e) = run_installation(&cfg, &tx) {
                let _ = tx.send(ProgressMsg::Error(e.to_string()));
            }
        });

        Self { progress: 0.0, status: "Starting...".into(), done: false, error: None, web_url: None, rx: Some(rx) }
    }

    /// Call this every tick (even without keypresses) to poll progress updates
    pub fn tick(&mut self) {
        if let Some(ref rx) = self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    ProgressMsg::Update(pct, text) => { self.progress = pct; self.status = text; }
                    ProgressMsg::Done(url) => { self.done = true; self.web_url = Some(url); }
                    ProgressMsg::Error(e) => { self.error = Some(e); }
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, _config: &mut InstallConfig) -> ScreenResult {
        self.tick();
        if self.done {
            if key.code == KeyCode::Enter {
                let _ = system::reboot();
            }
        }
        ScreenResult::Continue
    }
}

fn run_installation(cfg: &InstallConfig, tx: &mpsc::Sender<ProgressMsg>) -> anyhow::Result<()> {
    let target = std::path::Path::new("/mnt/target");
    let disk = cfg.disk.as_ref().unwrap();
    let efi = system::is_efi_booted();

    tx.send(ProgressMsg::Update(0.05, "Partitioning disk...".into()))?;
    system::partition_disk(disk, efi)?;

    tx.send(ProgressMsg::Update(0.10, "Formatting partitions...".into()))?;
    system::format_partitions(disk, efi)?;

    tx.send(ProgressMsg::Update(0.15, "Mounting target...".into()))?;
    std::fs::create_dir_all(target)?;
    system::mount_target(disk, target, efi)?;

    tx.send(ProgressMsg::Update(0.20, "Extracting Debian root filesystem...".into()))?;
    // rootfs.tar.gz is on the live ISO at /opt/vmm/rootfs.tar.gz
    system::extract_rootfs(std::path::Path::new("/opt/vmm/rootfs.tar.gz"), target)?;

    tx.send(ProgressMsg::Update(0.50, "Configuring system...".into()))?;
    system::set_hostname(target, &cfg.network.as_ref().unwrap().hostname)?;
    system::set_locale(target, if cfg.language == "de" { "de_DE.UTF-8" } else { "en_US.UTF-8" })?;
    system::set_timezone(target, &cfg.timezone)?;
    system::configure_fstab(target, disk, efi)?;

    tx.send(ProgressMsg::Update(0.55, "Configuring network...".into()))?;
    network::write_networkd_config(target, cfg.network.as_ref().unwrap())?;

    tx.send(ProgressMsg::Update(0.60, "Configuring NTP...".into()))?;
    system::configure_chrony(target, cfg.ntp_enabled, &cfg.ntp_server)?;

    tx.send(ProgressMsg::Update(0.65, "Creating users...".into()))?;
    system::set_root_password(target, &cfg.root_password)?;
    system::create_user(target, &cfg.username, &cfg.user_password, true)?;

    tx.send(ProgressMsg::Update(0.70, "Generating certificates...".into()))?;
    let hostname = &cfg.network.as_ref().unwrap().hostname;
    if cfg.self_signed_cert {
        certs::generate_self_signed(target, hostname)?;
    } else {
        certs::import_certificates(target, cfg.cert_path.as_ref().unwrap(), cfg.key_path.as_ref().unwrap())?;
    }

    tx.send(ProgressMsg::Update(0.75, "Writing service configuration...".into()))?;
    let role = cfg.role.as_ref().unwrap();
    match role {
        config::ApplianceRole::Server => {
            config::write_vmm_server_config(target, cfg.server_port, "/var/lib/vmm", "/var/log/vmm-server.log")?;
        }
        config::ApplianceRole::Cluster => {
            config::write_vmm_cluster_config(target, cfg.cluster_port, "/var/lib/vmm-cluster", "/var/log/vmm-cluster.log")?;
        }
    }
    config::ApplianceConfig {
        role: role.clone(),
        language: cfg.language.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
    }.save(&target.join("etc/vmm/appliance.toml"))?;

    tx.send(ProgressMsg::Update(0.80, "Configuring firewall...".into()))?;
    firewall::write_nftables_config(target, &firewall::FirewallConfig {
        ssh_port: 22,
        vmm_server_port: Some(cfg.server_port),
        vmm_cluster_port: if *role == config::ApplianceRole::Cluster { Some(cfg.cluster_port) } else { None },
    })?;

    tx.send(ProgressMsg::Update(0.85, "Configuring SSH...".into()))?;
    // Disable root SSH login: set PermitRootLogin no in sshd_config
    let sshd_config = target.join("etc/ssh/sshd_config.d/10-corevm.conf");
    std::fs::write(&sshd_config, "PermitRootLogin no\n")?;

    tx.send(ProgressMsg::Update(0.90, "Installing GRUB bootloader...".into()))?;
    system::install_grub(target, disk, efi)?;

    tx.send(ProgressMsg::Update(0.95, "Finalizing...".into()))?;
    system::unmount_target(target)?;

    let ip = cfg.network.as_ref().and_then(|n| n.address.as_ref())
        .map(|a| a.split('/').next().unwrap_or("").to_string())
        .unwrap_or_else(|| "<DHCP>".into());
    let port = if *role == config::ApplianceRole::Cluster { cfg.cluster_port } else { cfg.server_port };
    let url = format!("https://{}:{}", ip, port);

    tx.send(ProgressMsg::Done(url))?;
    Ok(())
}
```

- [ ] **Step 2: Wire into mod.rs — create ProgressState when entering Screen::Progress**

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-appliance/src/installer/progress.rs apps/vmm-appliance/src/installer/mod.rs
git commit -m "feat(appliance): installer progress screen with full installation pipeline"
```

---

## Task 9: DCUI — Main Loop, Status Bar & Menu

**Files:**
- Modify: `apps/vmm-appliance/src/dcui/mod.rs`
- Create: `apps/vmm-appliance/src/dcui/status.rs`

- [ ] **Step 1: Implement status.rs — status bar**

```rust
use sysinfo::System;
use std::process::Command;
use std::time::Instant;
use ratatui::prelude::*;
use ratatui::widgets::*;
use crate::common::config::{ApplianceConfig, ApplianceRole};

pub struct StatusBar {
    sys: System,
    last_refresh: Instant,
    hostname: String,
    ip: String,
    role: String,
    service_name: String,
    service_status: String,
    port: u16,
    version: String,
}

impl StatusBar {
    pub fn new() -> Self {
        let config = ApplianceConfig::load(std::path::Path::new("/etc/vmm/appliance.toml"))
            .unwrap_or_else(|_| ApplianceConfig {
                role: ApplianceRole::Server,
                language: "en".into(),
                version: "unknown".into(),
            });

        let service_name = match config.role {
            ApplianceRole::Server => "vmm-server",
            ApplianceRole::Cluster => "vmm-cluster",
        };

        Self {
            sys: System::new_all(),
            last_refresh: Instant::now(),
            hostname: String::new(),
            ip: String::new(),
            role: match config.role {
                ApplianceRole::Server => "Standalone Server".into(),
                ApplianceRole::Cluster => "Cluster Controller".into(),
            },
            service_name: service_name.into(),
            service_status: String::new(),
            port: 0, // read from config
            version: config.version,
        }
    }

    pub fn refresh(&mut self) {
        if self.last_refresh.elapsed().as_secs() < 5 { return; }
        self.last_refresh = Instant::now();
        self.sys.refresh_all();

        // Read hostname
        self.hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_default().trim().to_string();

        // Read IP from primary interface
        self.ip = crate::common::network::read_current_ip("")
            .ok().flatten().unwrap_or_else(|| "no IP".into());

        // Check service status
        self.service_status = Command::new("systemctl")
            .args(["is-active", &self.service_name])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".into());
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let uptime_secs = System::uptime();
        let days = uptime_secs / 86400;
        let hours = (uptime_secs % 86400) / 3600;
        let mins = (uptime_secs % 3600) / 60;

        let cpu = self.sys.global_cpu_usage();
        let ram_used = self.sys.used_memory() as f64 / 1_073_741_824.0;
        let ram_total = self.sys.total_memory() as f64 / 1_073_741_824.0;

        let status_indicator = if self.service_status == "active" { "●" } else { "○" };

        let text = vec![
            Line::from(format!("  CoreVM Appliance v{}          Role: {}", self.version, self.role)),
            Line::from(format!("  Hostname: {:<24}Uptime: {}d {}h {}m", self.hostname, days, hours, mins)),
            Line::from(format!("  IP: {:<28}CPU: {:.0}% | RAM: {:.1}/{:.1}GB", self.ip, cpu, ram_used, ram_total)),
            Line::from(format!("  {}: {} {:<16}Port: {}", self.service_name, status_indicator, self.service_status, self.port)),
            Line::from(format!("  https://{}:{}", self.ip, self.port)),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().fg(Color::Cyan));
        let paragraph = Paragraph::new(text).block(block);
        paragraph.render(area, buf);
    }
}
```

- [ ] **Step 2: Implement dcui/mod.rs — main loop with F-key menu**

```rust
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::time::Duration;

mod status;
mod network;
mod passwords;
mod ports;
mod certs;
mod services;
mod time;
mod logs;
mod update;
mod shell;
mod reboot;
mod diagnostics;
mod reset;

use status::StatusBar;

enum ActiveDialog {
    None,
    Network(network::NetworkDialog),
    Passwords(passwords::PasswordDialog),
    Ports(ports::PortDialog),
    Certs(certs::CertDialog),
    Services(services::ServiceDialog),
    Time(time::TimeDialog),
    Logs(logs::LogViewer),
    Update(update::UpdateDialog),
    Reboot(reboot::RebootDialog),
    Diagnostics(diagnostics::DiagnosticsDialog),
    Reset(reset::ResetDialog),
}

pub fn run() -> anyhow::Result<()> {
    let mut terminal = ratatui::init(); // ratatui::init() already enables raw mode
    let mut status = StatusBar::new();
    let mut dialog = ActiveDialog::None;

    loop {
        status.refresh();

        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Length(7),   // status bar
                Constraint::Length(1),   // spacer
                Constraint::Min(14),     // menu
                Constraint::Length(1),   // help
            ]).split(area);

            status.render(chunks[0], frame.buffer_mut());
            render_menu(frame, chunks[2]);

            // Render active dialog as overlay if any
            match &dialog {
                ActiveDialog::None => {}
                // ... render each dialog type
                _ => {}
            }
        })?;

        // Poll with timeout so status bar refreshes
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                // If dialog active, route to dialog
                match &mut dialog {
                    ActiveDialog::None => {
                        match key.code {
                            KeyCode::F(1) => dialog = ActiveDialog::Network(network::NetworkDialog::new()),
                            KeyCode::F(2) => dialog = ActiveDialog::Passwords(passwords::PasswordDialog::new()),
                            KeyCode::F(3) => dialog = ActiveDialog::Ports(ports::PortDialog::new()),
                            KeyCode::F(4) => dialog = ActiveDialog::Certs(certs::CertDialog::new()),
                            KeyCode::F(5) => dialog = ActiveDialog::Services(services::ServiceDialog::new()),
                            KeyCode::F(6) => dialog = ActiveDialog::Time(time::TimeDialog::new()),
                            KeyCode::F(7) => dialog = ActiveDialog::Logs(logs::LogViewer::new()),
                            KeyCode::F(8) => dialog = ActiveDialog::Update(update::UpdateDialog::new()),
                            KeyCode::F(10) => dialog = ActiveDialog::Reboot(reboot::RebootDialog::new()),
                            KeyCode::F(9) => {
                                // Shell escape — restore terminal, spawn bash, re-init on return
                                ratatui::restore();
                                crossterm::terminal::disable_raw_mode()?;
                                let _ = std::process::Command::new("/bin/bash").status();
                                crossterm::terminal::enable_raw_mode()?;
                                terminal = ratatui::init();
                            }
                            KeyCode::F(11) => dialog = ActiveDialog::Diagnostics(diagnostics::DiagnosticsDialog::new()),
                            KeyCode::F(12) => dialog = ActiveDialog::Reset(reset::ResetDialog::new()),
                            _ => {}
                        }
                    }
                    // Route key to active dialog, close on Esc/Done
                    _ => {
                        // Each dialog returns DialogResult::Continue | Close
                        // On Close: dialog = ActiveDialog::None
                    }
                }
            }
        }
    }
}

fn render_menu(frame: &mut Frame, area: Rect) {
    let items = vec![
        Line::from(vec![
            Span::styled(" [F1] ", Style::default().fg(Color::Yellow).bold()),
            Span::raw("Network"),
        ]),
        Line::from(vec![
            Span::styled(" [F2] ", Style::default().fg(Color::Yellow).bold()),
            Span::raw("Passwords"),
        ]),
        // ... F3 through F12
    ];
    let menu = Paragraph::new(items)
        .block(Block::default().title(" Menu ").borders(Borders::ALL));
    frame.render_widget(menu, area);
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 4: Commit**

```bash
git add apps/vmm-appliance/src/dcui/
git commit -m "feat(appliance): DCUI main loop with status bar and F-key menu"
```

---

## Task 10: DCUI — Dialog Implementations (Network, Passwords, Ports, Certs, Services)

**Files:**
- Create: `apps/vmm-appliance/src/dcui/network.rs`
- Create: `apps/vmm-appliance/src/dcui/passwords.rs`
- Create: `apps/vmm-appliance/src/dcui/ports.rs`
- Create: `apps/vmm-appliance/src/dcui/certs.rs`
- Create: `apps/vmm-appliance/src/dcui/services.rs`

Each dialog follows the same pattern: popup overlay, own key handling, calls `common::` functions to apply changes.

- [ ] **Step 1: Implement network.rs (F1)**

Reads current config from `/etc/systemd/network/`, shows interface/IP/gateway/DNS/hostname fields. On save: writes new config, calls `networkctl reload`.

- [ ] **Step 2: Implement passwords.rs (F2)**

Select user (root or standard user from `/etc/vmm/appliance.toml` context), enter new password twice. On save: calls `chpasswd`.

- [ ] **Step 3: Implement ports.rs (F3)**

Read current port from `/etc/vmm/vmm-server.toml` or `vmm-cluster.toml`. Edit field, validate. On save: update TOML, update nftables, restart service.

- [ ] **Step 4: Implement certs.rs (F4)**

Two options: regenerate self-signed or import. On regenerate: calls `common::certs::generate_self_signed`. On import: file path inputs. Restarts service after.

- [ ] **Step 5: Implement services.rs (F5)**

Shows service status (active/inactive/failed). Buttons for start/stop/restart/enable/disable. Calls `systemctl` commands.

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 7: Commit**

```bash
git add apps/vmm-appliance/src/dcui/
git commit -m "feat(appliance): DCUI dialogs for network, passwords, ports, certs, services"
```

---

## Task 11: DCUI — Time, Logs, Update, Diagnostics, Reset

**Files:**
- Create: `apps/vmm-appliance/src/dcui/time.rs`
- Create: `apps/vmm-appliance/src/dcui/logs.rs`
- Create: `apps/vmm-appliance/src/dcui/update.rs`
- Create: `apps/vmm-appliance/src/dcui/shell.rs`
- Create: `apps/vmm-appliance/src/dcui/reboot.rs`
- Create: `apps/vmm-appliance/src/dcui/diagnostics.rs`
- Create: `apps/vmm-appliance/src/dcui/reset.rs`

- [ ] **Step 1: Implement time.rs (F6)**

Timezone select (read from `/usr/share/zoneinfo/`), NTP toggle, NTP server input, manual time set, "Sync now" button (`chronyc makestep`).

- [ ] **Step 2: Implement logs.rs (F7)**

Reads log file (path from config), displays last N lines, auto-scrolls. Up/Down/PgUp/PgDown to scroll. Esc to close. Uses `std::io::BufReader` + `seek` to tail.

- [ ] **Step 3: Implement update.rs (F8)**

Text input for update package path. Validates file exists, reads version header, checks compatibility. On confirm: progress bar, stops service, backup to `/opt/vmm-backup/`, extract, restart.

- [ ] **Step 4: Implement shell.rs (F9)**

Just a warning `ConfirmDialog`. On confirm: handled in dcui/mod.rs (terminal restore → bash → re-init). This file just provides the warning dialog.

- [ ] **Step 5: Implement reboot.rs (F10)**

Submenu with three options: "Reboot", "Shutdown", "Cancel". Each with a confirmation dialog. Calls `nix::sys::reboot::reboot()` with appropriate mode, or `Command::new("shutdown").args(["-h", "now"])`.

- [ ] **Step 6: Implement diagnostics.rs (F11)**

Collects and displays: OS info, kernel version, CPU/RAM/disk, hardware (lspci), network interfaces, ping gateway, DNS resolution test, service status, disk health (df -h). Read-only scrollable view.

- [ ] **Step 7: Implement reset.rs (F12)**

Double confirmation dialog. On confirm: stops services, deletes `/var/lib/vmm/vmm.db` (or cluster equivalent), writes default config via `common::config::write_default_config`, optionally prompts to delete VM images, restarts services.

- [ ] **Step 8: Verify it compiles**

Run: `cargo check -p vmm-appliance`

- [ ] **Step 9: Commit**

```bash
git add apps/vmm-appliance/src/dcui/
git commit -m "feat(appliance): DCUI dialogs for time, logs, update, reboot, diagnostics, reset"
```

---

## Task 12: ISO Build Script

**Files:**
- Create: `tools/build-iso.sh`
- Create: `tools/iso/grub.cfg`
- Create: `tools/iso/isolinux.cfg`
- Create: `tools/iso/grub-installed.cfg`
- Create: `tools/iso/nftables.conf`

- [ ] **Step 1: Create GRUB config for ISO boot (UEFI)**

`tools/iso/grub.cfg`:
```
set timeout=5
set default=0

menuentry "CoreVM Appliance Installer" {
    linux /live/vmlinuz boot=live toram quiet
    initrd /live/initrd.img
}
```

- [ ] **Step 2: Create isolinux config for ISO boot (BIOS)**

`tools/iso/isolinux.cfg`:
```
DEFAULT corevm
TIMEOUT 50
PROMPT 0

LABEL corevm
    MENU LABEL CoreVM Appliance Installer
    KERNEL /live/vmlinuz
    APPEND initrd=/live/initrd.img boot=live toram quiet
```

- [ ] **Step 3: Create GRUB config template for installed system**

`tools/iso/grub-installed.cfg`:
```
GRUB_DEFAULT=0
GRUB_TIMEOUT=2
GRUB_TIMEOUT_STYLE=hidden
GRUB_DISTRIBUTOR="CoreVM Appliance"
GRUB_CMDLINE_LINUX_DEFAULT="quiet"
GRUB_CMDLINE_LINUX=""
```

- [ ] **Step 4: Create default nftables ruleset**

`tools/iso/nftables.conf`:
```
#!/usr/sbin/nft -f
flush ruleset

table inet filter {
    chain input {
        type filter hook input priority 0; policy drop;
        ct state established,related accept
        iif lo accept
        icmp type echo-request accept
        icmpv6 type { echo-request, nd-neighbor-solicit, nd-router-advert, nd-neighbor-advert } accept
        tcp dport 22 accept
        tcp dport 8443 accept
        # tcp dport 9443 accept  # uncommented by installer if cluster role
    }
    chain forward {
        type filter hook forward priority 0; policy drop;
    }
    chain output {
        type filter hook output priority 0; policy accept;
    }
}
```

- [ ] **Step 5: Create build-iso.sh**

```bash
#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
BUILD_DIR="$ROOT/dist/iso-build"
ISO_OUTPUT="$ROOT/dist/corevm-appliance.iso"

# Check prerequisites
for cmd in debootstrap xorriso mksquashfs grub-mkimage mtools; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "Missing: $cmd"; exit 1; }
done

echo "=== Building CoreVM Appliance ISO ==="

# 0. Build all binaries
echo "[1/7] Building binaries..."
cd "$ROOT/apps/vmm-ui" && npm install && npx vite build
cd "$ROOT"
cargo build --release -p vmm-appliance -p vmm-server -p vmm-cluster

# 1. Build installable root-FS tarball
echo "[2/7] Building root filesystem tarball..."
ROOTFS_DIR="$BUILD_DIR/rootfs"
rm -rf "$ROOTFS_DIR"
sudo debootstrap --variant=minbase --include=\
linux-image-amd64,grub-pc,grub-efi-amd64-bin,systemd,\
systemd-resolved,openssh-server,openssl,chrony,parted,\
e2fsprogs,dosfstools,iproute2,sudo,ca-certificates,\
util-linux,pciutils,nftables,locales \
    bookworm "$ROOTFS_DIR" http://deb.debian.org/debian

# Copy CoreVM binaries
sudo mkdir -p "$ROOTFS_DIR/opt/vmm"
sudo cp "$ROOT/target/release/vmm-appliance" "$ROOTFS_DIR/opt/vmm/"
sudo cp "$ROOT/target/release/vmm-server" "$ROOTFS_DIR/opt/vmm/"
sudo cp "$ROOT/target/release/vmm-cluster" "$ROOTFS_DIR/opt/vmm/"
sudo cp -r "$ROOT/apps/vmm-ui/dist" "$ROOTFS_DIR/opt/vmm/ui"
sudo cp -r "$ROOT/apps/vmm-server/assets/bios" "$ROOTFS_DIR/opt/vmm/bios"

# Copy systemd service files
sudo mkdir -p "$ROOTFS_DIR/etc/systemd/system"
cat <<'DCUI_SVC' | sudo tee "$ROOTFS_DIR/etc/systemd/system/vmm-dcui.service"
[Unit]
Description=CoreVM DCUI
After=multi-user.target
[Service]
Type=simple
ExecStart=/opt/vmm/vmm-appliance --mode dcui
StandardInput=tty
StandardOutput=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
Restart=always
[Install]
WantedBy=multi-user.target
DCUI_SVC

# Copy GRUB defaults for installed system
sudo cp "$SCRIPT_DIR/iso/grub-installed.cfg" "$ROOTFS_DIR/etc/default/grub"

# Copy nftables config
sudo cp "$SCRIPT_DIR/iso/nftables.conf" "$ROOTFS_DIR/etc/nftables.conf"

# Enable services (use --root= instead of chroot, since systemd is not PID 1 in the chroot)
sudo systemctl --root="$ROOTFS_DIR" enable vmm-dcui.service
sudo systemctl --root="$ROOTFS_DIR" enable nftables.service
sudo systemctl --root="$ROOTFS_DIR" enable systemd-networkd.service
sudo systemctl --root="$ROOTFS_DIR" enable systemd-resolved.service
sudo systemctl --root="$ROOTFS_DIR" enable ssh.service

# Disable getty on tty1 (DCUI takes over)
sudo systemctl --root="$ROOTFS_DIR" mask getty@tty1.service

# Symlink resolv.conf for systemd-resolved
sudo ln -sf /run/systemd/resolve/stub-resolv.conf "$ROOTFS_DIR/etc/resolv.conf"

# Build initramfs
sudo chroot "$ROOTFS_DIR" update-initramfs -u

# Pack rootfs tarball
echo "[3/7] Packing rootfs tarball..."
sudo tar czf "$BUILD_DIR/rootfs.tar.gz" -C "$ROOTFS_DIR" .

# 2. Build live environment
echo "[4/7] Building live environment..."
LIVE_DIR="$BUILD_DIR/live-root"
rm -rf "$LIVE_DIR"
sudo debootstrap --variant=minbase --include=\
linux-image-amd64,live-boot,systemd \
    bookworm "$LIVE_DIR" http://deb.debian.org/debian

# Copy installer binary + rootfs tarball into live env
sudo mkdir -p "$LIVE_DIR/opt/vmm"
sudo cp "$ROOT/target/release/vmm-appliance" "$LIVE_DIR/opt/vmm/"
sudo cp "$BUILD_DIR/rootfs.tar.gz" "$LIVE_DIR/opt/vmm/"

# Auto-start installer in live env
cat <<'INSTALLER_SVC' | sudo tee "$LIVE_DIR/etc/systemd/system/vmm-installer.service"
[Unit]
Description=CoreVM Installer
After=multi-user.target
[Service]
Type=simple
ExecStart=/opt/vmm/vmm-appliance --mode installer
StandardInput=tty
StandardOutput=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
[Install]
WantedBy=multi-user.target
INSTALLER_SVC

sudo systemctl --root="$LIVE_DIR" enable vmm-installer.service
sudo systemctl --root="$LIVE_DIR" mask getty@tty1.service

# 3. Assemble ISO
echo "[5/7] Creating squashfs..."
ISO_STAGING="$BUILD_DIR/iso-staging"
rm -rf "$ISO_STAGING"
mkdir -p "$ISO_STAGING/live" "$ISO_STAGING/boot/grub" "$ISO_STAGING/isolinux"

# Copy kernel + initramfs from live env
sudo cp "$LIVE_DIR/vmlinuz" "$ISO_STAGING/live/"
sudo cp "$LIVE_DIR/initrd.img" "$ISO_STAGING/live/"

# Create squashfs
sudo mksquashfs "$LIVE_DIR" "$ISO_STAGING/live/filesystem.squashfs" -comp xz -noappend

# Copy boot configs
cp "$SCRIPT_DIR/iso/grub.cfg" "$ISO_STAGING/boot/grub/"
cp "$SCRIPT_DIR/iso/isolinux.cfg" "$ISO_STAGING/isolinux/"
cp /usr/lib/ISOLINUX/isolinux.bin "$ISO_STAGING/isolinux/"
cp /usr/lib/syslinux/modules/bios/ldlinux.c32 "$ISO_STAGING/isolinux/"

# Build EFI boot image
echo "[6/7] Building EFI boot image..."
mkdir -p "$ISO_STAGING/boot/grub/x86_64-efi"
cp /usr/lib/grub/x86_64-efi/*.mod "$ISO_STAGING/boot/grub/x86_64-efi/"
grub-mkimage -o "$ISO_STAGING/boot/grub/bootx64.efi" \
    -p /boot/grub -O x86_64-efi \
    part_gpt part_msdos fat iso9660 normal boot linux search search_fs_uuid search_label configfile
# Create FAT image for EFI System Partition
dd if=/dev/zero of="$ISO_STAGING/boot/grub/efi.img" bs=1M count=4
mkfs.vfat "$ISO_STAGING/boot/grub/efi.img"
mmd -i "$ISO_STAGING/boot/grub/efi.img" ::/EFI ::/EFI/BOOT
mcopy -i "$ISO_STAGING/boot/grub/efi.img" "$ISO_STAGING/boot/grub/bootx64.efi" ::/EFI/BOOT/BOOTX64.EFI

echo "[7/8] Building ISO image..."
xorriso -as mkisofs \
    -o "$ISO_OUTPUT" \
    -isohybrid-mbr /usr/lib/ISOLINUX/isohdpfx.bin \
    -c isolinux/boot.cat \
    -b isolinux/isolinux.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    -eltorito-alt-boot \
    -e boot/grub/efi.img \
    -no-emul-boot -isohybrid-gpt-basdat \
    "$ISO_STAGING"

echo "[8/8] Done!"
echo "ISO: $ISO_OUTPUT"
ls -lh "$ISO_OUTPUT"
```

- [ ] **Step 6: Make build-iso.sh executable**

Run: `chmod +x tools/build-iso.sh`

- [ ] **Step 7: Commit**

```bash
git add tools/build-iso.sh tools/iso/
git commit -m "feat(appliance): ISO build script with GRUB/isolinux configs and nftables ruleset"
```

---

## Task 13: Integration Testing

**Files:**
- Modify: `apps/vmm-appliance/src/common/system.rs` (add `#[cfg(test)]` mock support)

Since this is an appliance that runs on bare metal, full integration testing requires a VM. But we can unit-test the logic layers.

- [ ] **Step 1: Add unit tests for config.rs**

Test `ApplianceConfig` serialization/deserialization roundtrip, `write_vmm_server_config` output format, `write_default_config`.

- [ ] **Step 2: Add unit tests for firewall.rs**

Test `write_nftables_config` generates correct rules for server-only and cluster roles.

- [ ] **Step 3: Add unit tests for network.rs**

Test `write_networkd_config` generates correct `.network` file for DHCP and static configs.

- [ ] **Step 4: Add unit tests for installer screen navigation**

Test `Screen::next()` and `Screen::prev()` transitions are correct.

- [ ] **Step 5: Run all tests**

Run: `cargo test -p vmm-appliance`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add apps/vmm-appliance/
git commit -m "test(appliance): unit tests for config, firewall, network, and screen navigation"
```

---

## Task 14: Documentation & Workspace Finalization

**Files:**
- Modify: `Cargo.toml` (ensure workspace is correct)

- [ ] **Step 1: Verify full workspace compiles**

Run: `cargo check` (entire workspace)

- [ ] **Step 2: Verify vmm-appliance builds in release mode**

Run: `cargo build --release -p vmm-appliance`

- [ ] **Step 3: Test CLI arg parsing**

Run: `./target/release/vmm-appliance --help`
Expected: Shows `--mode <installer|dcui>` usage.

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(appliance): complete vmm-appliance crate for CoreVM Appliance ISO"
```

---

## Execution Order & Dependencies

```
Task 1 (scaffold) ─────────────────────────────┐
Task 2 (widgets) ──────────────────────────────┤
Task 3 (common system layer) ──────────────────┤
                                                ├── Task 4 (installer state machine + welcome)
                                                ├── Task 9 (DCUI main loop + status)
                                                │
Task 4 ────── Task 5 (disk screen)             │
              Task 6 (network/tz/users)         │
              Task 7 (ports/certs/summary)      │
              Task 8 (progress/installation)    │
                                                │
Task 9 ────── Task 10 (DCUI dialogs 1)         │
              Task 11 (DCUI dialogs 2)          │
                                                │
Task 12 (ISO build script) ─── independent      │
Task 13 (tests) ─── after Tasks 1-11           │
Task 14 (finalization) ─── after all            │
```

**Parallelizable:** Tasks 4-8 (installer) and Tasks 9-11 (DCUI) can be developed in parallel after Tasks 1-3 are complete. Task 12 (ISO build) is independent and can be done anytime.
