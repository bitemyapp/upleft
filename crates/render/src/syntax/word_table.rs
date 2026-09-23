//! Port of `Syntax/WordTable.swift`: keyword lookup that allocates nothing at
//! scan time.
//!
//! Every keyword in every supported language is ASCII, so a sorted byte table
//! plus a binary search is exact.

use std::cmp::Ordering;
use std::collections::HashSet;

use super::scanner_core::Unit;
use super::syntax_contracts::SyntaxToken;

#[derive(Debug, Clone, Default)]
pub struct WordTable {
    /// Sorted ascending so `lookup` can binary-search.
    words: Vec<Vec<u8>>,
    tokens: Vec<SyntaxToken>,
    folds_case: bool,
}

impl WordTable {
    pub fn empty() -> WordTable {
        WordTable::new(false, &[])
    }

    /// `groups` are `(classification, spellings)`. Earlier groups win a
    /// collision, so a word listed as both a keyword and a type resolves to
    /// whichever group is listed first.
    pub fn new(folds_case: bool, groups: &[(SyntaxToken, &[&str])]) -> WordTable {
        let mut pairs: Vec<(Vec<u8>, SyntaxToken)> = Vec::new();
        let mut seen: HashSet<Vec<u8>> = HashSet::new();
        for (token, spellings) in groups {
            for spelling in spellings.iter() {
                let bytes: Vec<u8> =
                    spelling.bytes().map(|b| if folds_case { WordTable::lowered(b) } else { b }).collect();
                if !seen.insert(bytes.clone()) {
                    continue;
                }
                pairs.push((bytes, *token));
            }
        }
        pairs.sort_by(|a, b| {
            if WordTable::less(&a.0, &b.0) {
                Ordering::Less
            } else if WordTable::less(&b.0, &a.0) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        let (words, tokens) = pairs.into_iter().unzip();
        WordTable { words, tokens, folds_case }
    }

    /// Classification of `units[start..end]`, or `None` when the word is not a
    /// reserved spelling in this language.
    pub fn lookup(&self, units: &[Unit], start: usize, end: usize) -> Option<SyntaxToken> {
        let mut low = 0usize;
        let mut high = self.words.len();
        while low < high {
            let mid = (low + high) / 2;
            match self.compare(&self.words[mid], units, start, end) {
                Ordering::Equal => return Some(self.tokens[mid]),
                Ordering::Less => low = mid + 1,
                Ordering::Greater => high = mid,
            }
        }
        None
    }

    #[inline(always)]
    fn lowered(b: u8) -> u8 {
        if (0x41..=0x5A).contains(&b) { b + 0x20 } else { b }
    }

    fn less(a: &[u8], b: &[u8]) -> bool {
        for i in 0..a.len().min(b.len()) {
            if a[i] != b[i] {
                return a[i] < b[i];
            }
        }
        a.len() < b.len()
    }

    /// Orders a table entry against a slice of scanned units. Units above 0x7F
    /// cannot equal any ASCII byte and sort after all of them.
    #[inline]
    fn compare(&self, word: &[u8], units: &[Unit], start: usize, end: usize) -> Ordering {
        let length = end - start;
        let shared = word.len().min(length);
        for i in 0..shared {
            let raw = units[start + i];
            let unit = if self.folds_case && raw < 0x80 { WordTable::lowered(raw as u8) as Unit } else { raw };
            let byte = word[i] as Unit;
            if byte != unit {
                return if byte < unit { Ordering::Less } else { Ordering::Greater };
            }
        }
        if word.len() == length {
            return Ordering::Equal;
        }
        if word.len() < length { Ordering::Less } else { Ordering::Greater }
    }
}
