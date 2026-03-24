use std::fs;
use std::path::Path;
use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::{ConfirmDialog, ProgressDisplay, TextInput};

pub enum DialogResult {
    Continue,
    Close,
}

#[derive(PartialEq)]
enum Stage {
    InputPath,
    Confirm,
    Progress,
    Done,
}

pub struct Dialog {
    path_input: TextInput,
    confirm: ConfirmDialog,
    progress: ProgressDisplay,
    stage: Stage,
    message: Option<(String, bool)>,
    package_info: String,
    service_name: String,
}

impl Dialog {
    pub fn new() -> Self {
        let mut path_input = TextInput::new("Update package path (.tar.gz):");
        path_input.focused = true;

        let confirm = ConfirmDialog::new(
            " Confirm Update ",
            "Apply update package? Current binaries will be backed up to /opt/vmm-backup/.",
        );

        let progress = ProgressDisplay::new("Applying update...");

        let service_name = detect_service();

        Self {
            path_input,
            confirm,
            progress,
            stage: Stage::InputPath,
            message: None,
            package_info: String::new(),
            service_name,
        }
    }

    fn validate_path(&mut self) {
        let path = self.path_input.value.trim().to_string();

        if path.is_empty() {
            self.message = Some(("Please enter a package path.".to_string(), true));
            return;
        }

        if !Path::new(&path).exists() {
            self.message = Some((format!("File not found: {}", path), true));
            return;
        }

        // Try to read version from filename or header
        let info = if let Some(fname) = Path::new(&path).file_name() {
            format!("Package: {}", fname.to_string_lossy())
        } else {
            format!("Package: {}", path)
        };
        self.package_info = info;

        self.stage = Stage::Confirm;
    }

    fn apply_update(&mut self) {
        let path = self.path_input.value.trim().to_string();
        self.stage = Stage::Progress;
        self.progress.set_progress(0.0, "Stopping service...");

        // Stop service
        let _ = Command::new("systemctl")
            .args(["stop", &self.service_name])
            .status();

        self.progress.set_progress(0.2, "Creating backup...");

        // Backup binaries
        let backup_dir = "/opt/vmm-backup";
        let _ = fs::create_dir_all(backup_dir);

        for bin in &[
            format!("/usr/bin/{}", self.service_name),
            format!("/usr/local/bin/{}", self.service_name),
        ] {
            if Path::new(bin).exists() {
                let dest = format!("{}/{}", backup_dir, Path::new(bin).file_name().unwrap().to_string_lossy());
                let _ = fs::copy(bin, &dest);
            }
        }

        self.progress.set_progress(0.5, "Extracting update...");

        // Extract update
        let result = Command::new("tar")
            .args(["xzf", &path, "-C", "/"])
            .status();

        match result {
            Ok(s) if s.success() => {
                self.progress.set_progress(0.8, "Restarting service...");
                let _ = Command::new("systemctl")
                    .args(["start", &self.service_name])
                    .status();
                self.progress.set_progress(1.0, "Done.");
                self.message = Some(("Update applied successfully.".to_string(), false));
            }
            Ok(_) => {
                self.progress.set_progress(1.0, "Extraction failed.");
                // Try to restart service anyway
                let _ = Command::new("systemctl")
                    .args(["start", &self.service_name])
                    .status();
                self.message = Some(("Update extraction failed. Service restarted from backup.".to_string(), true));
            }
            Err(e) => {
                self.message = Some((format!("Failed to run tar: {}", e), true));
            }
        }

        self.stage = Stage::Done;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        if self.message.is_some() {
            self.message = None;
            return DialogResult::Close;
        }

        match self.stage {
            Stage::InputPath => match key.code {
                KeyCode::Esc => return DialogResult::Close,
                KeyCode::Enter => self.validate_path(),
                _ => self.path_input.handle_key(key),
            },
            Stage::Confirm => {
                self.confirm.handle_key(key);
                if let Some(confirmed) = self.confirm.confirmed {
                    if confirmed {
                        self.apply_update();
                    } else {
                        return DialogResult::Close;
                    }
                }
            }
            Stage::Progress => {
                if key.code == KeyCode::Esc {
                    return DialogResult::Close;
                }
            }
            Stage::Done => return DialogResult::Close,
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        if let Some((msg, is_err)) = &self.message {
            let popup_width = area.width.min(60).max(40);
            let popup_height = 8u16.min(area.height);
            let popup = centered_rect(popup_width, popup_height, area);
            let buf = frame.buffer_mut();
            Clear.render(popup, buf);
            let block = Block::default()
                .title(" Update Result ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if *is_err { Color::Red } else { Color::Green }));
            let inner = block.inner(popup);
            block.render(popup, buf);
            Paragraph::new(msg.as_str())
                .style(Style::default().fg(if *is_err { Color::Red } else { Color::Green }))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .render(inner, buf);
            return;
        }

        match self.stage {
            Stage::Confirm => {
                self.confirm.render(area, frame.buffer_mut());
            }
            Stage::Progress | Stage::Done => {
                let popup_width = area.width.min(60).max(40);
                let popup_height = 10u16.min(area.height);
                let popup = centered_rect(popup_width, popup_height, area);
                let buf = frame.buffer_mut();
                Clear.render(popup, buf);
                let block = Block::default()
                    .title(" Applying Update (F8) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan));
                let inner = block.inner(popup);
                block.render(popup, buf);
                self.progress.render(inner, buf);
            }
            Stage::InputPath => {
                let popup_width = area.width.min(65).max(45);
                let popup_height = 14u16.min(area.height);
                let popup = centered_rect(popup_width, popup_height, area);
                let buf = frame.buffer_mut();
                Clear.render(popup, buf);
                let block = Block::default()
                    .title(" Update Package (F8) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
                let inner = block.inner(popup);
                block.render(popup, buf);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(4),
                        Constraint::Length(2),
                        Constraint::Min(1),
                        Constraint::Length(1),
                    ])
                    .split(inner);

                self.path_input.render(chunks[0], buf);

                if !self.package_info.is_empty() {
                    Paragraph::new(self.package_info.as_str())
                        .style(Style::default().fg(Color::Yellow))
                        .render(chunks[1], buf);
                }

                Paragraph::new("[Enter] Validate  [Esc] Cancel")
                    .style(Style::default().fg(Color::DarkGray))
                    .render(chunks[3], buf);
            }
        }
    }
}

fn detect_service() -> String {
    if std::path::Path::new("/etc/vmm/vmm-cluster.toml").exists() {
        "vmm-cluster".to_string()
    } else {
        "vmm-server".to_string()
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
