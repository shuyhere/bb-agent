/// Ring buffer for Emacs-style kill/yank operations.
///
/// Tracks killed (deleted) text entries. Consecutive kills can accumulate
/// into a single entry. Supports yank (paste most recent) and yank-pop
/// (cycle through older entries).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KillRing {
    ring: Vec<String>,
}

impl KillRing {
    /// Add text to the kill ring.
    ///
    /// `prepend` controls whether accumulated text is prepended (backward
    /// deletion) or appended (forward deletion).
    pub fn push(&mut self, text: &str, prepend: bool, accumulate: bool) {
        if text.is_empty() {
            return;
        }

        if accumulate && !self.ring.is_empty() {
            if let Some(last) = self.ring.pop() {
                self.ring.push(if prepend {
                    format!("{text}{last}")
                } else {
                    format!("{last}{text}")
                });
            }
        } else {
            self.ring.push(text.to_string());
        }
    }

    /// Get most recent entry without modifying the ring.
    pub fn peek(&self) -> Option<&str> {
        self.ring.last().map(String::as_str)
    }

    /// Move last entry to front (for yank-pop cycling).
    pub fn rotate(&mut self) {
        if self.ring.len() > 1
            && let Some(last) = self.ring.pop()
        {
            self.ring.insert(0, last);
        }
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}
