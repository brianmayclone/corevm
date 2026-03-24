use std::process::Command;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::common::widgets::{ConfirmDialog, SelectList};

pub enum DialogResult {
    Continue,
    Close,
}

#[derive(PartialEq)]
enum Stage {
    Select,
    Confirm,
}

pub struct Dialog {
    action_list: SelectList,
    confirm: ConfirmDialog,
    stage: Stage,
    pending_action: String,
}

impl Dialog {
    pub fn new() -> Self {
        let mut action_list = SelectList::new(
            "Select action:",
            vec![
                "Reboot".to_string(),
                "Shutdown".to_string(),
                "Cancel".to_string(),
            ],
        );
        action_list.focused = true;

        let confirm = ConfirmDialog::new(
            " Confirm ",
            "Are you sure?",
        );

        Self {
            action_list,
            confirm,
            stage: Stage::Select,
            pending_action: String::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult {
        match self.stage {
            Stage::Select => match key.code {
                KeyCode::Esc => return DialogResult::Close,
                KeyCode::Enter => {
                    let action = self.action_list.selected_item().unwrap_or("Cancel").to_string();
                    if action == "Cancel" {
                        return DialogResult::Close;
                    }
                    self.pending_action = action.clone();
                    let msg = format!("{}? This will affect all running VMs.", action);
                    self.confirm = ConfirmDialog::new(
                        &format!(" Confirm {} ", action),
                        &msg,
                    );
                    self.stage = Stage::Confirm;
                }
                _ => self.action_list.handle_key(key),
            },
            Stage::Confirm => {
                self.confirm.handle_key(key);
                if let Some(confirmed) = self.confirm.confirmed {
                    if confirmed {
                        match self.pending_action.as_str() {
                            "Reboot" => {
                                let _ = Command::new("reboot").status();
                            }
                            "Shutdown" => {
                                let _ = Command::new("shutdown").args(["-h", "now"]).status();
                            }
                            _ => {}
                        }
                    }
                    return DialogResult::Close;
                }
            }
        }
        DialogResult::Continue
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        match self.stage {
            Stage::Select => {
                let popup_width = area.width.min(50).max(35);
                let popup_height = 12u16.min(area.height);
                let popup = centered_rect(popup_width, popup_height, area);

                let buf = frame.buffer_mut();
                Clear.render(popup, buf);

                let block = Block::default()
                    .title(" Reboot / Shutdown (F10) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

                let inner = block.inner(popup);
                block.render(popup, buf);

                self.action_list.render(inner, buf);

                let help_area = Rect {
                    y: inner.y + inner.height.saturating_sub(1),
                    height: 1,
                    ..inner
                };
                Paragraph::new("[↑↓] Select  [Enter] Confirm  [Esc] Cancel")
                    .style(Style::default().fg(Color::DarkGray))
                    .render(help_area, buf);
            }
            Stage::Confirm => {
                self.confirm.render(area, frame.buffer_mut());
            }
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}
