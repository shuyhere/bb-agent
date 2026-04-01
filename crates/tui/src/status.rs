use crossterm::style::{Color, Stylize};

/// Render a status bar line.
pub fn render_status(model: Option<&str>, tokens: Option<u64>, context_window: Option<u64>) -> String {
    let mut parts = Vec::new();

    if let Some(m) = model {
        parts.push(format!("model: {}", m.with(Color::Cyan)));
    }

    if let (Some(t), Some(cw)) = (tokens, context_window) {
        let pct = (t as f64 / cw as f64 * 100.0) as u64;
        let color = if pct > 80 {
            Color::Red
        } else if pct > 60 {
            Color::Yellow
        } else {
            Color::Green
        };
        parts.push(format!(
            "context: {}/{}k ({}%)",
            t / 1000,
            cw / 1000,
            pct.to_string().with(color),
        ));
    }

    if parts.is_empty() {
        return String::new();
    }

    format!(
        "{}",
        format!("─ {} ─", parts.join(" │ ")).with(Color::DarkGrey)
    )
}
