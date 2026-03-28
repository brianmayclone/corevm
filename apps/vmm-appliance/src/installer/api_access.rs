use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::common::widgets::{SelectList, render_installer_frame};
use super::{InstallConfig, ScreenResult};

pub struct ApiAccessState {
    select: SelectList,
    error: Option<String>,
}

impl ApiAccessState {
    pub fn new() -> Self {
        let mut select = SelectList::new(
            "Enable CLI/API access?",
            vec![
                "Yes — allow remote management via vmmctl CLI".to_string(),
                "No — only allow access via the web UI".to_string(),
            ],
        );
        select.focused = true;

        Self { select, error: None }
    }

    pub fn handle_key(&mut self, key: KeyEvent, config: &mut InstallConfig) -> ScreenResult {
        match key.code {
            KeyCode::Esc => return ScreenResult::Prev,
            KeyCode::Enter => {
                config.cli_access_enabled = self.select.selected == 0;
                return ScreenResult::Next;
            }
            _ => self.select.handle_key(key),
        }
        ScreenResult::Continue
    }

    pub fn render(&self, frame: &mut Frame, _config: &InstallConfig) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        let content = render_installer_frame(
            area, buf,
            "CLI / API Access",
            "[Up/Down] Select  [Enter] Continue  [Esc] Back",
            Some((6, 8)),
        );

        let col = centered_horizontal(content, 60);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),   // 0: description
                Constraint::Length(1),   // 1: gap
                Constraint::Length(6),   // 2: select list
                Constraint::Min(0),      // 3: spacer
                Constraint::Length(2),   // 4: info
                Constraint::Length(1),   // 5: error
            ])
            .split(content);

        let desc = Paragraph::new(
            "Allow remote management via the vmmctl command-line tool.\n\
             This enables the REST API for external CLI and script access."
        )
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
        desc.render(chunks[0], buf);

        let select_area = Rect { y: chunks[2].y, height: chunks[2].height, x: col.x, width: col.width };
        self.select.render(select_area, buf);

        let info = Paragraph::new(
            "You can change this later via the DCUI console or\n\
             by editing [api] cli_access_enabled in vmm-server.toml"
        )
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
        info.render(chunks[4], buf);

        if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center)
                .render(chunks[5], buf);
        }
    }
}

fn centered_horizontal(area: Rect, percent: u16) -> Rect {
    let width = (area.width * percent / 100).max(30).min(area.width);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    Rect { x, width, ..area }
}
