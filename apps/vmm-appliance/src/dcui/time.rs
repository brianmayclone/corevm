use crossterm::event::KeyEvent;
use ratatui::Frame;

pub enum DialogResult {
    Continue,
    Close,
}

pub struct Dialog;

impl Dialog {
    pub fn new() -> Self {
        Dialog
    }

    pub fn handle_key(&mut self, _key: KeyEvent) -> DialogResult {
        DialogResult::Close
    }

    pub fn render(&self, _frame: &mut Frame) {}
}
