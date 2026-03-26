use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::SelectList;

/// All CoreVM services managed by the appliance.
const SERVICES: &[(&str, &str)] = &[
    ("vmm-server",  "VMM Server — VM management API"),
    ("vmm-cluster", "VMM Cluster — Central authority"),
    ("vmm-san",     "CoreSAN — Software-Defined Storage"),
];

pub enum DialogResult {
    Continue,
    Close,
}

pub struct Dialog {
    service_list: SelectList,
    action_list: SelectList,
    focus: Focus,
    statuses: Vec<String>,
    message: Option<(String, bool)>,
}

#[derive(PartialEq)]
enum Focus {
    Service,
    Action,
}

impl Dialog {
    pub fn new() -> Self {
        let service_names: Vec<String> = SERVICES.iter()
            .map(|(name, desc)| format!("{:<14} {}", name, desc))
            .collect();

        let statuses: Vec<String> = SERVICES.iter()
            .map(|(name, _)| get_service_status(name))
            .collect();

        let mut service_list = SelectList::new("Service:", service_names);
        service_list.focused = true;

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
        action_list.selected = 2; // Default to Restart

        Self {
            service_list,
            action_list,
            focus: Focus::Service,
            statuses,
            message: None,
        }
    }

    fn selected_service(&self) -> &str {
        SERVICES.get(self.service_list.selected)
            .map(|(name, _)| *name)
            .unwrap_or("vmm-server")
    }

    fn execute_action(&mut self) {
        let service = self.selected_service().to_string();
        let action = match self.action_list.selected_item() {
            Some(a) => a.to_lowercase(),
            None => return,
        };

        match Command::new("systemctl")
            .args([action.as_str(), service.as_str()])
            .status()
        {
            Ok(s) if s.success() => {
                // Refresh all statuses
                for (i, (name, _)) in SERVICES.iter().enumerate() {
                    self.statuses[i] = get_service_status(name);
                }
                let status = &self.statuses[self.service_list.selected];
                self.message = Some((
                    format!("'{}' on {} succeeded. Status: {}", action, service, status),
                    false,
                ));
            }
            Ok(_) => {
                self.message = Some((
                    format!("'{}' on {} failed.", action, service),
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
            return DialogResult::Continue;
        }

        match key.code {
            KeyCode::Esc => return DialogResult::Close,
            KeyCode::Tab => {
                // Toggle focus between service list and action list
                match self.focus {
                    Focus::Service => {
                        self.focus = Focus::Action;
                        self.service_list.focused = false;
                        self.action_list.focused = true;
                    }
                    Focus::Action => {
                        self.focus = Focus::Service;
                        self.service_list.focused = true;
                        self.action_list.focused = false;
                    }
                }
            }
            KeyCode::Enter => {
                if self.focus == Focus::Action {
                    self.execute_action();
                } else {
                    // Switch to action list on Enter in service list
                    self.focus = Focus::Action;
                    self.service_list.focused = false;
                    self.action_list.focused = true;
                }
            }
            _ => {
                match self.focus {
                    Focus::Service => self.service_list.handle_key(key),
                    Focus::Action => self.action_list.handle_key(key),
                }
            }
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(70).max(50);
        let popup_height = 22u16.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" Services (F5) ")
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
                Constraint::Length(2),  // Status overview
                Constraint::Length(5),  // Service list
                Constraint::Length(1),  // Separator
                Constraint::Length(7),  // Action list
                Constraint::Min(1),
                Constraint::Length(1),  // Help
            ])
            .split(inner);

        // Status overview for all services
        let status_line: String = SERVICES.iter().enumerate()
            .map(|(i, (name, _))| {
                let status = &self.statuses[i];
                let indicator = match status.as_str() {
                    "active" => "●",
                    "inactive" => "○",
                    _ => "✖",
                };
                format!("{} {}", indicator, name)
            })
            .collect::<Vec<_>>()
            .join("  ");

        Paragraph::new(status_line)
            .style(Style::default().fg(Color::White))
            .render(chunks[0], buf);

        // Service selector
        self.service_list.render(chunks[1], buf);

        // Current service status
        let sel_status = &self.statuses[self.service_list.selected];
        let status_color = match sel_status.as_str() {
            "active" => Color::Green,
            "inactive" => Color::Yellow,
            _ => Color::Red,
        };
        Paragraph::new(format!("── {} ── status: {} ──", self.selected_service(), sel_status))
            .style(Style::default().fg(status_color))
            .render(chunks[2], buf);

        // Action list
        self.action_list.render(chunks[3], buf);

        Paragraph::new("[Tab] Switch panel  [↑↓] Select  [Enter] Execute  [Esc] Close")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[5], buf);
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
