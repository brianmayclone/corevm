use std::io::Write as IoWrite;
use std::process::{Command, Stdio};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::{PasswordInput, SelectList};

pub enum DialogResult {
    Continue,
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    User,
    Password,
    Confirm,
}

pub struct Dialog {
    user_list: SelectList,
    password: PasswordInput,
    confirm: PasswordInput,
    focus: Field,
    message: Option<(String, bool)>,
}

impl Dialog {
    pub fn new() -> Self {
        let users = load_users();
        let mut user_list = SelectList::new("Select user:", users);
        user_list.focused = true;

        let password = PasswordInput::new("New password:");
        let confirm = PasswordInput::new("Confirm password:");

        Self {
            user_list,
            password,
            confirm,
            focus: Field::User,
            message: None,
        }
    }

    fn set_focus(&mut self, f: Field) {
        self.user_list.focused = f == Field::User;
        self.password.set_focused(f == Field::Password);
        self.confirm.set_focused(f == Field::Confirm);
        self.focus = f;
    }

    fn save(&mut self) {
        let pass = self.password.value().to_string();
        let conf = self.confirm.value().to_string();

        if pass.is_empty() {
            self.message = Some(("Password cannot be empty.".to_string(), true));
            return;
        }
        if pass != conf {
            self.message = Some(("Passwords do not match.".to_string(), true));
            return;
        }

        let user = match self.user_list.selected_item() {
            Some(u) => u.to_string(),
            None => {
                self.message = Some(("No user selected.".to_string(), true));
                return;
            }
        };

        match run_chpasswd(&user, &pass) {
            Ok(_) => {
                self.message = Some((format!("Password for '{}' changed.", user), false));
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
            KeyCode::Tab => {
                let next = match self.focus {
                    Field::User => Field::Password,
                    Field::Password => Field::Confirm,
                    Field::Confirm => Field::User,
                };
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = match self.focus {
                    Field::User => Field::Confirm,
                    Field::Password => Field::User,
                    Field::Confirm => Field::Password,
                };
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                if self.focus == Field::Confirm {
                    self.save();
                } else {
                    let next = match self.focus {
                        Field::User => Field::Password,
                        Field::Password => Field::Confirm,
                        Field::Confirm => Field::Confirm,
                    };
                    self.set_focus(next);
                }
            }
            _ => match self.focus {
                Field::User => self.user_list.handle_key(key),
                Field::Password => self.password.handle_key(key),
                Field::Confirm => self.confirm.handle_key(key),
            },
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let popup_width = area.width.min(60).max(40);
        let popup_height = 20u16.min(area.height);
        let popup = centered_rect(popup_width, popup_height, area);

        let buf = frame.buffer_mut();
        Clear.render(popup, buf);

        let block = Block::default()
            .title(" Change Password (F2) ")
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
                Constraint::Length(5),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

        self.user_list.render(chunks[0], buf);
        self.password.render(chunks[1], buf);
        self.confirm.render(chunks[2], buf);
        Paragraph::new("[Tab] Next  [Enter on Confirm] Save  [Esc] Cancel")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[4], buf);
    }
}

fn load_users() -> Vec<String> {
    let mut users = vec!["root".to_string()];
    if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 7 {
                let uid: u32 = parts[2].parse().unwrap_or(0);
                let shell = parts[6];
                if uid >= 1000 && !shell.contains("nologin") && !shell.contains("false") {
                    users.push(parts[0].to_string());
                }
            }
        }
    }
    users
}

fn run_chpasswd(user: &str, password: &str) -> anyhow::Result<()> {
    let input = format!("{}:{}", user, password);
    let mut child = Command::new("chpasswd")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn chpasswd: {}", e))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to write to chpasswd: {}", e))?;
    }
    let status = child.wait()
        .map_err(|e| anyhow::anyhow!("Failed to wait for chpasswd: {}", e))?;
    if !status.success() {
        return Err(anyhow::anyhow!("chpasswd failed for user {}", user));
    }
    Ok(())
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
