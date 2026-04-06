use serde::{Deserialize, Serialize};

/// A package entry in settings — either a simple source string or a
/// filtered object with per-resource-type filters.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PackageEntry {
    Simple(String),
    Filtered(PackageFilter),
}

impl PackageEntry {
    /// Get the package source string.
    pub fn source(&self) -> &str {
        match self {
            PackageEntry::Simple(s) => s,
            PackageEntry::Filtered(f) => &f.source,
        }
    }

    /// Get the optional filter for extensions.
    pub fn extensions_filter(&self) -> Option<&[String]> {
        match self {
            PackageEntry::Simple(_) => None,
            PackageEntry::Filtered(f) => f.extensions.as_deref(),
        }
    }

    /// Get the optional filter for skills.
    pub fn skills_filter(&self) -> Option<&[String]> {
        match self {
            PackageEntry::Simple(_) => None,
            PackageEntry::Filtered(f) => f.skills.as_deref(),
        }
    }

    /// Get the optional filter for prompts.
    pub fn prompts_filter(&self) -> Option<&[String]> {
        match self {
            PackageEntry::Simple(_) => None,
            PackageEntry::Filtered(f) => f.prompts.as_deref(),
        }
    }
}

impl From<String> for PackageEntry {
    fn from(s: String) -> Self {
        PackageEntry::Simple(s)
    }
}

impl From<&str> for PackageEntry {
    fn from(s: &str) -> Self {
        PackageEntry::Simple(s.to_string())
    }
}

/// Filtered package entry with optional per-resource-type filters.
///
/// Filters layer on top of the manifest. They narrow down what is already
/// allowed:
/// - `None` (omitted key) = load all of that type
/// - `[]` (empty array) = load none of that type
/// - Patterns match relative paths from the package root
/// - `!pattern` excludes matches
/// - `+path` force-includes an exact path
/// - `-path` force-excludes an exact path
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PackageFilter {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<String>>,
}
