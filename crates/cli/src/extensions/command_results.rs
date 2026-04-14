use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtensionMenuItem {
    pub label: String,
    pub detail: Option<String>,
    /// Sub-argument that will be appended to `/<command>` when the user
    /// picks this item. Plain text; may contain spaces.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtensionPromptSpec {
    pub command: String,
    pub title: String,
    pub lines: Vec<String>,
    pub input_label: Option<String>,
    pub input_placeholder: Option<String>,
    /// Opaque state token passed back to the extension on submit as:
    /// `/<command> __resume <resume> -- <user-input>`.
    pub resume: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExtensionCommandOutcome {
    /// The extension returned nothing meaningful.
    Nothing,
    /// Plain status text to surface to the user.
    Text(String),
    /// Open an interactive select menu in the TUI. Picking an
    /// item re-invokes `/<command> <item.value>`.
    Menu {
        command: String,
        title: String,
        items: Vec<ExtensionMenuItem>,
    },
    /// Open a local input dialog (auth-style) owned by the extension.
    Prompt(ExtensionPromptSpec),
    /// Show `note` as a status banner and immediately dispatch `prompt`
    /// as a new user turn so the agent actually executes the plan the
    /// extension handed back (e.g. Shape's "New Agent Build" kickoff).
    Dispatch {
        note: Option<String>,
        prompt: String,
    },
    /// Activate a saved agent directly in the current TUI session
    /// without routing through the model loop.
    ActivateAgent {
        agent_id: String,
        note: Option<String>,
    },
}

impl ExtensionCommandOutcome {
    pub(crate) fn into_text(self) -> Option<String> {
        match self {
            ExtensionCommandOutcome::Text(text) => Some(text),
            ExtensionCommandOutcome::Dispatch { note, prompt } => {
                // Non-TUI callers (e.g. `bb run`) can't dispatch a
                // turn mid-flight, so fall back to printing both the note
                // and the prompt as plain text.
                let mut out = String::new();
                if let Some(note) = note {
                    out.push_str(&note);
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                out.push_str(&prompt);
                Some(out)
            }
            ExtensionCommandOutcome::ActivateAgent { agent_id, note } => {
                Some(note.unwrap_or_else(|| format!("Activate agent: {agent_id}")))
            }
            ExtensionCommandOutcome::Menu { title, items, .. } => {
                // Callers without a TUI fall back to plain text rendering.
                let mut lines = Vec::new();
                lines.push(title);
                for (idx, item) in items.iter().enumerate() {
                    if let Some(detail) = &item.detail {
                        lines.push(format!("  {}. {} — {}", idx + 1, item.label, detail));
                    } else {
                        lines.push(format!("  {}. {}", idx + 1, item.label));
                    }
                }
                Some(lines.join("\n"))
            }
            ExtensionCommandOutcome::Prompt(prompt) => {
                let mut lines = vec![prompt.title];
                lines.extend(prompt.lines);
                if let Some(label) = prompt.input_label {
                    lines.push(String::new());
                    lines.push(format!("{label}:"));
                }
                Some(lines.join("\n"))
            }
            ExtensionCommandOutcome::Nothing => None,
        }
    }
}

pub(super) fn parse_command_invocation(text: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = text.trim();
    let remainder = trimmed.strip_prefix('/')?;
    split_command_name_and_args(remainder)
}

fn split_command_name_and_args(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.find(char::is_whitespace) {
        Some(index) => {
            let name = trimmed[..index].trim();
            if name.is_empty() {
                return None;
            }
            let args = trimmed[index..].trim();
            Some((name, (!args.is_empty()).then_some(args)))
        }
        None => Some((trimmed, None)),
    }
}

pub(super) fn parse_command_menu_result(
    command: &str,
    value: &Value,
) -> Option<ExtensionCommandOutcome> {
    let menu = value.get("menu")?;
    if !menu.is_object() {
        return None;
    }
    let title = menu
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(command)
        .to_string();
    let raw_items = menu.get("items").and_then(Value::as_array)?;
    let mut items = Vec::with_capacity(raw_items.len());
    for raw in raw_items {
        let label = raw
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let value_str = raw
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if label.is_empty() && value_str.is_empty() {
            continue;
        }
        let label = if label.is_empty() {
            value_str.clone()
        } else {
            label
        };
        let detail = raw
            .get("detail")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        items.push(ExtensionMenuItem {
            label,
            detail,
            value: value_str,
        });
    }
    if items.is_empty() {
        return None;
    }
    Some(ExtensionCommandOutcome::Menu {
        command: command.to_string(),
        title,
        items,
    })
}

pub(super) fn parse_command_prompt_result(
    command: &str,
    value: &Value,
) -> Option<ExtensionCommandOutcome> {
    let prompt = value.get("prompt")?;
    if !prompt.is_object() {
        return None;
    }
    let resume = prompt
        .get("resume")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if resume.is_empty() {
        return None;
    }
    let title = prompt
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(command)
        .to_string();
    let lines = prompt
        .get("lines")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let input_label = prompt
        .get("inputLabel")
        .or_else(|| prompt.get("input_label"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|s| !s.is_empty());
    let input_placeholder = prompt
        .get("inputPlaceholder")
        .or_else(|| prompt.get("input_placeholder"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .filter(|s| !s.is_empty());
    Some(ExtensionCommandOutcome::Prompt(ExtensionPromptSpec {
        command: command.to_string(),
        title,
        lines,
        input_label,
        input_placeholder,
        resume,
    }))
}

/// A result shaped like `{ dispatch: { prompt: "...", note?: "..." } }`
/// (or `{ dispatch: "prompt text" }` for the short form) tells the TUI
/// controller to show `note` as a status banner AND to immediately submit
/// `prompt` as a user turn so the agent acts on it. This is how Shape hands
/// a build plan back to the main agent loop.
pub(super) fn parse_command_dispatch_result(value: &Value) -> Option<ExtensionCommandOutcome> {
    let dispatch = value.get("dispatch")?;

    if let Some(prompt) = dispatch.as_str() {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return None;
        }
        let note = value
            .get("message")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        return Some(ExtensionCommandOutcome::Dispatch {
            note,
            prompt: prompt.to_string(),
        });
    }

    if dispatch.is_object() {
        let prompt = dispatch
            .get("prompt")
            .or_else(|| dispatch.get("text"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let note = dispatch
            .get("note")
            .or_else(|| dispatch.get("message"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            });
        return Some(ExtensionCommandOutcome::Dispatch { note, prompt });
    }

    None
}

pub(super) fn parse_command_activate_agent_result(
    value: &Value,
) -> Option<ExtensionCommandOutcome> {
    let activate = value
        .get("activateAgent")
        .or_else(|| value.get("activate_agent"))?;

    if let Some(agent_id) = activate.as_str() {
        let agent_id = agent_id.trim();
        if agent_id.is_empty() {
            return None;
        }
        let note = value
            .get("message")
            .or_else(|| value.get("note"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        return Some(ExtensionCommandOutcome::ActivateAgent {
            agent_id: agent_id.to_string(),
            note,
        });
    }

    if activate.is_object() {
        let agent_id = activate
            .get("id")
            .or_else(|| activate.get("agentId"))
            .or_else(|| activate.get("agent_id"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        let note = activate
            .get("note")
            .or_else(|| activate.get("message"))
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        return Some(ExtensionCommandOutcome::ActivateAgent { agent_id, note });
    }

    None
}

pub(super) fn render_command_result(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("message").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    Some(serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
}
