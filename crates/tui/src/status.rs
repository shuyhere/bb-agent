//! Status bar / footer — matches pi's footer layout.
//!
//! Pi's footer shows:
//!   cwd | ↑input ↓output $cost (sub/api) context%/window (auto/manual) | provider model • thinking

use crossterm::style::{Color, Stylize};

pub struct FooterData {
    pub model_name: String,
    pub provider: String,
    pub thinking: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,
    pub context_tokens: u64,
    pub context_window: u64,
    pub cwd: String,
}

/// Render a full-width footer line matching pi's style.
pub fn render_footer(data: &FooterData, width: usize) -> String {
    let pct = if data.context_window > 0 {
        (data.context_tokens as f64 / data.context_window as f64 * 100.0) as u64
    } else {
        0
    };

    let pct_color = if pct > 80 {
        Color::Red
    } else if pct > 60 {
        Color::Yellow
    } else {
        Color::Green
    };

    let left = format!(
        "{}",
        shorten_path(&data.cwd, 30).with(Color::DarkGrey),
    );

    let center = format!(
        "{}{}  {}",
        format!("↑{} ↓{}", data.input_tokens, data.output_tokens).with(Color::DarkGrey),
        if data.cost > 0.0 {
            format!(" ${:.3}", data.cost).with(Color::DarkGrey).to_string()
        } else {
            String::new()
        },
        format!(
            "{}%/{}k",
            pct.to_string().with(pct_color),
            data.context_window / 1000,
        ),
    );

    let right = format!(
        "({}) {} {} {}",
        data.provider.clone().with(Color::DarkGrey),
        data.model_name.clone().with(Color::Cyan),
        "•".with(Color::DarkGrey),
        data.thinking.clone().with(Color::DarkGrey),
    );

    // Build the line — separator above + footer content
    let sep = format!("{}", "─".repeat(width).with(Color::DarkGrey));
    format!("{sep}\n{left}  {center}  {right}")
}

/// Simple status line (backward compat).
pub fn render_status(
    model: Option<&str>,
    tokens: Option<u64>,
    context_window: Option<u64>,
) -> String {
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

fn shorten_path(path: &str, max: usize) -> String {
    if path.len() <= max {
        return path.to_string();
    }
    // Show ~ for home
    let home = std::env::var("HOME").unwrap_or_default();
    let display = if path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    };
    if display.len() <= max {
        return display;
    }
    format!("...{}", &display[display.len() - max + 3..])
}
