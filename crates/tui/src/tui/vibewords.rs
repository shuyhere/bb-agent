use rand::Rng;

const STATUS_WORDS: &[&str] = &[
    "Thinking",
    "Inferring",
    "Processing",
    "Generating",
    "Composing",
    "Considering",
    "Contemplating",
    "Deliberating",
    "Determining",
    "Imagining",
    "Musing",
    "Pondering",
    "Puzzling",
    "Synthesizing",
    "Working",
    "Computing",
    "Creating",
    "Crafting",
    "Tinkering",
    "Vibing",
    "Beboppin'",
    "Booping",
    "Canoodling",
    "Dilly-dallying",
    "Flibbertigibbeting",
    "Lollygagging",
    "Razzle-dazzling",
    "Shenaniganing",
    "Tomfoolering",
    "Whatchamacalliting",
];

/// Returns a randomly selected TUI status vibe.
pub fn random_vibe() -> &'static str {
    let mut rng = rand::thread_rng();
    STATUS_WORDS[rng.gen_range(0..STATUS_WORDS.len())]
}

/// Returns a randomly selected vibe word that differs from `previous` when possible.
pub fn random_vibe_excluding(previous: Option<&str>) -> &'static str {
    if STATUS_WORDS.len() <= 1 {
        return STATUS_WORDS[0];
    }

    let mut next = random_vibe();
    if previous == Some(next) {
        for _ in 0..4 {
            next = random_vibe();
            if previous != Some(next) {
                break;
            }
        }
        if previous == Some(next) {
            let idx = STATUS_WORDS
                .iter()
                .position(|word| Some(*word) == previous)
                .unwrap_or(0);
            next = STATUS_WORDS[(idx + 1) % STATUS_WORDS.len()];
        }
    }
    next
}

/// Returns the TUI status vibe list.
pub fn all_vibes() -> &'static [&'static str] {
    STATUS_WORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vibe_list_matches_expected_curated_words() {
        assert_eq!(all_vibes().len(), 30);
        assert!(all_vibes().iter().all(|word| !word.trim().is_empty()));
        assert!(all_vibes().contains(&"Thinking"));
        assert!(all_vibes().contains(&"Synthesizing"));
        assert!(all_vibes().contains(&"Vibing"));
        assert!(all_vibes().contains(&"Beboppin'"));
        assert!(all_vibes().contains(&"Whatchamacalliting"));
    }

    #[test]
    fn random_vibe_is_from_list() {
        let vibe = random_vibe();
        assert!(all_vibes().contains(&vibe));
    }

    #[test]
    fn excluding_previous_changes_word() {
        let previous = all_vibes()[0];
        let next = random_vibe_excluding(Some(previous));
        assert_ne!(previous, next);
    }
}
