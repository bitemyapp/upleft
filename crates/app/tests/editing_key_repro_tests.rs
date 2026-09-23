//! Port of `Tests/DownrightAppTests/EditingKeyReproTests.swift`.
//!
//! Every key event these tests send reaches `DocumentWindowController`'s key
//! handler (`+Commands`' `wireKeyEventHandler`) before the text view types
//! it, so the whole suite exercises that layer; Tab reaches `perform(.indentList)`,
//! the heading test `perform(.promoteHeading)`, and the picker test
//! `markdownTextView(_:didRequestHeadingLevel:headingIndex:)`.
//!
//! The Swift suite is `@MainActor` and `.serialized`: this binary owns the
//! main thread (`harness = false`). Adapted, with the reason: Swift orders
//! the titled window in (`showWindow`, `makeKeyAndOrderFront`) before making
//! the text view first responder; AppKit pulls a titled window onto a display
//! once it is ordered in, so here the window stays parked at (-30000, -30000),
//! unordered, and only `makeFirstResponder` runs (see `controller_support`).
//! Every assertion is Swift's.

mod controller_support;

use controller_support::{
    Closing, Removing, make_controller, make_first_responder, mtm, press_command_a, press_delete,
    press_option_delete, press_tab, pump_main_run_loop, range_of, temporary_directory, temporary_file,
    type_character, utf16_length,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSStandardKeyBindingResponding, NSTextInputClient, NSTextLayoutFragmentEnumerationOptions};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSRect, NSSize, NSString};
use upleft_app::ai::document_state_store::DocumentStateStore;
use upleft_app::support::commands::Command;
use upleft_app::support::preferences::Preferences;
use upleft_core::{NSRange, PathToken, ZoomLevel};
use upleft_render::appkit_compat::{RectExt, ns};
use upleft_render::render_contracts::RenderMode;
use upleft_render::render_contracts::attribute_keys::dr_elided;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

fn unique() -> String {
    controller_support::document_support::unique()
}

fn text_view(controller: &Closing) -> Retained<MarkdownTextView> {
    controller.primary_container().text_view().clone()
}

fn set_content_size(controller: &Closing, width: CGFloat, height: CGFloat) {
    if let Some(window) = controller.window() {
        window.setContentSize(NSSize::new(width, height));
        window.layoutIfNeeded();
    }
}

fn layout_if_needed(controller: &Closing) {
    if let Some(window) = controller.window() {
        window.layoutIfNeeded();
    }
}

fn clip_origin_y(controller: &Closing) -> CGFloat {
    controller.primary_container().scroll_view().contentView().bounds().origin.y
}

/// `layout.enumerateTextLayoutFragments(from: documentRange.location,
/// options: [.ensuresLayout])`, collecting every fragment frame.
fn layout_fragment_frames(view: &MarkdownTextView) -> Vec<NSRect> {
    let layout = view.textLayoutManager().expect("a TextKit 2 layout manager");
    let document = layout.documentRange();
    layout.ensureLayoutForRange(&document);
    let frames = std::cell::RefCell::new(Vec::new());
    let block = block2::RcBlock::new(|fragment: std::ptr::NonNull<objc2_app_kit::NSTextLayoutFragment>| {
        // SAFETY: TextKit hands the block a live fragment.
        frames.borrow_mut().push(unsafe { fragment.as_ref() }.layoutFragmentFrame());
        objc2::runtime::Bool::YES
    });
    let _ = layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
        Some(&document.location()),
        NSTextLayoutFragmentEnumerationOptions::EnsuresLayout,
        &block,
    );
    frames.into_inner()
}

fn opening_document_starts_with_caret_only() {
    let url = temporary_file(&format!("EditingKeyReproInitialSelection-{}.md", unique()));
    let _remove = Removing(url.clone());
    let source = "# Title\n\nBody text.\n";
    let controller = Closing(make_controller(source, &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);

    assert!(pump_main_run_loop(|| view.selectedRange().length == 0, 1.0));
    assert_eq!(view.selectedRange().length, 0);
    assert_eq!(view.source_selected_range().length, 0);
}

fn opening_second_document_clears_previous_selection() {
    let first_url = temporary_file(&format!("EditingKeyReproPreviousSelection-{}.md", unique()));
    let second_url = temporary_file(&format!("EditingKeyReproNextSelection-{}.md", unique()));
    let _remove_first = Removing(first_url.clone());
    let _remove_second = Removing(second_url.clone());
    let controller = Closing(make_controller("# First\n\nold body\n", &first_url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    press_command_a(&view);
    assert!(view.selectedRange().length > 0);

    std::fs::write(second_url.path(), "# Second\n\nnew body\n").unwrap();
    controller.open(&second_url, RenderMode::Live).unwrap();

    // The open path clears AppKit's stale native selection synchronously,
    // then restores the new document's saved position on the next frame.
    // Include the persisted state so this cannot pass before that restore.
    assert!(pump_main_run_loop(
        || view.selectedRange().length == 0 && controller.markdown_document().state().selection_length == 0,
        1.0
    ));
    assert_eq!(view.selectedRange().length, 0);
    assert_eq!(view.source_selected_range().length, 0);
}

fn stale_full_document_selection_is_migrated() {
    let url = temporary_file(&format!("EditingKeyReproStaleSelection-{}.md", unique()));
    let _remove = Removing(url.clone());
    let source = "# Existing selection\n\nBody text.\n";
    std::fs::write(url.path(), source).unwrap();

    let mut state = DocumentStateStore::shared().state(&url);
    state.selection_location = 0;
    state.selection_length = utf16_length(source);
    DocumentStateStore::shared().save(&state, &url);

    let controller = Closing(controller_support::new_controller());
    controller.open(&url, RenderMode::Live).unwrap();
    controller_support::park(&controller);
    let view = text_view(&controller);
    make_first_responder(&controller, &view);

    assert!(pump_main_run_loop(
        || view.selectedRange().length == 0 && controller.markdown_document().state().selection_length == 0,
        1.0
    ));
    assert_eq!(view.source_selected_range().length, 0);
}

fn typing_through_real_key_down_mutates_document() {
    let url = temporary_file(&format!("EditingKeyRepro-{}.md", unique()));
    let _remove = Removing(url.clone());
    let controller = Closing(make_controller("# Title\n\nBody text.\n", &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    // Document mode hides the heading marker; a caret at the visible start
    // of `# Title` resolves after `# `, so seed the caret in the body.
    let body = range_of(&controller.markdown_document().text(), "Body text.");
    view.set_source_selected_ranges(&[NSRange::new(body.location, 0)]);
    assert!(pump_main_run_loop(|| view.rect_for_offset(body.location).is_some(), 1.0));

    let before = controller.markdown_document().text();
    type_character('x', &view);
    type_character('y', &view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text().contains("xyBody text."), 1.0));

    assert!(view.isEditable());
    assert_ne!(controller.markdown_document().text(), before);
    assert!(controller.markdown_document().text().contains("xyBody text."));
}

fn native_undo_redo_round_trip_typing() {
    let url = temporary_file(&format!("EditingKeyReproUndoRedo-{}.md", unique()));
    let _remove = Removing(url.clone());
    let source = "# Title\n\nBody text.\n";
    let controller = Closing(make_controller(source, &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    let body = range_of(source, "Body text.");
    view.set_source_selected_ranges(&[NSRange::new(body.location, 0)]);
    assert!(pump_main_run_loop(|| view.rect_for_offset(body.location).is_some(), 1.0));

    type_character('x', &view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text().contains("xBody text."), 1.0));
    let undo_manager = controller.markdown_document().undo_manager();
    assert!(undo_manager.canUndo());

    undo_manager.undo();
    assert!(pump_main_run_loop(|| controller.markdown_document().text() == source, 1.0));
    assert_eq!(controller.markdown_document().text(), source);
    assert!(undo_manager.canRedo());

    undo_manager.redo();
    assert!(pump_main_run_loop(|| controller.markdown_document().text().contains("xBody text."), 1.0));
    assert!(controller.markdown_document().text().contains("xBody text."));
}

fn typing_at_heading_start_extends_visible_title() {
    let url = temporary_file(&format!("EditingKeyReproHeading-{}.md", unique()));
    let _remove = Removing(url.clone());
    let controller = Closing(make_controller("# Title\n\nBody text.\n", &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    view.set_source_selected_ranges(&[NSRange::new(0, 0)]);
    assert!(pump_main_run_loop(|| view.rect_for_offset(0).is_some(), 1.0));

    type_character('x', &view);
    type_character('y', &view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text().starts_with("# xyTitle"), 1.0));

    // Hidden `# ` stays put; typing lands in the visible title.
    assert!(controller.markdown_document().text().starts_with("# xyTitle"));
}

fn select_all_replaces_the_whole_source_document() {
    let url = temporary_file(&format!("EditingKeyReproSelectAll-{}.md", unique()));
    let _remove = Removing(url.clone());
    let source = "## Existing heading\n\nBody.\n";
    let controller = Closing(make_controller(source, &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    press_command_a(&view);

    assert_eq!(view.source_selected_range(), NSRange::new(0, utf16_length(source)));

    for character in "## xyzxyz".chars() {
        type_character(character, &view);
    }
    assert!(pump_main_run_loop(|| controller.markdown_document().text() == "## xyzxyz", 1.0));

    assert_eq!(controller.markdown_document().text(), "## xyzxyz");
    assert_eq!(view.source_selected_range(), NSRange::new(9, 0));
}

fn select_all_survives_fully_elided_projection() {
    let url = temporary_file(&format!("EditingKeyReproElidedSelectAll-{}.md", unique()));
    let _remove = Removing(url.clone());
    let source = "A body with no heading is hidden at top-level zoom.\n";
    let controller = Closing(make_controller(source, &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    // A zero-length caret is intentionally a visibility probe for structural
    // zoom. Use a non-empty selection here so the projection can remain fully
    // elided while this test exercises Select All.
    view.set_source_selected_ranges(&[NSRange::new(0, 1)]);
    view.set_zoom_level(ZoomLevel::H1);
    let storage = view.textStorage().expect("the text storage");
    // SAFETY: index 0 is inside the non-empty storage.
    let elided: Option<Retained<AnyObject>> =
        unsafe { storage.attribute_atIndex_effectiveRange(dr_elided(), 0, std::ptr::null_mut()) };
    assert!(elided.is_some());

    press_command_a(&view);
    assert_eq!(view.source_selected_range(), NSRange::new(0, utf16_length(source)));

    type_character('x', &view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text() == "x", 1.0));
    assert_eq!(controller.markdown_document().text(), "x");
}

fn typing_keeps_caret_line_fixed() {
    let url = temporary_file(&format!("EditingKeyReproCamera-{}.md", unique()));
    let _remove = Removing(url.clone());
    let text = (0..50)
        .map(|index| format!("## Section {index}\n\nParagraph {index} stays on screen while the user types."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    set_content_size(&controller, 900.0, 640.0);
    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    let target = range_of(&text, "Paragraph 25");
    let mut caret = target.upper_bound();
    view.set_source_selected_ranges(&[NSRange::new(caret, 0)]);
    view.resize_to_fit_content();
    view.scroll_to_offset(target.location, ScrollPosition::Center, false);
    let screen_y = view.rect_for_offset(caret).expect("the caret rect").min_y() - clip_origin_y(&controller);

    for character in "abc".chars() {
        type_character(character, &view);
        caret += 1;
        let current_y = view.rect_for_offset(caret).expect("the caret rect").min_y() - clip_origin_y(&controller);
        assert!((current_y - screen_y).abs() < 1.0, "the key event moved the caret line");
    }
    assert!(pump_main_run_loop(|| controller.markdown_document().text().contains("abc"), 1.0));
    layout_if_needed(&controller);
    let settled_y = view.rect_for_offset(caret).expect("the caret rect").min_y() - clip_origin_y(&controller);
    assert!((settled_y - screen_y).abs() < 1.0, "the parse commit moved the caret line");
}

fn delete_through_real_key_down_mutates_document() {
    let url = temporary_file(&format!("EditingKeyReproDel-{}.md", unique()));
    let _remove = Removing(url.clone());
    let controller = Closing(make_controller("# Title\n\nBody text.\n", &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    // Put the caret at the end so delete-backward removes a real character.
    let end = utf16_length(&controller.markdown_document().text());
    view.set_source_selected_ranges(&[NSRange::new(end, 0)]);
    assert!(pump_main_run_loop(|| view.rect_for_offset(end).is_some(), 1.0));

    let before = controller.markdown_document().text();
    press_delete(&view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text() == "# Title\n\nBody text.", 1.0));

    assert!(view.isEditable());
    assert_ne!(controller.markdown_document().text(), before);
    assert_eq!(controller.markdown_document().text(), "# Title\n\nBody text.");
}

fn delete_word_through_real_key_down_mutates_document() {
    let url = temporary_file(&format!("EditingKeyReproWordDel-{}.md", unique()));
    let _remove = Removing(url.clone());
    let text = "alpha beta gamma\n";
    let controller = Closing(make_controller(text, &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    let gamma = range_of(text, "gamma");
    view.set_source_selected_ranges(&[NSRange::new(gamma.upper_bound(), 0)]);
    assert!(pump_main_run_loop(|| view.rect_for_offset(gamma.upper_bound()).is_some(), 1.0));

    press_option_delete(&view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text() == "alpha beta \n", 1.0));

    assert_eq!(controller.markdown_document().text(), "alpha beta \n");
    assert_eq!(view.source_selected_range(), NSRange::new(gamma.location, 0));
}

fn native_word_movement_preserves_source_caret() {
    let url = temporary_file(&format!("EditingKeyReproWordMove-{}.md", unique()));
    let _remove = Removing(url.clone());
    let text = "alpha beta gamma\n";
    let controller = Closing(make_controller(text, &url, RenderMode::Live));

    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    let gamma = range_of(text, "gamma");
    view.set_source_selected_ranges(&[NSRange::new(gamma.upper_bound(), 0)]);
    assert!(pump_main_run_loop(|| view.rect_for_offset(gamma.upper_bound()).is_some(), 1.0));

    view.moveWordBackward(None);

    assert_eq!(view.source_selected_range(), NSRange::new(gamma.location, 0));
}

fn split_panes_keep_interaction_state_independent() {
    let url = temporary_file(&format!("EditingKeyReproSplit-{}.md", unique()));
    let _remove = Removing(url.clone());
    let text = (0..45)
        .map(|index| format!("## Section {index}\n\nParagraph {index} keeps the two editing surfaces readable."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    if let Some(window) = controller.window() {
        window.setContentSize(NSSize::new(1000.0, 640.0));
    }
    controller.toggle_split_view();
    layout_if_needed(&controller);
    let primary = text_view(&controller);
    let split = controller.split_container().expect("the split pane").text_view().clone();
    primary.resize_to_fit_content();
    split.resize_to_fit_content();

    let first_target = range_of(&text, "Section 10");
    let second_target = range_of(&text, "Section 35");
    primary.scroll_to_offset(first_target.location, ScrollPosition::Top, false);
    split.scroll_to_offset(second_target.location, ScrollPosition::Top, false);
    let viewport_y = |view: &MarkdownTextView| {
        view.enclosingScrollView().map_or(0.0, |scroll| scroll.contentView().bounds().origin.y)
    };
    let primary_viewport = viewport_y(&primary);
    let split_selection = NSRange::new(second_target.location + 3, 0);
    split.set_source_selected_ranges(&[split_selection]);

    controller.markdown_text_view_did_change_selection(&split);
    controller.markdown_text_view_did_scroll(&split);

    assert_eq!(split.source_selected_range(), split_selection);
    assert_ne!(primary.source_selected_range(), split_selection);
    assert!((viewport_y(&primary) - primary_viewport).abs() < 1.0);

    make_first_responder(&controller, &split);
    let split_viewport = viewport_y(&split);
    controller.toggle_split_view();

    assert!(controller.split_container().is_none());
    let first_responder = controller.window().and_then(|window| window.firstResponder());
    assert!(first_responder.is_some_and(|responder| {
        std::ptr::eq(Retained::as_ptr(&responder).cast::<u8>(), Retained::as_ptr(&primary).cast::<u8>())
    }));
    assert_eq!(primary.source_selected_range(), split_selection);
    assert!((viewport_y(&primary) - split_viewport).abs() < 1.0);
}

fn opening_split_view_preserves_current_reading_position() {
    let url = temporary_file(&format!("EditingKeyReproSplitOpen-{}.md", unique()));
    let _remove = Removing(url.clone());
    let text = (0..60)
        .map(|index| {
            format!("## Section {index}\n\nParagraph {index} keeps the split opening at the current reading position.")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    set_content_size(&controller, 1000.0, 640.0);
    let primary = text_view(&controller);
    primary.resize_to_fit_content();
    let target = range_of(&text, "Section 40").location;
    primary.scroll_to_offset(target, ScrollPosition::Top, false);
    let expected = primary.top_visible_offset();
    assert!(expected > 0);

    controller.toggle_split_view();
    layout_if_needed(&controller);
    let split = controller.split_container().expect("the split pane").text_view().clone();
    split.resize_to_fit_content();

    // The half-width pane can resolve the same rendered heading to either
    // side of its hidden Markdown marker. Both offsets are the same line.
    assert!((split.top_visible_offset() - expected).abs() < 8);
}

fn enabling_path_resolution_warms_current_document() {
    let root = temporary_directory("EditingKeyReproPathPreference");
    let _remove = Removing(root.clone());
    let url = root.appending_path_component("document.md");
    std::fs::write(root.appending_path_component("generated.md").path(), "ready\n").unwrap();

    let original = Preferences::shared().values().resolve_path_tokens;
    Preferences::shared().update(|values| values.resolve_path_tokens = false);
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            let original = self.0;
            Preferences::shared().update(|values| values.resolve_path_tokens = original);
        }
    }
    let _restore = Restore(original);
    let controller = Closing(make_controller("`generated.md`", &url, RenderMode::Live));
    let token = PathToken::new("generated.md", None, None);
    assert!(controller.path_resolver().and_then(|resolver| resolver.cached_resolution(&token)).is_none());

    Preferences::shared().update(|values| values.resolve_path_tokens = true);
    // The sandboxed `Preferences.shared` does not post its change
    // notification (see `controller_support`); post it as the real one does.
    controller_support::post_preferences_did_change();
    // `await Task.yield()` until the warm lands or two seconds pass.
    let _ = pump_main_run_loop(
        || controller.path_resolver().and_then(|resolver| resolver.cached_resolution(&token)).is_some(),
        2.0,
    );
    assert_eq!(
        controller.path_resolver().and_then(|resolver| resolver.cached_resolution(&token)).map(|resolution| resolution.exists),
        Some(true)
    );
}

fn tab_after_live_edits_keeps_layout_and_viewport_stable() {
    let url = temporary_file(&format!("EditingKeyReproTab-{}.md", unique()));
    let _remove = Removing(url.clone());
    let sections = (0..30)
        .map(|index| format!("## Section {index}\n\nParagraph {index} stays readable with **emphasis** and `code`."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = format!("---\ntitle: Layout fixture\nauthor: Upleft\n---\n\n# Title\n\nFirst sentence.\n\n{sections}\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    set_content_size(&controller, 900.0, 640.0);
    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    let target = range_of(&text, "Paragraph 15 stays readable");
    view.set_source_selected_ranges(&[NSRange::new(target.upper_bound(), 0)]);
    view.resize_to_fit_content();
    view.scroll_to_offset(target.location, ScrollPosition::Center, false);
    let expected_y = clip_origin_y(&controller);
    assert!(expected_y > 100.0);

    type_character('x', &view);
    press_delete(&view);
    view.insertNewline(None);
    type_character('t', &view);
    type_character('a', &view);
    type_character('i', &view);
    type_character('l', &view);
    press_tab(&view);
    assert!(pump_main_run_loop(|| controller.markdown_document().text().contains("\ntail\t"), 1.0));
    layout_if_needed(&controller);
    view.resize_to_fit_content();

    assert!((clip_origin_y(&controller) - expected_y).abs() < 2.0);
    assert!(controller.markdown_document().text().contains("\ntail\t"));
    let frames = layout_fragment_frames(&view);
    for pair in frames.windows(2) {
        assert!(
            pair[1].min_y() + 0.5 >= pair[0].max_y(),
            "layout fragments overlap: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }
}

fn outline_jump_does_not_retain_fragments_from_the_old_camera() {
    let url = temporary_file(&format!("EditingKeyReproOutline-{}.md", unique()));
    let _remove = Removing(url.clone());
    let rows = (0..16).map(|index| format!("| Row {index} | Value {index} |")).collect::<Vec<_>>().join("\n");
    let padding = (0..35)
        .map(|index| format!("## Section {index}\n\nParagraph {index} keeps enough content between navigation targets."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = format!("# Title\n\n{padding}\n\n## Tables\n\n| Name | Value |\n| --- | --- |\n{rows}\n\n## End\n\nDone.\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    set_content_size(&controller, 900.0, 640.0);
    let view = text_view(&controller);
    view.resize_to_fit_content();
    let tables = range_of(&text, "## Tables").location;
    view.scroll_to_offset(tables, ScrollPosition::Top, false);

    let visible = controller.primary_container().scroll_view().contentView().documentVisibleRect();
    let visible_frames: Vec<NSRect> =
        layout_fragment_frames(&view).into_iter().filter(|frame| frame.intersects(visible)).collect();
    assert!(!visible_frames.is_empty());
    for pair in visible_frames.windows(2) {
        assert!(
            pair[1].min_y() + 0.5 >= pair[0].max_y(),
            "destination fragments overlap: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }
    assert!(view.top_visible_offset() >= tables - 4);
}

fn heading_command_keeps_viewport_stable() {
    let url = temporary_file(&format!("EditingKeyReproHeading-{}.md", unique()));
    let _remove = Removing(url.clone());
    let sections = (0..35)
        .map(|index| format!("## Section {index}\n\nParagraph {index} keeps the page tall.\n\n### Detail {index}\n\nBody."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = format!("# Title\n\n{sections}\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    set_content_size(&controller, 900.0, 640.0);
    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    let target = range_of(&text, "### Detail 20");
    view.set_source_selected_ranges(&[NSRange::new(target.location + 5, 0)]);
    view.resize_to_fit_content();
    view.scroll_to_offset(target.location, ScrollPosition::Center, false);
    let expected_screen_y =
        view.rect_for_offset(target.location).expect("the heading rect").min_y() - clip_origin_y(&controller);

    assert!(controller.perform(Command::PromoteHeading));
    assert!(pump_main_run_loop(|| controller.markdown_document().text().contains("## Detail 20"), 1.0));
    layout_if_needed(&controller);

    let actual_screen_y =
        view.rect_for_offset(target.location).expect("the heading rect").min_y() - clip_origin_y(&controller);
    assert!((actual_screen_y - expected_screen_y).abs() < 2.0);
    assert!(controller.markdown_document().text().contains("## Detail 20"));
}

fn heading_picker_keeps_clicked_heading_fixed() {
    let url = temporary_file(&format!("EditingKeyReproHeadingPicker-{}.md", unique()));
    let _remove = Removing(url.clone());
    let sections = (0..35)
        .map(|index| format!("## Section {index}\n\nParagraph {index} keeps the page tall."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = format!("# Title\n\n{sections}\n");
    let controller = Closing(make_controller(&text, &url, RenderMode::Live));

    set_content_size(&controller, 900.0, 640.0);
    let view = text_view(&controller);
    view.resize_to_fit_content();
    let heading_index = controller
        .markdown_document()
        .parsed()
        .headings
        .iter()
        .position(|heading| heading.title == "Section 20")
        .expect("the Section 20 heading");
    let offset = controller.markdown_document().parsed().headings[heading_index].range.location;
    view.scroll_to_offset(offset, ScrollPosition::Center, false);
    let before = view.rect_for_offset(offset).expect("the heading rect").min_y() - clip_origin_y(&controller);

    controller.markdown_text_view_did_request_heading_level(&view, Some(3), heading_index);
    layout_if_needed(&controller);

    assert_eq!(controller.markdown_document().parsed().headings[heading_index].level, 3);
    let after = view.rect_for_offset(offset).expect("the heading rect").min_y() - clip_origin_y(&controller);
    assert!((after - before).abs() < 2.0, "the picker moved its clicked heading");
}

fn marked_text_uses_valid_undo_grouping() {
    let url = temporary_file(&format!("EditingKeyReproIME-{}.md", unique()));
    let _remove = Removing(url.clone());
    let controller = Closing(make_controller("# 入力\n\nBody\n", &url, RenderMode::Live));
    let view = text_view(&controller);
    make_first_responder(&controller, &view);
    view.set_source_selected_ranges(&[NSRange::new(7, 0)]);

    // NSTextView raises an Objective-C exception when its private marked-text
    // undo registration runs without a group. Reaching the assertions is
    // therefore the regression proof for the crash boundary.
    let marked = NSString::from_str("かな");
    let replacement = ns(view.source_selected_range());
    // SAFETY: an `NSString` is a valid marked-text value.
    unsafe {
        view.setMarkedText_selectedRange_replacementRange(
            &marked,
            objc2_foundation::NSRange::new(2, 0),
            replacement,
        );
    }
    assert!(view.hasMarkedText());
    assert!(controller.markdown_document().text().contains("かな"));
    view.unmarkText();
    assert!(!view.hasMarkedText());
}

fn main() {
    controller_support::prepare();
    let _ = mtm();
    controller_support::main_thread::run(&[
        ("opening_document_starts_with_caret_only", opening_document_starts_with_caret_only),
        ("opening_second_document_clears_previous_selection", opening_second_document_clears_previous_selection),
        ("stale_full_document_selection_is_migrated", stale_full_document_selection_is_migrated),
        ("typing_through_real_key_down_mutates_document", typing_through_real_key_down_mutates_document),
        ("native_undo_redo_round_trip_typing", native_undo_redo_round_trip_typing),
        ("typing_at_heading_start_extends_visible_title", typing_at_heading_start_extends_visible_title),
        ("select_all_replaces_the_whole_source_document", select_all_replaces_the_whole_source_document),
        ("select_all_survives_fully_elided_projection", select_all_survives_fully_elided_projection),
        ("typing_keeps_caret_line_fixed", typing_keeps_caret_line_fixed),
        ("delete_through_real_key_down_mutates_document", delete_through_real_key_down_mutates_document),
        ("delete_word_through_real_key_down_mutates_document", delete_word_through_real_key_down_mutates_document),
        ("native_word_movement_preserves_source_caret", native_word_movement_preserves_source_caret),
        ("split_panes_keep_interaction_state_independent", split_panes_keep_interaction_state_independent),
        ("opening_split_view_preserves_current_reading_position", opening_split_view_preserves_current_reading_position),
        ("enabling_path_resolution_warms_current_document", enabling_path_resolution_warms_current_document),
        ("tab_after_live_edits_keeps_layout_and_viewport_stable", tab_after_live_edits_keeps_layout_and_viewport_stable),
        (
            "outline_jump_does_not_retain_fragments_from_the_old_camera",
            outline_jump_does_not_retain_fragments_from_the_old_camera,
        ),
        ("heading_command_keeps_viewport_stable", heading_command_keeps_viewport_stable),
        ("heading_picker_keeps_clicked_heading_fixed", heading_picker_keeps_clicked_heading_fixed),
        ("marked_text_uses_valid_undo_grouping", marked_text_uses_valid_undo_grouping),
    ]);
    controller_support::finish();
}
