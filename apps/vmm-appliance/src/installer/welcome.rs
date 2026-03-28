use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::config::ApplianceRole;
use crate::common::widgets::{SelectList, render_installer_frame};
use super::{InstallConfig, ScreenResult};

const LOGO: &str = r#"
   ____                 __   ____  ____
  / ___|___  _ __ ___  / /  / /\ \/ /  \  /|
 | |   / _ \| '__/ _ \| |  | |  \  / /\/  /
 | |__| (_) | | |  __/\ \  \ \  /  \ \  /
  \____\___/|_|  \___| \_\  \_\/_/\_\\/
"#;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    Role,
    Language,
}

pub struct WelcomeState {
    role_list: SelectList,
    lang_list: SelectList,
    focus: Focus,
}

impl WelcomeState {
    pub fn new() -> Self {
        let mut role_list = SelectList::new(
            "Installation Role:",
            vec![
                "Standalone Server".to_string(),
                "Cluster Controller".to_string(),
            ],
        );
        role_list.focused = true;

        let lang_list = SelectList::new(
            "Language:",
            vec![
                "English".to_string(),
                "Deutsch".to_string(),
            ],
        );

        Self {
            role_list,
            lang_list,
            focus: Focus::Role,
        }
    }

    fn focused_list_mut(&mut self) -> &mut SelectList {
        match self.focus {
            Focus::Role     => &mut self.role_list,
            Focus::Language => &mut self.lang_list,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match key.code {
            KeyCode::Esc => return ScreenResult::Quit,
            KeyCode::Tab | KeyCode::BackTab => {
                // Toggle focus
                match self.focus {
                    Focus::Role => {
                        self.focus = Focus::Language;
                        self.role_list.focused = false;
                        self.lang_list.focused = true;
                    }
                    Focus::Language => {
                        self.focus = Focus::Role;
                        self.lang_list.focused = false;
                        self.role_list.focused = true;
                    }
                }
            }
            KeyCode::Enter => {
                // Commit selections to config
                config.role = match self.role_list.selected {
                    0 => Some(ApplianceRole::Server),
                    _ => Some(ApplianceRole::Cluster),
                };
                config.language = match self.lang_list.selected {
                    0 => "en".to_string(),
                    _ => "de".to_string(),
                };
                return ScreenResult::Next;
            }
            other => {
                // Delegate arrow keys to the focused list
                let _ = other;
                self.focused_list_mut().handle_key(key);
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let content = render_installer_frame(
            area, buf,
            "Welcome",
            "[Tab] Switch field  [↑↓] Select  [Enter] Continue  [Esc] Quit",
            None,
        );

        // Layout within content area
        let logo_lines = LOGO.lines().count() as u16;
        let field_height = 8u16;

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(logo_lines),     // 0: logo
                Constraint::Length(1),              // 1: gap
                Constraint::Length(field_height),   // 2: role list
                Constraint::Length(1),              // 3: gap
                Constraint::Length(field_height),   // 4: language list
                Constraint::Min(0),                 // 5: spacer
            ])
            .split(content);

        // Logo
        Paragraph::new(LOGO)
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .render(chunks[0], buf);

        // Role list – centered block
        let role_area = centered_horizontal(chunks[2], 50);
        self.role_list.render(role_area, buf);

        // Language list – centered block
        let lang_area = centered_horizontal(chunks[4], 50);
        self.lang_list.render(lang_area, buf);
    }
}

/// Return a rect that is `percent`% wide, horizontally centered within `area`.
fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(30).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
