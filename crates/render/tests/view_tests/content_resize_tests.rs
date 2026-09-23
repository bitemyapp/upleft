//! Port of `ContentResizeTests.swift`.

#![allow(clippy::explicit_counter_loop)]

use objc2::MainThreadMarker;
use objc2_app_kit::{NSTextLayoutFragment, NSTextLayoutFragmentEnumerationOptions, NSTextSelectionDataSource};
use objc2_foundation::{NSPoint, NSSize, NSString};
use upleft_core::{NSRange, TextEdit};
use upleft_render::appkit_compat::RectExt;
use upleft_render::render_contracts::RenderMode;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::{ContentResizePolicy, ContentResizeRequest};

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("resize_request_merging", request_merging),
    ("resize_semantic_update_is_deferred", semantic_update_is_deferred),
    ("resize_line_count_update_is_deferred", line_count_update_is_deferred),
    ("resize_semantic_repair_stays_content_anchored_at_the_bottom", semantic_repair_stays_content_anchored_at_the_bottom),
    ("resize_local_typing_keeps_visible_content_stable", local_typing_keeps_visible_content_stable),
    ("resize_repeated_typing_does_not_stack_stale_fragments", repeated_typing_does_not_stack_stale_fragments),
    ("resize_non_local_reparse_keeps_the_pixel_viewport", non_local_reparse_keeps_the_pixel_viewport),
    ("resize_shared_storage_edit_keeps_the_pixel_viewport", shared_storage_edit_keeps_the_pixel_viewport),
    ("resize_local_edit_keeps_the_pixel_viewport_after_caret_repair", local_edit_keeps_the_pixel_viewport_after_caret_repair),
    ("resize_undo_camera_repair_runs_after_the_command", undo_camera_repair_runs_after_the_command),
    ("resize_shrink_is_deferred_at_the_bottom", shrink_is_deferred_at_the_bottom),
    ("resize_mode_switch_uses_deferred_viewport_path", mode_switch_uses_deferred_viewport_path),
    ("resize_mode_switch_preserves_deep_viewport_anchor", mode_switch_preserves_deep_viewport_anchor),
];

fn replace_all(storage: &objc2_app_kit::NSTextStorage, text: &str) {
    storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(0, storage.length()), &NSString::from_str(text));
}

fn request_merging(_mtm: MainThreadMarker) {
    use ContentResizeRequest::*;
    expect!(ContentResizePolicy::merge(None, Semantic) == Semantic);
    expect!(ContentResizePolicy::merge(Some(Semantic), LineCount) == LineCount);
    expect!(ContentResizePolicy::merge(Some(LineCount), ScrollRepair) == LineCount);
    expect!(ContentResizePolicy::merge(Some(Semantic), Viewport) == Viewport);
    expect!(ContentResizePolicy::merge(Some(Viewport), Immediate) == Immediate);
    expect!(ContentResizePolicy::idle_delay(Semantic) > 0.0);
    expect!(ContentResizePolicy::idle_delay(LineCount) > 0.0);
    expect!(ContentResizePolicy::idle_delay(Viewport) == 0.0);
}

fn semantic_update_is_deferred(mtm: MainThreadMarker) {
    let initial = "# Heading\n\nA short paragraph.";
    let (view, storage) = view_with(initial, rect(0.0, 0.0, 640.0, 400.0), mtm);
    view.update(parse(initial), &wholesale(), true);
    expect!(view.pending_resize_request_for_testing().is_none());

    let changed = "# Heading\n\nA longer paragraph with one more word.";
    replace_all(&storage, changed);
    view.update(parse(changed), &dirty(vec![NSRange::new(0, utf16_len(changed))]), true);
    expect!(view.pending_resize_request_for_testing() == Some(ContentResizeRequest::Semantic));
    expect!(pump(|| view.pending_resize_request_for_testing().is_none()), "still pending: {:?}", view.pending_resize_request_for_testing());
}

fn line_count_update_is_deferred(mtm: MainThreadMarker) {
    let initial = "one line";
    let (view, storage) = view_with(initial, rect(0.0, 0.0, 640.0, 400.0), mtm);
    view.update(parse(initial), &wholesale(), true);
    let changed = "one line\ntwo lines";
    replace_all(&storage, changed);
    view.update(parse(changed), &dirty(vec![NSRange::new(0, utf16_len(changed))]), true);
    expect!(view.pending_resize_request_for_testing() == Some(ContentResizeRequest::LineCount));
}

fn scroll_clip(container: &MarkdownContainerView, y: f64) {
    let clip = container.scroll_view().contentView();
    clip.scrollToPoint(NSPoint::new(0.0, y));
    container.scroll_view().reflectScrolledClipView(&clip);
}

fn semantic_repair_stays_content_anchored_at_the_bottom(mtm: MainThreadMarker) {
    let text = "# Heading\n\nA short document.\n\nEnd.";
    let (container, _storage) = container(text, 900.0, 700.0, mtm);
    let view = container.text_view();
    let settled_height = view.frame().size.height;
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, (settled_height - clip.bounds().size.height).max(0.0));
    for _ in 0..2 {
        view.update(parse(text), &dirty(vec![NSRange::new(utf16_len(text) - 1, 1)]), true);
        expect!(pump(|| view.pending_resize_request_for_testing().is_none()), "still pending: {:?}", view.pending_resize_request_for_testing());
    }
    expect!(view.frame().size.height <= settled_height + 0.5);
}

fn sections(count: usize, paragraph: &str) -> String {
    (0..count).map(|index| format!("## Section {index}\n\n{paragraph}")).collect::<Vec<_>>().join("\n\n")
}

fn local_typing_keeps_visible_content_stable(mtm: MainThreadMarker) {
    let text = sections(80, "A paragraph with enough words to form a stable line of document text.");
    let (container, storage) = container(&text, 900.0, 420.0, mtm);
    let view = container.text_view();
    expect!(view.cached_layout_element_count_for_testing() > 0);
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, 700.0);
    let anchor = view.top_visible_offset();
    let mut edit_offset = anchor + 8;
    let anchor_screen_y = || view.rect_for_offset(anchor).expect("anchor rect").min_y() - clip.bounds().origin.y;
    let before = anchor_screen_y();
    for character in "stable typing".chars() {
        expect!(view.perform_source_edit(NSRange::new(edit_offset, 0), &character.to_string(), "Edit"));
        expect!(view.cached_layout_element_count_for_testing() > 0, "typing discarded every resolved layout element");
        expect!((anchor_screen_y() - before).abs() < 0.5);
        view.update(parse(&storage_string(&storage)), &dirty(vec![NSRange::new(edit_offset, 1)]), true);
        expect!((anchor_screen_y() - before).abs() < 0.5);
        edit_offset += 1;
    }
    let cleared = pump(|| view.pending_resize_request_for_testing().is_none());
    expect!(cleared, "still pending: {:?}", view.pending_resize_request_for_testing());
    expect!((anchor_screen_y() - before).abs() < 0.5);
}

fn repeated_typing_does_not_stack_stale_fragments(mtm: MainThreadMarker) {
    let text = sections(80, "A paragraph with enough words to form several stable lines of rendered text.");
    let (container, storage) = container(&text, 900.0, 420.0, mtm);
    let view = container.text_view();
    scroll_clip(&container, 700.0);
    let mut edit_offset = view.top_visible_offset() + 8;
    for character in "stacked fragments".chars() {
        let expected_invalidation_start = view.paragraph_range_containing(edit_offset).location;
        expect!(view.perform_source_edit(NSRange::new(edit_offset, 0), &character.to_string(), "Edit"));
        let invalidated = view
            .last_fragment_invalidation_range_for_testing()
            .flatten()
            .expect("an invalidated range");
        expect!(invalidated.location == expected_invalidation_start);
        expect!(invalidated.upper_bound() == storage.length() as isize);
        view.update(parse(&storage_string(&storage)), &dirty(vec![NSRange::new(edit_offset, 1)]), true);
        edit_offset += 1;
    }

    let layout = view.textLayoutManager().expect("layout manager");
    layout.ensureLayoutForRange(&layout.documentRange());
    let previous_max_y = std::cell::Cell::new(-f64::MAX);
    let offsets = std::cell::RefCell::new(Vec::<isize>::new());
    let failures = std::cell::RefCell::new(Vec::<isize>::new());
    let start = layout.documentRange().location();
    let block = block2::StackBlock::new(|fragment: std::ptr::NonNull<NSTextLayoutFragment>| -> objc2::runtime::Bool {
        let fragment = unsafe { fragment.as_ref() };
        let offset = layout.offsetFromLocation_toLocation(&start, &fragment.rangeInElement().location());
        offsets.borrow_mut().push(offset);
        let frame = fragment.layoutFragmentFrame();
        if frame.min_y() + 0.5 < previous_max_y.get() {
            failures.borrow_mut().push(offset);
        }
        previous_max_y.set(frame.max_y());
        objc2::runtime::Bool::YES
    });
    layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
        Some(&start),
        NSTextLayoutFragmentEnumerationOptions(0),
        &block,
    );
    expect!(failures.borrow().is_empty(), "layout fragments overlap or move backwards at {:?}", failures.borrow());
    let offsets = offsets.into_inner();
    let mut sorted = offsets.clone();
    sorted.sort();
    expect!(offsets == sorted);
    let unique: std::collections::HashSet<isize> = offsets.iter().copied().collect();
    expect!(unique.len() == offsets.len());
}

fn non_local_reparse_keeps_the_pixel_viewport(mtm: MainThreadMarker) {
    let text = sections(80, "A paragraph with enough words to form a stable line of document text.");
    let (container, _storage) = container(&text, 900.0, 420.0, mtm);
    let view = container.text_view();
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, 703.0);
    let anchor = view.top_visible_offset();
    let screen_y = || view.rect_for_offset(anchor).expect("anchor rect").min_y() - clip.bounds().origin.y;
    let before = screen_y();
    view.update(parse(&text), &dirty(vec![NSRange::new(40, 1)]), true);
    expect!((screen_y() - before).abs() < 0.5, "the commit moved the page");
    expect!(pump(|| view.pending_resize_request_for_testing().is_none()), "still pending: {:?}", view.pending_resize_request_for_testing());
    expect!((screen_y() - before).abs() < 0.5, "the deferred resize moved the page");
}

fn shared_storage_edit_keeps_the_pixel_viewport(mtm: MainThreadMarker) {
    let text = sections(80, "A paragraph with enough words to form a stable line of document text.");
    let (container, storage) = container(&text, 900.0, 420.0, mtm);
    let view = container.text_view();
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, 703.0);
    let anchor = view.top_visible_offset();
    let edit_offset = (storage.length() as isize).min(anchor + 4);
    let screen_y = || view.rect_for_offset(anchor).expect("anchor rect").min_y() - clip.bounds().origin.y;
    let before = screen_y();
    let edit = TextEdit::new(NSRange::new(edit_offset, 0), "x", "External edit", None);
    view.prepare_for_external_document_edits(std::slice::from_ref(&edit));
    storage.replaceCharactersInRange_withString(
        objc2_foundation::NSRange::new(edit.range.location as usize, edit.range.length as usize),
        &NSString::from_str(&edit.replacement),
    );
    view.update(parse(&storage_string(&storage)), &dirty(vec![NSRange::new(edit_offset, 1)]), true);
    expect!((screen_y() - before).abs() < 0.5, "the shared-storage edit moved the page");
    expect!(pump(|| view.pending_resize_request_for_testing().is_none()), "still pending: {:?}", view.pending_resize_request_for_testing());
    expect!((screen_y() - before).abs() < 0.5, "the deferred shared-storage repair moved the page");
}

fn local_edit_keeps_the_pixel_viewport_after_caret_repair(mtm: MainThreadMarker) {
    let text = sections(80, "A paragraph with enough words to form a stable line of document text.");
    let (container, storage) = container(&text, 900.0, 420.0, mtm);
    let view = container.text_view();
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, 703.0);
    let anchor = view.top_visible_offset();
    let before = view.rect_for_offset(anchor).expect("anchor").min_y() - clip.bounds().origin.y;
    let edit_offset = (storage.length() as isize).min(anchor + 4);
    expect!(view.perform_source_edit(NSRange::new(edit_offset, 0), "x", "Edit"));
    view.update(parse(&storage_string(&storage)), &dirty(vec![NSRange::new(edit_offset, 1)]), true);
    // Simulate NSTextView's late make-caret-visible correction.
    scroll_clip(&container, 0.0);
    expect!(pump(|| {
        let Some(rect) = view.rect_for_offset(anchor) else { return false };
        (rect.min_y() - clip.bounds().origin.y - before).abs() < 0.5
    }));
    let after = view.rect_for_offset(anchor).expect("anchor").min_y() - clip.bounds().origin.y;
    expect!((after - before).abs() < 0.5, "the deferred caret repair moved the page");
}

fn undo_camera_repair_runs_after_the_command(mtm: MainThreadMarker) {
    let text = (0..80).map(|index| format!("## Section {index}\n\nParagraph {index}")).collect::<Vec<_>>().join("\n\n");
    let (container, storage) = container(&text, 900.0, 420.0, mtm);
    let view = container.text_view();
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, 703.0);
    view.preserve_viewport_across_undo_redo();
    storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(16, 1), &NSString::from_str("x"));
    view.update(parse(&storage_string(&storage)), &dirty(vec![NSRange::new(16, 1)]), true);
    scroll_clip(&container, 0.0);
    expect!(pump(|| (clip.bounds().origin.y - 703.0).abs() < 0.5));
    expect!((clip.bounds().origin.y - 703.0).abs() < 0.5);
    expect!(pump(|| (clip.bounds().origin.y - 703.0).abs() < 0.5));
}

fn shrink_is_deferred_at_the_bottom(mtm: MainThreadMarker) {
    let text = "# Heading\n\nShort.";
    let (container, _storage) = container(text, 900.0, 700.0, mtm);
    let view = container.text_view();
    let true_height = view.frame().size.height;
    view.setFrameSize(NSSize::new(view.frame().size.width, true_height + 400.0));
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, view.frame().size.height - clip.bounds().size.height);
    view.resize_to_fit_content();
    expect!(view.frame().size.height > true_height + 200.0, "shrank while the viewport was pinned to the bottom");
    scroll_clip(&container, 0.0);
    view.resize_to_fit_content();
    expect!((view.frame().size.height - true_height).abs() < 1.0);
}

fn mode_switch_uses_deferred_viewport_path(mtm: MainThreadMarker) {
    let source = "# Heading\n\nText";
    let (view, _storage) = view_with(source, rect(0.0, 0.0, 640.0, 400.0), mtm);
    view.update(parse(source), &wholesale(), true);
    view.set_mode(RenderMode::Source);
    expect!(view.pending_resize_request_for_testing() == Some(ContentResizeRequest::Viewport));
}

fn mode_switch_preserves_deep_viewport_anchor(mtm: MainThreadMarker) {
    let source = sections(70, "A paragraph with enough words to keep the source and rendered views tall.");
    let (container, _storage) = container(&source, 900.0, 420.0, mtm);
    let view = container.text_view();
    let clip = container.scroll_view().contentView();
    scroll_clip(&container, 900.0);
    let anchor = view.top_visible_offset();
    let before = view.rect_for_offset(anchor).expect("anchor").min_y() - clip.bounds().origin.y;

    view.set_mode(RenderMode::Source);
    expect!(pump(|| {
        let Some(rect) = view.rect_for_offset(anchor) else { return false };
        (rect.min_y() - clip.bounds().origin.y - before).abs() < 1.0
    }));
    let source_after = view.rect_for_offset(anchor).expect("anchor").min_y() - clip.bounds().origin.y;
    expect!((source_after - before).abs() < 1.0, "Source mode moved the reading position");

    view.set_mode(RenderMode::Live);
    expect!(pump(|| {
        let Some(rect) = view.rect_for_offset(anchor) else { return false };
        (rect.min_y() - clip.bounds().origin.y - before).abs() < 2.0
    }));
    let document_after = view.rect_for_offset(anchor).expect("anchor").min_y() - clip.bounds().origin.y;
    expect!((document_after - before).abs() < 2.0, "Document mode moved the reading position");
}

#[allow(dead_code)]
fn _trait(_: &dyn NSTextSelectionDataSource) {}
