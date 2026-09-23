//! Port of `Sources/DownrightApp/Support/JumpHistory.swift`.
//!
//! Back/forward through jump destinations (§7.1).
//!
//! Records outline jumps, followed links, change navigation, and search hits —
//! but deliberately **not** ordinary scrolling, which would fill the stack with
//! noise and make the two-finger swipe useless.

use upleft_foundation::url::FileUrl;

/// `JumpHistory.Entry`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub url: Option<FileUrl>,
    pub offset: isize,
    pub label: String,
}

impl Entry {
    pub fn new(url: Option<FileUrl>, offset: isize, label: impl Into<String>) -> Entry {
        Entry { url, offset, label: label.into() }
    }
}

/// `JumpHistory`.
#[derive(Clone, Debug)]
pub struct JumpHistory {
    entries: Vec<Entry>,
    index: isize,
}

/// Deep enough for a long reading session, bounded so it can't grow without
/// limit.
const LIMIT: usize = 100;

impl Default for JumpHistory {
    fn default() -> Self {
        JumpHistory::new()
    }
}

impl JumpHistory {
    pub fn new() -> JumpHistory {
        JumpHistory { entries: Vec::new(), index: -1 }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index >= 0 && self.index < self.entries.len() as isize - 1
    }

    pub fn current(&self) -> Option<&Entry> {
        if self.index >= 0 && self.index < self.entries.len() as isize {
            Some(&self.entries[self.index as usize])
        } else {
            None
        }
    }

    /// Records a jump *away from* `from` *to* `to`. Both ends are recorded so
    /// going back lands where you were looking, not where you jumped to.
    pub fn record(&mut self, from: Option<Entry>, to: Entry) {
        if let Some(from) = from {
            if self.index < 0 || self.entries.is_empty() {
                self.entries = vec![from];
                self.index = 0;
            } else if let Some(current) = self.current()
                && (current.offset - from.offset).abs() > 40
            {
                // The reader scrolled since the last recorded position; update
                // the current entry so "back" returns to where they were.
                let index = self.index as usize;
                self.entries[index] = from;
            }
        }

        // A new jump truncates the forward stack, as in every browser.
        if self.index < self.entries.len() as isize - 1 {
            self.entries.truncate((self.index + 1) as usize);
        }
        self.entries.push(to);
        if self.entries.len() > LIMIT {
            let excess = self.entries.len() - LIMIT;
            self.entries.drain(..excess);
        }
        self.index = self.entries.len() as isize - 1;
    }

    pub fn go_back(&mut self) -> Option<Entry> {
        if !self.can_go_back() {
            return None;
        }
        self.index -= 1;
        Some(self.entries[self.index as usize].clone())
    }

    pub fn go_forward(&mut self) -> Option<Entry> {
        if !self.can_go_forward() {
            return None;
        }
        self.index += 1;
        Some(self.entries[self.index as usize].clone())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index = -1;
    }
}
