use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::system::{detect_disks, is_efi_booted, get_ram_bytes, DiskInfo};
use crate::common::widgets::{SelectList, ConfirmDialog};
use super::{InstallConfig, ScreenResult};

#[derive(Debug, Clone, Copy, PartialEq)]
enum DiskFocus {
    List,
    Confirm,
}

pub struct DiskState {
    disks: Vec<DiskInfo>,
    list: SelectList,
    confirm: Option<ConfirmDialog>,
    focus: DiskFocus,
    error: Option<String>,
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const TB: u64 = 1_000_000_000_000;
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else {
        format!("{:.0} GB", bytes as f64 / GB as f64)
    }
}

fn build_partition_preview(disk_path: &str, disk_size_bytes: u64, efi: bool) -> String {
    let ram_gib = get_ram_bytes() / (1024 * 1024 * 1024);
    let disk_size_mib = disk_size_bytes / (1024 * 1024);
    let fixed_mib: u64 = 1 + if efi { 256 } else { 1 } + 512 + 8 * 1024 + 1024;
    let available_for_swap_mib = disk_size_mib.saturating_sub(fixed_mib);
    let swap_gb = ram_gib.min(8).max(1).min(available_for_swap_mib / 1024);

    let mut lines = Vec::new();
    if efi {
        lines.push("  /boot/efi  — 256 MB  (EFI System)".to_string());
    }
    lines.push("  /boot      — 512 MB".to_string());
    lines.push(format!("  swap       — {} GB", swap_gb));
    lines.push("  /          — 8 GB".to_string());
    lines.push("  /var/lib/vmm — remainder".to_string());

    format!(
        "ALL DATA on {} will be ERASED!\n\nPartition layout:\n{}",
        disk_path,
        lines.join("\n")
    )
}

impl DiskState {
    pub fn new() -> Self {
        let disks = detect_disks().unwrap_or_default();

        let items: Vec<String> = disks
            .iter()
            .map(|d| {
                let size = format_size(d.size_bytes);
                let path = d.path.display().to_string();
                if d.model.is_empty() {
                    format!("{} — {}", path, size)
                } else {
                    format!("{} — {} — {}", path, size, d.model)
                }
            })
            .collect();

        let mut list = SelectList::new("Select installation disk:", items);
        list.focused = true;

        Self {
            disks,
            list,
            confirm: None,
            focus: DiskFocus::List,
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match self.focus {
            DiskFocus::Confirm => {
                if let Some(dialog) = self.confirm.as_mut() {
                    dialog.handle_key(key);
                    if let Some(confirmed) = dialog.confirmed {
                        if confirmed {
                            let disk = &self.disks[self.list.selected];
                            config.disk = Some(disk.path.clone());
                            self.confirm = None;
                            self.focus = DiskFocus::List;
                            return ScreenResult::Next;
                        } else {
                            self.confirm = None;
                            self.focus = DiskFocus::List;
                            self.list.focused = true;
                        }
                    }
                }
            }
            DiskFocus::List => {
                match key.code {
                    KeyCode::Esc => return ScreenResult::Prev,
                    KeyCode::Enter => {
                        if self.disks.is_empty() {
                            self.error = Some("No disks detected. Cannot proceed.".to_string());
                        } else {
                            let disk = &self.disks[self.list.selected];
                            let path_str = disk.path.display().to_string();
                            let efi = is_efi_booted();
                            let msg = build_partition_preview(&path_str, disk.size_bytes, efi);
                            self.confirm = Some(ConfirmDialog::new(
                                " Confirm Disk Erasure ",
                                &msg,
                            ));
                            self.focus = DiskFocus::Confirm;
                            self.list.focused = false;
                        }
                    }
                    _ => {
                        self.list.handle_key(key);
                    }
                }
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        ratatui::widgets::Block::default()
            .style(Style::default().bg(Color::Black))
            .render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(1),   // 0: title
                Constraint::Length(1),   // 1: gap
                Constraint::Min(5),      // 2: disk list
                Constraint::Length(1),   // 3: gap
                Constraint::Length(1),   // 4: error or blank
                Constraint::Min(0),      // 5: spacer
                Constraint::Length(1),   // 6: help
            ])
            .split(area);

        Paragraph::new("Disk Selection")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .render(chunks[0], buf);

        let list_area = centered_horizontal(chunks[2], 70);
        self.list.render(list_area, buf);

        if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center)
                .render(chunks[4], buf);
        }

        Paragraph::new("[↑↓] Select disk  [Enter] Confirm  [Esc] Back")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .render(chunks[6], buf);

        if let Some(dialog) = &self.confirm {
            dialog.render(area, buf);
        }
    }
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(40).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
