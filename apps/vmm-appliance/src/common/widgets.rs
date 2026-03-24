use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph};
use crossterm::event::{KeyCode, KeyEvent};

// ---------------------------------------------------------------------------
// TextInput
// ---------------------------------------------------------------------------

pub struct TextInput {
    pub label: String,
    pub value: String,
    pub cursor: usize,
    pub focused: bool,
}

impl TextInput {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            value: String::new(),
            cursor: 0,
            focused: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                let byte_pos = self.char_to_byte(self.cursor);
                self.value.insert(byte_pos, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let byte_pos = self.char_to_byte(self.cursor);
                    self.value.remove(byte_pos);
                }
            }
            KeyCode::Delete => {
                let char_count = self.value.chars().count();
                if self.cursor < char_count {
                    let byte_pos = self.char_to_byte(self.cursor);
                    self.value.remove(byte_pos);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                let char_count = self.value.chars().count();
                if self.cursor < char_count {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.value.chars().count();
            }
            _ => {}
        }
    }

    fn char_to_byte(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.value.len())
    }

    pub fn render_with_display(&self, display: &str, area: Rect, buf: &mut Buffer) {
        // Split area: label on top line, input box below
        if area.height < 2 {
            return;
        }
        let label_area = Rect { height: 1, ..area };
        let input_area = Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        };

        // Render label
        Paragraph::new(self.label.as_str())
            .style(Style::default().fg(Color::White))
            .render(label_area, buf);

        // Build the display text with cursor indicator when focused
        let inner_width = (input_area.width.saturating_sub(2)) as usize;
        let chars: Vec<char> = display.chars().collect();

        // Scroll the view so cursor is always visible
        let scroll_offset = if self.cursor > inner_width {
            self.cursor - inner_width
        } else {
            0
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(if self.focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let inner = block.inner(input_area);
        block.render(input_area, buf);

        // Render characters with cursor highlight
        let mut x = inner.x;
        let visible_chars = &chars[scroll_offset..chars.len().min(scroll_offset + inner_width + 1)];
        for (i, &ch) in visible_chars.iter().enumerate() {
            if x >= inner.x + inner.width {
                break;
            }
            let abs_char_idx = scroll_offset + i;
            let style = if self.focused && abs_char_idx == self.cursor {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            buf[(x, inner.y)].set_char(ch).set_style(style);
            x += 1;
        }

        // Draw cursor at end if focused and cursor is at end
        if self.focused && self.cursor >= chars.len() && self.cursor.saturating_sub(scroll_offset) < inner_width as usize + 1 {
            let cursor_x = inner.x + (self.cursor - scroll_offset) as u16;
            if cursor_x < inner.x + inner.width {
                buf[(cursor_x, inner.y)]
                    .set_char(' ')
                    .set_style(Style::default().fg(Color::Black).bg(Color::Cyan));
            }
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_with_display(&self.value.clone(), area, buf);
    }
}

// ---------------------------------------------------------------------------
// PasswordInput
// ---------------------------------------------------------------------------

pub struct PasswordInput {
    inner: TextInput,
}

impl PasswordInput {
    pub fn new(label: &str) -> Self {
        Self {
            inner: TextInput::new(label),
        }
    }

    pub fn value(&self) -> &str {
        &self.inner.value
    }

    pub fn focused(&self) -> bool {
        self.inner.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.inner.focused = focused;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.inner.handle_key(key);
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let masked: String = self.inner.value.chars().map(|_| '*').collect();
        self.inner.render_with_display(&masked, area, buf);
    }
}

// ---------------------------------------------------------------------------
// SelectList
// ---------------------------------------------------------------------------

pub struct SelectList {
    pub label: String,
    pub items: Vec<String>,
    pub selected: usize,
    pub focused: bool,
}

impl SelectList {
    pub fn new(label: &str, items: Vec<String>) -> Self {
        Self {
            label: label.to_string(),
            items,
            selected: 0,
            focused: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down => {
                if !self.items.is_empty() && self.selected < self.items.len() - 1 {
                    self.selected += 1;
                }
            }
            _ => {}
        }
    }

    pub fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected).map(|s| s.as_str())
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 {
            return;
        }
        let label_area = Rect { height: 1, ..area };
        let list_area = Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        };

        Paragraph::new(self.label.as_str())
            .style(Style::default().fg(Color::White))
            .render(label_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(if self.focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let list_items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                if i == self.selected {
                    ListItem::new(item.as_str())
                        .style(Style::default().fg(Color::Black).bg(Color::Cyan))
                } else {
                    ListItem::new(item.as_str()).style(Style::default().fg(Color::White))
                }
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(self.selected));

        let list = List::new(list_items).block(block);
        StatefulWidget::render(list, list_area, buf, &mut state);
    }
}

// ---------------------------------------------------------------------------
// ConfirmDialog
// ---------------------------------------------------------------------------

pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub confirmed: Option<bool>,
    pub selected_yes: bool,
}

impl ConfirmDialog {
    pub fn new(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            confirmed: None,
            selected_yes: true,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Right => {
                self.selected_yes = !self.selected_yes;
            }
            KeyCode::Enter => {
                self.confirmed = Some(self.selected_yes);
            }
            KeyCode::Esc => {
                self.confirmed = Some(false);
            }
            _ => {}
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Calculate centered popup dimensions
        let popup_width = (area.width / 2).max(40).min(area.width);
        let popup_height = 7u16.min(area.height);
        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        // Clear background
        Clear.render(popup_area, buf);

        // Outer block with title
        let block = Block::default()
            .title(self.title.as_str())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        if inner.height == 0 {
            return;
        }

        // Message text
        let msg_area = Rect { height: inner.height.saturating_sub(2), ..inner };
        Paragraph::new(self.message.as_str())
            .style(Style::default().fg(Color::White))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .render(msg_area, buf);

        // Buttons row at bottom of inner area
        let buttons_y = inner.y + inner.height.saturating_sub(1);
        let yes_style = if self.selected_yes {
            Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let no_style = if !self.selected_yes {
            Style::default().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let yes_text = Span::styled(" [ Yes ] ", yes_style);
        let no_text = Span::styled(" [ No ] ", no_style);
        let buttons = Line::from(vec![yes_text, Span::raw("  "), no_text]);
        let buttons_area = Rect {
            y: buttons_y,
            height: 1,
            ..inner
        };
        Paragraph::new(buttons)
            .alignment(Alignment::Center)
            .render(buttons_area, buf);
    }
}

// ---------------------------------------------------------------------------
// ProgressDisplay
// ---------------------------------------------------------------------------

pub struct ProgressDisplay {
    pub label: String,
    pub progress: f64,
    pub status_text: String,
}

impl ProgressDisplay {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            progress: 0.0,
            status_text: String::new(),
        }
    }

    pub fn set_progress(&mut self, pct: f64, text: &str) {
        self.progress = pct.clamp(0.0, 1.0);
        self.status_text = text.to_string();
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 {
            return;
        }

        // Label above gauge
        let (label_area, rest) = if area.height >= 3 {
            (
                Some(Rect { height: 1, ..area }),
                Rect { y: area.y + 1, height: area.height - 1, ..area },
            )
        } else {
            (None, area)
        };

        if let Some(la) = label_area {
            Paragraph::new(self.label.as_str())
                .style(Style::default().fg(Color::White))
                .render(la, buf);
        }

        // Gauge
        let gauge_area = Rect { height: 1, ..rest };
        let ratio = self.progress.clamp(0.0, 1.0);
        let pct = (ratio * 100.0) as u16;
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .percent(pct)
            .label(format!("{}%", pct));
        gauge.render(gauge_area, buf);

        // Status text below gauge
        if rest.height >= 2 {
            let status_area = Rect {
                y: rest.y + 1,
                height: 1,
                ..rest
            };
            Paragraph::new(self.status_text.as_str())
                .style(Style::default().fg(Color::Gray))
                .render(status_area, buf);
        }
    }
}
