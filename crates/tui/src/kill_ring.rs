/// Ring buffer for Emacs-style kill/yank operations.
///
/// Tracks killed (deleted) text entries. Consecutive kills can accumulate
/// into a single entry. Supports yank (paste most recent) and yank-pop
/// (cycle through older entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillRingUpdate<'a> {
    Push(&'a str),
    Append(&'a str),
    Prepend(&'a str),
}

impl<'a> KillRingUpdate<'a> {
    fn text(self) -> &'a str {
        match self {
            Self::Push(text) | Self::Append(text) | Self::Prepend(text) => text,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KillRing {
    ring: Vec<String>,
}

impl KillRing {
    /// Record killed text in the ring.
    ///
    /// `Push` starts a fresh entry, while `Append`/`Prepend` keep consecutive
    /// forward/backward kills in a single logical entry.
    pub fn record(&mut self, update: KillRingUpdate<'_>) {
        let text = update.text();
        if text.is_empty() {
            return;
        }

        match update {
            KillRingUpdate::Push(text) => self.ring.push(text.to_string()),
            KillRingUpdate::Append(text) => {
                if let Some(last) = self.ring.last_mut() {
                    last.push_str(text);
                } else {
                    self.ring.push(text.to_string());
                }
            }
            KillRingUpdate::Prepend(text) => {
                if let Some(last) = self.ring.last_mut() {
                    last.insert_str(0, text);
                } else {
                    self.ring.push(text.to_string());
                }
            }
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
