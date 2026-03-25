//! Exposé view — shows a grid of live VM screen thumbnails for a folder.
//!
//! When the user clicks a folder header in the sidebar, the central panel
//! displays all VMs in that folder as a macOS-style Exposé grid.  Running
//! VMs show their live framebuffer; stopped VMs show their OS icon.
//! Clicking a thumbnail selects the VM.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::app::FrameBufferData;
use crate::ui::components::display::render_text_mode;
use crate::ui::components::sidebar::VmState;
use crate::ui::theme;

/// Per-VM info needed for one Exposé tile.
pub struct ExposeTile {
    pub uuid: String,
    pub name: String,
    pub state: VmState,
    pub framebuffer: Arc<Mutex<FrameBufferData>>,
    pub os_icon: Option<egui::TextureId>,
}

/// Persistent state for Exposé thumbnail textures (keyed by VM UUID).
pub struct ExposeState {
    textures: HashMap<String, egui::TextureHandle>,
    /// Last rendered framebuffer sequence per VM, to avoid redundant uploads.
    last_seq: HashMap<String, u64>,
}

impl Default for ExposeState {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            last_seq: HashMap::new(),
        }
    }
}

/// Result of rendering the Exposé view.
pub enum ExposeAction {
    /// User clicked a VM thumbnail — select it.
    SelectVm(String),
}

/// Render the Exposé grid for a set of VM tiles.
///
/// Returns an optional action (VM selection) if the user clicks a thumbnail.
pub fn render_expose(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    folder_name: &str,
    tiles: &[ExposeTile],
    state: &mut ExposeState,
) -> Option<ExposeAction> {
    let mut action = None;

    if tiles.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.label(
                    egui::RichText::new(format!("\u{1F4C1} {}", folder_name))
                        .size(20.0)
                        .color(theme::text_secondary()),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("No machines in this folder")
                        .size(13.0)
                        .color(theme::text_tertiary()),
                );
            });
        });
        return None;
    }

    // Update thumbnail textures for running/paused VMs
    for tile in tiles {
        if tile.state == VmState::Running || tile.state == VmState::Paused {
            update_thumbnail(ctx, tile, state);
        }
    }

    let available = ui.available_size();
    let tile_count = tiles.len();

    // Calculate grid layout — aim for a pleasant aspect ratio per tile
    let (cols, rows) = compute_grid(tile_count, available.x, available.y);

    // Tile dimensions with spacing
    let spacing = 20.0_f32;
    let header_height = 40.0; // folder name header
    let label_height = 32.0; // VM name + state below each tile

    let grid_width = available.x - spacing * 2.0;
    let grid_height = available.y - header_height - spacing;

    let tile_w = ((grid_width - spacing * (cols as f32 - 1.0)) / cols as f32).max(100.0);
    let tile_h = ((grid_height - spacing * (rows as f32 - 1.0)) / rows as f32 - label_height).max(60.0);

    // Maintain 16:10 aspect ratio within the tile box
    let aspect = 16.0 / 10.0;
    let (thumb_w, thumb_h) = if tile_w / tile_h > aspect {
        (tile_h * aspect, tile_h)
    } else {
        (tile_w, tile_w / aspect)
    };

    // Total cell size including label
    let cell_w = thumb_w;
    let cell_h = thumb_h + label_height;

    // Center the grid
    let total_grid_w = cell_w * cols as f32 + spacing * (cols as f32 - 1.0);
    let total_grid_h = cell_h * rows as f32 + spacing * (rows as f32 - 1.0);
    let offset_x = (available.x - total_grid_w) / 2.0;
    let offset_y = header_height + (available.y - header_height - total_grid_h) / 2.0;

    // Header
    let header_rect = ui.allocate_exact_size(
        egui::vec2(available.x, header_height),
        egui::Sense::hover(),
    ).0;
    ui.painter().text(
        egui::pos2(header_rect.center().x, header_rect.center().y),
        egui::Align2::CENTER_CENTER,
        format!("\u{1F4C1} {}", folder_name),
        egui::FontId::proportional(16.0),
        theme::text_secondary(),
    );

    // Allocate remaining space for the grid
    let (grid_rect, _) = ui.allocate_exact_size(
        egui::vec2(available.x, available.y - header_height),
        egui::Sense::hover(),
    );

    let origin = grid_rect.min + egui::vec2(offset_x, offset_y - header_height);

    for (i, tile) in tiles.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;

        let x = origin.x + col as f32 * (cell_w + spacing);
        let y = origin.y + row as f32 * (cell_h + spacing);

        let thumb_rect = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(thumb_w, thumb_h),
        );

        // Interactive area for the whole cell
        let cell_rect = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(cell_w, cell_h),
        );
        let resp = ui.allocate_rect(cell_rect, egui::Sense::click());

        let hovered = resp.hovered();

        // Shadow
        let shadow_rect = thumb_rect.expand(2.0).translate(egui::vec2(0.0, 2.0));
        ui.painter().rect_filled(shadow_rect, 10.0, theme::card_shadow());

        // Background
        let bg = if hovered { theme::widget_bg_hovered() } else { theme::card_bg() };
        ui.painter().rect_filled(thumb_rect, 8.0, bg);

        // Border — accent when hovered
        let border = if hovered {
            egui::Stroke::new(2.0, theme::accent_blue())
        } else {
            egui::Stroke::new(0.5, theme::card_border())
        };
        ui.painter().rect_stroke(thumb_rect, 8.0, border, egui::StrokeKind::Outside);

        // Content: live framebuffer or OS icon
        if (tile.state == VmState::Running || tile.state == VmState::Paused)
            && state.textures.contains_key(&tile.uuid)
        {
            // Live thumbnail
            let tex = &state.textures[&tile.uuid];
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            // Shrink slightly for rounded corner padding
            let inner = thumb_rect.shrink(1.0);
            ui.painter().image(tex.id(), inner, uv, egui::Color32::WHITE);
        } else {
            // Static: OS icon or power symbol
            let center = thumb_rect.center();
            if let Some(tex_id) = tile.os_icon {
                let icon_size = (thumb_h * 0.4).clamp(24.0, 48.0);
                let icon_rect = egui::Rect::from_center_size(
                    center,
                    egui::vec2(icon_size, icon_size),
                );
                let tint = theme::card_icon_stroke();
                egui::Image::new(egui::load::SizedTexture::new(tex_id, egui::vec2(icon_size, icon_size)))
                    .tint(tint)
                    .paint_at(ui, icon_rect);
            } else {
                ui.painter().circle_stroke(center, 20.0, egui::Stroke::new(1.5, theme::card_icon_stroke()));
                ui.painter().line_segment(
                    [center - egui::vec2(0.0, 10.0), center - egui::vec2(0.0, 22.0)],
                    egui::Stroke::new(1.5, theme::card_icon_stroke()),
                );
            }
        }

        // Status dot (top-right corner of thumbnail)
        if tile.state != VmState::Stopped {
            let dot_color = match tile.state {
                VmState::Running => theme::success_green(),
                VmState::Starting => theme::accent_blue(),
                VmState::Paused => theme::warning_orange(),
                VmState::Stopping => theme::warning_orange(),
                VmState::Stopped => unreachable!(),
            };
            let dot_center = egui::pos2(thumb_rect.right() - 8.0, thumb_rect.top() + 8.0);
            ui.painter().circle_filled(dot_center, 5.0, dot_color);
            ui.painter().circle_stroke(dot_center, 5.0, egui::Stroke::new(1.0, theme::card_bg()));
        }

        // VM name below thumbnail
        let name_pos = egui::pos2(x + cell_w / 2.0, y + thumb_h + 6.0);
        ui.painter().text(
            name_pos,
            egui::Align2::CENTER_TOP,
            &tile.name,
            egui::FontId::proportional(12.0),
            if hovered { theme::text_bright() } else { theme::text_primary() },
        );

        // State label
        let (state_label, state_color) = match tile.state {
            VmState::Running => ("Running", theme::success_green()),
            VmState::Starting => ("Starting\u{2026}", theme::accent_blue()),
            VmState::Paused => ("Paused", theme::warning_orange()),
            VmState::Stopping => ("Stopping\u{2026}", theme::warning_orange()),
            VmState::Stopped => ("Powered Off", theme::text_tertiary()),
        };
        let state_pos = egui::pos2(x + cell_w / 2.0, y + thumb_h + 20.0);
        ui.painter().text(
            state_pos,
            egui::Align2::CENTER_TOP,
            state_label,
            egui::FontId::proportional(10.0),
            state_color,
        );

        // Click → select VM
        if resp.clicked() {
            action = Some(ExposeAction::SelectVm(tile.uuid.clone()));
        }
    }

    // Request repaint while any VM is running (for live thumbnails)
    if tiles.iter().any(|t| t.state == VmState::Running) {
        ctx.request_repaint();
    }

    action
}

/// Compute a grid layout (cols, rows) that fills the available space well.
fn compute_grid(n: usize, width: f32, height: f32) -> (usize, usize) {
    if n == 0 {
        return (1, 1);
    }
    if n == 1 {
        return (1, 1);
    }

    // Try different column counts, pick the one with the best aspect ratio per cell
    let target_aspect = 16.0 / 10.0;
    let mut best_cols = 1;
    let mut best_score = f32::MAX;

    for cols in 1..=n.min(6) {
        let rows = (n + cols - 1) / cols;
        let cell_w = width / cols as f32;
        let cell_h = height / rows as f32;
        let cell_aspect = cell_w / cell_h;
        let score = (cell_aspect - target_aspect).abs();
        if score < best_score {
            best_score = score;
            best_cols = cols;
        }
    }

    let rows = (n + best_cols - 1) / best_cols;
    (best_cols, rows)
}

/// Update the Exposé thumbnail texture for a running VM.
fn update_thumbnail(
    ctx: &egui::Context,
    tile: &ExposeTile,
    state: &mut ExposeState,
) {
    let fb = match tile.framebuffer.lock() {
        Ok(fb) => fb,
        Err(_) => return,
    };

    // Skip if nothing changed
    let last = state.last_seq.get(&tile.uuid).copied().unwrap_or(0);
    if fb.seq == last {
        return;
    }

    // Build RGBA pixel data for the thumbnail texture.
    // In text mode we must rasterize the text buffer; in graphics mode
    // fb.pixels is already RGBA32 (converted by the VM thread), so use
    // it directly — exactly like DisplayWidget::update_texture does.
    let image = if fb.text_mode {
        let mut rgba = Vec::new();
        let (w, h) = render_text_mode(&fb.text_buffer, &mut rgba);
        if w == 0 || h == 0 || rgba.len() < (w as usize * h as usize * 4) {
            return;
        }
        egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &rgba[..w as usize * h as usize * 4],
        )
    } else if fb.width > 0 && fb.height > 0 && !fb.pixels.is_empty() {
        let expected = (fb.width as usize) * (fb.height as usize) * 4;
        if fb.pixels.len() < expected {
            return;
        }
        egui::ColorImage::from_rgba_unmultiplied(
            [fb.width as usize, fb.height as usize],
            &fb.pixels[..expected],
        )
    } else {
        return;
    };

    // Create or update texture
    if let Some(tex) = state.textures.get_mut(&tile.uuid) {
        tex.set(image, egui::TextureOptions::LINEAR);
    } else {
        let tex = ctx.load_texture(
            format!("expose_{}", tile.uuid),
            image,
            egui::TextureOptions::LINEAR,
        );
        state.textures.insert(tile.uuid.clone(), tex);
    }

    state.last_seq.insert(tile.uuid.clone(), fb.seq);
}
