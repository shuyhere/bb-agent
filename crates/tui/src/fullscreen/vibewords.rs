use rand::Rng;

const STATUS_WORDS: &[&str] = &[
    // BB-ish thinking / coding motion
    "thinking",
    "pondering",
    "mulling",
    "planning",
    "reading",
    "scanning",
    "grepping",
    "diffing",
    "patching",
    "stitching",
    "checking",
    "rechecking",
    "comparing",
    "tracing",
    "tracking",
    "sorting",
    "gathering",
    "indexing",
    "compacting",
    "summarizing",
    "polishing",
    "tidying",
    "smoothing",
    "untangling",
    "tinkering",
    "noodling",
    "scribbling",
    // Soft / cuddly BB personality
    "peeking",
    "listening",
    "humming",
    "purring",
    "blinking",
    "wiggling",
    "scooting",
    "sniffing",
    "snuffling",
    "nuzzling",
    "snuggling",
    "cuddling",
    "nesting",
    "settling",
    "drifting",
    "resting",
    "tiptoeing",
];

/// Returns a randomly selected fullscreen status vibe.
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

/// Returns the fullscreen status vibe list.
pub fn all_vibes() -> &'static [&'static str] {
    STATUS_WORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vibe_list_is_nonempty_and_all_ing_status_words() {
        assert!(all_vibes().len() >= 24);
        assert!(all_vibes().iter().all(|word| !word.trim().is_empty()));
        assert!(all_vibes().iter().all(|word| word.ends_with("ing")));
    }

    #[test]
    fn vibe_list_contains_bbish_status_words() {
        assert!(all_vibes().contains(&"thinking"));
        assert!(all_vibes().contains(&"patching"));
        assert!(all_vibes().contains(&"purring"));
        assert!(all_vibes().contains(&"snuggling"));
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
