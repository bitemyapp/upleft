//! Port of `View/FragmentProvider.swift`: chooses the `NSTextLayoutFragment`
//! subclass for every paragraph.
//!
//! This is where §6.2's per-element table becomes code: the same block draws
//! as an object in Read mode, as an object in Live mode until the caret
//! enters it, and as plain highlighted source in Source mode — one decision,
//! made in one place, from `RenderMode` and the caret.
//!
//! # The object-fragment seam
//!
//! The dispatch below is the Swift `switch` verbatim. The object fragments
//! themselves (tables, code blocks, math, Mermaid, images, front matter,
//! thematic breaks, callouts, list ornaments) are constructed through the
//! functions under "Object fragments" at the bottom of this file. Each one
//! returns `None` until its fragment is ported, and the provider then falls
//! back to a `ProseFragment` for that paragraph, which is what makes a
//! document using that kind fail the `render` conformance suite until it
//! lands. Porting a fragment means filling in its one function.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::NSTextElementProvider;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{
    NSTextElement, NSTextLayoutFragment, NSTextLayoutManager, NSTextLayoutManagerDelegate, NSTextLocation, NSTextRange,
    NSTextStorage,
};
use objc2_foundation::NSString;

use crate::appkit_compat::attribute_value;
use crate::core_types::NSRange;
use crate::fragments::fragment_base::{ElidedFragment, ElisionCueFragment, FragmentContext, ProseFragment};
use crate::render_contracts::{FragmentKind, FragmentPayload, RenderMode, attribute_keys};

pub struct FragmentProviderIvars {
    context: Rc<FragmentContext>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate method
    // keeps AppKit's signature. No Drop impl.
    #[unsafe(super(NSObject))]
    #[name = "FragmentProvider"]
    #[ivars = FragmentProviderIvars]
    pub struct FragmentProvider;

    unsafe impl NSObjectProtocol for FragmentProvider {}

    unsafe impl NSTextLayoutManagerDelegate for FragmentProvider {
        #[unsafe(method_id(textLayoutManager:textLayoutFragmentForLocation:inTextElement:))]
        fn text_layout_manager_text_layout_fragment_for_location(
            &self,
            _text_layout_manager: &NSTextLayoutManager,
            _location: &ProtocolObject<dyn NSTextLocation>,
            text_element: &NSTextElement,
        ) -> Retained<NSTextLayoutFragment> {
            self.fragment(text_element)
        }
    }
);

/// `CodeBlockFragment.Role`: which part of a fenced block a paragraph is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeBlockRole {
    OpenChrome,
    Body,
    CloseChrome,
    CollapsedChip,
}

/// What every object-fragment constructor receives.
pub struct ObjectFragmentRequest<'a> {
    pub text_element: &'a NSTextElement,
    pub element_range: &'a NSTextRange,
    pub payload: &'a FragmentPayload,
    pub context: &'a Rc<FragmentContext>,
}

impl FragmentProvider {
    pub fn new(context: Rc<FragmentContext>) -> Retained<FragmentProvider> {
        let this = Self::alloc().set_ivars(FragmentProviderIvars { context });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn context(&self) -> &Rc<FragmentContext> {
        &self.ivars().context
    }

    fn fragment(&self, text_element: &NSTextElement) -> Retained<NSTextLayoutFragment> {
        let context = &self.ivars().context;
        let element_range = text_element.elementRange();
        // Prose is a drawing fragment too.
        let plain = || -> Retained<NSTextLayoutFragment> {
            Retained::into_super(ProseFragment::new(text_element, element_range.as_deref(), Some(context)))
        };
        let (Some(manager), Some(element_range)) = (text_element.textContentManager(), element_range.clone()) else {
            return plain();
        };
        let document_start = manager.documentRange().location();
        let start = manager.offsetFromLocation_toLocation(&document_start, &element_range.location());
        let end = manager.offsetFromLocation_toLocation(&document_start, &element_range.endLocation());
        let source = NSRange::new(start, (end - start).max(0));

        // Elision wins over everything (§5.2, §7.1).
        if context.elision.borrow().is_elided(source.location) {
            let hidden = context.cue_elision.borrow().range_containing(source.location);
            if let Some(hidden) = hidden {
                let index = context.paragraph_index();
                if index.index_containing(source.location) == index.index_containing(hidden.location) {
                    drop(index);
                    return Retained::into_super(ElisionCueFragment::new(
                        text_element,
                        Some(&element_range),
                        hidden,
                        context,
                    ));
                }
            }
            return Retained::into_super(ElidedFragment::new(text_element, Some(&element_range)));
        }

        let Some(storage) = context.storage() else { return plain() };
        if !(source.location < storage.length() as isize) {
            return plain();
        }
        let Some(payload) = self.code_payload(&storage, source) else { return plain() };

        // Source mode never renders objects.
        let mode = context.mode.get();
        if mode == RenderMode::Source {
            return plain();
        }
        // §6.2: a caret inside an object or an explicit scoped source lens
        // swaps only that object to source.
        let editing = mode == RenderMode::Live
            && (context.is_caret_inside(payload.source_range()) || context.is_source_focused(payload.source_range()));

        let request = ObjectFragmentRequest {
            text_element,
            element_range: &element_range,
            payload: &payload,
            context,
        };
        match payload.kind() {
            FragmentKind::CodeBlock | FragmentKind::CollapsedCodeBlock => {
                self.code_fragment(&request, source, &storage).unwrap_or_else(plain)
            }
            FragmentKind::Table => {
                if editing {
                    return plain();
                }
                // The `|---|:--:|` row is syntax, not data: it collapses.
                let delimiter = payload.table_data().as_ref().map(|table| table.delimiter_range);
                if let Some(delimiter) = delimiter
                    && delimiter.contains(source.location)
                {
                    return Retained::into_super(ElidedFragment::new(text_element, Some(&element_range)));
                }
                table_row_fragment(&request).unwrap_or_else(plain)
            }
            FragmentKind::BlockMath => {
                if editing {
                    return plain();
                }
                math_fragment(&request).unwrap_or_else(plain)
            }
            FragmentKind::Mermaid => {
                if editing {
                    return plain();
                }
                mermaid_fragment(&request).unwrap_or_else(plain)
            }
            FragmentKind::Image => {
                if editing {
                    return plain();
                }
                image_fragment(&request).unwrap_or_else(plain)
            }
            FragmentKind::FrontMatter => {
                let fields = context.front_matter_fields.borrow().clone();
                if editing || fields.is_empty() {
                    return plain();
                }
                front_matter_fragment(&request, fields).unwrap_or_else(plain)
            }
            FragmentKind::ThematicBreak => thematic_break_fragment(&request).unwrap_or_else(plain),
            // The rule and the icon stay on while editing.
            FragmentKind::Callout => callout_fragment(&request).unwrap_or_else(plain),
            FragmentKind::ListOrnament => list_ornament_fragment(&request).unwrap_or_else(plain),
            FragmentKind::InlineMath => plain(),
        }
    }

    // MARK: - Code blocks

    fn code_fragment(
        &self,
        request: &ObjectFragmentRequest<'_>,
        source: NSRange,
        storage: &NSTextStorage,
    ) -> Option<Retained<NSTextLayoutFragment>> {
        let context = request.context;
        let payload = request.payload;
        let block = payload.source_range();
        let lines = self.line_count(block);
        let collapsed = context
            .collapse_overrides
            .borrow()
            .get(&block.location)
            .copied()
            .unwrap_or(payload.kind() == FragmentKind::CollapsedCodeBlock);

        if collapsed {
            // §5.1: one line — language, line count, click to expand.
            if !(source.location <= block.location) {
                return Some(Retained::into_super(ElidedFragment::new(
                    request.text_element,
                    Some(request.element_range),
                )));
            }
            return code_block_fragment(request, CodeBlockRole::CollapsedChip, lines);
        }

        let is_fence = self.paragraph_contains_fence_marker(storage, source);
        let role = if is_fence && source.location <= block.location {
            CodeBlockRole::OpenChrome
        } else if is_fence && source.upper_bound() >= block.upper_bound() {
            CodeBlockRole::CloseChrome
        } else {
            CodeBlockRole::Body
        };
        code_block_fragment(request, role, lines)
    }

    fn line_count(&self, range: NSRange) -> isize {
        let index = self.ivars().context.paragraph_index();
        if !(range.length > 0) {
            return 0;
        }
        let first = index.index_containing(range.location) as isize;
        let last = index.index_containing(range.location.max(range.upper_bound() - 1)) as isize;
        // Minus the two fence lines when they are present.
        (last - first - 1).max(0)
    }

    /// The `.drFragment` owning a paragraph: the innermost block that begins
    /// on this line, with code keeping first refusal.
    fn code_payload(&self, storage: &NSTextStorage, source: NSRange) -> Option<Retained<FragmentPayload>> {
        let start = payload_at(storage, source.location);
        if let Some(start) = &start
            && is_code(start.kind())
        {
            return Some(start.clone());
        }
        if let Some(code) = self.code_payload_after_whitespace(storage, source) {
            return Some(code);
        }
        self.innermost_payload_after_indent(storage, source).or(start)
    }

    /// The payload of the innermost block *beginning* on this paragraph.
    fn innermost_payload_after_indent(&self, storage: &NSTextStorage, source: NSRange) -> Option<Retained<FragmentPayload>> {
        let probe = container_prefix_end(storage, source);
        if !(probe < source.upper_bound() && probe < storage.length() as isize) {
            return None;
        }
        let payload = payload_at(storage, probe)?;
        let range = payload.source_range();
        if !(range.location >= source.location && range.location < source.upper_bound()) {
            return None;
        }
        Some(payload)
    }

    fn code_payload_after_whitespace(&self, storage: &NSTextStorage, source: NSRange) -> Option<Retained<FragmentPayload>> {
        let probe = first_non_whitespace_offset(storage, source);
        if !(probe > source.location && probe < source.upper_bound()) {
            return None;
        }
        let payload = payload_at(storage, probe)?;
        if !is_code(payload.kind()) {
            return None;
        }
        Some(payload)
    }

    /// True when the paragraph carries a fence marker (`.drMarker`).
    fn paragraph_contains_fence_marker(&self, storage: &NSTextStorage, source: NSRange) -> bool {
        let probe = first_non_whitespace_offset(storage, source);
        if !(probe < source.upper_bound()) {
            return false;
        }
        attribute_value(storage, attribute_keys::dr_marker(), probe as usize).is_some()
    }
}

fn payload_at(storage: &NSTextStorage, offset: isize) -> Option<Retained<FragmentPayload>> {
    attribute_value(storage, attribute_keys::dr_fragment(), offset as usize)?
        .downcast::<FragmentPayload>()
        .ok()
}

fn is_code(kind: FragmentKind) -> bool {
    matches!(kind, FragmentKind::CodeBlock | FragmentKind::CollapsedCodeBlock)
}

/// End of the leading container prefix: indentation and `>` markers.
fn container_prefix_end(storage: &NSTextStorage, source: NSRange) -> isize {
    let end = source.upper_bound().min(source.location + 256);
    let text: Retained<NSString> = storage.string();
    let mut probe = source.location;
    while probe < end {
        match text.characterAtIndex(probe as usize) {
            0x20 | 0x09 | 0x3E => probe += 1,
            _ => return probe,
        }
    }
    probe
}

/// First offset past the paragraph's leading marker prefix.
fn first_non_whitespace_offset(storage: &NSTextStorage, source: NSRange) -> isize {
    let end = source.upper_bound().min(source.location + 256);
    let text: Retained<NSString> = storage.string();
    let mut probe = source.location;
    let mut last_was_digit = false;
    while probe < end {
        let unit = text.characterAtIndex(probe as usize);
        match unit {
            0x20 | 0x09 | 0x3E | 0x2D | 0x2A | 0x2B => {
                last_was_digit = false;
                probe += 1;
            }
            0x30..=0x39 => {
                last_was_digit = true;
                probe += 1;
            }
            0x2E if last_was_digit => {
                last_was_digit = false;
                probe += 1;
            }
            _ => break,
        }
    }
    probe
}

// MARK: - Object fragments
//
// One constructor per Swift fragment class. Each returns `None` until that
// fragment is ported; see the module documentation.

/// `CodeBlockFragment(textElement:range:payload:context:role:lineCount:)`.
fn code_block_fragment(
    request: &ObjectFragmentRequest<'_>,
    role: CodeBlockRole,
    line_count: isize,
) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::code_block_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
        role,
        line_count,
    ))
}

/// `TableRowFragment.make(textElement:range:payload:context:)`, which may
/// itself decline (`nil`) and leave the paragraph as prose.
fn table_row_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    crate::fragments::table_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    )
}

/// `MathFragment(textElement:range:payload:context:)`.
fn math_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::math_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    ))
}

/// `MermaidFragment(textElement:range:payload:context:)`.
fn mermaid_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::mermaid_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    ))
}

/// `ImageFragment(textElement:range:payload:context:)`.
fn image_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::image_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    ))
}

/// `FrontMatterFragment(textElement:range:payload:context:fields:)`.
fn front_matter_fragment(
    request: &ObjectFragmentRequest<'_>,
    fields: Vec<(String, String)>,
) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::front_matter_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
        fields,
    ))
}

/// `ThematicBreakFragment(textElement:range:payload:context:)`.
fn thematic_break_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::thematic_break_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    ))
}

/// `CalloutFragment(textElement:range:payload:context:)`.
fn callout_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::callout_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    ))
}

/// `ListOrnamentFragment(textElement:range:payload:context:)`.
fn list_ornament_fragment(request: &ObjectFragmentRequest<'_>) -> Option<Retained<NSTextLayoutFragment>> {
    Some(crate::fragments::list_ornament_fragment::make(
        request.text_element,
        Some(request.element_range),
        request.payload,
        request.context,
    ))
}

/// Static geometry the view's hit testing borrows from object fragments.
/// Each returns `None` until its fragment is ported, which disables the hit
/// target it describes.
pub mod object_geometry {
    use objc2_core_foundation::{CGFloat, CGRect};

    use crate::theme::style_sheet::StyleSheet;

    /// `ListOrnamentFragment.taskHitRect(textEdge:centreY:bodySize:)`.
    pub fn task_hit_rect(text_edge: CGFloat, centre_y: CGFloat, body_size: CGFloat) -> Option<CGRect> {
        Some(crate::fragments::list_ornament_fragment::task_hit_rect(text_edge, centre_y, body_size))
    }

    /// `CodeBlockFragment.copyButtonRect(in:style:language:)`.
    pub fn code_copy_button_rect(band: CGRect, style: &StyleSheet, language: &str) -> Option<CGRect> {
        Some(crate::fragments::code_block_fragment::copy_button_rect(band, style, language))
    }
}
