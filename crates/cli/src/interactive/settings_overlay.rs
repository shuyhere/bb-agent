use bb_tui::component::{Component, Focusable};
use bb_tui::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::any::Any;

#[derive(Debug, Clone)]
pub enum SettingsAction {
    Changed(String, String), // (setting_id, new_value)
    Cancelled,
    Pending,
}

#[derive(Clone)]
pub struct SettingItem {
    pub id: String,
    pub label: String,
    pub description: String,
    pub current_value: String,
    pub values: Vec<String>,
}

pub struct SettingsOverlay {
    items: Vec<SettingItem>,
    selected: usize,
    action: SettingsAction,
    focused: bool,
}

const BORDER_COLOR: &str = "\x1b[38;2;178;148;187m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ACCENT: &str = "\x1b[38;2;138;190;183m";

impl SettingsOverlay {
    pub fn new(items: Vec<SettingItem>) -> Self {
        Self {
            items,
            selected: 0,
            action: SettingsAction::Pending,
            focused: true,
        }
    }

    pub fn action(&self) -> &SettingsAction {
        &self.action
    }

    fn cycle_value(&mut self, direction: i32) {
        if let Some(item) = self.items.get_mut(self.selected) {
            if item.values.is_empty() {
                return;
            }
            let current_idx = item.values.iter().position(|v| v == &item.current_value).unwrap_or(0);
            let next_idx = if direction > 0 {
                (current_idx + 1) % item.values.len()
            } else {
                if current_idx == 0 { item.values.len() - 1 } else { current_idx - 1 }
            };
            item.current_value = item.values[next_idx].clone();
            self.action = SettingsAction::Changed(
                item.id.clone(),
                item.current_value.clone(),
            );
        }
    }
}

impl Component for SettingsOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();
        let border = format!("{BORDER_COLOR}{}{RESET}", "\u{2500}".repeat(w));

        lines.push(border.clone());
        lines.push(format!("  {BOLD}Settings{RESET}  {DIM}(Enter/Space: cycle, Esc: close){RESET}"));
        lines.push(String::new());

        for (i, item) in self.items.iter().enumerate() {
            let is_selected = i == self.selected;
            let cursor = if is_selected { format!("{ACCENT}>{RESET} ") } else { "  ".to_string() };

            let value_display = format!("{ACCENT}{}{RESET}", item.current_value);

            let label_style = if is_selected { BOLD } else { "" };
            let label_end = if is_selected { RESET } else { "" };

            let line = format!(
                "  {cursor}{label_style}{}{label_end}  {value_display}",
                item.label,
            );

            let vis = visible_width(&line);
            let pad = w.saturating_sub(vis);
            lines.push(format!("{line}{}", " ".repeat(pad)));

            // Show description for selected item
            if is_selected {
                lines.push(format!("      {DIM}{}{RESET}", item.description));
            }
        }

        lines.push(String::new());
        lines.push(format!("{DIM}  Up/Down: navigate  Enter/Space/Right: next value  Left: prev value  Esc: close{RESET}"));
        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            (KeyCode::Down, _) => {
                if self.selected + 1 < self.items.len() {
                    self.selected += 1;
                }
            }
            (KeyCode::Enter, _)
            | (KeyCode::Char(' '), KeyModifiers::NONE)
            | (KeyCode::Right, _) => {
                self.cycle_value(1);
            }
            (KeyCode::Left, _) => {
                self.cycle_value(-1);
            }
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.action = SettingsAction::Cancelled;
            }
            _ => {}
        }
    }

    fn invalidate(&mut self) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Focusable for SettingsOverlay {
    fn focused(&self) -> bool {
        self.focused
    }
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}
