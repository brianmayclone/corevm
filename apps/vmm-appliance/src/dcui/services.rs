use std::path::Path;
use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::SelectList;

pub enum DialogResult {
    Continue,
    Close,
}

pub struct Dialog {
    action_list: SelectList,
    service_name: String,
    status: String,
    message: Option<(String, bool)>,
}

impl Dialog {
    pub fn new() -> Self {
        let service_name = detect_service();
        let status = get_service_status(&service_name);

        let mut action_list = SelectList::new(
            "Action:",
            vec![
                "Start".to_string(),
                "Stop".to_string(),
                "Restart".to_string(),
                "Enable".to_string(),
                "Disable".to_string(),
            ],
        );
        // Default to Restart
        action_list.selected = 2;
        action_list.focused = true;

        Self {
            action_list,
            service_name,
            status,
            message: None,
        }
    }

    fn execute_action(&mut self) {
        let action = match self.action_list.selected_item() {
            Some(a) => a.to_lowercase(),
            None => return,
        };

        match Command::new("systemctl")
            .args([action.as_str(), self.service_name.as_str()])
            .status()
        {
            Ok(s) if s.success() => {
                self.status = get_service_status(&self.service_name);
                self.message = Some((
                    format!("'{}' on {} succeeded. Status: {}", action, self.service_name, self.status),
                    false,
                ));
            }
            Ok(_) => {
                self.message = Some((
                    format!("'{}' on {} failed.", action, self.service_name),
                    true,
                ));
            }
            Err(e) => {
                self.message = Some((format!("Error: {}", e), true));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        if self.message.is_some() {
            self.message = None;
            return DialogResult::Close;
        }

        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Enter => self.execute_action(),
            _ => self.action_list.handle_key(key),
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(60).max(40);
        let popup_height = 18u16.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let title = format!(" Services (F5) - {} ", self.service_name);
        let block = Block::default()
            .title(title)
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

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(8),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

        let status_color = match self.status.as_str() {
            "active" => Color::Green,
            "inactive" => Color::Yellow,
            _ => Color::Red,
        };
        Paragraph::new(format!("Status: {}", self.status))
            .style(Style::default().fg(status_color).add_modifier(Modifier::BOLD))
            .render(chunks[0], buf);

        self.action_list.render(chunks[1], buf);

        Paragraph::new("[↑↓] Select action  [Enter] Execute  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[3], buf);
    }
}

fn detect_service() -> String {
    if Path::new("/etc/vmm/vmm-cluster.toml").exists() {
        "vmm-cluster".to_string()
    } else {
        "vmm-server".to_string()
    }
}

fn get_service_status(service: &str) -> String {
    Command::new("systemctl")
        .args(["is-active", service])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
