//! Port of `Tests/MarkdownRenderTests/ContentStorageTilingTests.swift`:
//! regression cover for the launch hang. `MarkdownContentStorage`'s element
//! ranges must tile `[0, length)`, walked the way the layout manager walks
//! them.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::StackBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{
    NSTextContentManagerEnumerationOptions, NSTextContentStorage, NSTextElement, NSTextElementProvider, NSTextStorage,
    NSTextStorageObserving,
};
use objc2_foundation::NSString;
use upleft_core::NSRange;
use upleft_render::engine::display_map::{DisplayMap, ParagraphIndex};
use upleft_render::view::markdown_content_storage::MarkdownContentStorage;

const WRAPPED: &str = "Alpha beta gamma delta\nepsilon zeta eta theta\niota kappa lambda\n\nA second block that stands on its own.";

fn text_storage(text: &str) -> Retained<NSTextStorage> {
    unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(text)] }
}

fn make_storage(text: &str) -> (Retained<MarkdownContentStorage>, Retained<NSTextStorage>) {
    let storage = text_storage(text);
    let content = MarkdownContentStorage::new();
    content.setTextStorage(Some(&storage));
    (content, storage)
}

fn physical_element_count(text: &str) -> usize {
    let content = NSTextContentStorage::new();
    let storage = text_storage(text);
    content.setTextStorage(Some(&storage));
    walk_elements(&content, storage.length() as isize).len()
}

fn utf16_index(text: &str, needle: &str) -> isize {
    let byte = text.find(needle).expect("needle");
    text[..byte].encode_utf16().count() as isize
}

/// Walks one element per call, resuming from the returned location.
fn walk_elements(content: &NSTextContentStorage, length: isize) -> Vec<NSRange> {
    let origin = content.documentRange().location();
    let mut ranges = Vec::new();
    let mut offset = 0isize;
    let mut steps = 0isize;
    while offset < length {
        steps += 1;
        assert!(steps <= length + 8, "enumeration never reached the end of the document");
        let from = content.locationFromLocation_withOffset(&origin, offset).expect("a text location");
        let delivered: Cell<Option<NSRange>> = Cell::new(None);
        let block = StackBlock::new(|element: NonNull<NSTextElement>| -> Bool {
            let element = unsafe { element.as_ref() };
            if let Some(range) = element.elementRange() {
                let location = content.offsetFromLocation_toLocation(&origin, &range.location()) as isize;
                let length = content.offsetFromLocation_toLocation(&range.location(), &range.endLocation()) as isize;
                delivered.set(Some(NSRange::new(location, length)));
            }
            Bool::NO
        });
        let resume =
            content.enumerateTextElementsFromLocation_options_usingBlock(Some(&from), NSTextContentManagerEnumerationOptions(0), &block);
        let range = delivered.get().unwrap_or_else(|| panic!("no element delivered from offset {offset}"));
        ranges.push(range);
        let resume = resume.unwrap_or_else(|| panic!("no resume location returned from offset {offset}"));
        let next = content.offsetFromLocation_toLocation(&origin, &resume) as isize;
        assert!(next > offset, "enumeration did not advance past offset {offset} — this is the launch hang");
        offset = next;
    }
    ranges
}

fn expect_tiles(ranges: &[NSRange], length: isize) {
    assert!(!ranges.is_empty(), "no element ranges at all");
    let mut cursor = 0;
    for range in ranges {
        assert_eq!(range.location, cursor, "element {range:?} does not start at {cursor}");
        cursor = range.upper_bound();
    }
    assert_eq!(cursor, length, "elements end at {cursor}, document is {length}");
}

#[test]
fn empty_paragraph_index_does_not_take_over_layout() {
    let (content, storage) = make_storage(WRAPPED);
    content.configure(&ParagraphIndex::empty(), &[], &DisplayMap::identity(), None);
    let ranges = walk_elements(&content, storage.length() as isize);
    expect_tiles(&ranges, storage.length() as isize);
    assert_eq!(ranges.len(), physical_element_count(WRAPPED));
    assert!(ranges.len() > 1);
}

#[test]
fn stale_shorter_paragraph_index_is_refused() {
    let (content, storage) = make_storage(WRAPPED);
    let stale = ParagraphIndex::from_text(&NSString::from_str("Alpha beta gamma delta\n"));
    assert!(stale.length < storage.length() as isize);
    content.configure(&stale, &[], &DisplayMap::identity(), None);
    let ranges = walk_elements(&content, storage.length() as isize);
    expect_tiles(&ranges, storage.length() as isize);
    assert_eq!(ranges.len(), physical_element_count(WRAPPED));
}

#[test]
fn physical_paragraphs_tile() {
    let (content, storage) = make_storage(WRAPPED);
    let index = ParagraphIndex::from_text(&NSString::from_str(WRAPPED));
    content.configure(&index, &[], &DisplayMap::identity(), None);
    let ranges = walk_elements(&content, storage.length() as isize);
    expect_tiles(&ranges, storage.length() as isize);
    let physical: Vec<NSRange> = (0..index.starts.len()).map(|at| index.range_at(at)).collect();
    assert_eq!(ranges, physical);
}

#[test]
fn straddling_reflow_group_tiles() {
    let (content, storage) = make_storage(WRAPPED);
    let index = ParagraphIndex::from_text(&NSString::from_str(WRAPPED));
    let start = utf16_index(WRAPPED, "beta");
    let end = utf16_index(WRAPPED, "kappa") + 5;
    let group = NSRange::new(start, end - start);
    assert!(index.paragraph_range_containing(group.location).location < group.location);
    assert!(index.paragraph_range_containing(group.upper_bound() - 1).upper_bound() > group.upper_bound());
    content.configure(&index, &[group], &DisplayMap::identity(), None);
    let ranges = walk_elements(&content, storage.length() as isize);
    expect_tiles(&ranges, storage.length() as isize);
    assert!(ranges.contains(&group));
}

#[test]
fn trailing_newline_tiles() {
    let text = format!("{WRAPPED}\n");
    let index = ParagraphIndex::from_text(&NSString::from_str(&text));
    assert_eq!(index.range_at(index.starts.len() - 1).length, 0);

    let (plain, plain_storage) = make_storage(&text);
    plain.configure(&index, &[], &DisplayMap::identity(), None);
    expect_tiles(&walk_elements(&plain, plain_storage.length() as isize), plain_storage.length() as isize);

    let start = utf16_index(&text, "Alpha");
    let end = utf16_index(&text, "lambda") + 6;
    let group = NSRange::new(start, end - start);
    let (grouped, grouped_storage) = make_storage(&text);
    grouped.configure(&index, &[group], &DisplayMap::identity(), None);
    let ranges = walk_elements(&grouped, grouped_storage.length() as isize);
    expect_tiles(&ranges, grouped_storage.length() as isize);
    assert!(ranges.contains(&group));
}

#[test]
fn reverse_enumeration_at_origin_stops_at_origin() {
    let (content, storage) = make_storage(WRAPPED);
    content.configure(&ParagraphIndex::from_text(&NSString::from_str(WRAPPED)), &[], &DisplayMap::identity(), None);
    let delivered = RefCell::new(0);
    let block = StackBlock::new(|_element: NonNull<NSTextElement>| -> Bool {
        *delivered.borrow_mut() += 1;
        Bool::YES
    });
    let origin = content.documentRange().location();
    let resume = content.enumerateTextElementsFromLocation_options_usingBlock(
        Some(&origin),
        NSTextContentManagerEnumerationOptions::Reverse,
        &block,
    );
    assert_eq!(*delivered.borrow(), 0);
    let resume = resume.expect("a resume location");
    assert_eq!(content.offsetFromLocation_toLocation(&origin, &resume), 0);
    assert!(storage.length() > 0);
}
