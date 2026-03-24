use std::path::Path;
use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::certs::{generate_self_signed, import_certificates};
use crate::common::widgets::{SelectList, TextInput};

pub enum DialogResult {
    Continue,
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Mode,
    CertPath,
    KeyPath,
}

pub struct Dialog {
    mode_list: SelectList,
    cert_path: TextInput,
    key_path: TextInput,
    focus: Field,
    message: Option<(String, bool)>,
}

impl Dialog {
    pub fn new() -> Self {
        let mut mode_list = SelectList::new(
            "Action:",
            vec![
                "Regenerate self-signed".to_string(),
                "Import custom certificates".to_string(),
            ],
        );
        mode_list.focused = true;

        let cert_path = TextInput::new("Certificate path:");
        let key_path = TextInput::new("Key path:");

        Self {
            mode_list,
            cert_path,
            key_path,
            focus: Field::Mode,
            message: None,
        }
    }

    fn is_import(&self) -> bool {
        self.mode_list.selected == 1
    }

    fn set_focus(&mut self, f: Field) {
        self.mode_list.focused = f == Field::Mode;
        self.cert_path.focused = f == Field::CertPath;
        self.key_path.focused = f == Field::KeyPath;
        self.focus = f;
    }

    fn save(&mut self) {
        let tls_target = Path::new("/");

        if self.is_import() {
            let cert = self.cert_path.value.trim().to_string();
            let key = self.key_path.value.trim().to_string();

            if cert.is_empty() || key.is_empty() {
                self.message = Some(("Certificate and key paths are required.".to_string(), true));
                return;
            }

            if !Path::new(&cert).exists() {
                self.message = Some((format!("Certificate not found: {}", cert), true));
                return;
            }
            if !Path::new(&key).exists() {
                self.message = Some((format!("Key not found: {}", key), true));
                return;
            }

            match import_certificates(tls_target, Path::new(&cert), Path::new(&key)) {
                Ok(_) => {}
                Err(e) => {
                    self.message = Some((format!("Import failed: {}", e), true));
                    return;
                }
            }
        } else {
            let hostname = std::fs::read_to_string("/etc/hostname")
                .unwrap_or_else(|_| "corevm-appliance".to_string());
            let cn = hostname.trim().to_string();

            match generate_self_signed(tls_target, &cn) {
                Ok(_) => {}
                Err(e) => {
                    self.message = Some((format!("Generation failed: {}", e), true));
                    return;
                }
            }
        }

        // Restart service
        let service = detect_service();
        let _ = Command::new("systemctl").args(["restart", &service]).status();

        self.message = Some(("Certificates updated. Service restarted.".to_string(), false));
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        if self.message.is_some() {
            self.message = None;
            return DialogResult::Close;
        }

        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Tab => {
                let next = match self.focus {
                    Field::Mode => {
                        if self.is_import() { Field::CertPath } else { Field::Mode }
                    }
                    Field::CertPath => Field::KeyPath,
                    Field::KeyPath => Field::Mode,
                };
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = match self.focus {
                    Field::Mode => {
                        if self.is_import() { Field::KeyPath } else { Field::Mode }
                    }
                    Field::CertPath => Field::Mode,
                    Field::KeyPath => Field::CertPath,
                };
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                if self.focus == Field::Mode && !self.is_import() {
                    self.save();
                } else if self.focus == Field::KeyPath {
                    self.save();
                } else {
                    let import = self.is_import();
                    let next = match self.focus {
                        Field::Mode => if import { Field::CertPath } else { Field::Mode },
                        Field::CertPath => Field::KeyPath,
                        Field::KeyPath => Field::KeyPath,
                    };
                    self.set_focus(next);
                }
            }
            _ => match self.focus {
                Field::Mode => self.mode_list.handle_key(key),
                Field::CertPath => self.cert_path.handle_key(key),
                Field::KeyPath => self.key_path.handle_key(key),
            },
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(65).max(45);
        let popup_height = if self.is_import() { 22u16 } else { 16u16 }.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" TLS Certificates (F4) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if let Some((msg, is_err)) = &self.message {
            let color = if *is_err { Color::Red } else { Color::Green };
            Paragraph::new(msg.as_str())
                .style(Style::default().fg(color))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .render(inner, buf);
            return;
        }

        if self.is_import() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            self.mode_list.render(chunks[0], buf);
            self.cert_path.render(chunks[1], buf);
            self.key_path.render(chunks[2], buf);
            Paragraph::new("[Tab] Next  [Enter on Key Path] Save  [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray))
                .render(chunks[4], buf);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            self.mode_list.render(chunks[0], buf);
            Paragraph::new("[Enter] Generate  [Tab] Switch mode  [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray))
                .render(chunks[2], buf);
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
