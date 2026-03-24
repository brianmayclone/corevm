use crossterm::event::KeyEvent;
use ratatui::Frame;
use super::{InstallConfig, ScreenResult};

pub struct TimezoneState {}

impl TimezoneState {
    pub fn new() -> Self { Self {} }

    pub fn handle_key(&mut self, _key: KeyEvent, _config: &mut InstallConfig) -> ScreenResult {
        ScreenResult::Continue
    }

    pub fn render(&self, _frame: &mut Frame, _config: &InstallConfig) {}
}
