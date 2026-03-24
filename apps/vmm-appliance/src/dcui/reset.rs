use std::fs;
use std::path::Path;
use std::process::Command;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::config::{write_default_config, ApplianceRole};
use crate::common::widgets::ConfirmDialog;

pub enum DialogResult {
    Continue,
    Close,
}

#[derive(PartialEq)]
enum Stage {
    Confirm1,
    Confirm2,
    ConfirmImages,
    Resetting,
    Done,
}

pub struct Dialog {
    confirm1: ConfirmDialog,
    confirm2: ConfirmDialog,
    confirm_images: ConfirmDialog,
    stage: Stage,
    message: Option<(String, bool)>,
}

impl Dialog {
    pub fn new() -> Self {
        let confirm1 = ConfirmDialog::new(
            " Factory Reset ",
            "Are you sure you want to factory reset this appliance?",
        );
        let confirm2 = ConfirmDialog::new(
            " Confirm Reset ",
            "This will delete ALL configuration. This cannot be undone. Continue?",
        );
        let confirm_images = ConfirmDialog::new(
            " Delete VM Images? ",
            "Also delete all VM disk images in /var/lib/vmm/images/? This is IRREVERSIBLE.",
        );

        Self {
            confirm1,
            confirm2,
            confirm_images,
            stage: Stage::Confirm1,
            message: None,
        }
    }

    fn do_reset(&mut self, delete_images: bool) {
        self.stage = Stage::Resetting;

        // Stop services
        for svc in &["vmm-server", "vmm-cluster"] {
            let _ = Command::new("systemctl").args(["stop", svc]).status();
        }

        // Delete databases
        for db in &["/var/lib/vmm/vmm.db", "/var/lib/vmm/vmm-cluster.db"] {
            if Path::new(db).exists() {
                let _ = fs::remove_file(db);
            }
        }

        // Delete VM images if requested
        if delete_images {
            let images_dir = "/var/lib/vmm/images";
            if Path::new(images_dir).exists() {
                let _ = fs::remove_dir_all(images_dir);
                let _ = fs::create_dir_all(images_dir);
            }
        }

        // Write default config
        let role = detect_role();
        match write_default_config(Path::new("/"), &role) {
            Ok(_) => {}
            Err(e) => {
                self.message = Some((format!("Reset partially failed: {}", e), true));
                self.stage = Stage::Done;
                return;
            }
        }

        // Restart services
        let service = match role {
            ApplianceRole::Server => "vmm-server",
            ApplianceRole::Cluster => "vmm-cluster",
        };
        let _ = Command::new("systemctl").args(["start", service]).status();

        self.message = Some(("Factory reset complete. Configuration restored to defaults.".to_string(), false));
        self.stage = Stage::Done;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        if self.message.is_some() {
            self.message = None;
            return DialogResult::Close;
        }

        match self.stage {
            Stage::Confirm1 => {
                self.confirm1.handle_key(key);
                if let Some(confirmed) = self.confirm1.confirmed {
                    if confirmed {
                        self.stage = Stage::Confirm2;
                    } else {
                        return DialogResult::Close;
                    }
                }
            }
            Stage::Confirm2 => {
                self.confirm2.handle_key(key);
                if let Some(confirmed) = self.confirm2.confirmed {
                    if confirmed {
                        self.stage = Stage::ConfirmImages;
                    } else {
                        return DialogResult::Close;
                    }
                }
            }
            Stage::ConfirmImages => {
                self.confirm_images.handle_key(key);
                if let Some(delete_images) = self.confirm_images.confirmed {
                    self.do_reset(delete_images);
                }
            }
            Stage::Resetting => {
                // waiting, ignore input
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
                .title(" Reset Result ")
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
            Stage::Confirm1 => self.confirm1.render(area, frame.buffer_mut()),
            Stage::Confirm2 => self.confirm2.render(area, frame.buffer_mut()),
            Stage::ConfirmImages => self.confirm_images.render(area, frame.buffer_mut()),
            Stage::Resetting | Stage::Done => {
                let popup_width = area.width.min(50).max(30);
                let popup_height = 5u16.min(area.height);
                let popup = centered_rect(popup_width, popup_height, area);
                let buf = frame.buffer_mut();
                Clear.render(popup, buf);
                let block = Block::default()
                    .title(" Factory Reset ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red));
                let inner = block.inner(popup);
                block.render(popup, buf);
                Paragraph::new("Resetting... please wait.")
                    .style(Style::default().fg(Color::Yellow))
                    .render(inner, buf);
            }
        }
    }
}

fn detect_role() -> ApplianceRole {
    if Path::new("/etc/vmm/vmm-cluster.toml").exists() {
        ApplianceRole::Cluster
    } else {
        ApplianceRole::Server
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
