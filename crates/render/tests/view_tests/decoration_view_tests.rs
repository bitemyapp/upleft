//! The `@MainActor` cases of `DecorationTests.swift` that need the text
//! view, its content storage, or a table fragment's projection of a live
//! edit (the engine-only cases are in `tests/decoration_tests.rs`).

use objc2::MainThreadMarker;
use objc2_app_kit::{NSParagraphStyle, NSSelectionAffinity, NSTextElementProvider, NSTextParagraph, NSTextStorageObserving};
use objc2_foundation::{NSString, NSValue};
use upleft_core::NSRange;
use upleft_render::appkit_compat::{attribute_value, enumerate_attribute, keys};
use upleft_render::engine::display_map::{DisplayMap, ParagraphIndex, RangeSet};
use upleft_render::engine::hard_wrap_reflow::HardWrapReflow;
use upleft_render::fragments::table_fragment::TableCellPresentation;
use upleft_render::render_contracts::{FragmentPayload, RenderMode, attribute_keys};
use upleft_render::view::markdown_content_storage::MarkdownContentStorage;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("decoration_list_text_and_wrapped_lines_share_one_content_edge", list_text_and_wrapped_lines_share_one_content_edge),
    ("decoration_block_content_storage_groups_source_wrapped_paragraphs", block_content_storage_groups_source_wrapped_paragraphs),
    (
        "decoration_length_changing_edit_keeps_downstream_fragment_ranges_aligned_until_parse",
        length_changing_edit_keeps_downstream_fragment_ranges_aligned_until_parse,
    ),
    ("decoration_multi_paragraph_selection_keeps_hidden_attributes_in_sync", multi_paragraph_selection_keeps_hidden_attributes_in_sync),
];

fn list_text_and_wrapped_lines_share_one_content_edge(mtm: MainThreadMarker) {
    let text = "- [ ] A task with enough words to wrap onto another visual line in a narrow measure.\n";
    let storage = text_storage(text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 280.0, 300.0), &storage, mtm);
    view.update(parse(text), &wholesale(), true);
    let content = range_of(text, "A task");
    let style = attribute_value(&storage, keys::paragraph_style(), content.location as usize)
        .and_then(|value| value.downcast::<NSParagraphStyle>().ok());
    expect!(style.is_some());
    let style = style.unwrap();
    expect!(style.firstLineHeadIndent() == style.headIndent());
    expect!(style.headIndent() > 0.0, "the ornament still needs a hanging column");
    expect!(style.paragraphSpacing() > 0.0, "task controls need breathing room");
}

fn block_content_storage_groups_source_wrapped_paragraphs(_mtm: MainThreadMarker) {
    let source = "first physical line\nsecond physical line\n";
    let document = parse(source);
    let units: Vec<u16> = source.encode_utf16().collect();
    let index = ParagraphIndex::from_text(&NSString::from_str(source));
    let storage = text_storage(source);
    let plan = HardWrapReflow::plan(&document, &units, &[], &[], true);
    let map = DisplayMap::new(index.clone(), plan.substitutions.clone());
    let content_storage = MarkdownContentStorage::new();
    content_storage.setTextStorage(Some(&storage));
    content_storage.configure(&index, &plan.ranges, &map, None);

    let elements = std::cell::RefCell::new(Vec::new());
    let block = block2::StackBlock::new(|element: std::ptr::NonNull<objc2_app_kit::NSTextElement>| -> objc2::runtime::Bool {
        // SAFETY: TextKit hands a live element for the call.
        elements.borrow_mut().push(objc2::Message::retain(unsafe { element.as_ref() }));
        objc2::runtime::Bool::YES
    });
    let _ = content_storage.enumerateTextElementsFromLocation_options_usingBlock(
        None,
        objc2_app_kit::NSTextContentManagerEnumerationOptions(0),
        &block,
    );
    let grouped = elements.into_inner().into_iter().filter_map(|element| element.downcast::<NSTextParagraph>().ok()).find(|paragraph| {
        paragraph.attributedString().string().to_string().contains("first physical line second physical line")
    });
    expect!(grouped.is_some());
    expect!(storage_string(&storage) == source);
}

fn length_changing_edit_keeps_downstream_fragment_ranges_aligned_until_parse(mtm: MainThreadMarker) {
    let source = "Intro contains three removable letters.\n\n| | |\n|---|---|\n| **Rendered diff** | Updates *in place* with `code`. |\n\n```swift\nlet answer = 42\n```";
    let storage = text_storage(source);
    let sheet = (*fallback_sheet()).clone();
    let view = MarkdownTextView::new(rect(0.0, 0.0, 900.0, 900.0), &storage, std::rc::Rc::new(sheet), mtm);
    view.set_mode(RenderMode::Live);
    view.update(parse(source), &wholesale(), true);

    let edit = range_of(source, "ont");
    expect!(view.perform_source_edit(edit, "", "Edit"));

    let current = storage_string(&storage);
    let table_start = range_of(&current, "| | |").location;
    let code_start = range_of(&current, "```swift").location;
    let table = attribute_value(&storage, attribute_keys::dr_fragment(), table_start as usize)
        .and_then(|value| value.downcast::<FragmentPayload>().ok())
        .expect("table payload");
    let code = attribute_value(&storage, attribute_keys::dr_fragment(), code_start as usize)
        .and_then(|value| value.downcast::<FragmentPayload>().ok())
        .expect("code payload");
    let data = table.table_data().expect("table data");
    let row = data.body_rows().first().copied().cloned().expect("a body row");

    expect!(table.source_range().location == table_start);
    expect!(code.source_range().location == code_start);
    expect!(TableCellPresentation::plain_text(&row.cells[0], &storage) == "Rendered diff");
    expect!(TableCellPresentation::plain_text(&row.cells[1], &storage) == "Updates in place with code.");
}

fn multi_paragraph_selection_keeps_hidden_attributes_in_sync(mtm: MainThreadMarker) {
    let text = "First **bold** line.\nSecond *italic* line.\n";
    let storage = text_storage(text);
    let view = MarkdownTextView::with_storage(rect(0.0, 0.0, 600.0, 400.0), &storage, mtm);
    view.set_mode(RenderMode::Live);
    view.update(parse(text), &wholesale(), true);

    let start = range_of(text, "bold").location;
    let end = range_of(text, "italic").upper_bound();
    let selected = NSRange::new(start, end - start);
    let text_kit = view.current_display_map().text_kit_range_for_source(selected);
    // SAFETY: a plain range value.
    let value = unsafe { NSValue::valueWithRange(objc2_foundation::NSRange::new(text_kit.location as usize, text_kit.length as usize)) };
    let ranges = objc2_foundation::NSArray::from_retained_slice(&[value]);
    view.setSelectedRanges_affinity_stillSelecting(&ranges, NSSelectionAffinity::Downstream, false);

    let mut attributed_hidden: Vec<NSRange> = Vec::new();
    enumerate_attribute(
        &storage,
        attribute_keys::dr_hidden(),
        objc2_foundation::NSRange::new(0, storage.length()),
        false,
        |value, range| {
            if value.is_some() {
                attributed_hidden.push(NSRange::new(range.location as isize, range.length as isize));
            }
            true
        },
    );
    expect!(RangeSet::normalized(&attributed_hidden) == view.current_display_map().hidden_ranges());
}
