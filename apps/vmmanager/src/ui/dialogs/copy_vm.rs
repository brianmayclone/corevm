use eframe::egui;
use crate::config::VmConfig;
use crate::ui::dialogs::{labeled_row, button_row, FIELD_MIN_WIDTH};
use crate::engine::platform;
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq)]
pub enum DiskCopyMode {
    /// Create a full copy of each disk image file
    Copy,
    /// Create a symbolic link to the original disk image
    Link,
}

pub struct CopyVmDialog {
    source_config: VmConfig,
    new_name: String,
    disk_mode: DiskCopyMode,
    pub open: bool,
    pub result: Option<CopyVmResult>,
    error: Option<String>,
    /// Progress state for background copy
    copying: bool,
    copy_progress: Option<String>,
}

pub struct CopyVmResult {
    pub config: VmConfig,
}

impl CopyVmDialog {
    pub fn new(source: &VmConfig) -> Self {
        Self {
            source_config: source.clone(),
            new_name: format!("{} (Copy)", source.name),
            disk_mode: DiskCopyMode::Copy,
            open: true,
            result: None,
            error: None,
            copying: false,
            copy_progress: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        if !self.open { return false; }

        let mut still_open = self.open;
        let mut button_close = false;

        egui::Window::new("Copy Virtual Machine")
            .open(&mut still_open)
            .collapsible(false)
            .resizable(false)
            .min_width(450.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{1F4CB}").size(18.0));
                    ui.add_space(4.0);
                    ui.label(format!("Copy \"{}\"", self.source_config.name));
                });
                ui.add_space(6.0);

                labeled_row(ui, "New Name:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_name).desired_width(FIELD_MIN_WIDTH));
                });

                // Show target directory
                let vm_dir = platform::vm_dir(&self.new_name);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Directory:");
                    ui.label(egui::RichText::new(vm_dir.to_string_lossy().as_ref())
                        .monospace()
                        .color(theme::text_placeholder()));
                });

                // Disk images section
                let has_disks = self.source_config.disk_images.iter().any(|d| !d.is_empty());
                if has_disks {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Disk Images").strong());
                    ui.add_space(4.0);

                    // List the disk images
                    for (i, disk) in self.source_config.disk_images.iter().enumerate() {
                        if !disk.is_empty() {
                            let filename = std::path::Path::new(disk)
                                .file_name()
                                .map(|f| f.to_string_lossy().to_string())
                                .unwrap_or_else(|| disk.clone());
                            let size_str = std::fs::metadata(disk)
                                .map(|m| format_size(m.len()))
                                .unwrap_or_else(|_| "?".into());
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.label(format!("Disk {}: {} ({})", i, filename, size_str));
                            });
                        }
                    }

                    ui.add_space(6.0);
                    labeled_row(ui, "Disk Mode:", |ui| {
                        egui::ComboBox::from_id_salt("copy_disk_mode")
                            .width(FIELD_MIN_WIDTH)
                            .selected_text(match self.disk_mode {
                                DiskCopyMode::Copy => "Copy (full duplicate)",
                                DiskCopyMode::Link => "Link (symlink to original)",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.disk_mode, DiskCopyMode::Copy,
                                    "Copy (full duplicate)");
                                ui.selectable_value(&mut self.disk_mode, DiskCopyMode::Link,
                                    "Link (symlink to original)");
                            });
                    });

                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(104.0);
                        let hint = match self.disk_mode {
                            DiskCopyMode::Copy => "Each disk image will be fully copied. This may take a while for large disks.",
                            DiskCopyMode::Link => "Disk images will be symlinked. Changes in one VM affect the other!",
                        };
                        ui.label(egui::RichText::new(hint).size(11.0).color(
                            if self.disk_mode == DiskCopyMode::Link { theme::warning_orange() } else { theme::text_tertiary() }
                        ));
                    });
                }

                if let Some(progress) = &self.copy_progress {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(progress);
                    });
                }

                if let Some(err) = &self.error {
                    ui.add_space(4.0);
                    ui.colored_label(theme::error_red(), err);
                }

                ui.add_space(4.0);
                ui.separator();

                let (ok, cancel) = button_row(ui, "Copy");
                if ok && !self.copying {
                    self.error = None;
                    match self.perform_copy() {
                        Ok(config) => {
                            self.result = Some(CopyVmResult { config });
                            button_close = true;
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
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

    fn perform_copy(&mut self) -> Result<VmConfig, String> {
        let new_name = self.new_name.trim().to_string();
        if new_name.is_empty() {
            return Err("Please enter a name for the copy.".into());
        }

        // Create the VM directory for the copy
        let vm_dir = platform::ensure_vm_dir(&new_name)
            .map_err(|e| format!("Failed to create VM directory: {}", e))?;

        // Clone the config with a new UUID and name
        let mut new_config = self.source_config.clone();
        new_config.uuid = uuid::Uuid::new_v4().to_string().replace("-", "");
        new_config.name = new_name;

        // Handle disk images
        let mut new_disk_paths = Vec::new();
        for (i, disk_path) in self.source_config.disk_images.iter().enumerate() {
            if disk_path.is_empty() {
                new_disk_paths.push(String::new());
                continue;
            }

            let src = std::path::Path::new(disk_path);
            if !src.exists() {
                // Keep the path as-is (will show as validation error)
                new_disk_paths.push(disk_path.clone());
                continue;
            }

            let filename = src.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("disk{}.img", i));
            let dest = vm_dir.join(&filename);

            match self.disk_mode {
                DiskCopyMode::Copy => {
                    std::fs::copy(src, &dest)
                        .map_err(|e| format!("Failed to copy disk {}: {}", filename, e))?;
                }
                DiskCopyMode::Link => {
                    #[cfg(unix)]
                    {
                        // Use the absolute path of the source for the symlink
                        let abs_src = std::fs::canonicalize(src)
                            .map_err(|e| format!("Failed to resolve path for {}: {}", filename, e))?;
                        std::os::unix::fs::symlink(&abs_src, &dest)
                            .map_err(|e| format!("Failed to create symlink for {}: {}", filename, e))?;
                    }
                    #[cfg(not(unix))]
                    {
                        return Err("Symlinks are only supported on Unix systems.".into());
                    }
                }
            }

            new_disk_paths.push(dest.to_string_lossy().to_string());
        }
        new_config.disk_images = new_disk_paths;

        // ISO image is shared (read-only), just keep the same path
        // MAC address: reset to dynamic so we don't get duplicate MACs
        new_config.mac_mode = crate::config::MacMode::Dynamic;
        new_config.mac_address = String::new();

        Ok(new_config)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}
