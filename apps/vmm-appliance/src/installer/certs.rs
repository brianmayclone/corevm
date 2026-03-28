use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::widgets::{SelectList, TextInput, render_installer_frame};
use super::{InstallConfig, ScreenResult};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    Mode,
    CertPath,
    KeyPath,
}

impl Field {
    fn next(self, import: bool) -> Self {
        match self {
            Field::Mode     => if import { Field::CertPath } else { Field::Mode },
            Field::CertPath => Field::KeyPath,
            Field::KeyPath  => Field::Mode,
        }
    }
    fn prev(self, import: bool) -> Self {
        match self {
            Field::Mode     => if import { Field::KeyPath } else { Field::Mode },
            Field::CertPath => Field::Mode,
            Field::KeyPath  => Field::CertPath,
        }
    }
}

pub struct CertsState {
    mode_list: SelectList,
    cert_path: TextInput,
    key_path: TextInput,
    focus: Field,
    error: Option<String>,
}

impl CertsState {
    pub fn new() -> Self {
        let mut mode_list = SelectList::new(
            "Certificate Mode:",
            vec![
                "Generate self-signed".to_string(),
                "Import custom".to_string(),
            ],
        );
        mode_list.focused = true;

        Self {
            mode_list,
            cert_path: TextInput::new("Certificate Path:"),
            key_path: TextInput::new("Private Key Path:"),
            focus: Field::Mode,
            error: None,
        }
    }

    fn is_import(&self) -> bool {
        self.mode_list.selected == 1
    }

    fn set_focus(&mut self, field: Field) {
        self.mode_list.focused = false;
        self.cert_path.focused = false;
        self.key_path.focused = false;
        match field {
            Field::Mode     => self.mode_list.focused = true,
            Field::CertPath => self.cert_path.focused = true,
            Field::KeyPath  => self.key_path.focused = true,
        }
        self.focus = field;
    }

    fn validate(&self) -> Result<(), String> {
        if self.is_import() {
            if self.cert_path.value.trim().is_empty() {
                return Err("Certificate path must not be empty.".to_string());
            }
            if self.key_path.value.trim().is_empty() {
                return Err("Key path must not be empty.".to_string());
            }
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        let import = self.is_import();
        match key.code {
            KeyCode::Esc => return ScreenResult::Prev,
            KeyCode::Tab => {
                let next = self.focus.next(import);
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = self.focus.prev(import);
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                match self.validate() {
                    Ok(()) => {
                        config.self_signed_cert = !self.is_import();
                        if self.is_import() {
                            config.cert_path = Some(self.cert_path.value.trim().into());
                            config.key_path = Some(self.key_path.value.trim().into());
                        } else {
                            config.cert_path = None;
                            config.key_path = None;
                        }
                        return ScreenResult::Next;
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
            _ => {
                match self.focus {
                    Field::Mode     => self.mode_list.handle_key(key),
                    Field::CertPath => self.cert_path.handle_key(key),
                    Field::KeyPath  => self.key_path.handle_key(key),
                }
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let content = render_installer_frame(
            area, buf,
            "TLS Certificates",
            "[Tab] Switch field  [↑↓] Select mode  [Enter] Continue  [Esc] Back",
            Some((7, 8)),
        );

        let import = self.is_import();

        let col = centered_horizontal(content, 60);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),   // 0: mode list
                Constraint::Length(1),   // 1: gap
                Constraint::Length(4),   // 2: cert path
                Constraint::Length(4),   // 3: key path
                Constraint::Min(0),      // 4: spacer
                Constraint::Length(1),   // 5: error
            ])
            .split(content);

        macro_rules! col_rect {
            ($chunk:expr) => {
                Rect { y: $chunk.y, height: $chunk.height, x: col.x, width: col.width }
            };
        }

        self.mode_list.render(col_rect!(chunks[0]), buf);

        let grey = Style::default().fg(Color::DarkGray);

        if import {
            self.cert_path.render(col_rect!(chunks[2]), buf);
            self.key_path.render(col_rect!(chunks[3]), buf);
        } else {
            render_greyed_field("Certificate Path:", col_rect!(chunks[2]), buf, grey);
            render_greyed_field("Private Key Path:", col_rect!(chunks[3]), buf, grey);
        }

        if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center)
                .render(chunks[5], buf);
        }
    }
}

fn render_greyed_field(label: &str, area: Rect, buf: &mut Buffer, style: Style) {
    if area.height < 2 {
        return;
    }
    let label_area = Rect { height: 1, ..area };
    let box_area = Rect { y: area.y + 1, height: area.height - 1, ..area };
    Paragraph::new(label)
        .style(style)
        .render(label_area, buf);
    ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .render(box_area, buf);
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(40).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
