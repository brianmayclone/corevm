use eframe::egui;
use crate::config::VmConfig;
use crate::ui::dialogs::{labeled_row, button_row, FIELD_MIN_WIDTH};
use crate::engine::platform;
use crate::ui::theme;

pub struct CreateVmDialog {
    name: String,
    ram_mb: u32,
    pub open: bool,
    pub created: Option<VmConfig>,
    error: Option<String>,
}

impl CreateVmDialog {
    pub fn new() -> Self {
        Self {
            name: "New VM".into(),
            ram_mb: 256,
            open: true,
            created: None,
            error: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        if !self.open { return false; }

        let mut still_open = self.open;
        let mut button_close = false;

        egui::Window::new("Create New Virtual Machine")
            .open(&mut still_open)
            .collapsible(false)
            .resizable(false)
            .min_width(400.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                labeled_row(ui, "Name:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(FIELD_MIN_WIDTH));
                });

                labeled_row(ui, "RAM:", |ui| {
                    egui::ComboBox::from_id_salt("create_vm_ram")
                        .width(FIELD_MIN_WIDTH)
                        .selected_text(format!("{} MB", self.ram_mb))
                        .show_ui(ui, |ui| {
                            for &mb in &[64, 128, 256, 512, 1024, 2048, 4096] {
                                ui.selectable_value(&mut self.ram_mb, mb, format!("{} MB", mb));
                            }
                        });
                });

                // Show the directory that will be created
                let vm_dir = platform::vm_dir(&self.name);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Directory:");
                    ui.label(egui::RichText::new(vm_dir.to_string_lossy().as_ref())
                        .monospace()
                        .color(theme::text_placeholder()));
                });

                if let Some(err) = &self.error {
                    ui.colored_label(theme::error_red(), err);
                }

                ui.separator();

                let (ok, cancel) = button_row(ui, "Create");
                if ok {
                    let name = self.name.trim().to_string();
                    if name.is_empty() {
                        self.error = Some("Please enter a VM name.".into());
                    } else {
                        // Create the VM machine directory
                        match platform::ensure_vm_dir(&name) {
                            Ok(_dir) => {
                                let mut config = VmConfig::default();
                                config.name = name;
                                config.ram_mb = self.ram_mb;
                                self.created = Some(config);
                                button_close = true;
                            }
                            Err(e) => {
                                self.error = Some(format!("Failed to create VM directory: {}", e));
                            }
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
}
