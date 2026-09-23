//! Port of `Engine/SyntaxRunCache.swift`: memoises `SyntaxHighlighter` output.
//!
//! Highlighting is the one genuinely expensive step in decoration, and a
//! keystroke in a paragraph must never re-lex the code blocks around it. The
//! cache is keyed by content, so a code block that moves because text above it
//! changed is still a hit (§3.5, §12).
//!
//! Swift keys on `code.hashValue` and confirms with `==`, both of which follow
//! Unicode canonical equivalence. The port does the same: code units compare
//! directly, and only text containing a unit at or above U+0300 (below which
//! every string is already NFC and equivalence is identity) is compared and
//! hashed through its NFC form.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use unicode_normalization::UnicodeNormalization;

use crate::syntax::syntax_contracts::{SyntaxHighlighter, SyntaxRun};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    language: String,
    length: usize,
    hash: u64,
}

struct Entry {
    code: Arc<[u16]>,
    runs: Arc<[SyntaxRun]>,
    stamp: u64,
}

struct State {
    entries: HashMap<Key, Entry>,
    stamp: u64,
}

pub struct SyntaxRunCache {
    state: Mutex<State>,
    capacity: usize,
}

impl Default for SyntaxRunCache {
    fn default() -> Self {
        SyntaxRunCache::new(256)
    }
}

fn needs_normalization(code: &[u16]) -> bool {
    code.iter().any(|&unit| unit >= 0x300)
}

fn nfc(code: &[u16]) -> Vec<char> {
    char::decode_utf16(code.iter().copied())
        .map(|scalar| scalar.unwrap_or(char::REPLACEMENT_CHARACTER))
        .nfc()
        .collect()
}

fn content_hash(code: &[u16]) -> u64 {
    let mut hasher = DefaultHasher::new();
    if needs_normalization(code) {
        for scalar in nfc(code) {
            (scalar as u32).hash(&mut hasher);
        }
    } else {
        // Identical to hashing the NFC scalars: below U+0300 every unit is a
        // scalar and the string is its own NFC form.
        for &unit in code {
            (unit as u32).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Swift `String ==`: canonical equivalence.
fn canonically_equal(a: &[u16], b: &[u16]) -> bool {
    if a == b {
        return true;
    }
    if !needs_normalization(a) && !needs_normalization(b) {
        return false;
    }
    nfc(a) == nfc(b)
}

impl SyntaxRunCache {
    pub fn new(capacity: usize) -> Self {
        SyntaxRunCache { state: Mutex::new(State { entries: HashMap::new(), stamp: 0 }), capacity }
    }

    /// Runs for `code` (UTF-16), highlighted once per distinct content.
    pub fn runs(&self, code: &[u16], language: Option<&str>, highlighter: &dyn SyntaxHighlighter) -> Arc<[SyntaxRun]> {
        let key = Key { language: language.unwrap_or("").to_owned(), length: code.len(), hash: content_hash(code) };
        let mut state = self.state.lock().unwrap_or_else(|poison| poison.into_inner());
        state.stamp = state.stamp.wrapping_add(1);
        let stamp = state.stamp;
        // The stored code is compared, not trusted to the hash: a collision
        // would otherwise colour one block with another's grammar.
        if let Some(hit) = state.entries.get_mut(&key)
            && canonically_equal(&hit.code, code)
        {
            hit.stamp = stamp;
            return hit.runs.clone();
        }
        let runs: Arc<[SyntaxRun]> = highlighter.highlight(code, language).into();
        state.entries.insert(key, Entry { code: code.into(), runs: runs.clone(), stamp });
        if state.entries.len() > self.capacity {
            self.evict(&mut state);
        }
        runs
    }

    pub fn remove_all(&self) {
        let mut state = self.state.lock().unwrap_or_else(|poison| poison.into_inner());
        state.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.state.lock().unwrap_or_else(|poison| poison.into_inner()).entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn evict(&self, state: &mut State) {
        // Drop the least recently used quarter in one pass.
        let mut stamps: Vec<(u64, Key)> =
            state.entries.iter().map(|(key, entry)| (entry.stamp, key.clone())).collect();
        stamps.sort_by_key(|(stamp, _)| *stamp);
        for (_, key) in stamps.into_iter().take(self.capacity / 4) {
            state.entries.remove(&key);
        }
    }
}
