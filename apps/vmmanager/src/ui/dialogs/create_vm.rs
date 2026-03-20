use eframe::egui;
use crate::config::{VmConfig, BiosType};
use crate::ui::dialogs::{labeled_row, button_row, FIELD_MIN_WIDTH};
use crate::engine::platform;
use crate::engine::iso_detect::{self, DetectedOs, IsoInfo};
use crate::ui::theme;

pub struct CreateVmDialog {
    name: String,
    ram_mb: u32,
    cpu_cores: u32,
    iso_path: String,
    create_disk: bool,
    disk_size_gb: u32,
    use_uefi: bool,
    pub open: bool,
    pub created: Option<VmConfig>,
    error: Option<String>,
    /// Detected OS info from the selected ISO.
    iso_info: Option<IsoInfo>,
    /// Whether auto-configuration has been applied from ISO detection.
    auto_configured: bool,
    /// Pending file browser request.
    pub wants_browse_iso: bool,
}

impl CreateVmDialog {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            ram_mb: 2048,
            cpu_cores: 1,
            iso_path: String::new(),
            create_disk: true,
            disk_size_gb: 25,
            use_uefi: false,
            open: true,
            created: None,
            error: None,
            iso_info: None,
            auto_configured: false,
            wants_browse_iso: false,
        }
    }

    /// Set the ISO path (called from file browser callback in app.rs).
    pub fn set_iso(&mut self, path: String) {
        self.iso_path = path;
        self.detect_iso();
    }

    /// Detect OS from the currently set ISO path.
    fn detect_iso(&mut self) {
        self.iso_info = None;
        self.auto_configured = false;

        if self.iso_path.is_empty() {
            return;
        }

        let path = std::path::Path::new(&self.iso_path);
        if !path.exists() {
            return;
        }

        if let Some(info) = iso_detect::detect_iso(path) {
            // Auto-configure VM settings based on detected OS
            if self.name.is_empty() {
                self.name = info.os.label().to_string();
            }
            self.ram_mb = info.os.suggested_ram_mb();
            self.cpu_cores = info.os.suggested_cpus();
            self.disk_size_gb = (info.os.suggested_disk_mb() / 1024) as u32;
            self.use_uefi = info.os.suggest_uefi();
            self.auto_configured = true;
            self.iso_info = Some(info);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        if !self.open { return false; }

        let mut still_open = self.open;
        let mut button_close = false;
        let max_h = (ctx.screen_rect().height() - 40.0).max(400.0);

        egui::Window::new("Create New Virtual Machine")
            .open(&mut still_open)
            .collapsible(false)
            .resizable(true)
            .min_width(520.0)
            .min_height(300.0)
            .max_height(max_h)
            .default_size([560.0, max_h.min(520.0)])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                // ── Installation Media ──
                ui.heading("Installation Media");
                ui.add_space(4.0);

                labeled_row(ui, "ISO Image:", |ui| {
                    let changed = ui.add(
                        egui::TextEdit::singleline(&mut self.iso_path).desired_width(FIELD_MIN_WIDTH)
                    ).changed();
                    if ui.button("Browse...").clicked() {
                        self.wants_browse_iso = true;
                    }
                    if changed {
                        self.detect_iso();
                    }
                });

                // Show detection result
                if let Some(ref info) = self.iso_info {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(110.0);
                        let icon = if info.os.is_windows() { "\u{1F5D4}" } // 🗔
                                   else if info.os.is_linux() { "\u{1F427}" } // 🐧
                                   else { "\u{1F4BF}" }; // 💿
                        ui.colored_label(
                            theme::success_green(),
                            format!("{} Detected: {}", icon, info.os.label()),
                        );
                    });
                    if self.auto_configured {
                        ui.horizontal(|ui| {
                            ui.add_space(110.0);
                            ui.colored_label(
                                theme::text_dim(),
                                "Settings auto-configured for this OS.",
                            );
                        });
                    }
                } else if !self.iso_path.is_empty() {
                    let path = std::path::Path::new(&self.iso_path);
                    if !path.exists() {
                        ui.horizontal(|ui| {
                            ui.add_space(110.0);
                            ui.colored_label(theme::error_red(), "ISO file not found.");
                        });
                    }
                }

                ui.add_space(12.0);

                // ── VM Configuration ──
                ui.heading("Virtual Machine");
                ui.add_space(4.0);

                labeled_row(ui, "Name:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(FIELD_MIN_WIDTH));
                });

                labeled_row(ui, "CPU Cores:", |ui| {
                    egui::ComboBox::from_id_salt("create_vm_cpu")
                        .width(120.0)
                        .selected_text(format!("{}", self.cpu_cores))
                        .show_ui(ui, |ui| {
                            for &c in &[1, 2, 4, 6, 8] {
                                ui.selectable_value(&mut self.cpu_cores, c, format!("{}", c));
                            }
                        });
                });

                labeled_row(ui, "RAM:", |ui| {
                    egui::ComboBox::from_id_salt("create_vm_ram")
                        .width(120.0)
                        .selected_text(format_ram(self.ram_mb))
                        .show_ui(ui, |ui| {
                            for &mb in &[256, 512, 1024, 2048, 4096, 8192, 16384] {
                                ui.selectable_value(&mut self.ram_mb, mb, format_ram(mb));
                            }
                        });
                });

                labeled_row(ui, "Firmware:", |ui| {
                    ui.radio_value(&mut self.use_uefi, false, "Legacy BIOS");
                    ui.radio_value(&mut self.use_uefi, true, "UEFI");
                });

                ui.add_space(12.0);

                // ── Hard Disk ──
                ui.heading("Hard Disk");
                ui.add_space(4.0);

                ui.checkbox(&mut self.create_disk, "Create a new virtual hard disk");

                if self.create_disk {
                    labeled_row(ui, "Disk Size:", |ui| {
                        egui::ComboBox::from_id_salt("create_vm_disk")
                            .width(120.0)
                            .selected_text(format!("{} GB", self.disk_size_gb))
                            .show_ui(ui, |ui| {
                                for &gb in &[8, 16, 20, 25, 32, 40, 50, 64, 80, 100, 128, 256] {
                                    ui.selectable_value(&mut self.disk_size_gb, gb, format!("{} GB", gb));
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.add_space(110.0);
                        ui.colored_label(
                            theme::text_dim(),
                            "Disk space is allocated on demand (sparse file).",
                        );
                    });
                }

                // Show the directory
                let vm_dir = platform::vm_dir(&self.name);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Location:");
                    ui.label(egui::RichText::new(vm_dir.to_string_lossy().as_ref())
                        .monospace()
                        .color(theme::text_placeholder()));
                });

                if let Some(err) = &self.error {
                    ui.add_space(4.0);
                    ui.colored_label(theme::error_red(), err);
                }

                ui.separator();

                let (ok, cancel) = button_row(ui, "Create");
                if ok {
                    self.do_create();
                    if self.created.is_some() {
                        button_close = true;
                    }
                }
                if cancel {
                    button_close = true;
                }
            });

        if button_close {
            self.open = false;
        } else {
            self.open = still_open;
        }
        self.open
    }

    fn do_create(&mut self) {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("Please enter a VM name.".into());
            return;
        }

        // Create VM directory
        let vm_dir = match platform::ensure_vm_dir(&name) {
            Ok(dir) => dir,
            Err(e) => {
                self.error = Some(format!("Failed to create VM directory: {}", e));
                return;
            }
        };

        let mut config = VmConfig::default();
        config.name = name.clone();
        config.ram_mb = self.ram_mb;
        config.cpu_cores = self.cpu_cores;
        config.iso_image = self.iso_path.clone();
        config.boot_order = crate::config::BootOrder::CdFirst;
        config.net_enabled = true;

        if self.use_uefi {
            config.bios_type = BiosType::Uefi;
        }

        // Apply OS-specific defaults from detection
        if let Some(ref info) = self.iso_info {
            config.guest_os = info.os.to_guest_os();
            if info.os.is_windows() {
                config.usb_tablet = true;
            }
        }

        // Create disk image if requested
        if self.create_disk {
            let disk_name = format!("{}.img", name.replace(' ', "_").to_lowercase());
            let disk_path = vm_dir.join(&disk_name);
            let size_bytes = self.disk_size_gb as u64 * 1024 * 1024 * 1024;

            match std::fs::File::create(&disk_path) {
                Ok(file) => {
                    if let Err(e) = file.set_len(size_bytes) {
                        self.error = Some(format!("Failed to create disk: {}", e));
                        return;
                    }
                }
                Err(e) => {
                    self.error = Some(format!("Failed to create disk: {}", e));
                    return;
                }
            }

            config.disk_images.push(disk_path.to_string_lossy().to_string());
        }

        self.created = Some(config);
    }
}

fn format_ram(mb: u32) -> String {
    if mb >= 1024 && mb % 1024 == 0 {
        format!("{} GB", mb / 1024)
    } else {
        format!("{} MB", mb)
    }
}
