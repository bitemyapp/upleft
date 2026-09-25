//! Port of `View/MarkdownTextView+Interaction.swift`: §7.1 — the pointer is
//! the primary interaction path. Hit testing, hover, clicks, context menus,
//! editing in source coordinates, copy and export, drops, and Quick Look.
//!
//! The Objective-C overrides that reach these methods are registered in
//! `markdown_text_view`'s `define_class!`.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::Cell;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, Message, msg_send};
use objc2_app_kit::NSAttributedStringAppKitDocumentFormats;
use objc2_app_kit::{
    NSBezierPath, NSCursor, NSDragOperation, NSDraggingInfo, NSEvent, NSEventModifierFlags, NSMenu,
    NSParagraphStyle, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeHTML, NSPasteboardTypePNG,
    NSPasteboardTypeRTF, NSPasteboardTypeString, NSPasteboardTypeTIFF, NSTextInputClient,
};
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSMutableAttributedString, NSPoint, NSRect, NSString,
    NSStringEnumerationOptions, NSURL,
};
use upleft_core::{NSRange, PathToken, ZoomLevel};

use crate::appkit_compat::{RectExt, WorkItem, attribute_at, attribute_value, enumerate_attribute, from_ns, keys, ns, rect};
use crate::engine::display_map::{ParagraphIndex, RangeSet};
use crate::engine::render_metrics;
use crate::render_contracts::{FragmentKind, FragmentPayload, RenderMode, SourceFocus, attribute_keys};
use crate::swift_compat::{smax, smin};
use crate::view::fragment_provider::object_geometry;
use crate::view::markdown_smart_paste::{
    MarkdownPasteMode, MarkdownPastePayload, MarkdownSmartPaste, downright_markdown_type,
};
use crate::view::markdown_text_view::MarkdownTextView;
use crate::view::markdown_text_view_delegate::{ContextTarget, ContextTargetKind, DocumentDrop, ScrollPosition};

#[derive(Debug, Clone, Copy)]
struct CheckboxHit {
    mark_offset: isize,
    checked: bool,
    block_range: NSRange,
}

/// A hit: the attribute value and its effective range, in source offsets.
pub type AttributeHit = (Retained<AnyObject>, NSRange);

impl MarkdownTextView {
    pub fn activate_link_at_caret(&self) -> bool {
        let Some((destination, range)) = self.link_at_caret() else { return false };
        if let Some(delegate) = self.delegate() {
            delegate.did_activate_link(self, &destination, range, NSEventModifierFlags::empty());
        }
        true
    }

    pub fn move_to_link(&self, forward: bool) -> bool {
        let Some(storage) = (unsafe { self.textStorage() }) else { return false };
        if !(storage.length() > 0) {
            return false;
        }
        let mut links: Vec<NSRange> = Vec::new();
        enumerate_attribute(
            &storage,
            attribute_keys::dr_link(),
            objc2_foundation::NSRange::new(0, storage.length()),
            false,
            |value, range| {
                if value.is_some_and(|value| value.downcast_ref::<NSString>().is_some()) {
                    links.push(from_ns(range));
                }
                true
            },
        );
        if links.is_empty() {
            return false;
        }
        let caret = self.source_selected_range().location;
        let target = if forward {
            links.iter().copied().find(|link| link.location > caret).unwrap_or(links[0])
        } else {
            links.iter().rev().copied().find(|link| link.location < caret).unwrap_or(links[links.len() - 1])
        };
        self.set_source_selected_ranges(&[NSRange::new(target.location, 0)]);
        self.scroll_to_offset(target.location, ScrollPosition::Visible, true);
        true
    }

    fn link_at_caret(&self) -> Option<(String, NSRange)> {
        let storage = unsafe { self.textStorage() }?;
        if !(storage.length() > 0) {
            return None;
        }
        let caret = self.source_selected_range().location.min(storage.length() as isize - 1);
        for offset in [caret, (caret - 1).max(0)] {
            let mut range = objc2_foundation::NSRange::new(0, 0);
            // SAFETY: `offset` is inside the storage; `range` is an out param.
            let value = unsafe {
                storage.attribute_atIndex_longestEffectiveRange_inRange(
                    attribute_keys::dr_link(),
                    offset as usize,
                    &mut range,
                    objc2_foundation::NSRange::new(0, storage.length()),
                )
            };
            if let Some(destination) = value.and_then(|value| value.downcast::<NSString>().ok()) {
                return Some((destination.to_string(), from_ns(range)));
            }
        }
        None
    }

    // MARK: - Hit testing

    /// Source offset for a point in view coordinates.
    pub fn source_offset_at(&self, point: NSPoint) -> isize {
        let index: usize = unsafe { msg_send![self, characterIndexForInsertionAtPoint: point] };
        self.current_display_map().source_offset_for_text_kit(index as isize)
    }

    /// Attribute under the pointer, checking the character before the
    /// insertion index too.
    pub fn attribute_at_point(&self, key: &NSString, point: NSPoint) -> Option<AttributeHit> {
        self.attribute_at_point_with_offset(key, point, self.source_offset_at(point))
    }

    /// A footnote's text, for its reference's tool tip: defined in the
    /// document, or (a part of a longer one, Upleft extension) in the rest.
    fn footnote_text(&self, identifier: &str) -> Option<String> {
        let document = self.parsed_document();
        if let Some(footnote) = document.footnotes.get(identifier) {
            return Some(crate::swift_compat::trim_whitespaces_and_newlines(&document.substring(footnote.content_range)).to_owned());
        }
        document
            .segment_context
            .footnotes
            .iter()
            .find(|(other, _)| other == identifier)
            .map(|(_, text)| crate::swift_compat::trim_whitespaces_and_newlines(text).to_owned())
    }

    fn attribute_at_point_with_offset(&self, key: &NSString, point: NSPoint, offset: isize) -> Option<AttributeHit> {
        let point_sensitive = key.isEqualToString(attribute_keys::dr_link())
            || key.isEqualToString(attribute_keys::dr_path_token())
            || key.isEqualToString(attribute_keys::dr_reference());
        let storage = unsafe { self.textStorage() }?;
        if !(storage.length() > 0) {
            return None;
        }
        for candidate in [offset, offset - 1] {
            if !(candidate >= 0 && candidate < storage.length() as isize) {
                continue;
            }
            let Some(hit) = self.attribute_value_at(key, candidate) else { continue };
            if point_sensitive && !self.rendered_text_contains(point, hit.1) {
                continue;
            }
            return Some(hit);
        }
        None
    }

    /// The same hit test against an offset the caller already resolved.
    pub fn attribute_at_source_offset(&self, key: &NSString, offset: isize) -> Option<AttributeHit> {
        let storage = unsafe { self.textStorage() }?;
        if !(storage.length() > 0) {
            return None;
        }
        for candidate in [offset, offset - 1] {
            if !(candidate >= 0 && candidate < storage.length() as isize) {
                continue;
            }
            if let Some(hit) = self.attribute_value_at(key, candidate) {
                return Some(hit);
            }
        }
        None
    }

    fn attribute_value_at(&self, key: &NSString, offset: isize) -> Option<AttributeHit> {
        let storage = unsafe { self.textStorage() }?;
        if !(storage.length() > 0 && offset >= 0 && offset < storage.length() as isize) {
            return None;
        }
        let (value, range) = attribute_at(&storage, key, offset as usize)?;
        Some((value, from_ns(range)))
    }

    pub fn fragment_payload_at_point(&self, point: NSPoint) -> Option<(Retained<FragmentPayload>, NSRange)> {
        self.fragment_payload_at_source_offset(self.source_offset_at(point))
    }

    pub fn fragment_payload_at_source_offset(&self, offset: isize) -> Option<(Retained<FragmentPayload>, NSRange)> {
        let (value, range) = self.attribute_at_source_offset(attribute_keys::dr_fragment(), offset)?;
        let payload = value.downcast::<FragmentPayload>().ok()?;
        Some((payload, range))
    }

    // MARK: - Hover (§7.1)

    pub(crate) fn mouse_moved(&self, event: &NSEvent) {
        self.update_hover(self.convertPoint_fromView(event.locationInWindow(), None));
    }

    pub(crate) fn mouse_exited(&self, event: &NSEvent) {
        if let Some(rail) = self.gutter_rail()
            && rail.window().as_deref().map(|w| w as *const _) == self.window().as_deref().map(|w| w as *const _)
            && rail.bounds().contains_point(rail.convertPoint_fromView(event.locationInWindow(), None))
        {
            return;
        }
        self.clear_hover_state();
    }

    fn update_hover(&self, point: NSPoint) {
        let mut needs_redraw = false;
        let offset = self.source_offset_at(point);
        let payload = self.fragment_payload_at_source_offset(offset).map(|hit| hit.0);
        let link_hit = self.attribute_at_point_with_offset(attribute_keys::dr_link(), point, offset);
        let context = self.fragment_context();

        let hovered_fragment = match payload.as_ref().map(|payload| payload.kind()) {
            Some(FragmentKind::CodeBlock | FragmentKind::CollapsedCodeBlock | FragmentKind::Image | FragmentKind::FrontMatter) => {
                payload.as_ref().map(|payload| payload.source_range())
            }
            _ => None,
        };
        if context.hovered_fragment_range.get() != hovered_fragment {
            context.hovered_fragment_range.set(hovered_fragment);
            needs_redraw = true;
        }

        let mut row_range: Option<NSRange> = None;
        if let Some(payload) = &payload
            && payload.kind() == FragmentKind::Table
            && let Some(table) = payload.table_data().as_ref()
        {
            row_range = table.rows.iter().find(|row| !row.is_header && row.range.contains(offset)).map(|row| row.range);
        }
        if context.hovered_table_row.get() != row_range {
            context.hovered_table_row.set(row_range);
            needs_redraw = true;
        }

        let heading_index = self.parsed_document().headings.iter().position(|heading| heading.range.contains(offset));
        if self.hovered_heading_index() != heading_index {
            self.set_hovered_heading_index(heading_index);
            if let Some(rail) = self.gutter_rail() {
                rail.setNeedsDisplay(true);
            }
        }

        let is_image = payload.as_ref().is_some_and(|payload| payload.kind() == FragmentKind::Image);
        let mut link_range: Option<NSRange> = None;
        if let Some(hit) = link_hit.as_ref().filter(|hit| hit.1.length > 0)
            && !is_image
        {
            link_range = Some(hit.1);
        } else if let Some(path_hit) = self.attribute_at_point_with_offset(attribute_keys::dr_path_token(), point, offset)
            && path_hit.1.length > 0
            && self
                .attribute_at_source_offset(attribute_keys::dr_path_exists(), offset)
                .and_then(|hit| hit.0.downcast::<objc2_foundation::NSNumber>().ok())
                .is_some_and(|value| value.boolValue())
        {
            link_range = Some(path_hit.1);
        }
        if link_range != self.ivars().hovered_link_range.get() {
            self.ivars().hovered_link_range.set(link_range);
            self.setNeedsDisplay(true);
        }

        let next_tool_tip: Option<String> = if is_image {
            let alt = payload
                .as_ref()
                .map(|payload| crate::swift_compat::trim_whitespaces_and_newlines(payload.detail()).to_owned())
                .unwrap_or_default();
            if alt.is_empty() { None } else { Some(alt) }
        } else if payload.as_ref().is_some_and(|payload| payload.kind() == FragmentKind::FrontMatter) {
            Some("Edit front matter".to_owned())
        } else if let Some(hit) = self.attribute_at_point_with_offset(attribute_keys::dr_reference(), point, offset)
            && let Ok(identifier) = hit.0.downcast::<NSString>()
            && let Some(text) = self.footnote_text(&identifier.to_string())
        {
            Some(text)
        } else {
            link_hit.and_then(|hit| hit.0.downcast::<NSString>().ok()).map(|destination| destination.to_string())
        };
        let current = self.toolTip().map(|tip| tip.to_string());
        if current != next_tool_tip {
            self.setToolTip(next_tool_tip.map(|tip| NSString::from_str(&tip)).as_deref());
        }

        if needs_redraw {
            self.setNeedsDisplay(true);
        }
    }

    pub fn clear_hover_state(&self) {
        let context = self.fragment_context();
        context.hovered_fragment_range.set(None);
        context.hovered_table_row.set(None);
        self.set_hovered_heading_index(None);
        self.ivars().hovered_link_range.set(None);
        self.setToolTip(None);
        self.setNeedsDisplay(true);
        if let Some(rail) = self.gutter_rail() {
            rail.setNeedsDisplay(true);
        }
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }

    /// One definition of "the pointer is over something that responds".
    fn has_interactive_target(&self, point: NSPoint) -> bool {
        if self.semantic_checkbox(point).is_some() {
            return true;
        }
        let offset = self.source_offset_at(point);
        let payload_kind = || self.fragment_payload_at_source_offset(offset).map(|hit| hit.0.kind());
        if self.mode() == RenderMode::Live {
            return self.attribute_at_point_with_offset(attribute_keys::dr_checkbox(), point, offset).is_some()
                || self.attribute_at_point_with_offset(attribute_keys::dr_link(), point, offset).is_some()
                || self.attribute_at_point_with_offset(attribute_keys::dr_reference(), point, offset).is_some()
                || self.attribute_at_point_with_offset(attribute_keys::dr_path_token(), point, offset).is_some()
                || payload_kind() == Some(FragmentKind::Image)
                || payload_kind() == Some(FragmentKind::FrontMatter);
        }
        if self.mode() != RenderMode::Read {
            return false;
        }
        self.attribute_at_point_with_offset(attribute_keys::dr_link(), point, offset).is_some()
            || self.attribute_at_point_with_offset(attribute_keys::dr_reference(), point, offset).is_some()
            || self.attribute_at_point_with_offset(attribute_keys::dr_path_token(), point, offset).is_some()
            || payload_kind() == Some(FragmentKind::Image)
            || payload_kind() == Some(FragmentKind::FrontMatter)
    }

    fn set_pointer_cursor(&self, interactive: bool) {
        if interactive {
            NSCursor::pointingHandCursor().set();
        } else if self.isEditable() {
            NSCursor::IBeamCursor().set();
        } else {
            NSCursor::arrowCursor().set();
        }
    }

    fn confirm_checkbox_toggle(&self, block_range: NSRange, checked: bool) {
        // A hosted view shows only the state its text holds: the host may
        // decline the toggle, and then nothing on screen should change.
        if self.is_hosted() {
            return;
        }
        if self.style_sheet().reduce_motion {
            self.setNeedsDisplayInRect(self.pulse_invalidation_rect(&[block_range]));
            return;
        }
        self.fragment_context().begin_checkbox_pulse(block_range, checked);
        self.arm_motion_driver();
    }

    /// The area a pulsing ornament can reach.
    pub fn pulse_invalidation_rect(&self, ranges: &[NSRange]) -> NSRect {
        let mut union = crate::appkit_compat::RECT_ZERO;
        for range in ranges {
            let Some(start) = self.rect_for_offset(range.location) else { continue };
            let end = self.rect_for_offset(range.location.max(range.upper_bound() - 1)).unwrap_or(start);
            union = if union.is_empty() { start.union(end) } else { union.union(start.union(end)) };
        }
        let bounds = self.bounds();
        if union.is_empty() {
            return bounds;
        }
        union.origin.x = bounds.min_x();
        union.size.width = bounds.width();
        union.inset_by(0.0, -render_metrics::VERTICAL_INSET)
    }

    pub(crate) fn cursor_update(&self, event: &NSEvent) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        self.set_pointer_cursor(self.has_interactive_target(point));
    }

    // MARK: - Click (§7.1)

    fn click_activates(&self, modifiers: NSEventModifierFlags, click_count: isize) -> bool {
        if self.mode() == RenderMode::Source {
            return false;
        }
        if click_count != 1 {
            return false;
        }
        let editing = NSEventModifierFlags::Option | NSEventModifierFlags::Shift | NSEventModifierFlags::Control;
        self.mode() == RenderMode::Read || (modifiers & editing).is_empty()
    }

    pub(crate) fn mouse_down(&self, event: &NSEvent) {
        self.interrupt_animated_scroll();
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let modifiers = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        let click_count = event.clickCount();
        let mut activate_after_tracking: Option<Box<dyn FnOnce()>> = None;

        if self.source_focus_done_rect().is_some_and(|done| done.inset_by(-4.0, -4.0).contains_point(point)) {
            self.clear_source_focus();
            return;
        }

        if self.handle_code_block_chrome(point) {
            return;
        }

        if self.mode() != RenderMode::Source
            && let Some((payload, _)) = self.fragment_payload_at_point(point)
            && payload.kind() == FragmentKind::FrontMatter
            && self.click_activates(modifiers, click_count)
        {
            if let Some(delegate) = self.delegate() {
                delegate.did_activate_front_matter_at(self, payload.source_range());
            }
            return;
        }

        if self.mode() != RenderMode::Source
            && let Some(hit) = self.attribute_at_point(attribute_keys::dr_elided(), point)
        {
            self.expand_elision(hit.1.location);
            return;
        }

        if let Some(hit) = self.semantic_checkbox(point) {
            if click_count != 1 {
                return;
            }
            if let Some(delegate) = self.delegate() {
                delegate.did_toggle_checkbox_at_mark_offset(self, hit.mark_offset);
            }
            self.confirm_checkbox_toggle(hit.block_range, !hit.checked);
            return;
        }

        if self.mode() != RenderMode::Source
            && let Some(hit) = self.attribute_at_point(attribute_keys::dr_checkbox(), point)
        {
            if click_count != 1 {
                return;
            }
            let was_checked = hit
                .0
                .downcast::<objc2_foundation::NSNumber>()
                .map(|value| value.boolValue())
                .unwrap_or(false);
            if let Some(delegate) = self.delegate() {
                delegate.did_toggle_checkbox_at_mark_offset(self, hit.1.location);
            }
            if let Some((payload, _)) = self.fragment_payload_at_point(point) {
                self.confirm_checkbox_toggle(payload.source_range(), !was_checked);
            }
            return;
        }

        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        if let Some(hit) = self.attribute_at_point(attribute_keys::dr_path_token(), point)
            && let Some(token) = path_token_of(&hit.0)
            && self.click_activates(modifiers, click_count)
        {
            let weak = weak.clone();
            let range = hit.1;
            activate_after_tracking = Some(Box::new(move || {
                let Some(view) = weak.load() else { return };
                if let Some(delegate) = view.delegate() {
                    delegate.did_activate_path_token(&view, &token, range);
                }
            }));
        }
        if let Some(hit) = self.attribute_at_point(attribute_keys::dr_link(), point)
            && let Ok(destination) = hit.0.clone().downcast::<NSString>()
            && self.click_activates(modifiers, click_count)
        {
            let weak = weak.clone();
            let range = hit.1;
            let destination = destination.to_string();
            activate_after_tracking = Some(Box::new(move || {
                let Some(view) = weak.load() else { return };
                if let Some(delegate) = view.delegate() {
                    delegate.did_activate_link(&view, &destination, range, modifiers);
                }
            }));
        }
        if let Some(hit) = self.attribute_at_point(attribute_keys::dr_reference(), point)
            && let Ok(identifier) = hit.0.clone().downcast::<NSString>()
            && let Some(footnote) = self.parsed_document().footnotes.get(&identifier.to_string()).cloned()
            && self.click_activates(modifiers, click_count)
        {
            let weak = weak.clone();
            activate_after_tracking = Some(Box::new(move || {
                let Some(view) = weak.load() else { return };
                if let Some(delegate) = view.delegate() {
                    delegate.did_navigate_to(&view, footnote.range.location);
                }
                view.scroll_to_offset(footnote.range.location, ScrollPosition::Visible, true);
            }));
        }
        if let Some((payload, _)) = self.fragment_payload_at_point(point)
            && payload.kind() == FragmentKind::Image
            && self.click_activates(modifiers, click_count)
        {
            let weak = weak.clone();
            activate_after_tracking = Some(Box::new(move || {
                let Some(view) = weak.load() else { return };
                if let Some(delegate) = view.delegate() {
                    delegate.did_activate_image(&view, payload.detail(), payload.source_range());
                }
            }));
        }

        if click_count == 1
            && modifiers.is_empty()
            && let Some(redirect) = self.redirected_code_fence_caret(point)
        {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            self.set_source_selected_ranges(&[NSRange::new(redirect, 0)]);
            self.handle_selection_changed(false, None);
            return;
        }

        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(self));
        }
        self.ivars().is_tracking_mouse_selection.set(true);
        self.ivars().suppresses_caret_reveal.set(true);
        let _: () = unsafe { msg_send![super(self), mouseDown: event] };
        self.ivars().is_tracking_mouse_selection.set(false);
        self.ivars().suppresses_caret_reveal.set(false);

        if let SourceFocus::Scoped(focus) = self.source_focus() {
            let selection = self.source_selected_range();
            let remains_inside = if selection.length == 0 {
                focus.contains(selection.location)
            } else {
                upleft_core::ns_range::ns_intersection_range(focus, selection).length > 0
            };
            if !remains_inside {
                self.clear_source_focus();
                return;
            }
        }
        self.handle_selection_changed(false, None);
        if self.source_selected_range().length == 0
            && let Some(activate) = activate_after_tracking
        {
            activate();
        }
    }

    fn semantic_checkbox(&self, point: NSPoint) -> Option<CheckboxHit> {
        if self.mode() == RenderMode::Source {
            return None;
        }
        let style_sheet = self.style_sheet();
        let column_left = self.textContainerOrigin().x;
        let sample_x = smin(smax(point.x, column_left + 1.0), column_left + style_sheet.measure_width - 1.0);
        let point_offset = self.source_offset_at(NSPoint::new(sample_x, point.y));
        let body_size = style_sheet.body_font().pointSize();
        let document = self.parsed_document();
        for task in &document.tasks {
            if let Some(focus) = self.source_focus().range()
                && focus.contains(task.mark_range.location)
            {
                continue;
            }
            if !(task.content_range.contains(point_offset) || task.mark_range.contains(point_offset)) {
                continue;
            }
            let Some(text_rect) = self.rect_for_offset(task.content_range.location) else { continue };
            let centre_y = text_rect.min_y() + smin(style_sheet.line_height, text_rect.height()) * 0.44;
            let Some(target) = object_geometry::task_hit_rect(text_rect.min_x(), centre_y, body_size) else { continue };
            if !target.contains_point(point) {
                continue;
            }
            let block = document.root.block_at(task.mark_range.location);
            return Some(CheckboxHit {
                mark_offset: task.mark_range.location,
                checked: task.is_checked,
                block_range: block.map_or(task.content_range, |block| block.range),
            });
        }
        None
    }

    pub(crate) fn cancel_operation(&self, sender: Option<&AnyObject>) {
        if self.source_focus() == SourceFocus::None {
            let _: () = unsafe { msg_send![super(self), cancelOperation: sender] };
            return;
        }
        self.clear_source_focus();
    }

    /// Where a click on a fenced block's chrome rows should really put the
    /// caret.
    fn redirected_code_fence_caret(&self, point: NSPoint) -> Option<isize> {
        if self.mode() == RenderMode::Source {
            return None;
        }
        let storage = unsafe { self.textStorage() }?;
        if !(storage.length() > 0) {
            return None;
        }
        let (payload, _) = self.fragment_payload_at_point(point)?;
        if payload.kind() != FragmentKind::CodeBlock {
            return None;
        }
        let block = upleft_core::ns_range::ns_intersection_range(payload.source_range(), NSRange::new(0, storage.length() as isize));
        if !(block.length > 0) {
            return None;
        }
        if let Some(focus) = self.source_focus().range()
            && upleft_core::ns_range::ns_intersection_range(focus, block).length > 0
        {
            return None;
        }
        let text = storage.string();
        let opening = from_ns(text.lineRangeForRange(objc2_foundation::NSRange::new(block.location as usize, 0)));
        let closing = from_ns(text.lineRangeForRange(objc2_foundation::NSRange::new((block.upper_bound() - 1) as usize, 0)));
        if opening.location == closing.location {
            return None;
        }
        let offset = self.source_offset_at(point);
        if offset < opening.upper_bound() {
            return Some(opening.upper_bound().min(block.upper_bound()));
        }
        if offset >= closing.location {
            return Some(block.location.max(closing.location - 1));
        }
        None
    }

    /// Copy button and collapsed-chip expansion.
    fn handle_code_block_chrome(&self, point: NSPoint) -> bool {
        let Some((payload, _)) = self.fragment_payload_at_point(point) else { return false };
        if !matches!(payload.kind(), FragmentKind::CodeBlock | FragmentKind::CollapsedCodeBlock) {
            return false;
        }
        let collapsed = self.is_collapsed(&payload);
        let Some(chrome) = self.rect_for_offset(payload.source_range().location) else { return false };
        if collapsed {
            self.set_code_block_collapsed(false, payload.source_range().location);
            return true;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return false };
        if !(storage.length() > 0) {
            return false;
        }
        let style_location = payload.source_range().location.max(0).min(storage.length() as isize - 1);
        let first_line_head_indent = attribute_value(&storage, keys::paragraph_style(), style_location as usize)
            .and_then(|value| value.downcast::<NSParagraphStyle>().ok())
            .map_or(render_metrics::CODE_INSET_X, |style| style.firstLineHeadIndent());
        let indent = smax(0.0, first_line_head_indent - render_metrics::CODE_INSET_X);
        let band = rect(indent, chrome.min_y(), smax(1.0, self.column_width() - indent), chrome.height());
        let Some(copy) = object_geometry::code_copy_button_rect(band, &self.style_sheet(), payload.detail()) else {
            return false;
        };
        let local = CGPoint::new(point.x - self.textContainerOrigin().x, point.y);
        if !copy.inset_by(-3.0, -3.0).contains_point(local) {
            return false;
        }
        self.copy_code_block(&payload)
    }

    pub fn copy_code_block_for_accessibility(&self) -> bool {
        let Some(storage) = (unsafe { self.textStorage() }) else { return false };
        let payload: Option<Retained<FragmentPayload>> = match self.fragment_context().hovered_fragment_range.get() {
            Some(range) if range.location >= 0 && range.location < storage.length() as isize => {
                attribute_value(&storage, attribute_keys::dr_fragment(), range.location as usize)
                    .and_then(|value| value.downcast::<FragmentPayload>().ok())
            }
            _ => {
                let selection = self.source_selected_range();
                if !(selection.location >= 0 && selection.location < storage.length() as isize) {
                    return false;
                }
                attribute_value(&storage, attribute_keys::dr_fragment(), selection.location as usize)
                    .and_then(|value| value.downcast::<FragmentPayload>().ok())
            }
        };
        let Some(payload) = payload else { return false };
        if !matches!(payload.kind(), FragmentKind::CodeBlock | FragmentKind::CollapsedCodeBlock) {
            return false;
        }
        self.copy_code_block(&payload)
    }

    fn copy_code_block(&self, payload: &FragmentPayload) -> bool {
        let code = self.code_text(payload);
        if code.is_empty() {
            return false;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        pasteboard.setString_forType(&NSString::from_str(&code), unsafe { NSPasteboardTypeString });
        let copied_range = payload.source_range();
        self.fragment_context().copied_code_range.set(Some(copied_range));
        if let Some(item) = self.ivars().copied_code_feedback_work_item.borrow_mut().take() {
            item.cancel();
        }
        self.setNeedsDisplay(true);
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(view) = weak.load() else { return };
            if view.fragment_context().copied_code_range.get() != Some(copied_range) {
                return;
            }
            view.fragment_context().copied_code_range.set(None);
            view.setNeedsDisplay(true);
        });
        *self.ivars().copied_code_feedback_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(0.9);
        true
    }

    fn code_text(&self, payload: &FragmentPayload) -> String {
        let Some(storage) = (unsafe { self.textStorage() }) else { return String::new() };
        let mut range = payload.source_range();
        range.location = range.location.max(0);
        range.length = range.length.min(storage.length() as isize - range.location);
        if !(range.length > 0) {
            return String::new();
        }
        let text = storage.attributedSubstringFromRange(ns(range)).string().to_string();
        let mut body: Vec<&str> = text.split('\n').collect();
        let is_fence = |line: Option<&&str>| -> bool {
            let Some(line) = line else { return false };
            let trimmed = crate::swift_compat::trim_whitespaces(line);
            let Some(marker) = trimmed.chars().next() else { return false };
            if marker != '`' && marker != '~' {
                return false;
            }
            trimmed.chars().take_while(|c| *c == marker).count() >= 3
        };
        if is_fence(body.first()) {
            body.remove(0);
        }
        if is_fence(body.last()) {
            body.pop();
        }
        if body.last().is_some_and(|line| line.is_empty()) {
            body.pop();
        }
        body.join("\n")
    }

    // MARK: - Context menus (§7.1)

    pub(crate) fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let offset = self.source_offset_at(point);
        let target = self.context_target(point, offset);
        if let Some(delegate) = self.delegate()
            && let Some(menu) = delegate.wants_context_menu_for(self, &target)
        {
            return Some(menu);
        }
        unsafe { msg_send![super(self), menuForEvent: event] }
    }

    fn context_target(&self, point: NSPoint, offset: isize) -> ContextTarget {
        if let Some(hit) = self.attribute_at_point_with_offset(attribute_keys::dr_path_token(), point, offset)
            && let Some(token) = path_token_of(&hit.0)
        {
            return ContextTarget::new(ContextTargetKind::PathToken(token), hit.1, Some(offset));
        }
        if let Some((payload, _)) = self.fragment_payload_at_point(point) {
            let range = payload.source_range();
            match payload.kind() {
                FragmentKind::CodeBlock | FragmentKind::CollapsedCodeBlock => {
                    return ContextTarget::new(ContextTargetKind::CodeBlock(range), range, Some(offset));
                }
                FragmentKind::Table => return ContextTarget::new(ContextTargetKind::Table(range), range, Some(offset)),
                FragmentKind::Image => {
                    return ContextTarget::new(ContextTargetKind::Image(payload.detail().to_owned()), range, Some(offset));
                }
                _ => {}
            }
        }
        if let Some(hit) = self.attribute_at_point_with_offset(attribute_keys::dr_link(), point, offset)
            && let Ok(destination) = hit.0.clone().downcast::<NSString>()
        {
            return ContextTarget::new(ContextTargetKind::Link(destination.to_string()), hit.1, Some(offset));
        }
        let document = self.parsed_document();
        if let Some(index) = document.headings.iter().position(|heading| heading.range.contains(offset)) {
            return ContextTarget::new(ContextTargetKind::Heading(index), document.headings[index].section_range, Some(offset));
        }
        let selection = self.source_selected_range();
        if selection.length > 0 && selection.contains(offset) {
            return ContextTarget::new(ContextTargetKind::Selection, selection, Some(offset));
        }
        ContextTarget::new(ContextTargetKind::Plain, NSRange::new(offset, 0), Some(offset))
    }

    /// Called by the gutter rail when a marker or anchor glyph is clicked.
    pub fn activate_heading_anchor(&self, index: usize, modifiers: NSEventModifierFlags) {
        if let Some(delegate) = self.delegate() {
            delegate.did_activate_heading_anchor(self, index, modifiers);
        }
    }

    // MARK: - Editing, in source coordinates

    /// Every mutation in the view funnels through here.
    pub fn perform_source_edit(&self, range: NSRange, replacement: &str, provenance: &str) -> bool {
        if !self.isEditable() {
            return false;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return false };
        let length = storage.length() as isize;
        let start = range.location.min(length);
        let clamped = NSRange::new(range.location.min(length).max(0), range.length.min(length - start).max(0));
        let viewport_anchor = self.capture_viewport_anchor();
        let undo = self.undoManager();
        let opened_undo_group = undo.as_ref().is_some_and(|undo| undo.groupingLevel() == 0);
        if opened_undo_group && let Some(undo) = &undo {
            undo.beginUndoGrouping();
        }
        let close_group = || {
            if opened_undo_group && let Some(undo) = &undo {
                undo.endUndoGrouping();
            }
        };
        let replacement_ns = NSString::from_str(replacement);
        if !self.shouldChangeTextInRange_replacementString(ns(clamped), Some(&replacement_ns)) {
            close_group();
            return false;
        }
        if self.zoom_level() != ZoomLevel::Everything {
            self.set_zoom_level(ZoomLevel::Everything);
        }

        let old_paragraphs: ParagraphIndex = self.paragraph_index();
        let old_hidden_ranges = self.current_display_map().base_hidden_ranges_for_edit_projection().to_vec();
        let deleted_text = storage.attributedSubstringFromRange(ns(clamped)).string().to_string();
        let inserted = replacement_ns.length() as isize;
        let projected_viewport_anchor = self.project_viewport_anchor(viewport_anchor, clamped, inserted);
        let preserves_paragraph_structure =
            !contains_paragraph_separator(&deleted_text) && !contains_paragraph_separator(replacement);
        self.project_fragment_payloads(clamped, inserted);
        self.set_last_mutation_provenance(provenance);
        self.begin_source_edit();
        storage.replaceCharactersInRange_withString(ns(clamped), &replacement_ns);
        self.rebuild_paragraph_index();
        self.adjust_scoped_source_focus(clamped, inserted);
        self.project_display_map_across_edit(
            clamped,
            inserted,
            &old_paragraphs,
            &old_hidden_ranges,
            preserves_paragraph_structure,
        );
        self.didChangeText();
        self.end_source_edit();

        if let Some(delegate) = self.delegate() {
            delegate.did_edit(self, clamped, inserted - clamped.length);
        }
        self.ivars().should_follow_caret_after_local_edit.set(true);
        self.ivars().local_edit_viewport_anchor.set(Some(projected_viewport_anchor));
        self.set_source_selected_ranges(&[NSRange::new(clamped.location + inserted, 0)]);
        self.handle_selection_changed(true, Some(projected_viewport_anchor));
        close_group();
        true
    }

    /// Keep reference-valued fragment metadata aligned with the storage.
    fn project_fragment_payloads(&self, range: NSRange, inserted_length: isize) {
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        if !(storage.length() > 0) {
            return;
        }
        let length = storage.length() as isize;
        let start = range.location.min(length);
        let mut seen: Vec<*const FragmentPayload> = Vec::new();
        enumerate_attribute(
            &storage,
            attribute_keys::dr_fragment(),
            ns(NSRange::new(start, (length - start).max(0))),
            false,
            |value, _| {
                let Some(payload) = value.and_then(|value| value.downcast_ref::<FragmentPayload>()) else { return true };
                let identity = payload as *const FragmentPayload;
                if seen.contains(&identity) {
                    return true;
                }
                seen.push(identity);
                payload.project_source_ranges(range, inserted_length);
                true
            },
        );
    }

    fn adjust_scoped_source_focus(&self, edit: NSRange, inserted_length: isize) {
        let SourceFocus::Scoped(focus) = self.source_focus() else { return };
        let delta = inserted_length - edit.length;
        let start = if edit.upper_bound() <= focus.location {
            focus.location + delta
        } else if edit.location < focus.location {
            edit.location
        } else {
            focus.location
        };
        let end = if edit.location >= focus.upper_bound() {
            focus.upper_bound()
        } else if edit.upper_bound() <= focus.upper_bound() {
            focus.upper_bound() + delta
        } else {
            edit.location + inserted_length
        };
        let storage_length = unsafe { self.textStorage() }.map_or(start.max(end), |storage| storage.length() as isize);
        let lower = start.min(storage_length).max(0);
        let upper = lower.max(end.min(storage_length));
        let index = self.paragraph_index();
        let first = index.paragraph_range_containing(lower);
        let last = index.paragraph_range_containing(lower.max(upper - 1));
        let adjusted = first.union(last);
        self.set_source_focus_value(SourceFocus::Scoped(adjusted));
        self.fragment_context().source_focus_range.set(Some(adjusted));
    }

    fn composing(&self) -> bool {
        self.hasMarkedText() || self.ivars().composing_paragraph.get().is_some()
    }

    fn with_undo_group(&self, body: impl FnOnce()) {
        let undo = self.undoManager();
        let opened = undo.as_ref().is_some_and(|undo| undo.groupingLevel() == 0);
        if opened && let Some(undo) = &undo {
            undo.beginUndoGrouping();
        }
        body();
        if opened && let Some(undo) = &undo {
            undo.endUndoGrouping();
        }
    }

    pub(crate) fn insert_text(&self, string: &AnyObject, replacement_range: objc2_foundation::NSRange) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            self.with_undo_group(|| {
                let _: () = unsafe { msg_send![super(self), insertText: string, replacementRange: replacement_range] };
            });
            return;
        }
        let text = if let Some(string) = string.downcast_ref::<NSString>() {
            string.to_string()
        } else if let Some(attributed) = string.downcast_ref::<NSAttributedString>() {
            attributed.string().to_string()
        } else {
            String::new()
        };
        let target = if replacement_range.location == objc2_foundation::NSNotFound as usize {
            self.source_selected_range()
        } else {
            self.current_display_map().source_range_for_text_kit(from_ns(replacement_range))
        };
        self.perform_source_edit(target, &text, "Edit");
    }

    /// Select All is a source operation even in rendered Document mode.
    pub(crate) fn select_all(&self, sender: Option<&AnyObject>) {
        let storage = unsafe { self.textStorage() };
        let (true, Some(storage)) = (self.isEditable(), storage) else {
            let _: () = unsafe { msg_send![super(self), selectAll: sender] };
            return;
        };
        self.set_source_selected_ranges(&[NSRange::new(0, storage.length() as isize)]);
        self.handle_selection_changed(false, None);
    }

    pub(crate) fn set_marked_text(
        &self,
        string: &AnyObject,
        selected_range: objc2_foundation::NSRange,
        replacement_range: objc2_foundation::NSRange,
    ) {
        if self.ivars().composing_paragraph.get().is_none() {
            let caret = self.source_selected_range().location;
            self.ivars().composing_paragraph.set(Some(self.paragraph_range_containing(caret)));
            self.refresh_display_map_for_composition();
        }
        self.with_undo_group(|| {
            let _: () = unsafe {
                msg_send![super(self), setMarkedText: string, selectedRange: selected_range, replacementRange: replacement_range]
            };
        });
    }

    pub(crate) fn unmark_text(&self) {
        let _: () = unsafe { msg_send![super(self), unmarkText] };
        if self.ivars().composing_paragraph.get().is_none() {
            return;
        }
        self.ivars().composing_paragraph.set(None);
        self.refresh_display_map_for_composition();
    }

    pub(crate) fn delete_backward(&self, sender: Option<&AnyObject>) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            let _: () = unsafe { msg_send![super(self), deleteBackward: sender] };
            return;
        }
        let selection = self.source_selected_range();
        if selection.length > 0 {
            self.perform_source_edit(selection, "", "Edit");
            return;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        if !(selection.location > 0) {
            return;
        }
        self.perform_source_edit(self.deletion_range_before(selection.location, &storage.string()), "", "Edit");
    }

    pub(crate) fn delete_forward(&self, sender: Option<&AnyObject>) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            let _: () = unsafe { msg_send![super(self), deleteForward: sender] };
            return;
        }
        let selection = self.source_selected_range();
        if selection.length > 0 {
            self.perform_source_edit(selection, "", "Edit");
            return;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        if !(selection.location < storage.length() as isize) {
            return;
        }
        self.perform_source_edit(self.deletion_range_after(selection.location, &storage.string()), "", "Edit");
    }

    pub(crate) fn delete_word_backward(&self, sender: Option<&AnyObject>) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            let _: () = unsafe { msg_send![super(self), deleteWordBackward: sender] };
            return;
        }
        let selection = self.source_selected_range();
        if selection.length > 0 {
            self.perform_source_edit(selection, "", "Edit");
            return;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        let Some(range) = self.word_deletion_range_before(selection.location, &storage.string()) else { return };
        self.perform_source_edit(range, "", "Edit");
    }

    pub(crate) fn delete_word_forward(&self, sender: Option<&AnyObject>) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            let _: () = unsafe { msg_send![super(self), deleteWordForward: sender] };
            return;
        }
        let selection = self.source_selected_range();
        if selection.length > 0 {
            self.perform_source_edit(selection, "", "Edit");
            return;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        let Some(range) = self.word_deletion_range_after(selection.location, &storage.string()) else { return };
        self.perform_source_edit(range, "", "Edit");
    }

    pub(crate) fn insert_newline(&self, sender: Option<&AnyObject>) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            let _: () = unsafe { msg_send![super(self), insertNewline: sender] };
            return;
        }
        self.perform_source_edit(self.source_selected_range(), "\n", "Edit");
    }

    pub(crate) fn apply_paste(&self, paste_mode: MarkdownPasteMode) {
        if !self.isEditable() {
            return;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        let Some(payload) = MarkdownSmartPaste::payload(&pasteboard, paste_mode) else { return };
        if payload == MarkdownPastePayload::Image {
            objc2_app_kit::NSBeep();
            return;
        }
        let range = self.source_selected_range();
        let selection = self.source_text(range);
        let context = MarkdownSmartPaste::context(range, &self.parsed_document(), self.mode());
        let replacement = MarkdownSmartPaste::replacement(&payload, &selection, context, paste_mode);
        let provenance = match paste_mode {
            MarkdownPasteMode::Smart => "Paste",
            MarkdownPasteMode::Markdown => "Paste as Markdown",
            MarkdownPasteMode::MatchStyle => "Paste and Match Style",
        };
        self.perform_source_edit(range, &replacement, provenance);
    }

    fn source_text(&self, range: NSRange) -> String {
        let Some(storage) = (unsafe { self.textStorage() }) else { return String::new() };
        if !(range.location >= 0 && range.upper_bound() <= storage.length() as isize) {
            return String::new();
        }
        storage.attributedSubstringFromRange(ns(range)).string().to_string()
    }

    pub(crate) fn delete_to_boundary(&self, sender: Option<&AnyObject>, paragraph: bool, beginning: bool) {
        if !self.isEditable() {
            return;
        }
        if self.composing() {
            let _: () = unsafe {
                match (paragraph, beginning) {
                    (true, true) => msg_send![super(self), deleteToBeginningOfParagraph: sender],
                    (true, false) => msg_send![super(self), deleteToEndOfParagraph: sender],
                    (false, true) => msg_send![super(self), deleteToBeginningOfLine: sender],
                    (false, false) => msg_send![super(self), deleteToEndOfLine: sender],
                }
            };
            return;
        }
        let selection = self.source_selected_range();
        if selection.length > 0 {
            self.perform_source_edit(selection, "", "Edit");
            return;
        }
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        let string = storage.string();
        let caret = selection.location.max(0).min(string.length() as isize);
        let containing = objc2_foundation::NSRange::new(caret as usize, 0);
        let boundary = from_ns(if paragraph {
            string.paragraphRangeForRange(containing)
        } else {
            string.lineRangeForRange(containing)
        });
        let range = if beginning {
            NSRange::new(boundary.location, caret - boundary.location)
        } else {
            let end = if paragraph {
                boundary.upper_bound()
            } else {
                let units: Vec<u16> = (0..string.length()).map(|index| string.characterAtIndex(index)).collect();
                RangeSet::content_end_of_paragraph(boundary, &units)
            };
            NSRange::new(caret, end - caret)
        };
        if !(range.length > 0) {
            return;
        }
        self.perform_source_edit(range, "", "Edit");
    }

    fn word_deletion_range_before(&self, caret: isize, string: &NSString) -> Option<NSRange> {
        let clamped_caret = caret.max(0).min(string.length() as isize);
        if !(clamped_caret > 0) {
            return None;
        }
        let word = first_substring_range(
            string,
            NSRange::new(0, clamped_caret),
            NSStringEnumerationOptions::ByWords | NSStringEnumerationOptions::Reverse,
        );
        if let Some(word) = word
            && word.location < clamped_caret
        {
            return Some(NSRange::new(word.location, clamped_caret - word.location));
        }
        Some(self.deletion_range_before(clamped_caret, string))
    }

    fn word_deletion_range_after(&self, caret: isize, string: &NSString) -> Option<NSRange> {
        let clamped_caret = caret.max(0).min(string.length() as isize);
        if !(clamped_caret < string.length() as isize) {
            return None;
        }
        let word = first_substring_range(
            string,
            NSRange::new(clamped_caret, string.length() as isize - clamped_caret),
            NSStringEnumerationOptions::ByWords,
        );
        if let Some(word) = word {
            return Some(NSRange::new(clamped_caret, word.upper_bound() - clamped_caret));
        }
        Some(self.deletion_range_after(clamped_caret, string))
    }

    /// A hidden marker is deleted whole.
    fn deletion_range_before(&self, caret: isize, string: &NSString) -> NSRange {
        let map = self.current_display_map();
        if let Some(hidden) = map.substitution_ending_at(caret)
            && hidden.is_hidden
        {
            return hidden.source_range;
        }
        from_ns(string.rangeOfComposedCharacterSequenceAtIndex((caret - 1) as usize))
    }

    fn deletion_range_after(&self, caret: isize, string: &NSString) -> NSRange {
        let map = self.current_display_map();
        if let Some(hidden) = map.substitution_starting_at(caret)
            && hidden.is_hidden
        {
            return hidden.source_range;
        }
        from_ns(string.rangeOfComposedCharacterSequenceAtIndex(caret as usize))
    }

    // MARK: - Copy and export (§9.5)

    /// Standard Copy follows the visible surface; raw Markdown travels as a
    /// private alternate flavour.
    pub(crate) fn copy(&self, _sender: Option<&AnyObject>) {
        let range = self.source_selected_range();
        let Some(storage) = (unsafe { self.textStorage() }) else { return };
        if !(range.length > 0) {
            return;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        self.write_selection_flavours(&pasteboard, &storage, range);
    }

    pub(crate) fn write_selection(&self, pboard: &NSPasteboard, _types: &NSArray<NSString>) -> bool {
        let range = self.source_selected_range();
        let Some(storage) = (unsafe { self.textStorage() }) else { return false };
        if !(range.length > 0) {
            return false;
        }
        self.write_selection_flavours(pboard, &storage, range);
        true
    }

    fn write_selection_flavours(&self, pasteboard: &NSPasteboard, storage: &objc2_app_kit::NSTextStorage, range: NSRange) {
        let visible = self.exportable_attributed_string(range);
        let markdown_range = self.lossless_markdown_range(range);
        let markdown = storage.attributedSubstringFromRange(ns(markdown_range)).string();
        let markdown_type = downright_markdown_type();
        let types: Retained<NSArray<NSString>> = unsafe {
            NSArray::from_slice(&[NSPasteboardTypeString, NSPasteboardTypeRTF, NSPasteboardTypeHTML, &*markdown_type])
        };
        unsafe { pasteboard.declareTypes_owner(&types, None) };
        pasteboard.setString_forType(&visible.string(), unsafe { NSPasteboardTypeString });
        pasteboard.setString_forType(&markdown, &markdown_type);
        let html = crate::clipboard_semantic_html::ClipboardSemanticHTML::render(&markdown.to_string());
        pasteboard.setString_forType(&NSString::from_str(&html), unsafe { NSPasteboardTypeHTML });
        let rtf = unsafe {
            visible.RTFFromRange_documentAttributes(objc2_foundation::NSRange::new(0, visible.length()), &NSDictionary::new())
        };
        if let Some(data) = rtf {
            pasteboard.setData_forType(Some(&data), unsafe { NSPasteboardTypeRTF });
        }
    }

    /// Hidden substitutions at both edges belong to a fully selected visible
    /// span.
    fn lossless_markdown_range(&self, range: NSRange) -> NSRange {
        let substitutions = self.current_display_map().substitutions();
        let mut result = range;
        let mut changed = true;
        while changed {
            changed = false;
            if let Some(leading) =
                substitutions.iter().rev().find(|sub| sub.is_hidden && sub.source_range.upper_bound() == result.location)
            {
                result = NSRange::new(leading.source_range.location, result.upper_bound() - leading.source_range.location);
                changed = true;
            }
            if let Some(trailing) =
                substitutions.iter().find(|sub| sub.is_hidden && sub.source_range.location == result.upper_bound())
            {
                result.length = trailing.source_range.upper_bound() - result.location;
                changed = true;
            }
        }
        result
    }

    /// Rendered-selection copy: the display string with Downright's private
    /// attributes stripped and links made real.
    pub fn attributed_string_for_rich_text_copy(&self, range: NSRange) -> Retained<NSAttributedString> {
        let Some(storage) = (unsafe { self.textStorage() }) else { return NSAttributedString::new() };
        let length = storage.length() as isize;
        let lo = range.location.min(length).max(0);
        let hi = lo.max(range.upper_bound().min(length));
        if !(hi > lo) {
            return NSAttributedString::new();
        }
        let out = NSMutableAttributedString::new();
        let mut cursor = lo;
        for sub in self.current_display_map().substitutions() {
            if !(sub.source_range.location >= lo && sub.source_range.upper_bound() <= hi) {
                continue;
            }
            if sub.source_range.location > cursor {
                out.appendAttributedString(
                    &storage.attributedSubstringFromRange(ns(NSRange::new(cursor, sub.source_range.location - cursor))),
                );
            }
            if let Some(replacement) = &sub.replacement {
                out.appendAttributedString(replacement);
            }
            cursor = sub.source_range.upper_bound();
        }
        if cursor < hi {
            out.appendAttributedString(&storage.attributedSubstringFromRange(ns(NSRange::new(cursor, hi - cursor))));
        }

        let whole = objc2_foundation::NSRange::new(0, out.length());
        let mut links: Vec<(objc2_foundation::NSRange, Retained<NSURL>)> = Vec::new();
        enumerate_attribute(&out, attribute_keys::dr_link(), whole, false, |value, range| {
            if let Some(destination) = value.and_then(|value| value.downcast_ref::<NSString>())
                && let Some(url) = NSURL::URLWithString(destination)
            {
                links.push((range, url));
            }
            true
        });
        for key in Self::private_attribute_keys() {
            out.removeAttribute_range(key, whole);
        }
        for (range, url) in links {
            unsafe { out.addAttribute_value_range(keys::link(), &url, range) };
        }
        Retained::into_super(out)
    }

    /// The selection as another application should receive it: layout
    /// fillers removed.
    pub fn exportable_attributed_string(&self, range: NSRange) -> Retained<NSAttributedString> {
        let out = NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &self.attributed_string_for_rich_text_copy(range),
        );
        let text = out.string();
        let mut filler_runs: Vec<objc2_foundation::NSRange> = Vec::new();
        let mut run_start: Option<usize> = None;
        for index in 0..text.length() {
            if is_layout_filler(text.characterAtIndex(index)) {
                if run_start.is_none() {
                    run_start = Some(index);
                }
            } else if let Some(start) = run_start.take() {
                filler_runs.push(objc2_foundation::NSRange::new(start, index - start));
            }
        }
        if let Some(start) = run_start {
            filler_runs.push(objc2_foundation::NSRange::new(start, text.length() - start));
        }
        for run in filler_runs.into_iter().rev() {
            out.deleteCharactersInRange(run);
        }
        Retained::into_super(out)
    }

    /// `MarkdownTextView.privateAttributeKeys`.
    pub fn private_attribute_keys() -> [&'static NSString; 18] {
        [
            attribute_keys::dr_hidden(),
            attribute_keys::dr_marker(),
            attribute_keys::dr_fragment(),
            attribute_keys::dr_block(),
            attribute_keys::dr_heading(),
            attribute_keys::dr_link(),
            attribute_keys::dr_path_token(),
            attribute_keys::dr_path_exists(),
            attribute_keys::dr_checkbox(),
            attribute_keys::dr_change(),
            attribute_keys::dr_reference(),
            attribute_keys::dr_elided(),
            attribute_keys::dr_gutter_marker(),
            attribute_keys::dr_search_hit(),
            attribute_keys::dr_current_search_hit(),
            attribute_keys::dr_speech_highlight(),
            attribute_keys::dr_inline_code(),
            attribute_keys::dr_source_focus(),
        ]
    }

    /// Text spoken by the native speech service.
    pub fn rendered_string_for_speech(&self, source_range: NSRange) -> String {
        self.speech_projection(source_range).0
    }

    /// Convert a range reported by the speech synthesizer back to source
    /// space.
    pub fn source_range_for_speech_range(&self, rendered_range: NSRange, source_range: NSRange) -> Option<NSRange> {
        if !(rendered_range.location >= 0 && rendered_range.length >= 0) {
            return None;
        }
        let (_, speech_to_text_kit) = self.speech_projection(source_range);
        if !(rendered_range.upper_bound() as usize <= speech_to_text_kit.len()) {
            return None;
        }
        let map = self.current_display_map();
        let base = map.text_kit_offset_for_source(source_range.location);
        let (text_kit_start, text_kit_end) = if rendered_range.length == 0 {
            let start = if (rendered_range.location as usize) < speech_to_text_kit.len() {
                speech_to_text_kit[rendered_range.location as usize]
            } else {
                speech_to_text_kit.last().map_or(0, |last| last + 1)
            };
            (start, start)
        } else {
            (
                speech_to_text_kit[rendered_range.location as usize],
                speech_to_text_kit[(rendered_range.upper_bound() - 1) as usize] + 1,
            )
        };
        let text_kit_range = NSRange::new(base + text_kit_start, text_kit_end - text_kit_start);
        let mapped = map.source_range_for_text_kit(text_kit_range);
        if !(mapped.location >= source_range.location && mapped.upper_bound() <= source_range.upper_bound()) {
            return None;
        }
        Some(mapped)
    }

    fn speech_projection(&self, source_range: NSRange) -> (String, Vec<isize>) {
        let attributed = self.attributed_string_for_rich_text_copy(source_range);
        let string = attributed.string();
        let mut units: Vec<u16> = Vec::with_capacity(string.length());
        let mut map: Vec<isize> = Vec::with_capacity(string.length());
        for index in 0..string.length() {
            let unit = string.characterAtIndex(index);
            if unit == 0x2060 || unit == 0x200B || unit == 0xFEFF {
                continue;
            }
            units.push(unit);
            map.push(index as isize);
        }
        (String::from_utf16_lossy(&units), map)
    }

    /// §9.5: export the selection as an image.
    pub fn image_for_selection(&self, range: NSRange) -> Option<Retained<objc2_app_kit::NSImage>> {
        let start = self.rect_for_offset(range.location)?;
        let end = self.rect_for_offset(range.upper_bound())?;
        let mut union = start.union(end);
        union.origin.x = 0.0;
        union.size.width = self.bounds().size.width;
        union = union.inset_by(0.0, -6.0);
        if !(union.width() > 1.0 && union.height() > 1.0) {
            return None;
        }
        let rep = self.bitmapImageRepForCachingDisplayInRect(union)?;
        self.cacheDisplayInRect_toBitmapImageRep(union, &rep);
        let image = objc2_app_kit::NSImage::initWithSize(objc2_app_kit::NSImage::alloc(), union.size);
        image.addRepresentation(&rep);
        Some(image)
    }

    // MARK: - Dropping files and images onto the document (§7.1)

    /// `MarkdownTextView.documentDropTypes`.
    pub fn document_drop_types() -> Vec<Retained<NSString>> {
        unsafe {
            vec![
                NSPasteboardTypeFileURL.retain(),
                NSPasteboardTypePNG.retain(),
                NSPasteboardTypeTIFF.retain(),
                NSString::from_str("public.jpeg"),
                NSString::from_str("public.heic"),
            ]
        }
    }

    /// Additive and idempotent: our types are added while editable and
    /// removed again when the surface stops being editable.
    pub(crate) fn update_drag_type_registration_additions(&self) {
        let ours = Self::document_drop_types();
        let registered = self.registeredDraggedTypes();
        let mut types: Vec<Retained<NSString>> =
            registered.iter().filter(|kind| !ours.iter().any(|our| our.isEqualToString(kind))).collect();
        if self.isEditable() {
            types.extend(ours);
        }
        if types.is_empty() {
            self.unregisterDraggedTypes();
        } else {
            let array = NSArray::from_retained_slice(&types);
            self.registerForDraggedTypes(&array);
        }
    }

    fn drop_for(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> DocumentDrop {
        let pasteboard = sender.draggingPasteboard();
        let location = sender.draggingLocation();
        DocumentDrop::new(pasteboard, self.source_offset_at(self.convertPoint_fromView(location, None)))
    }

    pub(crate) fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let candidate = self.drop_for(sender);
        let claims = self.isEditable() && self.delegate().is_some_and(|delegate| delegate.can_accept_drop(self, &candidate));
        self.ivars().claims_active_drag.set(claims);
        if !claims {
            self.set_drop_insertion_offset(None);
            return unsafe { msg_send![super(self), draggingEntered: sender] };
        }
        self.set_drop_insertion_offset(Some(candidate.source_offset));
        NSDragOperation::Copy
    }

    pub(crate) fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        if !self.ivars().claims_active_drag.get() {
            return unsafe { msg_send![super(self), draggingUpdated: sender] };
        }
        self.set_drop_insertion_offset(Some(self.drop_for(sender).source_offset));
        NSDragOperation::Copy
    }

    pub(crate) fn dragging_exited(&self, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
        self.set_drop_insertion_offset(None);
        if !self.ivars().claims_active_drag.get() {
            let _: () = unsafe { msg_send![super(self), draggingExited: sender] };
            return;
        }
        self.ivars().claims_active_drag.set(false);
    }

    pub(crate) fn prepare_for_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
        if self.ivars().claims_active_drag.get() {
            return true;
        }
        unsafe { msg_send![super(self), prepareForDragOperation: sender] }
    }

    pub(crate) fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
        if !self.ivars().claims_active_drag.get() {
            return unsafe { msg_send![super(self), performDragOperation: sender] };
        }
        let released = self.drop_for(sender);
        self.set_drop_insertion_offset(None);
        self.ivars().claims_active_drag.set(false);
        self.delegate().is_some_and(|delegate| delegate.did_accept_drop(self, &released))
    }

    /// Moves the drop caret, repainting only the two lines it can have been
    /// on.
    pub(crate) fn set_drop_insertion_offset(&self, offset: Option<isize>) {
        if offset == self.ivars().drop_insertion_offset.get() {
            return;
        }
        let previous = self.ivars().drop_insertion_offset.get();
        self.ivars().drop_insertion_offset.set(offset);
        for candidate in [previous, offset] {
            let Some(rect) = candidate.and_then(|candidate| self.drop_caret_rect(candidate)) else {
                self.setNeedsDisplay(true);
                return;
            };
            self.setNeedsDisplayInRect(rect.inset_by(-3.0, -3.0));
        }
    }

    fn drop_caret_rect(&self, source_offset: isize) -> Option<NSRect> {
        let width: CGFloat = 2.0;
        if let Some(found) = self.rect_for_offset(source_offset) {
            return Some(rect(found.min_x() - width / 2.0, found.min_y(), width, found.height()));
        }
        if !(source_offset > 0) {
            return None;
        }
        let found = self.rect_for_offset(source_offset - 1)?;
        Some(rect(found.max_x() - width / 2.0, found.min_y(), width, found.height()))
    }

    pub(crate) fn draw_drop_insertion_caret(&self, dirty_rect: NSRect) {
        let Some(offset) = self.ivars().drop_insertion_offset.get() else { return };
        let Some(caret) = self.drop_caret_rect(offset).filter(|caret| caret.intersects(dirty_rect)) else { return };
        self.style_sheet().accent.setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(caret, 1.0, 1.0).fill();
    }

    // MARK: - Quick Look (§7.1)

    pub(crate) fn quick_look(&self, event: &NSEvent) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        if let Some(target) = self.quick_look_target(point)
            && self.delegate().is_some_and(|delegate| delegate.wants_quick_look_for(self, &target))
        {
            return;
        }
        let _: () = unsafe { msg_send![super(self), quickLookWithEvent: event] };
    }

    pub fn quick_look_target(&self, point: NSPoint) -> Option<ContextTarget> {
        let offset = self.source_offset_at(point);
        previewable(self.context_target(point, offset)).or_else(|| self.previewable_target(offset))
    }

    /// What a Quick Look at the current selection would preview.
    pub fn quick_look_target_at_selection(&self) -> Option<ContextTarget> {
        let selection = self.source_selected_range();
        if !(selection.length > 0 || self.primary_source_caret().is_some()) {
            return None;
        }
        self.previewable_target(selection.location)
    }

    fn previewable_target(&self, offset: isize) -> Option<ContextTarget> {
        if let Some(hit) = self.attribute_at_source_offset(attribute_keys::dr_path_token(), offset)
            && let Some(token) = path_token_of(&hit.0)
        {
            return Some(ContextTarget::new(ContextTargetKind::PathToken(token), hit.1, Some(offset)));
        }
        if let Some((payload, _)) = self.fragment_payload_at_source_offset(offset)
            && payload.kind() == FragmentKind::Image
        {
            return Some(ContextTarget::new(
                ContextTargetKind::Image(payload.detail().to_owned()),
                payload.source_range(),
                Some(offset),
            ));
        }
        if let Some(hit) = self.attribute_at_source_offset(attribute_keys::dr_link(), offset)
            && let Ok(destination) = hit.0.downcast::<NSString>()
        {
            return Some(ContextTarget::new(ContextTargetKind::Link(destination.to_string()), hit.1, Some(offset)));
        }
        None
    }

    /// Space, but only where it can never be a character.
    pub fn handle_quick_look_space(&self, event: &NSEvent) -> bool {
        if self.isEditable()
            || event.keyCode() != 49
            || !(event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask).is_empty()
        {
            return false;
        }
        let Some(target) = self.quick_look_target_at_selection() else { return false };
        self.delegate().is_some_and(|delegate| delegate.wants_quick_look_for(self, &target))
    }
}

fn previewable(target: ContextTarget) -> Option<ContextTarget> {
    match target.kind {
        ContextTargetKind::PathToken(_) | ContextTargetKind::Image(_) | ContextTargetKind::Link(_) => Some(target),
        _ => None,
    }
}

/// `PathToken` values ride on the storage as the engine's attribute object.
fn path_token_of(value: &AnyObject) -> Option<PathToken> {
    value.downcast_ref::<crate::swift_value::PathTokenValue>().map(|value| value.token().clone())
}

fn contains_paragraph_separator(string: &str) -> bool {
    string.chars().any(|c| matches!(c as u32, 0x0A | 0x0D | 0x0085 | 0x2028 | 0x2029))
}

fn is_layout_filler(unit: u16) -> bool {
    matches!(unit, 0x2060 | 0x200B | 0xFEFF)
}

/// The first substring range `enumerateSubstrings(in:options:)` reports.
fn first_substring_range(string: &NSString, range: NSRange, options: NSStringEnumerationOptions) -> Option<NSRange> {
    let found: std::rc::Rc<Cell<Option<NSRange>>> = std::rc::Rc::new(Cell::new(None));
    let sink = found.clone();
    let block = block2::RcBlock::new(
        move |_substring: *mut NSString,
         substring_range: objc2_foundation::NSRange,
         _enclosing: objc2_foundation::NSRange,
         stop: std::ptr::NonNull<objc2::runtime::Bool>| {
            sink.set(Some(from_ns(substring_range)));
            unsafe { stop.as_ptr().write(objc2::runtime::Bool::YES) };
        },
    );
    string.enumerateSubstringsInRange_options_usingBlock(ns(range), options, &block);
    found.get()
}
