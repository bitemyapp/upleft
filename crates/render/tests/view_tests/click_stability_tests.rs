//! Port of `ClickStabilityTests.swift`: clicking must never move the
//! document under the pointer, and a double click on a checkbox toggles it
//! once.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::NSTextStorage;
use objc2_foundation::NSPoint;
use upleft_core::NSRange;
use upleft_render::appkit_compat::{enumerate_attribute, RectExt};
use upleft_render::render_contracts::{RenderMode, attribute_keys};
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("click_keeps_the_clicked_line_still", click_keeps_the_clicked_line_still),
    ("click_does_not_unhide_distant_markers", click_does_not_unhide_distant_markers),
    ("reveal_still_round_trips", reveal_still_round_trips),
    ("click_does_not_move_the_viewport", click_does_not_move_the_viewport),
    ("point_hit_testing_uses_text_kit_two_geometry", point_hit_testing_uses_text_kit_two_geometry),
    ("point_hit_testing_resolves_rendered_link", point_hit_testing_resolves_rendered_link),
    ("whitespace_after_link_is_not_interactive", whitespace_after_link_is_not_interactive),
    ("checkbox_double_click_does_not_toggle_twice", checkbox_double_click_does_not_toggle_twice),
];

fn document() -> String {
    let mut lines: Vec<String> = vec!["# Title".into(), String::new()];
    for index in 0..60 {
        lines.push(format!(
            "Paragraph {index} carries **strong emphasis** and _light emphasis_ plus `inline code` and a [link](https://example.com/some/long/path) so the line sits close to the wrap boundary when markers reveal."
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn harness(mtm: MainThreadMarker) -> (Retained<MarkdownContainerView>, Retained<MarkdownTextView>, String, Retained<NSTextStorage>) {
    let text = document();
    let storage = text_storage(&text);
    let container = MarkdownContainerView::new(&storage, fallback_sheet(), mtm);
    container.setFrame(rect(0.0, 0.0, 900.0, 600.0));
    container.layoutSubtreeIfNeeded();
    let view = container.text_view().clone();
    view.set_mode(RenderMode::Live);
    view.update(parse(&text), &wholesale(), true);
    view.resize_to_fit_content();
    container.layoutSubtreeIfNeeded();
    (container, view, text, storage)
}

/// What `mouseDown` does once AppKit resolved the gesture into a caret.
fn click(view: &MarkdownTextView, offset: isize) {
    view.set_source_selected_ranges(&[NSRange::new(offset, 0)]);
    view.handle_selection_changed_for_testing(false);
}

fn scroll_clip(container: &MarkdownContainerView, y: f64) {
    let clip = container.scroll_view().contentView();
    clip.scrollToPoint(NSPoint::new(clip.bounds().origin.x, y));
    container.scroll_view().reflectScrolledClipView(&clip);
}

fn click_keeps_the_clicked_line_still(mtm: MainThreadMarker) {
    let (container, view, text, _storage) = harness(mtm);
    let clip = container.scroll_view().contentView();
    let first = range_of(&text, "Paragraph 2 carries").location;
    click(&view, first + 4);
    scroll_clip(&container, 1200.0);

    let target = range_of(&text, "Paragraph 30 carries").location + 4;
    let before = view.rect_for_offset(target).expect("rect before");
    let screen_y_before = before.min_y() - clip.bounds().origin.y;
    click(&view, target);
    let after = view.rect_for_offset(target).expect("rect after");
    let screen_y_after = after.min_y() - clip.bounds().origin.y;
    expect!(
        (screen_y_after - screen_y_before).abs() < 1.0,
        "the clicked line moved {}pt on screen",
        screen_y_after - screen_y_before
    );
}

fn hidden_characters(storage: &NSTextStorage) -> isize {
    let mut total = 0isize;
    enumerate_attribute(
        storage,
        attribute_keys::dr_hidden(),
        objc2_foundation::NSRange::new(0, storage.length()),
        false,
        |value, range| {
            if value.is_some() {
                total += range.length as isize;
            }
            true
        },
    );
    total
}

fn click_does_not_unhide_distant_markers(mtm: MainThreadMarker) {
    let (_container, view, text, storage) = harness(mtm);
    let baseline = hidden_characters(&storage);
    expect!(baseline > 0, "nothing was hidden to begin with");
    click(&view, range_of(&text, "Paragraph 2 carries").location + 4);
    click(&view, range_of(&text, "Paragraph 30 carries").location + 4);
    let after = hidden_characters(&storage);
    expect!(after == baseline, "a click unhid {} characters it never touched", baseline - after);
}

fn reveal_still_round_trips(mtm: MainThreadMarker) {
    let (_container, view, text, storage) = harness(mtm);
    let paragraph = range_of(&text, "Paragraph 30 carries");
    let rest = NSRange::new(paragraph.location, utf16_len(&text) - paragraph.location);
    let marker = range_of_in(&text, "**", rest);
    let bold = range_of_in(&text, "strong emphasis", rest);
    let marker_is_hidden = || {
        upleft_render::appkit_compat::attribute_value(&storage, attribute_keys::dr_hidden(), marker.location as usize).is_some()
    };
    let marker_is_laid_out = || {
        view.current_display_map()
            .substitutions()
            .iter()
            .any(|sub| sub.source_range.location == marker.location && sub.is_hidden)
    };
    expect!(marker_is_hidden(), "the marker should start hidden");
    expect!(marker_is_laid_out(), "the marker should start out of the layout");
    click(&view, bold.location + 2);
    expect!(!marker_is_hidden(), "the marker under the caret did not reveal");
    expect!(!marker_is_laid_out(), "the marker under the caret did not reveal on screen");
    click(&view, range_of(&text, "Paragraph 2 carries").location + 4);
    expect!(marker_is_laid_out(), "the marker stayed on screen after the caret left");
    expect!(marker_is_hidden(), "the marker did not re-hide when the caret left");
}

fn click_does_not_move_the_viewport(mtm: MainThreadMarker) {
    let (container, view, text, _storage) = harness(mtm);
    let clip = container.scroll_view().contentView();
    click(&view, range_of(&text, "Paragraph 2 carries").location + 4);
    scroll_clip(&container, 1200.0);
    let y_before = clip.bounds().origin.y;
    click(&view, range_of(&text, "Paragraph 30 carries").location + 4);
    expect!((clip.bounds().origin.y - y_before).abs() < 1.0, "the click scrolled the viewport");
}

fn point_hit_testing_uses_text_kit_two_geometry(mtm: MainThreadMarker) {
    let (_container, view, text, _storage) = harness(mtm);
    let target = range_of(&text, "Paragraph 30 carries").location + 5;
    let line = view.rect_for_offset(target).expect("rect");
    let text_kit_offset: usize = unsafe {
        objc2::msg_send![&*view, characterIndexForInsertionAtPoint: NSPoint::new(line.min_x() + 2.0, line.mid_y())]
    };
    let source_offset = view.current_display_map().source_offset_for_text_kit(text_kit_offset as isize);
    expect!(
        (source_offset - target).abs() <= 8,
        "point resolved to source offset {source_offset}, expected near {target}"
    );
}

fn live_container(text: &str, mtm: MainThreadMarker) -> (Retained<MarkdownContainerView>, Retained<MarkdownTextView>) {
    let storage = text_storage(text);
    let container = MarkdownContainerView::with_storage(&storage, mtm);
    container.setFrame(rect(0.0, 0.0, 900.0, 600.0));
    container.layoutSubtreeIfNeeded();
    let view = container.text_view().clone();
    view.set_mode(RenderMode::Live);
    view.update(parse(text), &wholesale(), true);
    view.resize_to_fit_content();
    container.layoutSubtreeIfNeeded();
    (container, view)
}

fn point_hit_testing_resolves_rendered_link(mtm: MainThreadMarker) {
    let text = "# Link\n\n[Jump to target](#target)\n\n## Target\n\nReached.";
    let (_container, view) = live_container(text, mtm);
    let link = range_of(text, "Jump to target");
    let start = view.rect_for_offset(link.location).expect("start");
    let end = view.rect_for_offset(link.upper_bound()).expect("end");
    let point = NSPoint::new((start.min_x() + end.min_x()) * 0.5, start.mid_y());
    let hit = view.source_offset_at(point);
    expect!(
        hit >= link.location && hit < link.upper_bound(),
        "rendered link point resolved to source offset {hit}, expected {link:?}"
    );
    expect!(view.attribute_at_point(attribute_keys::dr_link(), point).is_some());
}

fn whitespace_after_link_is_not_interactive(mtm: MainThreadMarker) {
    let text = "[Jump](https://example.com) after";
    let (_container, view) = live_container(text, mtm);
    let link = range_of(text, "Jump");
    let gap = view.rect_for_offset(link.upper_bound()).expect("gap");
    let point = NSPoint::new(gap.min_x() + (gap.width() * 0.5).max(1.0), gap.mid_y());
    expect!(view.attribute_at_point(attribute_keys::dr_link(), point).is_none());
}

struct ToggleDelegate {
    toggled_offsets: std::cell::RefCell<Vec<isize>>,
}

impl upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate for ToggleDelegate {
    fn did_toggle_checkbox_at_mark_offset(&self, _view: &MarkdownTextView, offset: isize) {
        self.toggled_offsets.borrow_mut().push(offset);
    }
}

fn checkbox_double_click_does_not_toggle_twice(mtm: MainThreadMarker) {
    use objc2_app_kit::{NSBackingStoreType, NSEvent, NSEventModifierFlags, NSEventType, NSWindow, NSWindowStyleMask};
    use objc2::MainThreadOnly;

    let text = "- [ ] First task\n- [x] Second task\n";
    let storage = text_storage(text);
    let container = MarkdownContainerView::with_storage(&storage, mtm);
    container.setFrame(rect(0.0, 0.0, 900.0, 600.0));
    container.layoutSubtreeIfNeeded();
    let view = container.text_view().clone();
    view.set_mode(RenderMode::Live);
    view.update(parse(text), &wholesale(), true);
    view.resize_to_fit_content();
    container.layoutSubtreeIfNeeded();

    let delegate: std::rc::Rc<ToggleDelegate> = std::rc::Rc::new(ToggleDelegate { toggled_offsets: Default::default() });
    let as_dyn: std::rc::Rc<dyn upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate> = delegate.clone();
    view.set_markdown_delegate(Some(std::rc::Rc::downgrade(&as_dyn)));
    let task = view.parsed_document().tasks.first().cloned().expect("a task");
    let text_rect = view.rect_for_offset(task.content_range.location).expect("task rect");
    let centre_y = text_rect.min_y() + view.style_sheet().line_height.min(text_rect.height()) * 0.44;
    let target = upleft_render::fragments::list_ornament_fragment::task_hit_rect(
        text_rect.min_x(),
        centre_y,
        view.style_sheet().body_font().pointSize(),
    );

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(0.0, 0.0, 900.0, 600.0),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(&container));
    window.makeKeyAndOrderFront(None);
    let location = view.convertPoint_toView(NSPoint::new(target.mid_x(), target.mid_y()), None);

    for click_count in [1isize, 2] {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            NSEventType::LeftMouseDown,
            location,
            NSEventModifierFlags::empty(),
            objc2_foundation::NSProcessInfo::processInfo().systemUptime(),
            window.windowNumber(),
            None,
            click_count,
            click_count,
            1.0,
        )
        .expect("mouse event");
        view.mouseDown(&event);
    }
    window.orderOut(None);

    let toggled = delegate.toggled_offsets.borrow().clone();
    expect!(toggled == vec![task.mark_range.location], "toggled {toggled:?}, expected [{}]", task.mark_range.location);
}
