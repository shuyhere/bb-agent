use bb_provider::registry::{Model, ModelRegistry};
use crossterm::event::KeyEvent;

use crate::select_list::{SelectAction, SelectItem, SelectList};

/// Result of model selection.
pub struct ModelSelection {
    pub provider: String,
    pub model_id: String,
    pub name: String,
    pub context_window: u64,
    pub reasoning: bool,
}

/// Model selector overlay using SelectList.
pub struct ModelSelector {
    list: SelectList,
    models: Vec<Model>,
}

impl ModelSelector {
    /// Create a new model selector from the registry.
    pub fn new(registry: &ModelRegistry, max_visible: usize) -> Self {
        Self::from_models(registry.list().to_vec(), max_visible)
    }

    pub fn from_models(models: Vec<Model>, max_visible: usize) -> Self {
        let items: Vec<SelectItem> = models
            .iter()
            .map(|m| {
                let thinking_tag = if m.reasoning { " 🧠" } else { "" };
                let ctx = format_context(m.context_window);
                let detail = format!(
                    "[{}] {}ctx{}",
                    m.provider, ctx, thinking_tag,
                );
                SelectItem {
                    label: m.id.clone(),
                    detail: Some(format!("{} · {}", detail, m.name)),
                    value: format!("{}:{}", m.provider, m.id),
                }
            })
            .collect();

        Self {
            list: SelectList::new(items, max_visible),
            models,
        }
    }

    /// Render the selector.
    pub fn render(&self, width: u16) -> Vec<String> {
        let mut lines = vec![
            format!("Select Model"),
            format!(""),
        ];
        lines.extend(self.list.render(width));
        lines
    }

    /// Handle a key event. Returns None for no action, Some for selection/cancel.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Result<ModelSelection, ()>> {
        match self.list.handle_key(key) {
            SelectAction::None => None,
            SelectAction::Cancelled => Some(Err(())),
            SelectAction::Selected(value) => {
                // value is "provider:model_id"
                if let Some(pos) = value.find(':') {
                    let provider = &value[..pos];
                    let model_id = &value[pos + 1..];
                    if let Some(model) = self.models.iter().find(|m| m.provider == provider && m.id == model_id) {
                        Some(Ok(ModelSelection {
                            provider: model.provider.clone(),
                            model_id: model.id.clone(),
                            name: model.name.clone(),
                            context_window: model.context_window,
                            reasoning: model.reasoning,
                        }))
                    } else {
                        Some(Err(()))
                    }
                } else {
                    Some(Err(()))
                }
            }
        }
    }

    /// Update the search filter.
    pub fn set_search(&mut self, query: &str) {
        self.list.set_search(query);
    }
}

fn format_context(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens / 1_000_000)
    } else {
        format!("{}k", tokens / 1_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_selector_creation() {
        let registry = ModelRegistry::new();
        let selector = ModelSelector::new(&registry, 10);
        let lines = selector.render(80);
        assert!(!lines.is_empty());
        // Should have "Select Model" header
        assert!(lines[0].contains("Select Model"));
    }

    #[test]
    fn test_model_selector_select() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let registry = ModelRegistry::new();
        let mut selector = ModelSelector::new(&registry, 10);

        // Press Enter on first item
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let result = selector.handle_key(key);
        assert!(result.is_some());
        let selection = result.unwrap().unwrap();
        assert!(!selection.model_id.is_empty());
        assert!(!selection.provider.is_empty());
    }

    #[test]
    fn test_model_selector_filter() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let registry = ModelRegistry::new();
        let mut selector = ModelSelector::new(&registry, 10);

        // Type 'g' to filter
        let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        selector.handle_key(key);

        let lines = selector.render(80);
        // Should show filtered results
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_model_selector_escape() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let registry = ModelRegistry::new();
        let mut selector = ModelSelector::new(&registry, 10);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let result = selector.handle_key(key);
        assert!(matches!(result, Some(Err(()))));
    }
}
