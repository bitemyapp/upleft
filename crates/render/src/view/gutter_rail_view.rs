//! Port of `View/GutterRailView.swift`: §6.1a — the reason Live mode does not
//! jump.
//!
//! The rail owns only state that earns a pointer action: change bars,
//! Source-mode line numbers, and the contextual H1…H6 control. It is a plain
//! `NSView` positioned from the layout manager's fragment geometry.

use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol};
use objc2_app_kit::NSAccessibility;
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibilityCustomAction, NSAttributedStringNSStringDrawing, NSBezierPath, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSFont, NSFontWeightMedium, NSMenu, NSMenuItem,
    NSResponder, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSAttributedString, NSObject, NSPoint, NSRect, NSSize, NSString};
use upleft_core::{HeadingNode, NSRange};

use crate::appkit_compat::{RectExt, attributed_string, from_ns, keys, rect};
use crate::core_types::ChangeKind;
use crate::render_contracts::RenderMode;
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::markdown_text_view::MarkdownTextView;

pub struct GutterRailViewIvars {
    text_view: ObjcWeak<MarkdownTextView>,
    markers: RefCell<Vec<(isize, String, isize)>>,
    change_bars: RefCell<Vec<(ChangeKind, NSRange)>>,
    line_starts: RefCell<Vec<isize>>,
    heading_menu_action: RefCell<Option<Retained<HeadingMenuAction>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
}

define_class!(
    // SAFETY: NSView's designated initialiser `initWithFrame:` is forwarded in
    // `new`; overrides keep AppKit's signatures. No Drop impl.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "GutterRailView"]
    #[ivars = GutterRailViewIvars]
    pub struct GutterRailView;

    unsafe impl NSObjectProtocol for GutterRailView {}

    impl GutterRailView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty_rect: NSRect) {
            self.draw(dirty_rect);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            self.refresh_tracking_areas();
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) {
            self.update_heading_hover(self.convertPoint_fromView(event.locationInWindow(), None));
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.update_heading_hover(self.convertPoint_fromView(event.locationInWindow(), None));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let Some(text_view) = self.text_view() else { return };
            if text_view.hovered_heading_index().is_none() {
                return;
            }
            text_view.set_hovered_heading_index(None);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.handle_mouse_down(event);
        }
    }
);

impl GutterRailView {
    /// Gap between the rail's right edge and the shared right edge everything
    /// in the rail hangs from.
    const MARKER_INSET: CGFloat = 8.0;

    pub fn new(text_view: &MarkdownTextView, mtm: MainThreadMarker) -> Retained<GutterRailView> {
        let this = Self::alloc(mtm).set_ivars(GutterRailViewIvars {
            text_view: ObjcWeak::from(text_view),
            markers: RefCell::new(Vec::new()),
            change_bars: RefCell::new(Vec::new()),
            line_starts: RefCell::new(vec![0]),
            heading_menu_action: RefCell::new(None),
            tracking_area: RefCell::new(None),
        });
        let this: Retained<GutterRailView> = unsafe { msg_send![super(this), initWithFrame: crate::appkit_compat::RECT_ZERO] };
        // `clipsToBounds` defaults to false from macOS 14; clip, and fill
        // `bounds`, never `dirtyRect`.
        this.setClipsToBounds(true);
        text_view.set_gutter_rail(&this);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { objc2_app_kit::NSAccessibilityGroupRole }));
        this.setAccessibilityLabel(Some(&NSString::from_str("Document margin")));
        this.setAccessibilityHelp(Some(&NSString::from_str("Heading level and change controls")));
        this.install_accessibility_actions();
        let _: () = unsafe { msg_send![&*this, updateTrackingAreas] };
        this
    }

    fn install_accessibility_actions(&self) {
        let weak_level: ObjcWeak<GutterRailView> = ObjcWeak::from(self);
        let weak_fold = weak_level.clone();
        let choose_level = RcBlock::new(move || -> Bool {
            let Some(rail) = weak_level.load() else { return Bool::NO };
            let Some(text_view) = rail.text_view() else { return Bool::NO };
            let Some(index) = current_heading_index(&text_view) else { return Bool::NO };
            Bool::new(rail.present_heading_menu(index, None))
        });
        let toggle_fold = RcBlock::new(move || -> Bool {
            let Some(rail) = weak_fold.load() else { return Bool::NO };
            let Some(text_view) = rail.text_view() else { return Bool::NO };
            let Some(index) = current_heading_index(&text_view) else { return Bool::NO };
            if let Some(delegate) = text_view.markdown_delegate() {
                delegate.did_activate_heading_anchor(&text_view, index, NSEventModifierFlags::Option);
            }
            Bool::YES
        });
        let actions = NSArray::from_retained_slice(&[
            NSAccessibilityCustomAction::initWithName_handler(
                NSAccessibilityCustomAction::alloc(),
                &NSString::from_str("Choose current heading level"),
                Some(&choose_level),
            ),
            NSAccessibilityCustomAction::initWithName_handler(
                NSAccessibilityCustomAction::alloc(),
                &NSString::from_str("Toggle current section fold"),
                Some(&toggle_fold),
            ),
        ]);
        self.setAccessibilityCustomActions(Some(&actions));
    }

    fn text_view(&self) -> Option<Retained<MarkdownTextView>> {
        self.ivars().text_view.load()
    }

    /// Recomputed when the document changes; positions are resolved per draw.
    pub fn reload(&self) {
        let Some(text_view) = self.text_view() else { return };
        let document = text_view.parsed_document();
        *self.ivars().markers.borrow_mut() = text_view.ivars().engine.borrow().gutter_markers(&document);
        *self.ivars().change_bars.borrow_mut() = text_view.change_marks().iter().map(|mark| (mark.kind, mark.range)).collect();
        let mut line_starts = vec![0isize];
        let source: Retained<NSString> =
            unsafe { text_view.textStorage() }.map_or_else(|| NSString::from_str(""), |storage| storage.string());
        let length = source.length();
        let mut cursor = 0usize;
        while cursor < length {
            let range = source.lineRangeForRange(objc2_foundation::NSRange::new(cursor, 0));
            if !(range.location + range.length > cursor) {
                break;
            }
            cursor = range.location + range.length;
            if cursor < length {
                line_starts.push(cursor as isize);
            }
        }
        *self.ivars().line_starts.borrow_mut() = line_starts;
        self.setNeedsDisplay(true);
    }

    fn draw(&self, dirty_rect: NSRect) {
        let Some(text_view) = self.text_view() else { return };
        let style = text_view.style_sheet();
        let bounds = self.bounds();
        style.background.setFill();
        crate::appkit_compat::rect_fill(dirty_rect.intersection(bounds));

        let visible = self.convertRect_fromView(text_view.visibleRect(), Some(&text_view));

        if text_view.mode() == RenderMode::Source {
            self.draw_line_numbers(&style, &text_view, visible);
        }

        // §8.1: changed blocks get a coloured bar in the margin.
        let change_bar_height = style.line_height;
        for (kind, range) in self.ivars().change_bars.borrow().iter() {
            let Some(block) = self.block_rect(*range, &text_view).filter(|block| block.intersects(visible)) else {
                continue;
            };
            let color = style.change_color(*kind);
            let height = smax(change_bar_height, block.height());
            match kind {
                ChangeKind::Inserted => {
                    color.setFill();
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                        rect(bounds.max_x() - 5.0, block.min_y(), 2.5, height),
                        1.25,
                        1.25,
                    )
                    .fill();
                }
                ChangeKind::Modified => {
                    color.setStroke();
                    let path = NSBezierPath::bezierPath();
                    path.setLineWidth(2.0);
                    let pattern: [CGFloat; 2] = [3.0, 2.0];
                    unsafe { path.setLineDash_count_phase(pattern.as_ptr(), 2, 0.0) };
                    path.moveToPoint(NSPoint::new(bounds.max_x() - 3.5, block.min_y()));
                    path.lineToPoint(NSPoint::new(bounds.max_x() - 3.5, block.min_y() + height));
                    path.stroke();
                }
                ChangeKind::Deleted => {
                    let wedge = NSBezierPath::bezierPath();
                    wedge.moveToPoint(NSPoint::new(bounds.max_x() - 7.0, block.min_y()));
                    wedge.lineToPoint(NSPoint::new(bounds.max_x() - 2.0, block.min_y() + 4.0));
                    wedge.lineToPoint(NSPoint::new(bounds.max_x() - 7.0, block.min_y() + 8.0));
                    wedge.closePath();
                    color.setFill();
                    wedge.fill();
                }
            }
        }

        if text_view.mode() == RenderMode::Source {
            return;
        }

        let active_index = self.active_heading_index();
        let document = text_view.parsed_document();
        let Some(index) = text_view.hovered_heading_index().or(active_index) else { return };
        if index >= document.headings.len() {
            return;
        }
        let heading = &document.headings[index];
        let Some(row) = self.row_rect(heading.range, &text_view).filter(|row| row.intersects(visible)) else { return };

        let chip = self.heading_chip_rect(index, row, &text_view);
        let title = self.heading_chip_title(heading, text_view.hovered_heading_index() == Some(index));
        style.inline_code_background.colorWithAlphaComponent(0.72).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(chip, 4.0, 4.0).fill();
        let size = title.size();
        title.drawAtPoint(NSPoint::new(chip.min_x() + 5.0, chip.min_y() + (chip.height() - size.height) / 2.0));
    }

    fn draw_line_numbers(&self, style: &StyleSheet, text_view: &MarkdownTextView, visible: NSRect) {
        let top = text_view.top_visible_offset();
        let line_starts = self.ivars().line_starts.borrow().clone();
        let first = line_starts.iter().rposition(|start| *start <= top).unwrap_or(0).saturating_sub(2);
        let font = style.mono_font(Some(smax(11.0, style.body_font().pointSize() * 0.58)));
        let color = style.marker.colorWithAlphaComponent(0.72);
        let bounds = self.bounds();
        for index in first..line_starts.len() {
            let Some(row) = self.row_rect(NSRange::new(line_starts[index], 0), text_view) else { continue };
            if row.min_y() > visible.max_y() + 40.0 {
                break;
            }
            if !(row.max_y() >= visible.min_y() - 40.0) {
                continue;
            }
            let label = attributed_string(
                &(index + 1).to_string(),
                &[(keys::font(), &*font as &AnyObject), (keys::foreground_color(), &*color)],
            );
            let size = label.size();
            label.drawAtPoint(NSPoint::new(
                smax(2.0, bounds.size.width - Self::MARKER_INSET - size.width),
                row.min_y() + smax(0.0, (style.line_height - size.height) / 2.0),
            ));

            let next_y = if index + 1 < line_starts.len() {
                self.row_rect(NSRange::new(line_starts[index + 1], 0), text_view).map(|next| next.min_y())
            } else {
                None
            };
            let Some(next_y) = next_y else { continue };
            let mut continuation_y = row.min_y() + style.line_height;
            let continuation =
                attributed_string("↳", &[(keys::font(), &*font as &AnyObject), (keys::foreground_color(), &*color)]);
            let continuation_size = continuation.size();
            while continuation_y + style.line_height * 0.5 < next_y {
                continuation.drawAtPoint(NSPoint::new(
                    smax(2.0, bounds.size.width - Self::MARKER_INSET - continuation_size.width),
                    continuation_y + smax(0.0, (style.line_height - continuation_size.height) / 2.0),
                ));
                continuation_y += style.line_height;
            }
        }
    }

    fn active_heading_index(&self) -> Option<usize> {
        let text_view = self.text_view()?;
        let caret = text_view.primary_source_caret()?;
        text_view.parsed_document().headings.iter().position(|heading| heading.range.contains(caret))
    }

    fn row_rect(&self, range: NSRange, text_view: &MarkdownTextView) -> Option<NSRect> {
        let found = text_view.rect_for_offset(range.location)?;
        let converted = self.convertRect_fromView(found, Some(text_view));
        Some(rect(0.0, converted.min_y(), self.bounds().size.width, smax(converted.height(), 1.0)))
    }

    fn heading_chip_title(&self, heading: &HeadingNode, highlighted: bool) -> Retained<NSAttributedString> {
        let font = NSFont::systemFontOfSize_weight(10.5, unsafe { NSFontWeightMedium });
        let text_view = self.text_view();
        let color: Retained<NSColor> = if highlighted {
            text_view.as_ref().map_or_else(NSColor::controlAccentColor, |view| view.style_sheet().accent.clone())
        } else {
            text_view.as_ref().map_or_else(NSColor::secondaryLabelColor, |view| view.style_sheet().text_faint.clone())
        };
        attributed_string(
            &format!("H{}", heading.level),
            &[(keys::font(), &*font as &AnyObject), (keys::foreground_color(), &*color)],
        )
    }

    fn heading_chip_rect(&self, index: usize, row: NSRect, text_view: &MarkdownTextView) -> NSRect {
        let document = text_view.parsed_document();
        let title = self.heading_chip_title(&document.headings[index], false);
        let size = title.size();
        let bounds = self.bounds();
        rect(
            bounds.max_x() - Self::MARKER_INSET - size.width - 10.0,
            row.min_y() + smax(0.0, (text_view.style_sheet().line_height - 18.0) / 2.0),
            size.width + 10.0,
            18.0,
        )
    }

    /// Return only the heading whose visible chip contains `point`.
    pub fn heading_index_at(&self, point: NSPoint) -> Option<usize> {
        let text_view = self.text_view()?;
        if text_view.mode() == RenderMode::Source {
            return None;
        }
        let index = text_view.hovered_heading_index().or_else(|| self.active_heading_index())?;
        let document = text_view.parsed_document();
        if index >= document.headings.len() {
            return None;
        }
        let row = self.row_rect(document.headings[index].range, &text_view)?;
        let chip = self.heading_chip_rect(index, row, &text_view);
        if chip.intersects(self.bounds()) && chip.contains_point(point) { Some(index) } else { None }
    }

    // MARK: - Pointer handoff

    fn refresh_tracking_areas(&self) {
        if let Some(area) = self.ivars().tracking_area.borrow_mut().take() {
            self.removeTrackingArea(&area);
        }
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                self.bounds(),
                options,
                Some(self),
                None,
            )
        };
        self.addTrackingArea(&area);
        *self.ivars().tracking_area.borrow_mut() = Some(area);
    }

    fn update_heading_hover(&self, point: NSPoint) {
        let Some(text_view) = self.text_view() else { return };
        let index = self.heading_index_at(point);
        if text_view.hovered_heading_index() == index {
            return;
        }
        text_view.set_hovered_heading_index(index);
        self.setNeedsDisplay(true);
        text_view.setNeedsDisplay(true);
    }

    /// The full vertical span of `range`: the first line's top to the last
    /// line's bottom.
    fn block_rect(&self, range: NSRange, text_view: &MarkdownTextView) -> Option<NSRect> {
        let start = text_view.rect_for_offset(range.location)?;
        let last = text_view.rect_for_offset(range.location.max(range.upper_bound() - 1)).unwrap_or(start);
        let top = self.convertRect_fromView(start, Some(text_view));
        let bottom = self.convertRect_fromView(last, Some(text_view));
        let min_y = smin(top.min_y(), bottom.min_y());
        let max_y = smax(top.max_y(), bottom.max_y());
        Some(rect(0.0, min_y, self.bounds().size.width, smax(1.0, max_y - min_y)))
    }

    // MARK: - Clicking (§7.1)

    fn handle_mouse_down(&self, event: &NSEvent) {
        let Some(text_view) = self.text_view() else { return };
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let modifiers = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        if !modifiers.is_empty() {
            return;
        }
        let Some(index) = self.heading_index_at(point) else { return };
        let document = text_view.parsed_document();
        if index >= document.headings.len() {
            return;
        }
        let Some(row) = self.row_rect(document.headings[index].range, &text_view) else { return };
        if !self.heading_chip_rect(index, row, &text_view).contains_point(point) {
            return;
        }
        self.present_heading_menu(index, Some(point));
    }

    fn present_heading_menu(&self, index: usize, requested_point: Option<NSPoint>) -> bool {
        let Some(text_view) = self.text_view() else { return false };
        let document = text_view.parsed_document();
        if index >= document.headings.len() {
            return false;
        }
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(&*text_view);
        let action = HeadingMenuAction::new(
            Box::new(move |level| {
                let Some(text_view) = weak.load() else { return };
                if let Some(delegate) = text_view.markdown_delegate() {
                    delegate.did_request_heading_level(&text_view, level, index);
                }
            }),
            self.mtm(),
        );
        *self.ivars().heading_menu_action.borrow_mut() = Some(action.clone());
        let menu = NSMenu::initWithTitle(NSMenu::alloc(self.mtm()), &NSString::from_str("Heading Level"));
        for level in 1..=6isize {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(self.mtm()),
                    &NSString::from_str(&format!("Heading {level}")),
                    Some(sel!(choose:)),
                    &NSString::from_str(""),
                )
            };
            unsafe { item.setTarget(Some(&action)) };
            item.setTag(level);
            item.setState(if level == document.headings[index].level {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            menu.addItem(&item);
        }
        menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
        let body = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                &NSString::from_str("Body Text"),
                Some(sel!(chooseBody:)),
                &NSString::from_str(""),
            )
        };
        unsafe { body.setTarget(Some(&action)) };
        menu.addItem(&body);
        let bounds = self.bounds();
        let point = if let Some(point) = requested_point {
            point
        } else if let Some(row) = self.row_rect(document.headings[index].range, &text_view) {
            let chip = self.heading_chip_rect(index, row, &text_view);
            NSPoint::new(chip.mid_x(), smin(smax(bounds.min_y() + 8.0, chip.max_y()), bounds.max_y() - 8.0))
        } else {
            NSPoint::new(bounds.mid_x(), bounds.mid_y())
        };
        menu.popUpMenuPositioningItem_atLocation_inView(None, point, Some(self));
        true
    }
}

fn current_heading_index(text_view: &MarkdownTextView) -> Option<usize> {
    let offset = text_view.primary_source_caret().unwrap_or_else(|| text_view.top_visible_offset());
    let document = text_view.parsed_document();
    let index = text_view
        .hovered_heading_index()
        .or_else(|| document.headings.iter().rposition(|heading| heading.range.location <= offset))?;
    (index < document.headings.len()).then_some(index)
}

pub struct HeadingMenuActionIvars {
    handler: Box<dyn Fn(Option<isize>)>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements. No Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "HeadingMenuAction"]
    #[ivars = HeadingMenuActionIvars]
    pub struct HeadingMenuAction;

    unsafe impl NSObjectProtocol for HeadingMenuAction {}

    impl HeadingMenuAction {
        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSMenuItem) {
            (self.ivars().handler)(Some(sender.tag()));
        }

        #[unsafe(method(chooseBody:))]
        fn choose_body(&self, _sender: &NSMenuItem) {
            (self.ivars().handler)(None);
        }
    }
);

impl HeadingMenuAction {
    fn new(handler: Box<dyn Fn(Option<isize>)>, mtm: MainThreadMarker) -> Retained<HeadingMenuAction> {
        let this = Self::alloc(mtm).set_ivars(HeadingMenuActionIvars { handler });
        unsafe { msg_send![super(this), init] }
    }
}

#[allow(dead_code)]
fn _unused(_: NSSize, _: fn(objc2_foundation::NSRange) -> NSRange) {
    let _ = from_ns;
}
