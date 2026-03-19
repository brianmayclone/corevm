//! Dialog windows: VM creation, disk management, settings, about, snapshots.
//!
//! Also provides shared helpers (`labeled_row`, `button_row`) and constants
//! used across all dialog implementations.

pub mod about;
pub mod add_disk;
pub mod create_disk;
pub mod create_vm;
pub mod disk_pool;
pub mod settings;
pub mod snapshots;

use eframe::egui;
use crate::ui::theme;

pub const LABEL_WIDTH: f32 = 100.0;
pub const FIELD_MIN_WIDTH: f32 = 220.0;
pub const BUTTON_SIZE: egui::Vec2 = egui::vec2(80.0, 28.0);

pub fn labeled_row(ui: &mut egui::Ui, label: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, 20.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| { ui.label(label); },
        );
        add_contents(ui);
    });
}

pub fn button_row(ui: &mut egui::Ui, ok_label: &str) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new("Cancel").min_size(BUTTON_SIZE)).clicked() {
                cancel = true;
            }
            if ui.add(egui::Button::new(ok_label).fill(theme::accent_blue()).min_size(BUTTON_SIZE)).clicked() {
                ok = true;
            }
        });
    });
    (ok, cancel)
}

// Re-exports for convenient access
pub use about::AboutDialog;
pub use add_disk::{AddDiskDialog, AddDiskMode};
pub use create_disk::CreateDiskDialog;
pub use create_vm::CreateVmDialog;
pub use disk_pool::DiskPoolDialog;
pub use settings::SettingsDialog;
pub use snapshots::SnapshotsDialog;
