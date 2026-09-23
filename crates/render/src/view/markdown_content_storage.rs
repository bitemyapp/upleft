//! Port of `View/MarkdownContentStorage.swift`: TextKit 2 content storage that
//! can present one Markdown block as one layout element while keeping the
//! source range byte-for-byte intact.
//!
//! A normal `NSTextContentStorage` creates one paragraph element per physical
//! newline. Markdown prose often uses those newlines only as source wrapping;
//! grouping the ranges here lets TextKit wrap the prose against the document
//! measure instead. Every replacement published to this storage is source-
//! length preserving, so element and source coordinates stay aligned.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, Message, define_class, msg_send};
use objc2_app_kit::{
    NSMutableParagraphStyle, NSParagraphStyle, NSTextContentManager, NSTextContentManagerEnumerationOptions,
    NSTextContentStorage, NSTextElement, NSTextElementProvider, NSTextLocation, NSTextParagraph, NSTextRange,
    NSTextStorage, NSTextStorageObserving,
};
use objc2_foundation::{NSAttributedString, NSMutableAttributedString, NSString};

use crate::appkit_compat::{attribute_value, attributes_at, keys, ns};
use crate::core_types::NSRange;
use crate::engine::display_map::{DisplayMap, ParagraphIndex, RangeSet};

// MARK: - MarkdownTextParagraph

pub struct MarkdownTextParagraphIvars {
    content_range: Retained<NSTextRange>,
    separator_range: Option<Retained<NSTextRange>>,
}

define_class!(
    /// A grouped element whose content range is the whole element and whose
    /// separator is the trailing terminator (Swift's private
    /// `MarkdownTextParagraph`).
    // SAFETY: NSTextParagraph's designated initialiser is
    // `initWithAttributedString:`, forwarded in `new`. No Drop impl.
    #[unsafe(super(NSTextParagraph))]
    #[name = "MarkdownTextParagraph"]
    #[ivars = MarkdownTextParagraphIvars]
    struct MarkdownTextParagraph;

    impl MarkdownTextParagraph {
        #[unsafe(method_id(paragraphContentRange))]
        fn paragraph_content_range(&self) -> Option<Retained<NSTextRange>> {
            Some(self.ivars().content_range.clone())
        }

        #[unsafe(method_id(paragraphSeparatorRange))]
        fn paragraph_separator_range(&self) -> Option<Retained<NSTextRange>> {
            self.ivars().separator_range.clone()
        }
    }
);

impl MarkdownTextParagraph {
    fn new(
        attributed_string: &NSAttributedString,
        text_content_manager: &NSTextContentManager,
        element_range: &NSTextRange,
        separator_range: Option<Retained<NSTextRange>>,
    ) -> Retained<MarkdownTextParagraph> {
        let this = Self::alloc().set_ivars(MarkdownTextParagraphIvars {
            content_range: element_range.retain(),
            separator_range,
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithAttributedString: attributed_string] };
        this.setTextContentManager(Some(text_content_manager));
        this.setElementRange(Some(element_range));
        this
    }
}

// MARK: - MarkdownContentStorage

pub struct MarkdownContentStorageIvars {
    source_ranges: RefCell<Vec<NSRange>>,
    element_cache: RefCell<HashMap<usize, Retained<NSTextParagraph>>>,
    display_map: RefCell<DisplayMap>,
    uses_custom_layout: Cell<bool>,
    suspended_custom_layout: Cell<bool>,
}

define_class!(
    // SAFETY: NSTextContentStorage's initialiser is `init`, forwarded in
    // `new`. The enumeration override keeps AppKit's signature and contract
    // (see the comments on the return value). No Drop impl.
    #[unsafe(super(NSTextContentStorage))]
    #[name = "MarkdownContentStorage"]
    #[ivars = MarkdownContentStorageIvars]
    pub struct MarkdownContentStorage;

    unsafe impl NSObjectProtocol for MarkdownContentStorage {}

    impl MarkdownContentStorage {
        #[unsafe(method_id(enumerateTextElementsFromLocation:options:usingBlock:))]
        fn enumerate_text_elements(
            &self,
            text_location: Option<&ProtocolObject<dyn NSTextLocation>>,
            options: NSTextContentManagerEnumerationOptions,
            block: &DynBlock<dyn Fn(NonNull<NSTextElement>) -> Bool + '_>,
        ) -> Option<Retained<ProtocolObject<dyn NSTextLocation>>> {
            self.enumerate(text_location, options, block)
        }
    }
);

impl MarkdownContentStorage {
    pub fn new() -> Retained<MarkdownContentStorage> {
        let this = Self::alloc().set_ivars(MarkdownContentStorageIvars {
            source_ranges: RefCell::new(Vec::new()),
            element_cache: RefCell::new(HashMap::new()),
            display_map: RefCell::new(DisplayMap::identity()),
            uses_custom_layout: Cell::new(false),
            suspended_custom_layout: Cell::new(false),
        });
        // SAFETY: NSTextContentStorage's initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn cached_element_count_for_testing(&self) -> usize {
        self.ivars().element_cache.borrow().len()
    }

    pub fn configure(
        &self,
        paragraph_index: &ParagraphIndex,
        reflow_ranges: &[NSRange],
        display_map: &DisplayMap,
        invalidated_ranges: Option<&[NSRange]>,
    ) {
        let ivars = self.ivars();
        let previous_ranges = ivars.source_ranges.borrow().clone();
        let previously_used_custom_layout = ivars.uses_custom_layout.get() || ivars.suspended_custom_layout.get();
        ivars.suspended_custom_layout.set(false);
        *ivars.display_map.borrow_mut() = display_map.clone();
        let length = self.textStorage().map_or(paragraph_index.length, |storage| storage.length() as isize);
        let ranges = Self::layout_ranges(paragraph_index, reflow_ranges, length);
        // Only take over layout when the ranges describe the whole document.
        ivars.uses_custom_layout.set(Self::tiles(&ranges, length));
        let uses_custom_layout = ivars.uses_custom_layout.get();
        if let Some(invalidated_ranges) = invalidated_ranges
            && previously_used_custom_layout
            && uses_custom_layout
        {
            // Preserve the exact prefix whose source ranges and presentation
            // are unchanged.
            ivars.element_cache.borrow_mut().retain(|&index, _| {
                if !(index < previous_ranges.len() && index < ranges.len() && previous_ranges[index] == ranges[index]) {
                    return false;
                }
                !invalidated_ranges
                    .iter()
                    .any(|range| upleft_core::ns_range::ns_intersection_range(*range, ranges[index]).length > 0)
            });
        } else {
            ivars.element_cache.borrow_mut().clear();
        }
        *ivars.source_ranges.borrow_mut() = if uses_custom_layout { ranges } else { Vec::new() };
    }

    /// True when `ranges` partition `[0, length)` with no gap and no overlap.
    fn tiles(ranges: &[NSRange], length: isize) -> bool {
        if !(length > 0) || ranges.is_empty() {
            return false;
        }
        let mut cursor = 0;
        for range in ranges {
            if range.location != cursor {
                return false;
            }
            cursor = range.upper_bound();
        }
        cursor == length
    }

    pub fn suspend_custom_layout(&self) {
        let ivars = self.ivars();
        ivars.suspended_custom_layout.set(ivars.uses_custom_layout.get());
        ivars.uses_custom_layout.set(false);
        *ivars.display_map.borrow_mut() = DisplayMap::identity();
    }

    fn enumerate(
        &self,
        text_location: Option<&ProtocolObject<dyn NSTextLocation>>,
        options: NSTextContentManagerEnumerationOptions,
        block: &DynBlock<dyn Fn(NonNull<NSTextElement>) -> Bool + '_>,
    ) -> Option<Retained<ProtocolObject<dyn NSTextLocation>>> {
        let ivars = self.ivars();
        // `configure` already proved the ranges tile the document; re-check
        // only the length, which an edit can move out from under them.
        let storage = self.textStorage();
        let applies = ivars.uses_custom_layout.get()
            && storage.as_ref().is_some_and(|storage| {
                ivars.source_ranges.borrow().last().map(|range| range.upper_bound()) == Some(storage.length() as isize)
            });
        let Some(storage) = storage.filter(|_| applies) else {
            return unsafe {
                msg_send![super(self), enumerateTextElementsFromLocation: text_location, options: options, usingBlock: block]
            };
        };

        let reverse = options.contains(NSTextContentManagerEnumerationOptions::Reverse);
        let document_length = storage.length() as isize;
        let document_start = self.documentRange().location();
        let requested_offset = match text_location {
            Some(location) => self.offsetFromLocation_toLocation(&document_start, location),
            None => {
                if reverse {
                    document_length
                } else {
                    0
                }
            }
        };

        // The returned location must move past every element handed to
        // `block`, including the one `block` stopped on.
        if reverse {
            let mut index = self.last_index_starting_before(requested_offset);
            let mut edge = requested_offset;
            while index >= 0 {
                let element = self.element(index as usize, &storage);
                let wants_more = block.call((NonNull::from(&**element),)).as_bool();
                edge = ivars.source_ranges.borrow()[index as usize].location;
                if !wants_more || index == 0 {
                    break;
                }
                index -= 1;
            }
            return self.locationFromLocation_withOffset(&document_start, edge);
        }

        let mut index = self.first_index_ending_after(requested_offset);
        let mut edge = requested_offset;
        let count = ivars.source_ranges.borrow().len();
        while index < count {
            let element = self.element(index, &storage);
            let wants_more = block.call((NonNull::from(&**element),)).as_bool();
            edge = ivars.source_ranges.borrow()[index].upper_bound();
            index += 1;
            if !wants_more {
                break;
            }
        }
        self.locationFromLocation_withOffset(&document_start, edge)
    }

    /// First element that extends past `offset`, or `count` when none does.
    fn first_index_ending_after(&self, offset: isize) -> usize {
        let ranges = self.ivars().source_ranges.borrow();
        let (mut low, mut high) = (0usize, ranges.len());
        while low < high {
            let middle = (low + high) / 2;
            if ranges[middle].upper_bound() > offset {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        low
    }

    /// Last element that begins before `offset`, or the final element when
    /// none does.
    fn last_index_starting_before(&self, offset: isize) -> isize {
        if !(offset > 0) {
            return -1;
        }
        let ranges = self.ivars().source_ranges.borrow();
        let (mut low, mut high) = (0usize, ranges.len());
        while low < high {
            let middle = (low + high) / 2;
            if ranges[middle].location < offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low as isize - 1
    }

    fn element(&self, index: usize, storage: &NSTextStorage) -> Retained<NSTextParagraph> {
        if let Some(cached) = self.ivars().element_cache.borrow().get(&index) {
            return cached.clone();
        }

        let source_range = self.ivars().source_ranges.borrow()[index];
        let display = self.ivars().display_map.borrow().display_string_for_source_range(source_range, storage);
        let anchored = Self::anchoring_leading_style(
            &display.unwrap_or_else(|| storage.attributedSubstringFromRange(ns(source_range))),
            source_range.location,
            storage,
        );
        let attributed: Retained<NSAttributedString> = match self.paragraph_style(source_range, storage) {
            Some(style) => {
                let styled = NSMutableAttributedString::initWithAttributedString(NSMutableAttributedString::alloc(), &anchored);
                unsafe {
                    styled.addAttribute_value_range(
                        keys::paragraph_style(),
                        &style,
                        objc2_foundation::NSRange::new(0, styled.length()),
                    )
                };
                Retained::into_super(styled)
            }
            None => anchored,
        };
        let document_start = self.documentRange().location();
        let start = self.locationFromLocation_withOffset(&document_start, source_range.location);
        let end = self.locationFromLocation_withOffset(&document_start, source_range.upper_bound());
        let element_range = match (start, end) {
            (Some(start), Some(end)) => {
                NSTextRange::initWithLocation_endLocation(NSTextRange::alloc(), &start, Some(&end))
            }
            _ => None,
        };
        let Some(element_range) = element_range else {
            let paragraph = NSTextParagraph::initWithAttributedString(NSTextParagraph::alloc(), Some(&attributed));
            paragraph.setTextContentManager(Some(self));
            return paragraph;
        };
        let separator_range = Self::separator_range(source_range, &storage.string(), &document_start, self);
        let paragraph = MarkdownTextParagraph::new(&attributed, self, &element_range, separator_range);
        let paragraph: Retained<NSTextParagraph> = Retained::into_super(paragraph);
        self.ivars().element_cache.borrow_mut().insert(index, paragraph.clone());
        paragraph
    }

    /// TextKit takes a paragraph's indent, spacing and line height from its
    /// first character; anchor a synthetic leading run to the source's
    /// paragraph style and font.
    fn anchoring_leading_style(
        display: &NSAttributedString,
        offset: isize,
        storage: &NSTextStorage,
    ) -> Retained<NSAttributedString> {
        if !(display.length() > 0 && storage.length() > 0) {
            return display.retain();
        }
        let mut head = objc2_foundation::NSRange::new(0, 0);
        // SAFETY: index 0 is inside a non-empty string; `head` is an out param.
        let leading = unsafe { display.attribute_atIndex_effectiveRange(keys::paragraph_style(), 0, &mut head) };
        if leading.is_some() {
            return display.retain();
        }
        let anchor = (offset.max(0)).min(storage.length() as isize - 1) as usize;
        let source = attributes_at(storage, anchor);
        let styled = NSMutableAttributedString::initWithAttributedString(NSMutableAttributedString::alloc(), display);
        if let Some(paragraph) = source.objectForKey(keys::paragraph_style()) {
            unsafe { styled.addAttribute_value_range(keys::paragraph_style(), &paragraph, head) };
        }
        if let Some(font) = source.objectForKey(keys::font()) {
            unsafe { styled.addAttribute_value_range(keys::font(), &font, head) };
        }
        Retained::into_super(styled)
    }

    /// The one paragraph style a grouped element should lay out with, or
    /// `None` when the storage already says it.
    fn paragraph_style(&self, source_range: NSRange, storage: &NSTextStorage) -> Option<Retained<NSParagraphStyle>> {
        if !(source_range.length > 0 && source_range.upper_bound() <= storage.length() as isize) {
            return None;
        }
        let text: Retained<NSString> = storage.string();
        let first_line = text.paragraphRangeForRange(objc2_foundation::NSRange::new(source_range.location as usize, 0));
        let is_grouped = ((first_line.location + first_line.length) as isize) < source_range.upper_bound();
        let content_offset = self.first_content_offset(source_range);

        if !(is_grouped || content_offset > source_range.location) {
            return None;
        }
        let base = attribute_value(storage, keys::paragraph_style(), content_offset as usize)
            .and_then(|value| value.downcast::<NSParagraphStyle>().ok())?;
        if !is_grouped {
            return Some(base);
        }
        let trailing = attribute_value(storage, keys::paragraph_style(), (source_range.upper_bound() - 1) as usize)
            .and_then(|value| value.downcast::<NSParagraphStyle>().ok());
        let Some(trailing) = trailing else { return Some(base) };
        if trailing.paragraphSpacing() == base.paragraphSpacing() {
            return Some(base);
        }
        let unified: Retained<NSMutableParagraphStyle> = unsafe { msg_send![&*base, mutableCopy] };
        unified.setParagraphSpacing(trailing.paragraphSpacing());
        let copy: Retained<NSParagraphStyle> = unsafe { msg_send![&*unified, copy] };
        Some(copy)
    }

    /// First offset in the element not covered by a display substitution.
    fn first_content_offset(&self, source_range: NSRange) -> isize {
        let mut offset = source_range.location;
        for substitution in self.ivars().display_map.borrow().substitutions_in(source_range) {
            if substitution.source_range.location <= offset && substitution.source_range.upper_bound() > offset {
                offset = substitution.source_range.upper_bound();
            }
        }
        offset.min(source_range.location.max(source_range.upper_bound() - 1))
    }

    fn separator_range(
        source_range: NSRange,
        text: &NSString,
        document_start: &ProtocolObject<dyn NSTextLocation>,
        manager: &NSTextContentStorage,
    ) -> Option<Retained<NSTextRange>> {
        if !(source_range.length > 0) {
            return None;
        }
        let last = text.characterAtIndex((source_range.upper_bound() - 1) as usize);
        let separator_length: isize = match last {
            0x0A => {
                if source_range.length > 1 && text.characterAtIndex((source_range.upper_bound() - 2) as usize) == 0x0D {
                    2
                } else {
                    1
                }
            }
            0x0D | 0x0085 | 0x2028 | 0x2029 => 1,
            _ => {
                let end = manager.locationFromLocation_withOffset(document_start, source_range.upper_bound())?;
                return Some(NSTextRange::initWithLocation(NSTextRange::alloc(), &end));
            }
        };
        if !(separator_length <= source_range.length) {
            return None;
        }
        let start = manager.locationFromLocation_withOffset(document_start, source_range.upper_bound() - separator_length)?;
        let end = manager.locationFromLocation_withOffset(document_start, source_range.upper_bound())?;
        NSTextRange::initWithLocation_endLocation(NSTextRange::alloc(), &start, Some(&end))
    }

    fn layout_ranges(paragraph_index: &ParagraphIndex, reflow_ranges: &[NSRange], length: isize) -> Vec<NSRange> {
        // `disjoint`, not `normalized`: two reflow groups that merely touch
        // are two paragraphs.
        let groups: Vec<NSRange> = RangeSet::disjoint(reflow_ranges)
            .into_iter()
            .filter(|group| group.location >= 0 && group.upper_bound() <= length)
            .collect();
        let mut result = Vec::new();
        let mut cursor = 0;
        for group in groups {
            Self::append_physical_paragraphs(cursor, group.location, paragraph_index, &mut result);
            result.push(group);
            cursor = group.upper_bound();
        }
        Self::append_physical_paragraphs(cursor, length, paragraph_index, &mut result);
        result
    }

    fn append_physical_paragraphs(lower: isize, upper: isize, paragraph_index: &ParagraphIndex, result: &mut Vec<NSRange>) {
        if !(upper > lower) {
            return;
        }
        let bounds = NSRange::new(lower, upper - lower);
        let first = paragraph_index.index_containing(lower);
        let last = paragraph_index.index_containing(lower.max(upper - 1));
        for index in first..=last {
            // Clip rather than skip, so the element ranges tile the document.
            let clipped = upleft_core::ns_range::ns_intersection_range(paragraph_index.range_at(index), bounds);
            if !(clipped.length > 0) {
                continue;
            }
            result.push(clipped);
        }
    }
}

#[allow(dead_code)]
fn _protocols(_: &dyn NSTextElementProvider) {}
