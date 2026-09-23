//! Rust counterpart of `oracle/Sources/downright-oracle/IncrementalDump.swift`:
//! a wholesale decoration followed by a fixed sequence of edits, each
//! committed the way the app commits a parse (reparse, `ASTDiff.dirtySet`,
//! `decorate` on the same engine and storage).

use objc2_foundation::NSString;
use serde_json::Value;
use upleft_core::ast_diff::ASTDiff;
use upleft_core::parser::MarkdownParser;
use upleft_core::{DirtySet, NSRange};

use super::decorate::{self, engine, text_storage};
use super::json::Object;
use super::{Failure, Request};
use upleft_render::engine::ns_range;

pub const STEP_COUNT: usize = 8;

pub struct Edit {
    pub label: &'static str,
    pub range: NSRange,
    pub replacement: &'static str,
}

/// `offset` clamped into the text and moved off the low half of a surrogate
/// pair.
fn snap(offset: isize, text: &[u16]) -> isize {
    let n = text.len() as isize;
    let mut p = 0.max(offset.min(n));
    while p > 0 && p < n && (0xDC00..=0xDFFF).contains(&text[p as usize]) {
        p -= 1;
    }
    p
}

fn is_ascii_letter(c: u16) -> bool {
    (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c)
}

fn is_break(c: u16) -> bool {
    c == 0x0A || c == 0x0D
}

fn line_start(offset: isize, text: &[u16]) -> isize {
    let mut s = offset;
    while s > 0 && !is_break(text[(s - 1) as usize]) {
        s -= 1;
    }
    s
}

/// `IncrementalDump.edit(_:in:)`.
pub fn edit(step: usize, text: &[u16]) -> Edit {
    let n = text.len() as isize;
    match step {
        0 => Edit { label: "insert x at 1/3", range: NSRange::new(snap(n / 3, text), 0), replacement: "x" },
        1 => {
            let p = snap(2 * n / 3, text);
            if p >= n {
                return Edit { label: "delete at 2/3", range: NSRange::new(p, 0), replacement: "" };
            }
            let c = text[p as usize];
            let length = if (0xD800..=0xDBFF).contains(&c) && p + 1 < n { 2 } else { 1 };
            Edit { label: "delete at 2/3", range: NSRange::new(p, length), replacement: "" }
        }
        2 => Edit { label: "newline at 1/2", range: NSRange::new(snap(n / 2, text), 0), replacement: "\n" },
        3 => {
            let start = snap(n / 4, text);
            let mut i = start;
            while i < n {
                if is_ascii_letter(text[i as usize]) {
                    let mut j = i;
                    while j < n && is_ascii_letter(text[j as usize]) {
                        j += 1;
                    }
                    if j - i >= 3 {
                        return Edit { label: "replace word", range: NSRange::new(i, j - i), replacement: "renamed" };
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
            Edit { label: "replace word", range: NSRange::new(start, 0), replacement: "renamed" }
        }
        4 => {
            let s = line_start(snap(3 * n / 4, text), text);
            Edit { label: "heading at 3/4", range: NSRange::new(s, 0), replacement: "# " }
        }
        5 => {
            let s = line_start(snap(n / 5, text), text);
            let mut e = s;
            while e < n && !is_break(text[e as usize]) {
                e += 1;
            }
            if e < n {
                e += if text[e as usize] == 0x0D && e + 1 < n && text[(e + 1) as usize] == 0x0A { 2 } else { 1 };
            }
            Edit { label: "delete line at 1/5", range: NSRange::new(s, e - s), replacement: "" }
        }
        6 => Edit { label: "emphasis at 3/5", range: NSRange::new(snap(3 * n / 5, text), 0), replacement: "**" },
        _ => Edit { label: "list at 4/5", range: NSRange::new(snap(4 * n / 5, text), 0), replacement: "\n\n- item\n" },
    }
}

fn dirty_json(dirty: &DirtySet) -> Value {
    Object::new()
        .with("isWholesale", dirty.is_wholesale)
        .with("ranges", Value::Array(dirty.ranges.iter().map(|r| decorate::range(*r)).collect()))
        .build()
}

/// `incremental <file.md> <out.json> [--mode M] [--theme NAME] [--dark]`.
pub fn run(request: &Request) -> Result<(), Failure> {
    let text = super::markup::read_text(&request.input)?;
    let mut engine = engine(request)?;
    let storage = text_storage(&text);
    let mut document = MarkdownParser::parse(&text);
    let initial = engine.decorate(&storage, &document, &DirtySet::wholesale());

    let mut steps = Vec::new();
    for step in 0..STEP_COUNT {
        let units: Vec<u16> = storage.string().to_string().encode_utf16().collect();
        let edit = edit(step, &units);
        storage.replaceCharactersInRange_withString(ns_range(edit.range), &NSString::from_str(edit.replacement));
        let fresh = MarkdownParser::parse(&storage.string().to_string());
        let dirty = ASTDiff::dirty_set(Some(&document), &fresh);
        let bounds = engine.decorated_bounds(&dirty, &fresh, storage.length() as isize);
        let result = engine.decorate(&storage, &fresh, &dirty);
        steps.push(
            Object::new()
                .with("label", edit.label)
                .with("edit", decorate::range(edit.range))
                .with("replacement", edit.replacement)
                .with("dirty", dirty_json(&dirty))
                .with("bounds", Value::Array(bounds.iter().map(|r| decorate::range(*r)).collect()))
                .with("result", decorate::result(result))
                .build(),
        );
        document = fresh;
    }
    let value = Object::new()
        .with("initial", decorate::result(initial))
        .with("steps", Value::Array(steps))
        .with("storage", decorate::storage(&storage))
        .build();
    super::json::write(&value, &request.output)?;
    Ok(())
}
