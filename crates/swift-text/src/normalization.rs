//! The Swift standard library's NFC, which is what `String ==`, `<` and
//! hashing compare (`NFD.swift` and `NFC.swift`'s `_internalNFD` /
//! `_internalNFC`), ported step for step. The Unicode data (combining
//! classes, quick-check values, canonical decompositions and primary
//! composites) comes from `unicode-normalization`, which is on Unicode 17.0 as
//! Swift 6.4 is; the `unicode` conformance suite checks the result against
//! Swift's `==` and `<` for every decomposable scalar.
//!
//! The algorithm differs from the standard one in one observable place.
//! Swift's NFD closes a segment before every *source* scalar with canonical
//! combining class 0, including ccc-0 composites whose decomposition begins
//! with non-starters (U+0F73, U+0F75, U+0F81). So `"\u{307}\u{F73}"` is not
//! `==` to `"\u{307}\u{F71}\u{F72}"` in Swift (the standard orders the three
//! marks together), while `"a\u{307}\u{F73}"` is `==` to both reorderings.

use std::collections::VecDeque;

use unicode_normalization::IsNormalized;

/// `Unicode._NormData`: the combining class and the two quick-check bits.
#[derive(Clone, Copy, Debug)]
struct Item {
    scalar: char,
    ccc: u8,
    nfc_qc: bool,
    nfd_qc: bool,
}

#[inline]
fn item(scalar: char) -> Item {
    // The stdlib answers without data below U+00C0 (ccc 0, both QC Yes).
    if (scalar as u32) < 0xC0 {
        return Item { scalar, ccc: 0, nfc_qc: true, nfd_qc: true };
    }
    let once = std::iter::once(scalar);
    Item {
        scalar,
        ccc: unicode_normalization::char::canonical_combining_class(scalar),
        nfc_qc: unicode_normalization::is_nfc_quick(once.clone()) == IsNormalized::Yes,
        nfd_qc: unicode_normalization::is_nfd_quick(once) == IsNormalized::Yes,
    }
}

/// `_NFDNormalizer` over a scalar source.
pub struct SwiftNfd<I: Iterator<Item = char>> {
    source: I,
    pending: Option<char>,
    buffer: Vec<Item>,
    ready: VecDeque<Item>,
    done: bool,
}

impl<I: Iterator<Item = char>> SwiftNfd<I> {
    pub fn new(source: I) -> Self {
        SwiftNfd { source, pending: None, buffer: Vec::new(), ready: VecDeque::new(), done: false }
    }

    /// `_NormDataBuffer.sort()` (a stable insertion sort by ccc), then emit.
    fn flush_buffer(&mut self) {
        self.buffer.sort_by_key(|item| item.ccc);
        self.ready.extend(self.buffer.drain(..));
    }

    fn next_item(&mut self) -> Option<Item> {
        loop {
            if let Some(ready) = self.ready.pop_front() {
                return Some(ready);
            }
            if self.done {
                return None;
            }
            let scalar = match self.pending.take() {
                Some(pending) => pending,
                None => match self.source.next() {
                    Some(scalar) => scalar,
                    None => {
                        self.done = true;
                        self.flush_buffer();
                        continue;
                    }
                },
            };
            let data = item(scalar);
            // A starter closes the segment in the buffer.
            if data.ccc == 0 && !self.buffer.is_empty() {
                self.pending = Some(scalar);
                self.flush_buffer();
                continue;
            }
            if data.nfd_qc {
                if data.ccc == 0 {
                    return Some(data);
                }
                self.buffer.push(data);
                continue;
            }
            // Hangul and the decomposition table alike: the full canonical
            // decomposition, each scalar with its own data.
            let buffer = &mut self.buffer;
            unicode_normalization::char::decompose_canonical(scalar, |d| buffer.push(item(d)));
        }
    }
}

impl<I: Iterator<Item = char>> Iterator for SwiftNfd<I> {
    type Item = char;

    #[inline]
    fn next(&mut self) -> Option<char> {
        self.next_item().map(|item| item.scalar)
    }
}

/// `_NFCNormalizer` over a scalar source.
pub struct SwiftNfc<I: Iterator<Item = char>> {
    nfd: SwiftNfd<I>,
    composee: Option<char>,
    buffer: Vec<Item>,
    ready: VecDeque<char>,
    done: bool,
}

impl<I: Iterator<Item = char>> SwiftNfc<I> {
    pub fn new(source: I) -> Self {
        SwiftNfc { nfd: SwiftNfd::new(source), composee: None, buffer: Vec::new(), ready: VecDeque::new(), done: false }
    }

    /// `compose(_:andNonNFCQC:)`: Hangul algorithmically, else the table of
    /// primary composites.
    #[inline]
    fn compose(x: char, y: char) -> Option<char> {
        unicode_normalization::char::compose(x, y)
    }
}

impl<I: Iterator<Item = char>> Iterator for SwiftNfc<I> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            if let Some(ready) = self.ready.pop_front() {
                return Some(ready);
            }
            if self.done {
                return None;
            }
            let Some(current) = self.nfd.next_item() else {
                // `flush()`: the leftover composee, then the buffer.
                self.done = true;
                if let Some(composee) = self.composee.take() {
                    self.ready.push_back(composee);
                }
                self.ready.extend(self.buffer.drain(..).map(|item| item.scalar));
                continue;
            };

            // Scalars before the first starter have nothing to compose with.
            let Some(composee) = self.composee else {
                if current.ccc != 0 {
                    return Some(current.scalar);
                }
                self.composee = Some(current.scalar);
                continue;
            };

            let Some(last) = self.buffer.last().map(|item| item.ccc) else {
                // A simple non-blocked pair <composee, current>.
                if !current.nfc_qc
                    && let Some(composed) = Self::compose(composee, current.scalar)
                {
                    self.composee = Some(composed);
                    continue;
                }
                if current.ccc == 0 {
                    self.composee = Some(current.scalar);
                    return Some(composee);
                }
                self.buffer.push(current);
                continue;
            };

            // <composee, [buffer], current>: blocked unless the buffer's
            // highest class is below current's.
            if !(last < current.ccc) {
                if current.ccc == 0 {
                    self.composee = Some(current.scalar);
                    self.ready.push_back(composee);
                    self.ready.extend(self.buffer.drain(..).map(|item| item.scalar));
                    continue;
                }
                self.buffer.push(current);
                continue;
            }
            if !current.nfc_qc
                && let Some(composed) = Self::compose(composee, current.scalar)
            {
                self.composee = Some(composed);
                continue;
            }
            self.buffer.push(current);
        }
    }
}

/// Swift's NFC of `s` (`s.unicodeScalars._internalNFC`).
#[inline]
pub fn swift_nfc(s: &str) -> SwiftNfc<std::str::Chars<'_>> {
    SwiftNfc::new(s.chars())
}

/// Swift's NFD of `s` (`s.unicodeScalars._internalNFD`).
#[inline]
pub fn swift_nfd(s: &str) -> SwiftNfd<std::str::Chars<'_>> {
    SwiftNfd::new(s.chars())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nfc(s: &str) -> String {
        swift_nfc(s).collect()
    }

    #[test]
    fn standard_cases() {
        assert_eq!(nfc("e\u{301}"), "\u{E9}");
        assert_eq!(nfc("\u{212A}"), "K");
        assert_eq!(nfc("\u{1100}\u{1161}\u{11A8}"), "\u{AC01}");
        assert_eq!(nfc("\u{AC00}\u{11A8}"), "\u{AC01}");
        assert_eq!(nfc("a\u{323}\u{302}"), "\u{1EAD}");
        assert_eq!(nfc("a\u{302}\u{323}"), "\u{1EAD}");
        assert_eq!(nfc("\u{301}a"), "\u{301}a");
        assert_eq!(nfc("a\u{305}\u{300}b"), "a\u{305}\u{300}b");
        assert_eq!(nfc("a\u{300}\u{305}b"), "\u{E0}\u{305}b");
    }

    /// Probed on Swift 6.4: a ccc-0 source scalar closes the segment.
    #[test]
    fn swift_segments_at_source_starters() {
        assert_eq!(nfc("\u{307}\u{F73}"), "\u{307}\u{F71}\u{F72}");
        assert_eq!(nfc("\u{307}\u{F71}\u{F72}"), "\u{F71}\u{F72}\u{307}");
        assert_eq!(nfc("a\u{307}\u{F73}"), "\u{227}\u{F71}\u{F72}");
        assert_eq!(nfc("\u{F73}\u{307}"), "\u{F71}\u{F72}\u{307}");
    }
}
