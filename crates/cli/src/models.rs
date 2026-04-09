use bb_provider::registry::ModelRegistry;

pub fn list_models(search: Option<&str>) {
    let registry = ModelRegistry::new();

    println!(
        "{:<14} {:<36} {:>8} {:>8} {:>9} {:>6}",
        "provider", "model", "context", "max-out", "thinking", "images"
    );

    for model in registry.list() {
        // Apply search filter
        if let Some(term) = search {
            let term = term.to_lowercase();
            let matches = model.id.to_lowercase().contains(&term)
                || model.name.to_lowercase().contains(&term)
                || model.provider.to_lowercase().contains(&term);
            if !matches {
                continue;
            }
        }

        let context = format_tokens(model.context_window);
        let max_out = format_tokens(model.max_tokens);
        let thinking = if model.reasoning { "yes" } else { "no" };
        let images = if model.supports_images() { "yes" } else { "no" };

        println!(
            "{:<14} {:<36} {:>8} {:>8} {:>9} {:>6}",
            model.provider, model.id, context, max_out, thinking, images
        );
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        format!("{tokens}")
    }
}
