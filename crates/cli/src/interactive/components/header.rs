#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderComponent {
    pub title: String,
    pub status: Option<String>,
    pub hints: Vec<String>,
}

impl HeaderComponent {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status: None,
            hints: Vec::new(),
        }
    }

    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    pub fn with_hints(mut self, hints: Vec<String>) -> Self {
        self.hints = hints;
        self
    }

    pub fn render_line(&self) -> String {
        let mut parts = vec![self.title.clone()];

        if let Some(status) = &self.status {
            if !status.is_empty() {
                parts.push(status.clone());
            }
        }

        if !self.hints.is_empty() {
            parts.push(self.hints.join("  "));
        }

        parts.join("  ")
    }
}

pub fn format_keys(keys: &[&str]) -> String {
    match keys {
        [] => String::new(),
        [key] => (*key).to_string(),
        _ => keys.join("/"),
    }
}

pub fn key_text(keys: &[&str]) -> String {
    format_keys(keys)
}

pub fn key_hint(keys: &[&str], description: &str) -> String {
    let keys = key_text(keys);
    if keys.is_empty() {
        description.to_string()
    } else {
        format!("{keys} {description}")
    }
}

pub fn raw_key_hint(key: &str, description: &str) -> String {
    if key.is_empty() {
        description.to_string()
    } else {
        format!("{key} {description}")
    }
}
