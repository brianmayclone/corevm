use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::widgets::{PasswordInput, TextInput};
use super::{InstallConfig, ScreenResult};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Field {
    RootPassword,
    RootConfirm,
    Username,
    UserPassword,
    UserConfirm,
}

impl Field {
    fn next(self) -> Self {
        match self {
            Field::RootPassword => Field::RootConfirm,
            Field::RootConfirm  => Field::Username,
            Field::Username     => Field::UserPassword,
            Field::UserPassword => Field::UserConfirm,
            Field::UserConfirm  => Field::RootPassword,
        }
    }

    fn prev(self) -> Self {
        match self {
            Field::RootPassword => Field::UserConfirm,
            Field::RootConfirm  => Field::RootPassword,
            Field::Username     => Field::RootConfirm,
            Field::UserPassword => Field::Username,
            Field::UserConfirm  => Field::UserPassword,
        }
    }
}

pub struct UsersState {
    root_password: PasswordInput,
    root_confirm: PasswordInput,
    username: TextInput,
    user_password: PasswordInput,
    user_confirm: PasswordInput,
    focus: Field,
    error: Option<String>,
}

impl UsersState {
    pub fn new() -> Self {
        let mut root_password = PasswordInput::new("Root Password:");
        root_password.set_focused(true);

        Self {
            root_password,
            root_confirm: PasswordInput::new("Confirm Root Password:"),
            username: TextInput::new("Username:"),
            user_password: PasswordInput::new("User Password:"),
            user_confirm: PasswordInput::new("Confirm User Password:"),
            focus: Field::RootPassword,
            error: None,
        }
    }

    fn set_focus(&mut self, field: Field) {
        self.root_password.set_focused(false);
        self.root_confirm.set_focused(false);
        self.username.focused = false;
        self.user_password.set_focused(false);
        self.user_confirm.set_focused(false);

        match field {
            Field::RootPassword => self.root_password.set_focused(true),
            Field::RootConfirm  => self.root_confirm.set_focused(true),
            Field::Username     => self.username.focused = true,
            Field::UserPassword => self.user_password.set_focused(true),
            Field::UserConfirm  => self.user_confirm.set_focused(true),
        }
        self.focus = field;
    }

    fn validate(&self) -> Result<(), String> {
        if self.root_password.value().is_empty() {
            return Err("Root password must not be empty.".to_string());
        }
        if self.root_password.value() != self.root_confirm.value() {
            return Err("Root passwords do not match.".to_string());
        }
        let username = self.username.value.trim();
        if username.is_empty() {
            return Err("Username must not be empty.".to_string());
        }
        if self.user_password.value().is_empty() {
            return Err("User password must not be empty.".to_string());
        }
        if self.user_password.value() != self.user_confirm.value() {
            return Err("User passwords do not match.".to_string());
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match key.code {
            KeyCode::Esc => return ScreenResult::Prev,
            KeyCode::Tab => {
                let next = self.focus.next();
                self.set_focus(next);
            }
            KeyCode::BackTab => {
                let prev = self.focus.prev();
                self.set_focus(prev);
            }
            KeyCode::Enter => {
                match self.validate() {
                    Ok(()) => {
                        config.root_password = self.root_password.value().to_string();
                        config.username = self.username.value.trim().to_string();
                        config.user_password = self.user_password.value().to_string();
                        return ScreenResult::Next;
                    }
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
            _ => {
                match self.focus {
                    Field::RootPassword => self.root_password.handle_key(key),
                    Field::RootConfirm  => self.root_confirm.handle_key(key),
                    Field::Username     => self.username.handle_key(key),
                    Field::UserPassword => self.user_password.handle_key(key),
                    Field::UserConfirm  => self.user_confirm.handle_key(key),
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
                Constraint::Length(4),   // 2: root password
                Constraint::Length(4),   // 3: root confirm
                Constraint::Length(1),   // 4: gap
                Constraint::Length(4),   // 5: username
                Constraint::Length(4),   // 6: user password
                Constraint::Length(4),   // 7: user confirm
                Constraint::Min(0),      // 8: spacer
                Constraint::Length(1),   // 9: error
                Constraint::Length(1),   // 10: help
            ])
            .split(area);

        Paragraph::new("User Accounts")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .render(chunks[0], buf);

        let col = centered_horizontal(area, 60);

        macro_rules! col_rect {
            ($chunk:expr) => {
                Rect { y: $chunk.y, height: $chunk.height, x: col.x, width: col.width }
            };
        }

        self.root_password.render(col_rect!(chunks[2]), buf);
        self.root_confirm.render(col_rect!(chunks[3]), buf);
        self.username.render(col_rect!(chunks[5]), buf);
        self.user_password.render(col_rect!(chunks[6]), buf);
        self.user_confirm.render(col_rect!(chunks[7]), buf);

        if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center)
                .render(chunks[9], buf);
        }

        Paragraph::new("[Tab] Next field  [Enter] Continue  [Esc] Back")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .render(chunks[10], buf);
    }
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(40).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
