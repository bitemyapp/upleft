//! Mirrors `oracle/Sources/downright-oracle/UnicodeDump.swift` (`unicode`):
//! `upleft-swift-text`'s String, Character and CharacterSet answers for every
//! Unicode scalar and for the strings in `corpus/unicode/strings.txt`.

use std::path::Path;

use serde_json::Value;
use upleft_swift_text::{self as swift_text, CharSet};

use super::Failure;
use super::json::{self, Object};

/// Bit order of the per-Character masks (see the Swift).
const PROPERTY_NAMES: [&str; 10] = [
    "isLetter", "isNumber", "isWhitespace", "isNewline", "isPunctuation", "isSymbol", "isUppercase", "isLowercase", "isCased",
    "isASCII",
];

fn mask(g: &str) -> i64 {
    let bits = [
        swift_text::is_letter(g),
        swift_text::is_number(g),
        swift_text::is_whitespace(g),
        swift_text::is_newline(g),
        swift_text::is_punctuation(g),
        swift_text::is_symbol(g),
        swift_text::is_uppercase(g),
        swift_text::is_lowercase(g),
        swift_text::is_cased(g),
        swift_text::is_ascii_character(g),
    ];
    bits.iter().enumerate().filter(|(_, bit)| **bit).map(|(index, _)| 1i64 << index).sum()
}

const CHARACTER_SETS: [(&str, CharSet); 14] = [
    ("whitespaces", CharSet::Whitespaces),
    ("whitespacesAndNewlines", CharSet::WhitespacesAndNewlines),
    ("newlines", CharSet::Newlines),
    ("alphanumerics", CharSet::Alphanumerics),
    ("charactersIn:<>", CharSet::Chars("<>")),
    ("charactersIn:+-.", CharSet::Chars("+-.")),
    ("charactersIn:-", CharSet::Chars("-")),
    ("charactersIn:|", CharSet::Chars("|")),
    ("charactersIn:[]", CharSet::Chars("[]")),
    ("charactersIn:/?#\\", CharSet::Chars("/?#\\")),
    ("charactersIn:/:\\0", CharSet::Chars("/:\0")),
    ("charactersIn:./", CharSet::Chars("./")),
    ("charactersIn:#", CharSet::Chars("#")),
    ("charactersIn:editorPath", CharSet::Chars("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/._+-")),
];

#[derive(Default, Clone)]
struct Runs {
    out: Vec<Value>,
    start: Option<u32>,
    previous: u32,
}

impl Runs {
    fn add(&mut self, value: u32, member: bool) {
        if member {
            match self.start {
                Some(open) if self.previous + 1 != value => {
                    self.out.push(Value::Array(vec![open.into(), self.previous.into()]));
                    self.start = Some(value);
                }
                None => self.start = Some(value),
                _ => {}
            }
            self.previous = value;
        } else if let Some(open) = self.start.take() {
            self.out.push(Value::Array(vec![open.into(), self.previous.into()]));
        }
    }

    fn finish(mut self) -> Value {
        if let Some(open) = self.start.take() {
            self.out.push(Value::Array(vec![open.into(), self.previous.into()]));
        }
        Value::Array(self.out)
    }
}

fn scalars(s: &str) -> Value {
    Value::Array(s.chars().map(|c| (c as u32).into()).collect())
}

fn is_single(s: &str, c: char) -> bool {
    let mut chars = s.chars();
    chars.next() == Some(c) && chars.next().is_none()
}

pub fn run(input: &Path, output: &Path) -> Result<(), Failure> {
    let text = super::markup::read_text(input)?;
    json::write(&document(&text), output)?;
    Ok(())
}

pub fn document(input: &str) -> Value {
    let mut properties = vec![Runs::default(); PROPERTY_NAMES.len()];
    let mut sets = vec![Runs::default(); CHARACTER_SETS.len()];
    let mut lowercased = Vec::new();
    let mut uppercased = Vec::new();
    let mut combining = Vec::new();
    let mut combining_run: Option<(u32, u32, u8)> = None;
    let mut scalar_count = 0i64;

    for value in 0u32..=0x10FFFF {
        let Some(scalar) = char::from_u32(value) else { continue };
        scalar_count += 1;
        let mut buffer = [0u8; 4];
        let string: &str = scalar.encode_utf8(&mut buffer);
        let bits = mask(string);
        for (index, runs) in properties.iter_mut().enumerate() {
            runs.add(value, bits & (1 << index) != 0);
        }
        for (index, (_, set)) in CHARACTER_SETS.iter().enumerate() {
            sets[index].add(value, set.contains(scalar));
        }

        let lower = swift_text::lowercased(string);
        if !is_single(&lower, scalar) {
            lowercased.push(Value::Array(vec![value.into(), scalars(&lower)]));
        }
        let upper = swift_text::uppercased(string);
        if !is_single(&upper, scalar) {
            uppercased.push(Value::Array(vec![value.into(), scalars(&upper)]));
        }

        let ccc = swift_text::canonical_combining_class(scalar);
        match &mut combining_run {
            Some(run) if run.2 == ccc && run.1 + 1 == value => run.1 = value,
            _ => {
                if let Some(run) = combining_run
                    && run.2 != 0
                {
                    combining.push(Value::Array(vec![run.0.into(), run.1.into(), run.2.into()]));
                }
                combining_run = Some((value, value, ccc));
            }
        }

    }
    if let Some(run) = combining_run
        && run.2 != 0
    {
        combining.push(Value::Array(vec![run.0.into(), run.1.into(), run.2.into()]));
    }

    let mut property_object = Object::new();
    for (name, runs) in PROPERTY_NAMES.iter().zip(properties) {
        property_object = property_object.with(name, runs.finish());
    }
    let mut set_object = Object::new();
    for ((name, _), runs) in CHARACTER_SETS.iter().zip(sets) {
        set_object = set_object.with(name, runs.finish());
    }

    let (strings, pair_count, triples, comparisons) = parse_strings(input);
    Object::new()
        .with("scalarCount", scalar_count)
        .with("properties", property_object.build())
        .with("characterSets", set_object.build())
        .with("lowercased", Value::Array(lowercased))
        .with("uppercased", Value::Array(uppercased))
        .with("combiningClass", Value::Array(combining))
        .with("normalization", normalization(&triples))
        .with("comparisons", compare(&comparisons))
        .with("strings", Value::Array(strings.iter().map(|s| string_facts(s)).collect()))
        .with("pairs", pairs(&strings[..pair_count.min(strings.len())]))
        .build()
}

fn scalar_string(hexes: &str) -> String {
    hexes
        .split(' ')
        .filter(|hex| !hex.is_empty())
        .map(|hex| char::from_u32(u32::from_str_radix(hex, 16).expect("hex")).expect("scalar"))
        .collect()
}

type Triple = (String, String, String);
type Parsed = (Vec<String>, usize, Vec<Triple>, Vec<(String, String)>);

/// `S <hex scalars>` strings and `N <scalar> : <NFD> : <NFC>` triples; the
/// header comment names the pair-set size.
fn parse_strings(input: &str) -> Parsed {
    let mut strings = Vec::new();
    let mut triples = Vec::new();
    let mut comparisons = Vec::new();
    let mut pair_count = 0;
    for line in input.split('\n').filter(|line| !line.is_empty()) {
        if let Some(comment) = line.strip_prefix('#') {
            if let Some(start) = comment.find("The first ")
                && let Some(end) = comment.find(" strings")
            {
                pair_count = comment[start + "The first ".len()..end].parse().unwrap_or(0);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix('N') {
            let parts: Vec<&str> = rest.split(':').collect();
            triples.push((scalar_string(parts[0]), scalar_string(parts[1]), scalar_string(parts[2])));
            continue;
        }
        if let Some(rest) = line.strip_prefix('C') {
            let (a, b) = rest.split_once('/').expect("C a / b");
            comparisons.push((scalar_string(a), scalar_string(b)));
            continue;
        }
        let Some(rest) = line.strip_prefix('S') else { continue };
        strings.push(scalar_string(rest));
    }
    (strings, pair_count, triples, comparisons)
}

/// One base-32 digit per comparison pair: bits `a == b` 1, `a < b` 2,
/// `b < a` 4, `a.hasPrefix(b)` 8, `a.contains(b)` 16.
fn compare(pairs: &[(String, String)]) -> Value {
    let digits: Vec<char> = "0123456789abcdefghijklmnopqrstuv".chars().collect();
    let results: String = pairs
        .iter()
        .map(|(a, b)| {
            let mut bits = 0;
            if swift_text::str_eq(a, b) {
                bits |= 1;
            }
            if swift_text::str_less(a, b) {
                bits |= 2;
            }
            if swift_text::str_less(b, a) {
                bits |= 4;
            }
            if swift_text::has_prefix(a, b) {
                bits |= 8;
            }
            if swift_text::contains(a, b) {
                bits |= 16;
            }
            digits[bits]
        })
        .collect();
    Object::new().with("count", pairs.len() as i64).with("results", results).build()
}

/// One base-32 digit per triple (scalar, NFD, NFC): bits `s == nfd` 1,
/// `s == nfc` 2, `nfd == nfc` 4, `s < nfd` 8, `nfd < s` 16.
fn normalization(triples: &[Triple]) -> Value {
    let digits: Vec<char> = "0123456789abcdefghijklmnopqrstuv".chars().collect();
    let results: String = triples
        .iter()
        .map(|(scalar, nfd, nfc)| {
            let mut bits = 0;
            if swift_text::str_eq(scalar, nfd) {
                bits |= 1;
            }
            if swift_text::str_eq(scalar, nfc) {
                bits |= 2;
            }
            if swift_text::str_eq(nfd, nfc) {
                bits |= 4;
            }
            if swift_text::str_less(scalar, nfd) {
                bits |= 8;
            }
            if swift_text::str_less(nfd, scalar) {
                bits |= 16;
            }
            digits[bits]
        })
        .collect();
    Object::new().with("count", triples.len() as i64).with("results", results).build()
}

fn string_facts(s: &str) -> Value {
    Object::new()
        .with("characters", Value::Array(swift_text::graphemes(s).map(|g| (g.chars().count() as i64).into()).collect()))
        .with("count", swift_text::count(s) as i64)
        .with("masks", Value::Array(swift_text::graphemes(s).map(|g| mask(g).into()).collect()))
        .with("lowercased", swift_text::lowercased(s))
        .with("uppercased", swift_text::uppercased(s))
        .with("trimmedWhitespaces", swift_text::trim_whitespaces(s))
        .with("trimmedWhitespacesAndNewlines", swift_text::trim_whitespaces_and_newlines(s))
        .with(
            "reversedCharacters",
            Value::Array(swift_text::graphemes(s).rev().map(|g| (g.chars().count() as i64).into()).collect()),
        )
        .build()
}

/// One row per string: a base-32 digit per partner, bits `==` 1, `<` 2,
/// `hasPrefix` 4, `hasSuffix` 8, `contains` 16.
fn pairs(strings: &[String]) -> Value {
    let digits: Vec<char> = "0123456789abcdefghijklmnopqrstuv".chars().collect();
    Value::Array(
        strings
            .iter()
            .map(|a| {
                let row: String = strings
                    .iter()
                    .map(|b| {
                        let mut bits = 0;
                        if swift_text::str_eq(a, b) {
                            bits |= 1;
                        }
                        if swift_text::str_less(a, b) {
                            bits |= 2;
                        }
                        if swift_text::has_prefix(a, b) {
                            bits |= 4;
                        }
                        if swift_text::has_suffix(a, b) {
                            bits |= 8;
                        }
                        if swift_text::contains(a, b) {
                            bits |= 16;
                        }
                        digits[bits]
                    })
                    .collect();
                Value::String(row)
            })
            .collect(),
    )
}
