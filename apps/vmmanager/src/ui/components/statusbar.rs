use eframe::egui;
use egui::Color32;
use crate::ui::theme;

/// A single device shown in the status bar
pub struct DeviceInfo {
    pub icon: &'static str,
    pub label: String,
    pub active: bool,
}

/// Runtime metrics for the status bar
pub struct VmMetrics {
    pub state_label: &'static str,
    pub devices: Vec<DeviceInfo>,
}

impl Default for VmMetrics {
    fn default() -> Self {
        Self {
            state_label: "Stopped",
            devices: Vec::new(),
        }
    }
}

/// Render the status bar at the bottom.
pub fn render_statusbar(
    ctx: &egui::Context,
    metrics: Option<&VmMetrics>,
    vm_selected: bool,
    last_key: Option<&str>,
) {
    egui::TopBottomPanel::bottom("statusbar")
        .exact_height(26.0)
        .frame(
            egui::Frame::new()
                .fill(theme::statusbar_bg())
                .inner_margin(egui::Margin::symmetric(12, 0)),
        )
        .show(ctx, |ui| {
            // Subtle top border
            let rect = ui.max_rect();
            ui.painter().line_segment(
                [rect.left_top(), rect.right_top()],
                egui::Stroke::new(0.5, theme::border_color()),
            );

            ui.horizontal_centered(|ui| {
                let label_color = theme::text_secondary();
                let value_color = theme::text_primary();
                ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 0.0);

                match metrics {
                    Some(m) => {
                        // State dot
                        let dot_color = match m.state_label {
                            "Running" => theme::success_green(),
                            "Paused" => theme::warning_orange(),
                            _ => theme::text_tertiary(),
                        };
                        ui.colored_label(dot_color, "\u{25CF}");
                        ui.colored_label(value_color, egui::RichText::new(m.state_label).size(11.0));
                    }
                    None => {
                        let msg = if vm_selected { "Ready" } else { "No VM selected" };
                        ui.colored_label(label_color, egui::RichText::new(msg).size(11.0));
                    }
                }

                // Right side: device icons + last key
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.style_mut().spacing.item_spacing = egui::vec2(6.0, 0.0);

                    // Last key (rightmost)
                    if let Some(key_str) = last_key {
                        ui.colored_label(
                            theme::text_tertiary(),
                            egui::RichText::new(format!("Key: {}", key_str)).size(11.0),
                        );
                        ui.add_space(6.0);
                    }

                    // Device icons with activity dots (right-to-left, so render in reverse)
                    if let Some(m) = metrics {
                        for device in m.devices.iter().rev() {
                            let resp = ui.colored_label(
                                theme::text_secondary(),
                                egui::RichText::new(device.icon).size(16.0),
                            );

                            // Green activity dot (bottom-right of icon)
                            if device.active {
                                let icon_rect = resp.rect;
                                let dot_center = icon_rect.right_bottom() - egui::vec2(2.0, 4.0);
                                ui.painter().circle_filled(dot_center, 3.0, theme::success_green());
                            }

                            resp.on_hover_text(&device.label);
                        }
                    }
                });
            });
        });
}
