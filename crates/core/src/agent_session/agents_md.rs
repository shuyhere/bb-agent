/// Pure logic: merge multiple AGENTS.md / CLAUDE.md content strings into one.
/// The input should be ordered from most-global to most-local.
/// Returns `None` if the input is empty.
#[cfg(test)]
pub(crate) fn merge_agents_md_contents(contents: &[String]) -> Option<String> {
    if contents.is_empty() {
        None
    } else {
        Some(contents.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_empty() {
        assert_eq!(merge_agents_md_contents(&[]), None);
    }

    #[test]
    fn test_merge_single() {
        let contents = vec!["# Global rules".to_string()];
        assert_eq!(
            merge_agents_md_contents(&contents),
            Some("# Global rules".to_string())
        );
    }

    #[test]
    fn test_merge_multiple() {
        let contents = vec![
            "# Global".to_string(),
            "# Project".to_string(),
            "# Local".to_string(),
        ];
        assert_eq!(
            merge_agents_md_contents(&contents),
            Some("# Global\n\n---\n\n# Project\n\n---\n\n# Local".to_string())
        );
    }
}
