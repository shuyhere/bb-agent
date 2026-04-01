use bb_tui::component::Component;
use bb_tui::model_selector::{ModelSelection, ModelSelector};
use crossterm::event::KeyEvent;
use std::any::Any;

pub(super) enum ModelSelectorOverlayAction {
    Selected(ModelSelection),
    Cancelled,
}

pub(super) struct ModelSelectorOverlay {
    selector: ModelSelector,
    current_model: String,
    initial_search: Option<String>,
    pending_action: Option<ModelSelectorOverlayAction>,
}

impl ModelSelectorOverlay {
    pub(super) fn new(
        selector: ModelSelector,
        current_model: String,
        initial_search: Option<String>,
    ) -> Self {
        Self {
            selector,
            current_model,
            initial_search,
            pending_action: None,
        }
    }

    pub(super) fn take_action(&mut self) -> Option<ModelSelectorOverlayAction> {
        self.pending_action.take()
    }
}

impl Component for ModelSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let purple = "[38;2;178;148;187m";
        let dim = "[90m";
        let reset = "[0m";
        let inner_width = width.saturating_sub(2);
        let border = format!("{purple}{}{reset}", "─".repeat(width.max(1) as usize));
        let mut lines = vec![
            border.clone(),
            format!(" {purple}Select model{reset}"),
            format!(" {dim}Current: {}{reset}", self.current_model),
        ];
        if let Some(search) = self.initial_search.as_deref().filter(|s| !s.is_empty()) {
            lines.push(format!(" {dim}Search: {search}{reset}"));
        } else {
            lines.push(format!(" {dim}Type to search · Enter select · Esc cancel{reset}"));
        }
        lines.push(String::new());
        lines.extend(
            self.selector
                .render(inner_width)
                .into_iter()
                .map(|line| format!(" {line}")),
        );
        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match self.selector.handle_key(*key) {
            Some(Ok(selection)) => {
                self.pending_action = Some(ModelSelectorOverlayAction::Selected(selection))
            }
            Some(Err(())) => self.pending_action = Some(ModelSelectorOverlayAction::Cancelled),
            None => {}
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
