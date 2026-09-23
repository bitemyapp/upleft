//! Helpers shared by the engine test ports (the `private func`s at the top of
//! each Swift test file).
#![allow(dead_code)]

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSParagraphStyle, NSTextStorage};
use objc2_foundation::{NSAttributedString, NSString};
use upleft_core::NSRange;
use upleft_render::engine::decoration_engine::DecorationEngine;
use upleft_render::engine::display_map::{DisplayMap, ParagraphIndex};
use upleft_render::render_contracts::{RenderMode, Theme};
use upleft_render::theme::style_sheet::StyleSheet;

pub fn appearance(dark: bool) -> Retained<NSAppearance> {
    // SAFETY: AppKit exports the appearance names as immutable globals.
    let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
    NSAppearance::appearanceNamed(name).expect("aqua appearances exist")
}

/// `StyleSheet(theme: .fallback, appearance: NSAppearance(named: …))`.
pub fn style_sheet(dark: bool) -> StyleSheet {
    StyleSheet::new(Theme::fallback(), &appearance(dark), None)
}

/// `DecorationEngine` with `mode`'s policy over the light fallback sheet.
pub fn engine(mode: RenderMode) -> DecorationEngine {
    let mut engine = DecorationEngine::new(style_sheet(false));
    engine.set_policy(mode.policy());
    engine
}

/// `NSTextStorage(string:)`.
pub fn storage(text: &str) -> Retained<NSTextStorage> {
    let string = NSString::from_str(text);
    // SAFETY: `initWithString:` on a freshly allocated NSTextStorage.
    unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*string] }
}

/// `NSAttributedString(string:)`.
pub fn attributed(text: &str) -> Retained<NSAttributedString> {
    NSAttributedString::from_nsstring(&NSString::from_str(text))
}

/// `storage.attribute(key, at:, effectiveRange: nil)`.
pub fn attribute(storage: &NSAttributedString, key: &NSString, at: isize) -> Option<Retained<AnyObject>> {
    // SAFETY: reading an attribute value; the effective range is not asked for.
    unsafe { storage.attribute_atIndex_effectiveRange(key, at as usize, std::ptr::null_mut()) }
}

pub fn paragraph_style(storage: &NSAttributedString, at: isize) -> Option<Retained<NSParagraphStyle>> {
    attribute(storage, upleft_render::engine::keys::paragraph_style(), at)
        .and_then(|value| value.downcast::<NSParagraphStyle>().ok())
}

/// UTF-16 units of `text`.
pub fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `(text as NSString).length`.
pub fn length(text: &str) -> isize {
    text.encode_utf16().count() as isize
}

/// `(text as NSString).range(of: needle)` for texts where a literal search
/// is exact.
pub fn find(text: &str, needle: &str) -> NSRange {
    let haystack = utf16(text);
    let needle = utf16(needle);
    let location = haystack
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
        .expect("needle present");
    NSRange::new(location as isize, needle.len() as isize)
}

pub fn paragraphs(text: &str) -> ParagraphIndex {
    ParagraphIndex::from_text(&NSString::from_str(text))
}

/// `displayText(_:hidden:)`: each paragraph's display string, concatenated.
pub fn display_text(source: &str, hidden: &[NSRange]) -> String {
    let index = paragraphs(source);
    let map = DisplayMap::with_hidden(index.clone(), hidden);
    let storage = attributed(source);
    let units = utf16(source);
    let mut out = String::new();
    for paragraph in 0..index.starts.len() {
        let range = index.range_at(paragraph);
        match map.display_string_for_paragraph(range, &storage, true) {
            Some(substituted) => out += &substituted.string().to_string(),
            None => out += &String::from_utf16_lossy(&units[range.as_usize_range()]),
        }
    }
    out
}
