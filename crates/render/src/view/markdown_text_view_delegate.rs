//! Port of `View/MarkdownTextViewDelegate.swift`: everything the text surface
//! hands back to the app.
//!
//! The render package owns no windows, no files, and no editor integrations —
//! it reports what happened in source coordinates and the app decides.

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSMenu, NSPasteboard};

use crate::core_types::{NSRange, PathToken};
use crate::render_contracts::SourceFocus;
use crate::view::markdown_text_view::MarkdownTextView;

/// Where a scroll target lands in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPosition {
    Top,
    Center,
    /// Scroll only if the target is currently off screen.
    Visible,
}

/// What the pointer is over, for §7.1's context-menu table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextTargetKind {
    /// Index into `ParsedDocument.headings`.
    Heading(usize),
    CodeBlock(NSRange),
    PathToken(PathToken),
    Image(String),
    Link(String),
    Table(NSRange),
    Selection,
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTarget {
    pub kind: ContextTargetKind,
    pub source_range: NSRange,
    /// Source offset under the pointer when the menu was opened.
    pub hit_offset: Option<isize>,
}

impl ContextTarget {
    pub fn new(kind: ContextTargetKind, source_range: NSRange, hit_offset: Option<isize>) -> ContextTarget {
        ContextTarget { kind, source_range, hit_offset }
    }
}

/// A drag hovering over — or dropped on — the document surface.
#[derive(Debug, Clone)]
pub struct DocumentDrop {
    pub pasteboard: Retained<NSPasteboard>,
    /// Source UTF-16 offset under the pointer, already converted out of
    /// TextKit's hybrid space by `DisplayMap`.
    pub source_offset: isize,
}

impl DocumentDrop {
    pub fn new(pasteboard: Retained<NSPasteboard>, source_offset: isize) -> DocumentDrop {
        DocumentDrop { pasteboard, source_offset }
    }
}

/// Everything the text surface hands back to the app. Every method has the
/// Swift protocol extension's default, so a host implements only what it
/// cares about.
#[allow(unused_variables)]
pub trait MarkdownTextViewDelegate {
    fn did_activate_link(&self, view: &MarkdownTextView, destination: &str, range: NSRange, modifiers: NSEventModifierFlags) {}
    fn did_activate_image(&self, view: &MarkdownTextView, source: &str, range: NSRange) {}
    fn did_activate_front_matter_at(&self, view: &MarkdownTextView, range: NSRange) {}
    fn did_activate_path_token(&self, view: &MarkdownTextView, token: &PathToken, range: NSRange) {}
    fn did_toggle_checkbox_at_mark_offset(&self, view: &MarkdownTextView, offset: isize) {}
    fn did_activate_heading_anchor(&self, view: &MarkdownTextView, heading_index: usize, modifiers: NSEventModifierFlags) {}
    fn did_request_heading_level(&self, view: &MarkdownTextView, level: Option<isize>, heading_index: usize) {}
    fn wants_context_menu_for(&self, view: &MarkdownTextView, target: &ContextTarget) -> Option<Retained<NSMenu>> {
        None
    }
    /// Return false to draw a path token as missing (§8.4).
    fn path_exists_for(&self, view: &MarkdownTextView, token: &PathToken) -> bool {
        true
    }
    fn did_change_selection(&self, view: &MarkdownTextView) {}
    fn did_scroll(&self, view: &MarkdownTextView) {}
    fn did_change_source_focus(&self, view: &MarkdownTextView, focus: SourceFocus) {}
    /// Text was edited in Live mode. The app owns the document and reparses.
    fn did_edit(&self, view: &MarkdownTextView, range: NSRange, delta: isize) {}
    /// The view moved the reader within the same document on its own.
    fn did_navigate_to(&self, view: &MarkdownTextView, destination: isize) {}
    fn did_request_text_size_steps(&self, view: &MarkdownTextView, steps: isize) {}
    fn did_request_smart_text_zoom(&self, view: &MarkdownTextView) {}
    /// Every scroll event, offered to the host first. Return `true` to
    /// consume it.
    fn should_claim_scroll_gesture(&self, view: &MarkdownTextView, event: &NSEvent) -> bool {
        false
    }
    fn can_accept_drop(&self, view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        false
    }
    fn did_accept_drop(&self, view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        false
    }
    fn wants_quick_look_for(&self, view: &MarkdownTextView, target: &ContextTarget) -> bool {
        false
    }
}
