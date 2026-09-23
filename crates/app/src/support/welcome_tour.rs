//! Port of `Sources/DownrightApp/Support/WelcomeTour.swift`.
//!
//! The bundled tour is written with command tokens instead of copied shortcut
//! glyphs. A tour opened after a remap must teach the current command.
//!
//! Token delimiters are found with Foundation's `range(of:)` (a non-literal
//! `NSString` search), as the Swift file finds them.

use upleft_swift_text as swift_text;

use super::commands::{Command, KeyBinding};
use super::keybindings::KeybindingStore;

pub struct WelcomeTour;

/// `WelcomeTour.Error`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WelcomeTourError {
    MalformedToken(String),
    UnknownCommand(String),
    MissingBinding(Command),
}

impl std::fmt::Display for WelcomeTourError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WelcomeTourError::MalformedToken(token) => write!(f, "malformedToken({token:?})"),
            WelcomeTourError::UnknownCommand(name) => write!(f, "unknownCommand({name:?})"),
            WelcomeTourError::MissingBinding(command) => write!(f, "missingBinding({})", command.raw_value()),
        }
    }
}

impl std::error::Error for WelcomeTourError {}

/// `text.range(of: needle)` (Foundation, no options), as a byte range of
/// `text`. `NSString` reports UTF-16 offsets; they are mapped back here.
fn range_of(text: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if text.is_ascii() {
        let found = text.find(needle)?;
        return Some(found..found + needle.len());
    }
    let units = swift_text::ns::utf16(text);
    let haystack = swift_text::ns::foundation::ns_from_utf16(&units);
    let found = objc2::rc::autoreleasepool(|_| {
        swift_text::ns::foundation::range_of(
            &haystack,
            needle,
            objc2_foundation::NSStringCompareOptions::empty(),
            swift_text::NSRange::new(0, units.len() as isize),
        )
    });
    if found.location == swift_text::NS_NOT_FOUND {
        return None;
    }
    let byte_offset = |utf16_offset: isize| -> usize {
        let mut units_seen = 0isize;
        for (index, c) in text.char_indices() {
            if units_seen >= utf16_offset {
                return index;
            }
            units_seen += c.len_utf16() as isize;
        }
        text.len()
    };
    Some(byte_offset(found.location)..byte_offset(found.location + found.length))
}

impl WelcomeTour {
    /// Renders `{{shortcut:commandName}}` and `{{command:commandName}}` tokens.
    /// A deliberately cleared binding is rendered as `Unassigned` so the
    /// bundled tour remains openable and truthful after a remap.
    pub fn render(source: &str, binding: impl Fn(Command) -> Option<KeyBinding>) -> Result<String, WelcomeTourError> {
        let mut output = String::new();
        let mut cursor = 0;
        while let Some(start) = range_of(&source[cursor..], "{{").map(|range| cursor + range.start..cursor + range.end) {
            output.push_str(&source[cursor..start.start]);
            let Some(end) = range_of(&source[start.end..], "}}").map(|range| start.end + range.start..start.end + range.end)
            else {
                return Err(WelcomeTourError::MalformedToken(source[start.start..].to_owned()));
            };
            let token = &source[start.end..end.start];
            output.push_str(&Self::replacement(token, &binding)?);
            cursor = end.end;
        }
        output.push_str(&source[cursor..]);
        Ok(output)
    }

    /// [`render`](Self::render) with Swift's default `binding:` argument,
    /// `KeybindingStore.shared.primaryBinding(for:)`.
    pub fn render_with_store(source: &str) -> Result<String, WelcomeTourError> {
        Self::render(source, |command| KeybindingStore::shared().primary_binding(command))
    }

    /// Finds every instructional token so resource tests can validate the
    /// tour against the command table without starting an app window.
    pub fn tokens(source: &str) -> Vec<String> {
        let mut values = Vec::new();
        let mut cursor = 0;
        while let Some(start) = range_of(&source[cursor..], "{{").map(|range| cursor + range.start..cursor + range.end) {
            let Some(end) = range_of(&source[start.end..], "}}").map(|range| start.end + range.start..start.end + range.end)
            else {
                break;
            };
            values.push(source[start.end..end.start].to_owned());
            cursor = end.end;
        }
        values
    }

    fn replacement(token: &str, binding: &impl Fn(Command) -> Option<KeyBinding>) -> Result<String, WelcomeTourError> {
        let parts = swift_text::split(token, ':', 1, true);
        let kinds = ["shortcut", "command"];
        if parts.len() != 2 || !kinds.iter().any(|kind| swift_text::str_eq(parts[0], kind)) {
            return Err(WelcomeTourError::MalformedToken(token.to_owned()));
        }
        let Some(command) = Command::from_raw_value(parts[1]) else {
            return Err(WelcomeTourError::UnknownCommand(parts[1].to_owned()));
        };
        if swift_text::str_eq(parts[0], "shortcut") {
            Ok(binding(command).map(|binding| binding.display_string()).unwrap_or_else(|| "Unassigned".to_owned()))
        } else {
            Ok(command.title().to_owned())
        }
    }
}
