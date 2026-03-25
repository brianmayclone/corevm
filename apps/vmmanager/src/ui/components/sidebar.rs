use std::collections::HashMap;
use std::path::Path;

use eframe::egui;
use egui::Color32;
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VmState {
    Stopped,
    Starting,
    Running,
    Paused,
    Stopping,
}

pub struct FolderEntry {
    pub name: String,
    pub vm_uuids: Vec<String>,
    pub expanded: bool,
}

/// Actions returned from the sidebar to the app
pub enum SidebarAction {
    /// A VM is being dragged to a folder (vm_uuid, target_folder_index or None for root)
    MoveVm { vm_uuid: String, target_folder: Option<usize> },
    /// Reorder a VM within a folder (or root) by inserting before another VM
    ReorderVm { vm_uuid: String, folder: Option<usize>, insert_index: usize },
    /// Request to create a new VM
    CreateVm,
    /// Request to create a new folder
    CreateFolder,
    /// Request to rename a folder
    RenameFolder(usize),
    /// Request to delete a folder (VMs move to root)
    DeleteFolder(usize),
    /// Request to delete a VM
    DeleteVm(String),
    /// Power on a VM
    StartVm(String),
    /// Power off a VM
    StopVm(String),
    /// Pause a VM
    PauseVm(String),
    /// Resume a paused VM
    ResumeVm(String),
    /// Take a snapshot
    SnapshotVm(String),
    /// Take a screenshot
    ScreenshotVm(String),
    /// Open VM settings
    ConfigureVm(String),
    /// Request to copy/clone a VM
    CopyVm(String),
    /// A folder was clicked — show Exposé view for this folder
    SelectFolder(usize),
}

pub struct SidebarLayout {
    pub folders: Vec<FolderEntry>,
    pub root_vms: Vec<String>,
}

impl Default for SidebarLayout {
    fn default() -> Self {
        Self {
            folders: vec![FolderEntry {
                name: "My Machines".to_string(),
                vm_uuids: Vec::new(),
                expanded: true,
            }],
            root_vms: Vec::new(),
        }
    }
}

impl SidebarLayout {
    pub fn load(path: &Path) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        let mut folders = Vec::new();
        let mut root_vms = Vec::new();
        let mut current_folder: Option<FolderEntry> = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix("folder:") {
                if let Some(f) = current_folder.take() {
                    folders.push(f);
                }
                current_folder = Some(FolderEntry {
                    name: name.to_string(),
                    vm_uuids: Vec::new(),
                    expanded: true,
                });
            } else if let Some(uuid) = line.strip_prefix("vm:") {
                if let Some(ref mut f) = current_folder {
                    f.vm_uuids.push(uuid.to_string());
                } else {
                    root_vms.push(uuid.to_string());
                }
            } else if line == "end" {
                if let Some(f) = current_folder.take() {
                    folders.push(f);
                }
            }
        }
        if let Some(f) = current_folder.take() {
            folders.push(f);
        }

        // Ensure at least one default folder
        if folders.is_empty() {
            folders.push(FolderEntry {
                name: "My Machines".to_string(),
                vm_uuids: Vec::new(),
                expanded: true,
            });
        }

        Self { folders, root_vms }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::fmt::Write as _;
        let mut out = String::new();
        for folder in &self.folders {
            let _ = writeln!(out, "folder:{}", folder.name);
            for uuid in &folder.vm_uuids {
                let _ = writeln!(out, "vm:{}", uuid);
            }
            let _ = writeln!(out, "end");
        }
        for uuid in &self.root_vms {
            let _ = writeln!(out, "vm:{}", uuid);
        }
        std::fs::write(path, out)
    }

    /// Ensure a newly created VM is placed in the first folder
    pub fn add_vm(&mut self, uuid: String) {
        if !self.folders.is_empty() {
            self.folders[0].vm_uuids.push(uuid);
        } else {
            self.root_vms.push(uuid);
        }
    }

    /// Move a VM from wherever it is to a target folder (or root if None)
    pub fn move_vm(&mut self, uuid: &str, target_folder: Option<usize>) {
        // Remove from all folders and root
        for folder in &mut self.folders {
            folder.vm_uuids.retain(|u| u != uuid);
        }
        self.root_vms.retain(|u| u != uuid);

        // Add to target
        match target_folder {
            Some(idx) if idx < self.folders.len() => {
                self.folders[idx].vm_uuids.push(uuid.to_string());
            }
            _ => {
                self.root_vms.push(uuid.to_string());
            }
        }
    }

    /// Reorder a VM within a folder (or root) by moving it to `insert_index`
    pub fn reorder_vm(&mut self, uuid: &str, folder: Option<usize>, insert_index: usize) {
        // Remove from everywhere first
        for f in &mut self.folders {
            f.vm_uuids.retain(|u| u != uuid);
        }
        self.root_vms.retain(|u| u != uuid);

        // Insert at the target position
        match folder {
            Some(idx) if idx < self.folders.len() => {
                let list = &mut self.folders[idx].vm_uuids;
                let pos = insert_index.min(list.len());
                list.insert(pos, uuid.to_string());
            }
            _ => {
                let pos = insert_index.min(self.root_vms.len());
                self.root_vms.insert(pos, uuid.to_string());
            }
        }
    }

    /// Remove a VM from the layout entirely
    pub fn remove_vm(&mut self, uuid: &str) {
        for folder in &mut self.folders {
            folder.vm_uuids.retain(|u| u != uuid);
        }
        self.root_vms.retain(|u| u != uuid);
    }

    /// Ensure all known VM UUIDs are in the layout somewhere; orphans go to first folder
    pub fn ensure_all_vms(&mut self, all_uuids: &[String]) {
        let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
        for folder in &self.folders {
            for u in &folder.vm_uuids {
                known.insert(u.clone());
            }
        }
        for u in &self.root_vms {
            known.insert(u.clone());
        }
        let orphans: Vec<String> = all_uuids.iter()
            .filter(|u| !known.contains(u.as_str()))
            .cloned()
            .collect();
        for uuid in orphans {
            if !self.folders.is_empty() {
                self.folders[0].vm_uuids.push(uuid);
            } else {
                self.root_vms.push(uuid);
            }
        }
    }
}

/// Style a context menu UI to remove button frames (call at the start of a context_menu closure).
fn style_context_menu(ui: &mut egui::Ui) {
    ui.style_mut().visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
}

/// Render a context menu item with an optional icon in a fixed-width left column.
/// If `icon` is None, the space is left blank for alignment.
fn menu_item(
    ui: &mut egui::Ui,
    icon: Option<&str>,
    label: &str,
    enabled: bool,
    icon_width: f32,
    text_color: Option<Color32>,
) -> bool {
    let mut clicked = false;
    ui.add_enabled_ui(enabled, |ui| {
        let resp = ui.horizontal(|ui| {
            let icon_text = icon.unwrap_or("");
            ui.add_sized(
                [icon_width, ui.spacing().interact_size.y],
                egui::Label::new(egui::RichText::new(icon_text).size(13.0)),
            );
            let mut rt = egui::RichText::new(label);
            if let Some(color) = text_color {
                if enabled {
                    rt = rt.color(color);
                }
            }
            ui.add(egui::Button::new(rt).frame(false))
        });
        if resp.inner.clicked() {
            clicked = true;
        }
    });
    clicked
}

/// Render a single VM entry with drag&drop reordering support.
fn render_vm_entry(
    ui: &mut egui::Ui,
    uuid: &str,
    vm_index: usize,
    vm_names: &HashMap<String, String>,
    vm_states: &HashMap<String, VmState>,
    vm_icons: &HashMap<String, egui::TextureId>,
    vm_errors: &HashMap<String, Vec<String>>,
    selected: &mut Option<String>,
    drag_vm: &mut Option<String>,
    drop_target: &mut Option<(Option<usize>, usize)>, // (folder, insert_index)
    actions: &mut Vec<SidebarAction>,
    folder_names: &[String],
    current_folder: Option<usize>,
) {
    let name = vm_names
        .get(uuid)
        .map(|s| s.as_str())
        .unwrap_or("Unknown VM");
    let state = vm_states.get(uuid).copied().unwrap_or(VmState::Stopped);
    let errors = vm_errors.get(uuid);
    let has_errors = errors.map_or(false, |e| !e.is_empty());

    // Combined icon + label as a single selectable widget
    let is_selected = selected.as_deref() == Some(uuid);
    let icon_size = 20.0;
    let padding = egui::vec2(4.0, 2.0);
    let icon_text_gap = 4.0;

    let label_text = if state == VmState::Running {
        egui::RichText::new(name).strong()
    } else {
        egui::RichText::new(name)
    };
    let widget_text: egui::WidgetText = label_text.into();
    let galley = widget_text.into_galley(ui, Some(egui::TextWrapMode::Truncate), ui.available_width() - icon_size - icon_text_gap - padding.x * 2.0, egui::FontSelection::Default);

    let row_height = icon_size.max(galley.size().y) + padding.y * 2.0;
    let desired_size = egui::vec2(ui.available_width(), row_height);
    let (rect, resp) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    if resp.clicked() {
        *selected = Some(uuid.to_string());
    }

    // Drop target indicator: show a line above or below this entry when another VM is dragged over it
    if drag_vm.is_some() && drag_vm.as_deref() != Some(uuid) {
        if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
            if rect.contains(pointer_pos) {
                let insert_above = pointer_pos.y < rect.center().y;
                let insert_idx = if insert_above { vm_index } else { vm_index + 1 };
                *drop_target = Some((current_folder, insert_idx));

                // Draw insertion indicator line
                let line_y = if insert_above { rect.top() } else { rect.bottom() };
                let line_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 4.0, line_y - 1.0),
                    egui::pos2(rect.right() - 4.0, line_y + 1.0),
                );
                ui.painter().rect_filled(line_rect, egui::CornerRadius::same(1), theme::accent_blue());
            }
        }
    }

    // Paint background with rounded corners
    let is_being_dragged = drag_vm.as_deref() == Some(uuid);
    let visuals = ui.style().interact_selectable(&resp, is_selected);
    if is_being_dragged {
        // Semi-transparent when being dragged
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), visuals.bg_fill.gamma_multiply(0.5));
    } else if is_selected || resp.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), visuals.bg_fill);
    }

    let content_rect = rect.shrink2(padding);

    // Paint icon (error warning replaces OS icon)
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(content_rect.left(), content_rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    if has_errors {
        // Warning icon in place of OS icon, with tooltip
        ui.painter().text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{26A0}",
            egui::FontId::proportional(16.0),
            theme::error_red(),
        );
        // Tooltip on the whole row
        if resp.hovered() {
            let tooltip_text = errors.unwrap().join("\n");
            resp.clone().on_hover_text(tooltip_text);
        }
    } else if let Some(&tex_id) = vm_icons.get(uuid) {
        let tint = if state == VmState::Running {
            theme::icon_tint_active()
        } else {
            theme::icon_tint_inactive()
        };
        let img = egui::Image::new(egui::load::SizedTexture::new(tex_id, egui::vec2(icon_size, icon_size)))
            .tint(tint);
        img.paint_at(ui, icon_rect);

        // Status dot overlay (bottom-right of icon)
        if state != VmState::Stopped {
            let dot_color = match state {
                VmState::Running => theme::success_green(),
                VmState::Starting => theme::accent_blue(),
                VmState::Paused => theme::warning_orange(),
                VmState::Stopping => theme::warning_orange(),
                VmState::Stopped => unreachable!(),
            };
            let dot_center = icon_rect.right_bottom() - egui::vec2(3.0, 3.0);
            ui.painter().circle_filled(dot_center, 4.0, dot_color);
            ui.painter().circle_stroke(dot_center, 4.0, egui::Stroke::new(1.0, theme::sidebar_bg()));
        }
    }

    // Paint text
    let text_pos = egui::pos2(
        content_rect.left() + icon_size + icon_text_gap,
        content_rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, visuals.text_color());

    let inner_resp = resp;

    // Drag source
    if inner_resp.dragged() {
        *drag_vm = Some(uuid.to_string());
    }

    // Context menu
    inner_resp.context_menu(|ui| {
        let icon_width = 20.0; // consistent left column for icons

        ui.label(egui::RichText::new(name).strong());
        ui.separator();

        // Power actions — enabled/disabled based on state
        let is_stopped = state == VmState::Stopped;
        let is_running = state == VmState::Running;
        let is_paused = state == VmState::Paused;

        if menu_item(ui, Some("\u{25B6}"), "Start", is_stopped, icon_width, None) {
            actions.push(SidebarAction::StartVm(uuid.to_string()));
            ui.close_menu();
        }
        if menu_item(ui, Some("\u{23F8}"), "Pause", is_running, icon_width, None) {
            actions.push(SidebarAction::PauseVm(uuid.to_string()));
            ui.close_menu();
        }
        if menu_item(ui, Some("\u{25B6}"), "Resume", is_paused, icon_width, None) {
            actions.push(SidebarAction::ResumeVm(uuid.to_string()));
            ui.close_menu();
        }
        if menu_item(ui, Some("\u{23FB}"), "Power Off", !is_stopped, icon_width, None) {
            actions.push(SidebarAction::StopVm(uuid.to_string()));
            ui.close_menu();
        }

        ui.separator();

        // Snapshot & Screenshot — only when running
        if menu_item(ui, Some("\u{1F4BE}"), "Take Snapshot", is_running, icon_width, None) {
            actions.push(SidebarAction::SnapshotVm(uuid.to_string()));
            ui.close_menu();
        }
        if menu_item(ui, Some("\u{1F4F7}"), "Take Screenshot", is_running, icon_width, None) {
            actions.push(SidebarAction::ScreenshotVm(uuid.to_string()));
            ui.close_menu();
        }

        ui.separator();

        // Configuration
        if menu_item(ui, Some("\u{2699}"), "Settings...", is_stopped, icon_width, None) {
            actions.push(SidebarAction::ConfigureVm(uuid.to_string()));
            ui.close_menu();
        }

        // "Move to" submenu
        if folder_names.len() > 1 || current_folder.is_none() {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [icon_width, ui.spacing().interact_size.y],
                    egui::Label::new(egui::RichText::new("\u{1F4C1}").size(13.0)),
                );
                ui.menu_button("Move to...", |ui| {
                    for (i, fname) in folder_names.iter().enumerate() {
                        if Some(i) == current_folder {
                            continue;
                        }
                        if ui.button(fname).clicked() {
                            actions.push(SidebarAction::MoveVm {
                                vm_uuid: uuid.to_string(),
                                target_folder: Some(i),
                            });
                            ui.close_menu();
                        }
                    }
                });
            });
        }

        if menu_item(ui, Some("\u{1F4CB}"), "Copy VM...", true, icon_width, None) {
            actions.push(SidebarAction::CopyVm(uuid.to_string()));
            ui.close_menu();
        }
        ui.separator();
        if menu_item(ui, Some("\u{1F5D1}"), "Delete VM", true, icon_width, Some(theme::error_red())) {
            actions.push(SidebarAction::DeleteVm(uuid.to_string()));
            ui.close_menu();
        }
    });
}

/// Sidebar state for rename dialog and drag&drop
pub struct SidebarState {
    pub rename_folder_idx: Option<usize>,
    pub rename_buffer: String,
    pub show_new_folder: bool,
    pub new_folder_name: String,
    pub confirm_delete_vm: Option<String>,
    /// Currently dragged VM UUID (persisted across frames)
    pub dragging_vm: Option<String>,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            rename_folder_idx: None,
            rename_buffer: String::new(),
            show_new_folder: false,
            new_folder_name: String::new(),
            confirm_delete_vm: None,
            dragging_vm: None,
        }
    }
}

pub fn render_sidebar(
    ctx: &egui::Context,
    layout: &mut SidebarLayout,
    vm_names: &HashMap<String, String>,
    vm_states: &HashMap<String, VmState>,
    vm_icons: &HashMap<String, egui::TextureId>,
    vm_errors: &HashMap<String, Vec<String>>,
    selected: &mut Option<String>,
    sidebar_state: &mut SidebarState,
) -> Vec<SidebarAction> {
    let mut actions = Vec::new();
    let mut drop_target: Option<(Option<usize>, usize)> = None;
    let folder_names: Vec<String> = layout.folders.iter().map(|f| f.name.clone()).collect();

    egui::SidePanel::left("sidebar")
        .exact_width(230.0)
        .frame(
            egui::Frame::new()
                .fill(theme::sidebar_bg())
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;

            // Sidebar toolbar — edge-to-edge, no margin
            {
                let toolbar_height = 26.0;
                let (toolbar_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), toolbar_height),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(
                    toolbar_rect,
                    egui::CornerRadius::ZERO,
                    theme::widget_bg_inactive(),
                );

                let inner = toolbar_rect.shrink2(egui::vec2(8.0, 2.0));
                let mut toolbar_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                toolbar_ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Machines")
                            .size(11.0)
                            .strong()
                            .color(theme::text_secondary()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().button_padding = egui::vec2(8.0, 1.0);
                        let add_btn = ui.add(
                            egui::Button::new(egui::RichText::new("+").size(11.0).strong().color(theme::text_on_accent()))
                                .fill(theme::accent_blue())
                                .corner_radius(egui::CornerRadius::same(3)),
                        ).on_hover_text("New...");
                        let menu_id = add_btn.id.with("add_menu");
                        if add_btn.clicked() {
                            ui.memory_mut(|mem| mem.toggle_popup(menu_id));
                        }
                        if ui.memory(|mem| mem.is_popup_open(menu_id)) {
                            let area_resp = egui::Area::new(menu_id)
                                .order(egui::Order::Foreground)
                                .fixed_pos(add_btn.rect.left_bottom())
                                .show(ui.ctx(), |ui| {
                                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                                        ui.set_min_width(180.0);
                                        ui.style_mut().visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                                        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                                        ui.style_mut().visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                                        if ui.add(egui::Button::new("\u{1F4C1}  Create Folder...").frame(false)).clicked() {
                                            sidebar_state.show_new_folder = true;
                                            sidebar_state.new_folder_name = "New Folder".to_string();
                                            ui.memory_mut(|mem| mem.close_popup());
                                        }
                                        ui.separator();
                                        if ui.add(egui::Button::new("\u{1F5A5}  Create Virtual Machine").frame(false)).clicked() {
                                            actions.push(SidebarAction::CreateVm);
                                            ui.memory_mut(|mem| mem.close_popup());
                                        }
                                    });
                                });
                            // Close on click outside, but not on the button itself (toggle handles that)
                            if area_resp.response.clicked_elsewhere() && !add_btn.rect.contains(ui.ctx().pointer_latest_pos().unwrap_or_default()) {
                                ui.memory_mut(|mem| mem.close_popup());
                            }
                        }
                    });
                });
            }
            ui.add_space(4.0);

            // Content area with padding
            let content_margin = egui::Margin::symmetric(10, 0);
            egui::Frame::new().inner_margin(content_margin).show(ui, |ui| {

            // New folder inline editor
            if sidebar_state.show_new_folder {
                ui.horizontal(|ui| {
                    ui.label("\u{1F4C1}");
                    let te = egui::TextEdit::singleline(&mut sidebar_state.new_folder_name)
                        .desired_width(140.0);
                    let resp = ui.add(te);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui.small_button("\u{2713}").clicked() {
                        let name = sidebar_state.new_folder_name.trim().to_string();
                        if !name.is_empty() {
                            layout.folders.push(FolderEntry {
                                name,
                                vm_uuids: Vec::new(),
                                expanded: true,
                            });
                        }
                        sidebar_state.show_new_folder = false;
                    }
                    if ui.small_button("\u{2717}").clicked() {
                        sidebar_state.show_new_folder = false;
                    }
                });
                ui.add_space(2.0);
            }

            // Rename folder inline editor
            if let Some(rename_idx) = sidebar_state.rename_folder_idx {
                if rename_idx < layout.folders.len() {
                    ui.horizontal(|ui| {
                        ui.label("\u{1F4C1}");
                        let te = egui::TextEdit::singleline(&mut sidebar_state.rename_buffer)
                            .desired_width(140.0);
                        let resp = ui.add(te);
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui.small_button("\u{2713}").clicked() {
                            let name = sidebar_state.rename_buffer.trim().to_string();
                            if !name.is_empty() {
                                layout.folders[rename_idx].name = name;
                            }
                            sidebar_state.rename_folder_idx = None;
                        }
                        if ui.small_button("\u{2717}").clicked() {
                            sidebar_state.rename_folder_idx = None;
                        }
                    });
                }
            }

            // Render folders
            let num_folders = layout.folders.len();
            for fi in 0..num_folders {
                let folder_name = layout.folders[fi].name.clone();
                let folder_expanded = layout.folders[fi].expanded;

                // Custom folder header: arrow (toggles expand) + label (selects folder / Exposé)
                let row_height = 22.0;
                let arrow_width = 18.0;
                let full_width = ui.available_width();
                let (header_rect, header_resp) = ui.allocate_exact_size(
                    egui::vec2(full_width, row_height),
                    egui::Sense::click(),
                );

                // Hover highlight on the whole header row
                if header_resp.hovered() {
                    ui.painter().rect_filled(header_rect, egui::CornerRadius::same(4), theme::widget_bg_hovered());
                }

                // Arrow region (left part) — only clicking here toggles expand/collapse
                let arrow_rect = egui::Rect::from_min_size(
                    header_rect.min,
                    egui::vec2(arrow_width, row_height),
                );
                let arrow_resp = ui.allocate_rect(arrow_rect, egui::Sense::click());
                if arrow_resp.clicked() {
                    layout.folders[fi].expanded = !layout.folders[fi].expanded;
                }

                // Draw the arrow
                let arrow_char = if folder_expanded { "\u{25BE}" } else { "\u{25B8}" }; // ▾ or ▸
                ui.painter().text(
                    arrow_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    arrow_char,
                    egui::FontId::proportional(11.0),
                    theme::text_secondary(),
                );

                // Folder icon + name (right of arrow) — clicking here selects folder (Exposé)
                let label_rect = egui::Rect::from_min_max(
                    egui::pos2(header_rect.min.x + arrow_width, header_rect.min.y),
                    header_rect.max,
                );
                let label_resp = ui.allocate_rect(label_rect, egui::Sense::click());

                ui.painter().text(
                    egui::pos2(label_rect.min.x + 2.0, label_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("\u{1F4C1} {}", folder_name),
                    egui::FontId::proportional(13.0),
                    theme::folder_header_text(),
                );

                // Click on folder label → select folder (Exposé view)
                if label_resp.clicked() {
                    *selected = None;
                    actions.push(SidebarAction::SelectFolder(fi));
                }

                // Context menu on the whole header
                header_resp.context_menu(|ui| {
                    ui.label(egui::RichText::new(&folder_name).strong());
                    ui.separator();
                    if ui.button("\u{270F} Rename").clicked() {
                        sidebar_state.rename_folder_idx = Some(fi);
                        sidebar_state.rename_buffer = folder_name.clone();
                        ui.close_menu();
                    }
                    if num_folders > 1 {
                        if ui.button("\u{1F5D1} Delete Folder").clicked() {
                            actions.push(SidebarAction::DeleteFolder(fi));
                            ui.close_menu();
                        }
                    }
                });

                // Drop target: if dragging a VM over this folder header
                if sidebar_state.dragging_vm.is_some() {
                    if header_resp.hovered() && ui.input(|i| i.pointer.any_released()) {
                        if let Some(vm_uuid) = sidebar_state.dragging_vm.take() {
                            actions.push(SidebarAction::MoveVm {
                                vm_uuid,
                                target_folder: Some(fi),
                            });
                        }
                    }
                }

                // Folder contents (only when expanded)
                if folder_expanded {
                    // Indent the contents slightly
                    ui.indent(format!("folder_content_{}", fi), |ui| {
                        let vm_uuids: Vec<String> = layout.folders[fi].vm_uuids.clone();
                        for (vi, uuid) in vm_uuids.iter().enumerate() {
                            render_vm_entry(
                                ui, uuid, vi, vm_names, vm_states, vm_icons, vm_errors, selected,
                                &mut sidebar_state.dragging_vm, &mut drop_target, &mut actions, &folder_names, Some(fi),
                            );
                        }
                        if vm_uuids.is_empty() {
                            ui.colored_label(
                                theme::empty_folder_text(),
                                egui::RichText::new("  No machines").size(12.0).italics(),
                            );
                        }
                    });
                }
            }

            // Root VMs (outside folders)
            if !layout.root_vms.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.colored_label(theme::text_tertiary(), egui::RichText::new("Unsorted").size(12.0));
                let root_uuids = layout.root_vms.clone();
                for (vi, uuid) in root_uuids.iter().enumerate() {
                    render_vm_entry(
                        ui, uuid, vi, vm_names, vm_states, vm_icons, vm_errors, selected,
                        &mut sidebar_state.dragging_vm, &mut drop_target, &mut actions, &folder_names, None,
                    );
                }
            }
            }); // close content Frame
        });

    // Handle drag&drop reorder: when mouse released while dragging a VM
    if let Some(vm_uuid) = sidebar_state.dragging_vm.clone() {
        if ctx.input(|i| i.pointer.any_released()) {
            if let Some((folder, insert_index)) = drop_target {
                actions.push(SidebarAction::ReorderVm {
                    vm_uuid,
                    folder,
                    insert_index,
                });
            }
            sidebar_state.dragging_vm = None;
        }
    }

    actions
}
