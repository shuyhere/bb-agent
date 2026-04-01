/// Fuzzy matching utilities.
/// Matches if all query characters appear in order (not necessarily consecutive).
/// Lower score = better match.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuzzyMatch {
    pub matches: bool,
    pub score: f64,
}

pub fn fuzzy_match(query: &str, text: &str) -> FuzzyMatch {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let text_chars: Vec<char> = text_lower.chars().collect();

    let match_query = |normalized_query: &str| -> FuzzyMatch {
        if normalized_query.is_empty() {
            return FuzzyMatch {
                matches: true,
                score: 0.0,
            };
        }

        let query_chars: Vec<char> = normalized_query.chars().collect();
        if query_chars.len() > text_chars.len() {
            return FuzzyMatch {
                matches: false,
                score: 0.0,
            };
        }

        let mut query_index = 0usize;
        let mut score = 0.0;
        let mut last_match_index: Option<usize> = None;
        let mut consecutive_matches = 0usize;

        for (i, ch) in text_chars.iter().enumerate() {
            if query_index >= query_chars.len() {
                break;
            }

            if *ch == query_chars[query_index] {
                let is_word_boundary = i == 0
                    || matches!(text_chars[i - 1], ' ' | '\t' | '\n' | '\r' | '-' | '_' | '.' | '/' | ':');

                if last_match_index == Some(i.saturating_sub(1)) {
                    consecutive_matches += 1;
                    score -= (consecutive_matches * 5) as f64;
                } else {
                    consecutive_matches = 0;
                    if let Some(last) = last_match_index {
                        score += ((i - last - 1) * 2) as f64;
                    }
                }

                if is_word_boundary {
                    score -= 10.0;
                }

                score += i as f64 * 0.1;

                last_match_index = Some(i);
                query_index += 1;
            }
        }

        if query_index < query_chars.len() {
            return FuzzyMatch {
                matches: false,
                score: 0.0,
            };
        }

        FuzzyMatch {
            matches: true,
            score,
        }
    };

    let primary_match = match_query(&query_lower);
    if primary_match.matches {
        return primary_match;
    }

    let swapped_query = swapped_query(&query_lower);
    let Some(swapped_query) = swapped_query else {
        return primary_match;
    };

    let swapped_match = match_query(&swapped_query);
    if !swapped_match.matches {
        return primary_match;
    }

    FuzzyMatch {
        matches: true,
        score: swapped_match.score + 5.0,
    }
}

pub fn fuzzy_filter<T, F>(items: Vec<T>, query: &str, get_text: F) -> Vec<T>
where
    F: Fn(&T) -> &str,
{
    if query.trim().is_empty() {
        return items;
    }

    let tokens: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return items;
    }

    let mut results: Vec<(T, f64)> = Vec::new();

    for item in items {
        let text = get_text(&item);
        let mut total_score = 0.0;
        let mut all_match = true;

        for token in &tokens {
            let matched = fuzzy_match(token, text);
            if matched.matches {
                total_score += matched.score;
            } else {
                all_match = false;
                break;
            }
        }

        if all_match {
            results.push((item, total_score));
        }
    }

    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    results.into_iter().map(|(item, _)| item).collect()
}

fn swapped_query(query_lower: &str) -> Option<String> {
    let chars: Vec<char> = query_lower.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let first_digit = chars.iter().position(|c| c.is_ascii_digit());
    if let Some(idx) = first_digit {
        if idx > 0
            && chars[..idx].iter().all(|c| c.is_ascii_lowercase())
            && chars[idx..].iter().all(|c| c.is_ascii_digit())
        {
            let letters: String = chars[..idx].iter().collect();
            let digits: String = chars[idx..].iter().collect();
            return Some(format!("{digits}{letters}"));
        }
    }

    let first_letter = chars.iter().position(|c| c.is_ascii_lowercase());
    if let Some(idx) = first_letter {
        if idx > 0
            && chars[..idx].iter().all(|c| c.is_ascii_digit())
            && chars[idx..].iter().all(|c| c.is_ascii_lowercase())
        {
            let digits: String = chars[..idx].iter().collect();
            let letters: String = chars[idx..].iter().collect();
            return Some(format!("{letters}{digits}"));
        }
    }

    None
}
