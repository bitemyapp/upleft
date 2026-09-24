//! Port of `Engine/DecorationEngine.swift`: turns a `ParsedDocument` into
//! attributes on an `NSTextStorage`.
//!
//! **The invariant this whole file exists to hold: it never mutates a single
//! character** (§3.1). `decorate` calls `setAttributes`, `addAttributes` and
//! `addAttribute` and nothing else. Marker *hiding* is not this type's job —
//! it is a display-string substitution driven by `hidden_ranges` and
//! `DisplayMap`.
//!
//! Every framework call is the one the Swift makes, with the same arguments,
//! in the same order: attribute runs, including their boundaries, come out
//! identical. Attribute dictionaries that Swift rebuilds per call are built
//! once here where their contents cannot differ, which changes nothing
//! `NSTextStorage` can observe.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, Message, msg_send};
use objc2_app_kit::{
    NSColor, NSFont, NSFontDescriptorSymbolicTraits, NSFontWeightSemibold, NSMutableParagraphStyle,
    NSParagraphStyle, NSTextAlignment, NSTextStorage,
};
use objc2_foundation::{
    NSAttributedStringEnumerationOptions, NSDictionary, NSMutableDictionary, NSNumber, NSString,
    NSStringCompareOptions,
};
use upleft_core::safe_html::{SafeHTMLAlignment, SafeHTMLDocument, SafeHTMLKind};
use upleft_core::swift_text::{self, ns::NSStringExt};
use upleft_core::{BlockContent, DirtySet, InlineKind, InlineSpan, MDBlock, NSRange, NS_NOT_FOUND, ParsedDocument};

use super::block_style::{BaseAttributes, BlockContext, BlockStyleFactory, WritingDirection, frozen, mutable_copy};
use super::display_map::RangeSet;
use super::marker_policy::MarkerPolicy;
use super::render_metrics;
use super::syntax_run_cache::SyntaxRunCache;
use super::{from_ns_range, keys, ns_range};
use crate::render_contracts::{DecorationPolicy, FragmentKind, FragmentPayload, RenderMode, attribute_keys};
use crate::swift_value::{BlockIdentityValue, PathTokenValue};
use crate::syntax::builtin_syntax_highlighter::BuiltinSyntaxHighlighter;
use crate::syntax::syntax_contracts::{SyntaxHighlighter, SyntaxToken};
use crate::theme::style_sheet::StyleSheet;

/// What one `decorate` call did. Counts rather than ranges: the engine is on
/// the keystroke path (§12).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DecorationResult {
    /// Number of attribute applications performed.
    pub attribute_ranges: isize,
    pub fragment_count: isize,
    /// Seconds (`TimeInterval`).
    pub elapsed: f64,
}

/// One attribute application, with `range` relative to the block's start so
/// a cached program survives the block moving.
struct AttributeOp {
    range: NSRange,
    attributes: Retained<NSDictionary<NSString, AnyObject>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ProgramKey {
    hash: u64,
    kind: isize,
    list_depth: isize,
    quote_depth: isize,
    length: isize,
    /// A task label reserves the checkbox column, a bullet does not (§11.3).
    task: bool,
    /// A program recorded during a wholesale Source pass carries no font and
    /// no paragraph style; the two shapes must never share an entry.
    plain_typography: bool,
}

const PROGRAM_CACHE_CAPACITY: usize = 4096;

/// Style-sheet-derived attribute values the Swift recomputes per call. Their
/// contents are fixed for a style sheet, so they are built once.
struct EngineValues {
    yes: Retained<NSNumber>,
    underline_single: Retained<NSNumber>,
    completed_foreground: Retained<NSColor>,
    completed_strikethrough: Retained<NSColor>,
    link_underline: Retained<NSColor>,
    summary_background: Retained<NSColor>,
    table_header_background: Retained<NSColor>,
    table_cell_background: Retained<NSColor>,
    table_kern: Retained<NSNumber>,
    diff_added_background: Retained<NSColor>,
    diff_removed_background: Retained<NSColor>,
    callout_title_font: Retained<NSFont>,
}

impl EngineValues {
    fn new(sheet: &StyleSheet) -> EngineValues {
        EngineValues {
            yes: NSNumber::new_bool(true),
            underline_single: NSNumber::new_isize(1),
            completed_foreground: sheet.text.colorWithAlphaComponent(0.55),
            completed_strikethrough: sheet.text.colorWithAlphaComponent(0.40),
            link_underline: sheet.link.colorWithAlphaComponent(if sheet.increase_contrast { 0.75 } else { 0.35 }),
            summary_background: sheet.surface.colorWithAlphaComponent(0.34),
            table_header_background: sheet.surface.colorWithAlphaComponent(0.26),
            table_cell_background: sheet.surface.colorWithAlphaComponent(0.12),
            table_kern: NSNumber::new_f64(render_metrics::TABLE_COLUMN_GAP * 0.5),
            diff_added_background: sheet.code_color(SyntaxToken::DiffAdded).colorWithAlphaComponent(0.12),
            diff_removed_background: sheet.code_color(SyntaxToken::DiffRemoved).colorWithAlphaComponent(0.12),
            // SAFETY: AppKit exports the weight as an immutable global.
            callout_title_font: NSFont::systemFontOfSize_weight(sheet.body_font().pointSize(), unsafe {
                NSFontWeightSemibold
            }),
        }
    }
}

/// A base font, kept alive so its address stays its own, and a font
/// derived from it.
type FontPair = (Retained<NSFont>, Retained<NSFont>);

/// Fonts derived from other fonts, memoised. AppKit resolves the same font
/// for the same request every time; the cache only skips the descriptor work.
#[derive(Default)]
struct FontMemo {
    /// `(base font pointer, bold, italic)`; the value keeps the base alive so
    /// its address cannot be reused.
    emphasized: HashMap<(usize, bool, bool), FontPair>,
    /// `monoFont(size:)` by the size's bits.
    mono: HashMap<u64, Retained<NSFont>>,
    /// `withSize(_:)` by `(base pointer, size bits)`.
    resized: HashMap<(usize, u64), FontPair>,
}

struct DecorateState<'a> {
    document: &'a ParsedDocument,
    storage: &'a NSTextStorage,
    /// `storage.string`, the backing string, fetched once.
    text: Retained<NSString>,
    /// `storage.length`; decoration never changes it.
    storage_length: isize,
    target: NSRange,
    attribute_ranges: isize,
    fragment_count: isize,
    /// Whether `document.substring(_:)` hands out bridged strings.
    bridged: Option<bool>,
}

impl<'a> DecorateState<'a> {
    fn new(document: &'a ParsedDocument, storage: &'a NSTextStorage) -> DecorateState<'a> {
        let text = storage.string();
        let storage_length = text.length() as isize;
        DecorateState {
            document,
            storage,
            text,
            storage_length,
            target: NSRange::new(0, 0),
            attribute_ranges: 0,
            fragment_count: 0,
            bridged: None,
        }
    }

    fn bridged(&mut self) -> bool {
        if let Some(bridged) = self.bridged {
            return bridged;
        }
        let bridged = swift_text::bridges_substrings(&self.document.utf16);
        self.bridged = Some(bridged);
        bridged
    }

    #[inline]
    fn paragraph_range(&self, range: NSRange) -> NSRange {
        from_ns_range(self.text.paragraphRangeForRange(ns_range(range)))
    }
}

/// An attribute application's attributes: a ready dictionary, or key/value
/// pairs built into one only if the application survives its guards.
enum Attrs<'a> {
    Dict(&'a NSDictionary<NSString, AnyObject>),
    Pairs(&'a [(&'a NSString, &'a AnyObject)]),
}

/// Turns a `ParsedDocument` into attributes on an `NSTextStorage`.
pub struct DecorationEngine {
    style_sheet: StyleSheet,
    policy: DecorationPolicy,
    highlighter: Arc<dyn SyntaxHighlighter>,
    collapse_line_count: isize,
    styles: BlockStyleFactory,
    syntax_cache: SyntaxRunCache,
    /// Set for the duration of one wholesale Source-shaped `decorate` call
    /// whose `.font` and `.paragraphStyle` output is about to be overwritten.
    discards_typography: bool,
    program_cache: HashMap<ProgramKey, Rc<Vec<AttributeOp>>>,
    /// Keys seen exactly once: a block is only worth caching the *second*
    /// time it appears.
    program_seen: HashSet<ProgramKey>,
    cached_separator_style: Option<Retained<NSParagraphStyle>>,
    values: EngineValues,
    fonts: RefCell<FontMemo>,
    /// Hosted streaming (Upleft extension): a Mermaid or math fence the
    /// stream has not closed yet decorates as a plain code block.
    renders_open_fences_as_code: bool,
    /// Hosted views (Upleft extension) decorate every block live. A replayed
    /// program restyles only its block's own range, where the live path also
    /// restyles the physical paragraph around it (a list item's marker), so
    /// with the cache a block's attributes depend on how often it was
    /// decorated before; a streamed message must end up as if decorated once.
    uses_program_cache: bool,
    /// Hosted segments (Upleft extension): the text continues a document
    /// shown above it, so its first heading is not the document's first.
    continues_document: bool,
}

impl DecorationEngine {
    pub fn new(style_sheet: StyleSheet) -> DecorationEngine {
        DecorationEngine::with_highlighter(style_sheet, Arc::new(BuiltinSyntaxHighlighter::new()))
    }

    pub fn with_highlighter(style_sheet: StyleSheet, highlighter: Arc<dyn SyntaxHighlighter>) -> DecorationEngine {
        let styles = BlockStyleFactory::new(&style_sheet);
        let values = EngineValues::new(&style_sheet);
        DecorationEngine {
            style_sheet,
            policy: RenderMode::Read.policy(),
            highlighter,
            collapse_line_count: render_metrics::CODE_COLLAPSE_LINE_COUNT as isize,
            styles,
            syntax_cache: SyntaxRunCache::default(),
            discards_typography: false,
            program_cache: HashMap::new(),
            program_seen: HashSet::new(),
            cached_separator_style: None,
            values,
            fonts: RefCell::new(FontMemo::default()),
            renders_open_fences_as_code: false,
            uses_program_cache: true,
            continues_document: false,
        }
    }

    // MARK: Configuration

    pub fn style_sheet(&self) -> &StyleSheet {
        &self.style_sheet
    }

    /// Assigning always rebuilds the derived styles and drops the cache.
    pub fn set_style_sheet(&mut self, style_sheet: StyleSheet) {
        self.style_sheet = style_sheet;
        self.styles = BlockStyleFactory::new(&self.style_sheet);
        self.values = EngineValues::new(&self.style_sheet);
        *self.fonts.borrow_mut() = FontMemo::default();
        self.cached_separator_style = None;
        self.program_cache.clear();
        self.program_seen.clear();
        self.syntax_cache.remove_all();
    }

    pub fn policy(&self) -> DecorationPolicy {
        self.policy
    }

    pub fn set_policy(&mut self, policy: DecorationPolicy) {
        let old = self.policy;
        self.policy = policy;
        if policy == old {
            return;
        }
        self.program_cache.clear();
        self.program_seen.clear();
    }

    pub fn code_collapse_line_count(&self) -> isize {
        self.collapse_line_count
    }

    pub fn set_code_collapse_line_count(&mut self, value: isize) {
        self.collapse_line_count = 10_000.min(1.max(value));
    }

    /// See `uses_program_cache`.
    pub fn set_uses_program_cache(&mut self, enabled: bool) {
        self.uses_program_cache = enabled;
        if !enabled {
            self.program_cache.clear();
            self.program_seen.clear();
        }
    }

    /// See `MarkdownTextView::set_streaming`.
    pub fn set_renders_open_fences_as_code(&mut self, enabled: bool) {
        self.renders_open_fences_as_code = enabled;
    }

    /// Hosted segments (Upleft extension): see `continues_document`.
    pub fn set_continues_document(&mut self, continues: bool) {
        self.continues_document = continues;
    }

    /// An open Mermaid or math fence at the end of a streaming message, as
    /// the code block it will be decorated as until it closes.
    fn open_fence_as_code(&self, block: &MDBlock, document: &ParsedDocument) -> Option<MDBlock> {
        if !self.renders_open_fences_as_code || !is_open_fence_at_end(block, document) {
            return None;
        }
        let (language, content_range) = match block.content {
            BlockContent::Mermaid { source_range } => ("mermaid", source_range),
            BlockContent::MathBlock { latex_range } => ("math", latex_range),
            _ => return None,
        };
        let mut code = block.clone();
        code.content = BlockContent::CodeBlock { language: Some(language.to_owned()), is_fenced: true, content_range };
        Some(code)
    }

    /// The policy shape Source mode uses: markers styled in place rather than
    /// hidden, and nothing rendered as an object.
    fn policy_is_source_shaped(&self) -> bool {
        !self.policy.renders_fragments && !self.policy.hides_block_markers && !self.policy.hides_inline_markers
    }

    // MARK: - Decorate

    /// The ranges `decorate` will actually rewrite, which are wider than the
    /// dirty set: each range grows to whole blocks.
    pub fn decorated_bounds(&self, dirty: &DirtySet, document: &ParsedDocument, length: isize) -> Vec<NSRange> {
        let full = NSRange::new(0, length);
        if full.length <= 0 {
            return Vec::new();
        }
        if dirty.is_wholesale {
            return vec![full];
        }
        let grown: Vec<NSRange> = dirty
            .ranges
            .iter()
            .filter_map(|range| clip(block_bounds(*range, document), full))
            .collect();
        RangeSet::normalized(&grown)
    }

    /// Applies attributes for the given source ranges. `dirty.is_wholesale`
    /// means redecorate everything.
    pub fn decorate(&mut self, storage: &NSTextStorage, document: &ParsedDocument, dirty: &DirtySet) -> DecorationResult {
        let started = Instant::now();
        let elapsed = |started: Instant| started.elapsed().as_secs_f64();
        let full = NSRange::new(0, storage.length() as isize);
        if full.length <= 0 {
            return DecorationResult { elapsed: elapsed(started), ..Default::default() };
        }

        // Settle the document's direction before any paragraph style is built.
        self.styles.set_base_writing_direction(WritingDirection::of(&document.text, 1024));

        let targets: Vec<NSRange> = if dirty.is_wholesale {
            vec![full]
        } else if dirty.ranges.is_empty() {
            return DecorationResult { elapsed: elapsed(started), ..Default::default() };
        } else {
            // Grow each dirty range to whole blocks: a paragraph's style is a
            // property of the paragraph, not of the characters that changed.
            let grown: Vec<NSRange> = dirty
                .ranges
                .iter()
                .filter_map(|range| clip(block_bounds(*range, document), full))
                .collect();
            RangeSet::normalized(&grown)
        };
        if targets.is_empty() {
            return DecorationResult { elapsed: elapsed(started), ..Default::default() };
        }

        // Only a *wholesale* Source pass earns the shortcut.
        self.discards_typography = dirty.is_wholesale && self.policy_is_source_shaped();

        let mut state = DecorateState::new(document, storage);
        let document_base = self.document_base_attributes();

        storage.beginEditing();
        for target in targets {
            // Wipe first so no attribute from a previous parse survives.
            // SAFETY: the dictionary maps attribute keys to attribute values.
            unsafe { storage.setAttributes_range(Some(&document_base.all), ns_range(target)) };
            state.attribute_ranges += 1;
            state.target = target;
            let children = &document.root.children;
            for child_index in intersecting_child_indices(children, state.target) {
                self.walk(&children[child_index], BlockContext::ROOT, &mut state);
            }
            self.collapse_separators(&mut state);
        }
        storage.endEditing();
        self.discards_typography = false;

        if self.program_cache.len() > PROGRAM_CACHE_CAPACITY {
            self.program_cache.clear();
            self.program_seen.clear();
        }

        DecorationResult {
            attribute_ranges: state.attribute_ranges,
            fragment_count: state.fragment_count,
            elapsed: elapsed(started),
        }
    }

    // MARK: - Block separators

    /// Blank lines *between* blocks collapse to a hairline: inter-block air is
    /// `paragraphSpacing`'s job. Source mode is exempt (§3.2).
    fn collapse_separators(&mut self, state: &mut DecorateState) {
        if !self.policy.hides_block_markers {
            return;
        }
        let separator = self.separator_paragraph_style();
        let document = state.document;
        let text = &document.utf16;
        let length = text.len() as isize;
        let line_starts = &document.line_starts;

        // Work line by line: whole blank lines are unambiguous.
        for index in line_indices(state.target, line_starts) {
            let line_start = line_starts[index];
            let next_start = if index + 1 < line_starts.len() { line_starts[index + 1] } else { length };
            if next_start <= line_start {
                continue;
            }

            // Content of the line, terminator excluded.
            let mut content_end = next_start;
            while content_end > line_start {
                let ch = text[(content_end - 1) as usize];
                if !(ch == 0x0A || ch == 0x0D) {
                    break;
                }
                content_end -= 1;
            }
            if content_end > line_start {
                continue;
            }

            // Blank lines inside a fence, a math block, front matter or a table
            // are content.
            if let Some(block) = document.root.block_at(line_start) {
                match block.content {
                    BlockContent::CodeBlock { .. }
                    | BlockContent::Mermaid { .. }
                    | BlockContent::MathBlock { .. }
                    | BlockContent::FrontMatter(_)
                    | BlockContent::Table(_)
                    | BlockContent::HtmlBlock => continue,
                    _ => {}
                }
            }

            let line_range = NSRange::new(line_start, next_start - line_start);
            let Some(clipped) = clip(line_range, state.target) else { continue };
            if clipped.length <= 0 {
                continue;
            }
            // SAFETY: a paragraph style for the paragraph-style key.
            unsafe {
                state
                    .storage
                    .addAttribute_value_range(keys::paragraph_style(), separator.as_ref(), ns_range(clipped));
            }
            state.attribute_ranges += 1;
        }
    }

    fn separator_paragraph_style(&mut self) -> Retained<NSParagraphStyle> {
        if let Some(cached) = &self.cached_separator_style {
            return cached.clone();
        }
        let style = NSMutableParagraphStyle::new();
        style.setMinimumLineHeight(1.0);
        style.setMaximumLineHeight(1.0);
        style.setLineSpacing(0.0);
        style.setParagraphSpacing(0.0);
        style.setParagraphSpacingBefore(0.0);
        let result = frozen(&style);
        self.cached_separator_style = Some(result.clone());
        result
    }

    /// Base attributes for text that belongs to no block.
    fn document_base_attributes(&mut self) -> Rc<BaseAttributes> {
        let zero = NSRange::new(0, 0);
        self.styles
            .base_attributes(&MDBlock::new(BlockContent::Paragraph, zero, zero), BlockContext::ROOT)
    }

    // MARK: - Walk

    fn walk(&mut self, block: &MDBlock, context: BlockContext, state: &mut DecorateState) {
        if !(block.range.location < state.target.upper_bound() && state.target.location < block.range.upper_bound()) {
            return;
        }

        let mut child_context = context;
        match &block.content {
            BlockContent::Document => {
                for child_index in intersecting_child_indices(&block.children, state.target) {
                    self.walk(&block.children[child_index], context, state);
                }
            }

            BlockContent::List { .. } => {
                child_context.list_depth += 1;
                for child_index in intersecting_child_indices(&block.children, state.target) {
                    self.walk(&block.children[child_index], child_context, state);
                }
                self.apply_trailing_list_spacing(block, state);
            }

            BlockContent::BlockQuote => {
                self.emit_fragment(FragmentKind::Callout, block, "", state);
                child_context.quote_depth += 1;
                child_context.callout_kind = None;
                self.apply_base(block, context, state);
                for child_index in intersecting_child_indices(&block.children, state.target) {
                    self.walk(&block.children[child_index], child_context, state);
                }
            }

            BlockContent::Callout { kind, title } => {
                let detail = format!("{}|{}", kind.raw_value(), title.as_deref().unwrap_or(""));
                self.emit_fragment(FragmentKind::Callout, block, &detail, state);
                child_context.quote_depth += 1;
                child_context.callout_kind = Some(*kind);
                self.apply_base(block, context, state);
                for child_index in intersecting_child_indices(&block.children, state.target) {
                    self.walk(&block.children[child_index], child_context, state);
                }
                if let Some(title) = title
                    && !title.is_empty()
                {
                    let units = &state.document.utf16;
                    let source: &[u16] = if block.range.location >= 0 && block.range.upper_bound() <= state.document.length {
                        &units[block.range.as_usize_range()]
                    } else {
                        &[]
                    };
                    let source = swift_text::ns::foundation::ns_from_utf16(source);
                    let bracket = source.rangeOfString(&NSString::from_str("]"));
                    let search_start = if bracket.location as isize != NS_NOT_FOUND { bracket.location + 1 } else { 0 };
                    let search_range = objc2_foundation::NSRange::new(search_start, source.length() - search_start);
                    let found = source.rangeOfString_options_range(
                        &NSString::from_str(title),
                        NSStringCompareOptions::empty(),
                        search_range,
                    );
                    if found.location as isize != NS_NOT_FOUND {
                        let color = self.style_sheet.callout_color(*kind);
                        self.apply(
                            Attrs::Pairs(&[
                                (keys::font(), self.values.callout_title_font.as_ref()),
                                (keys::foreground_color(), color.as_ref()),
                            ]),
                            NSRange::new(block.range.location + found.location as isize, found.length as isize),
                            state,
                        );
                    }
                }
            }

            BlockContent::ListItem { ordinal, checkbox } => {
                child_context.ordinal = *ordinal;
                // A task's own text is a child paragraph block; it needs the
                // task marker column (§11.3).
                child_context.task = checkbox.is_some();
                self.apply_base(block, child_context, state);
                self.apply_block_marker(block, child_context, state);
                let ornament = if let Some(checkbox) = checkbox {
                    if checkbox.is_checked { "task:checked".to_owned() } else { "task:unchecked".to_owned() }
                } else if let Some(ordinal) = ordinal {
                    format!("ordered:{ordinal}")
                } else {
                    format!("unordered:{}", 1.max(context.list_depth))
                };
                self.emit_fragment(FragmentKind::ListOrnament, block, &ornament, state);
                if let Some(checkbox) = checkbox {
                    let flag = NSNumber::new_bool(checkbox.is_checked);
                    self.apply(Attrs::Pairs(&[(attribute_keys::dr_checkbox(), flag.as_ref())]), checkbox.mark_range, state);
                }
                self.apply_inlines_if_leaf(block, context, state);
                for child_index in intersecting_child_indices(&block.children, state.target) {
                    self.walk(&block.children[child_index], child_context, state);
                }
                if checkbox.is_some_and(|checkbox| checkbox.is_checked) {
                    let completed = [
                        (keys::foreground_color(), self.values.completed_foreground.as_ref() as &AnyObject),
                        (keys::strikethrough_style(), self.values.underline_single.as_ref()),
                        (keys::strikethrough_color(), self.values.completed_strikethrough.as_ref()),
                    ];
                    // A completed task strikes out *its own* text and stops.
                    if block.children.is_empty() {
                        self.apply(Attrs::Pairs(&completed), block.content_range, state);
                    } else {
                        for child in &block.children {
                            if !child.content.is_list() {
                                self.apply(Attrs::Pairs(&completed), child.range, state);
                            }
                        }
                    }
                }
            }

            BlockContent::CodeBlock { language, content_range, .. } => {
                self.decorate_code_block(block, language.as_deref(), *content_range, context, state);
            }

            BlockContent::Mermaid { .. } | BlockContent::MathBlock { .. }
                if let Some(code) = self.open_fence_as_code(block, state.document) =>
            {
                let BlockContent::CodeBlock { language, content_range, .. } = &code.content else { unreachable!() };
                self.decorate_code_block(&code, language.as_deref(), *content_range, context, state);
            }

            BlockContent::Mermaid { source_range } => {
                self.apply_base(block, context, state);
                let detail = state.document.substring(*source_range);
                self.emit_fragment(FragmentKind::Mermaid, block, &detail, state);
            }

            BlockContent::MathBlock { latex_range } => {
                self.apply_base(block, context, state);
                let detail = state.document.substring(*latex_range);
                self.emit_fragment(FragmentKind::BlockMath, block, &detail, state);
            }

            BlockContent::Table(data) => {
                self.apply_base(block, context, state);
                let payload = self.emit_fragment(FragmentKind::Table, block, "", state);
                if let Some(payload) = payload {
                    payload.set_table_data(Some(data.clone()));
                }
                let block_font = self.styles.font(&block.content);
                for row in &data.rows {
                    for cell in &row.cells {
                        self.apply_inlines(&cell.inlines, context, &block_font, false, false, state);
                    }
                }
            }

            BlockContent::ThematicBreak => {
                self.apply_base(block, context, state);
                self.emit_fragment(FragmentKind::ThematicBreak, block, "", state);
            }

            BlockContent::FrontMatter(matter) => {
                self.apply_base(block, context, state);
                let detail = matter.fields.first().map_or("", |field| field.key.as_str());
                self.emit_fragment(FragmentKind::FrontMatter, block, detail, state);
                let accent = self.style_sheet.accent.clone();
                let text = self.style_sheet.text.clone();
                for field in &matter.fields {
                    self.apply(Attrs::Pairs(&[(keys::foreground_color(), accent.as_ref())]), field.key_range, state);
                    self.apply(Attrs::Pairs(&[(keys::foreground_color(), text.as_ref())]), field.value_range, state);
                }
            }

            BlockContent::Heading { .. }
            | BlockContent::Paragraph
            | BlockContent::HtmlBlock
            | BlockContent::FootnoteDefinition { .. } => {
                if let Some(html) = &block.safe_html
                    && html.is_safe
                {
                    self.apply_base(block, context, state);
                    self.apply_safe_html(html, block, state);
                } else if let Some(program) = self.cached_program(block, context) {
                    self.apply_program(&program, block.range.location, state);
                    let attributes = self.styles.base_attributes(block, context);
                    self.apply_first_heading_spacing(block, &attributes.paragraph_style, state);
                    self.apply_hard_wrap_continuation_spacing(block, &attributes.paragraph_style, state);
                } else {
                    self.apply_base(block, context, state);
                    self.apply_block_marker(block, context, state);
                    self.apply_inlines_if_leaf(block, context, state);
                }
                self.apply_block_identity(block, state);
                self.emit_inline_image_fragment_if_solitary(block, state);
                for child_index in intersecting_child_indices(&block.children, state.target) {
                    self.walk(&block.children[child_index], context, state);
                }
            }
        }
    }

    // MARK: - Attribute application

    fn apply_trailing_list_spacing(&mut self, list: &MDBlock, state: &mut DecorateState) {
        if self.discards_typography {
            return;
        }
        let Some(last) = list.children.last() else { return };
        if last.range.length <= 0 {
            return;
        }
        let length = state.storage_length;
        let offset = (length - 1).min(last.range.location.max(last.range.upper_bound() - 1));
        if offset < 0 {
            return;
        }
        let paragraph = state.paragraph_range(NSRange::new(offset, 0));
        if !(paragraph.location < state.target.upper_bound() && state.target.location < paragraph.upper_bound()) {
            return;
        }
        // SAFETY: reading an attribute value; the effective range is not asked for.
        let existing = unsafe {
            state
                .storage
                .attribute_atIndex_effectiveRange(keys::paragraph_style(), offset as usize, std::ptr::null_mut())
        };
        let Some(existing) = existing.and_then(|value| value.downcast::<NSParagraphStyle>().ok()) else { return };
        let spaced = mutable_copy(&existing);
        spaced.setParagraphSpacing(render_metrics::snap_up(
            self.style_sheet.line_height * 0.45,
            1f64.max(self.style_sheet.baseline_grid),
        ));
        let spaced = frozen(&spaced);
        self.apply(Attrs::Pairs(&[(keys::paragraph_style(), spaced.as_ref())]), paragraph, state);
    }

    fn apply(&self, attributes: Attrs, range: NSRange, state: &mut DecorateState) {
        let Some(clipped) = clip(range, state.target) else { return };
        if clipped.length <= 0 {
            return;
        }
        if clipped.upper_bound() > state.storage_length {
            return;
        }
        if self.discards_typography {
            // The one place the Source shortcut is enforced. An operation that
            // carried nothing but a font or a paragraph style disappears
            // entirely rather than splitting an attribute run for nothing.
            let surviving: Retained<NSDictionary<NSString, AnyObject>> = match attributes {
                Attrs::Pairs(pairs) => {
                    let kept: Vec<&(&NSString, &AnyObject)> = pairs
                        .iter()
                        .filter(|(key, _)| !is_typography_key(key))
                        .collect();
                    if kept.is_empty() {
                        return;
                    }
                    let keys_: Vec<&NSString> = kept.iter().map(|(key, _)| *key).collect();
                    let values: Vec<&AnyObject> = kept.iter().map(|(_, value)| *value).collect();
                    NSDictionary::from_slices(&keys_, &values)
                }
                Attrs::Dict(dictionary) => {
                    // SAFETY: `mutableCopy` of an attribute dictionary.
                    let copy: Retained<NSMutableDictionary<NSString, AnyObject>> =
                        unsafe { msg_send![dictionary, mutableCopy] };
                    copy.removeObjectForKey(keys::font());
                    copy.removeObjectForKey(keys::paragraph_style());
                    if copy.count() == 0 {
                        return;
                    }
                    Retained::into_super(copy)
                }
            };
            // SAFETY: attribute keys to attribute values.
            unsafe { state.storage.addAttributes_range(&surviving, ns_range(clipped)) };
            state.attribute_ranges += 1;
            return;
        }
        match attributes {
            Attrs::Dict(dictionary) => {
                // SAFETY: attribute keys to attribute values.
                unsafe { state.storage.addAttributes_range(dictionary, ns_range(clipped)) };
            }
            Attrs::Pairs(pairs) => {
                let dictionary = pairs_dictionary(pairs);
                // SAFETY: attribute keys to attribute values.
                unsafe { state.storage.addAttributes_range(&dictionary, ns_range(clipped)) };
            }
        }
        state.attribute_ranges += 1;
    }

    fn apply_block_identity(&self, block: &MDBlock, state: &mut DecorateState) {
        let identity = BlockIdentityValue::new(block.identity);
        self.apply(Attrs::Pairs(&[(attribute_keys::dr_block(), identity.as_ref())]), block.range, state);
    }

    fn apply_base(&mut self, block: &MDBlock, context: BlockContext, state: &mut DecorateState) {
        let attributes = self.styles.base_attributes(block, context);
        let mut paragraph_style = attributes.paragraph_style.clone();
        if !self.discards_typography
            && self.style_sheet.theme.typography.optical_margins
            && context.list_depth == 0
            && matches!(block.content, BlockContent::Paragraph)
            && opens_with_quotation_mark(&state.document.substring(block.content_range))
        {
            let paragraph = mutable_copy(&paragraph_style);
            paragraph.setFirstLineHeadIndent(
                paragraph.firstLineHeadIndent() - self.styles.font(&block.content).pointSize() * 0.34,
            );
            paragraph_style = frozen(&paragraph);
        }

        // A paragraph style must reach only paragraphs that *begin* inside this
        // block's own content (the nested-task indentation bug). The split also
        // keeps the base font/colour over the whole block.
        self.apply(Attrs::Dict(&attributes.without_paragraph), block.range, state);
        self.apply_own_paragraph_style(&paragraph_style, block, state);
        self.apply_block_identity(block, state);

        self.apply_first_heading_spacing(block, &paragraph_style, state);
        self.apply_hard_wrap_continuation_spacing(block, &paragraph_style, state);
    }

    /// Applies `style` paragraph by paragraph across the block, skipping any
    /// paragraph that *begins* inside a child block.
    fn apply_own_paragraph_style(&self, style: &NSParagraphStyle, block: &MDBlock, state: &mut DecorateState) {
        if self.discards_typography {
            return;
        }
        let length = state.storage_length;
        let children = &block.children;
        let mut cursor = block.range.location;
        let end = block.range.upper_bound();
        while cursor < end && cursor < length {
            let paragraph = state.paragraph_range(NSRange::new(cursor, 0));
            if paragraph.length <= 0 {
                break;
            }
            let owned_by_child = children
                .iter()
                .any(|child| child.range.length > 0 && child.range.contains(paragraph.location));
            if !owned_by_child
                && let Some(clipped) = clip(paragraph, state.target)
                && clipped.length > 0
            {
                self.apply(Attrs::Pairs(&[(keys::paragraph_style(), style.as_ref())]), paragraph, state);
            }
            if paragraph.upper_bound() <= cursor {
                break;
            }
            cursor = paragraph.upper_bound();
        }
    }

    fn apply_first_heading_spacing(&self, block: &MDBlock, original: &NSParagraphStyle, state: &mut DecorateState) {
        if self.discards_typography {
            return;
        }
        if !matches!(block.content, BlockContent::Heading { .. }) {
            return;
        }
        let is_first = !self.continues_document
            && state
                .document
                .headings
                .first()
                .is_some_and(|heading| heading.range.location == block.range.location);
        // A heading that follows another heading has no prose to be
        // separated from; the full gap there reads as a missing section.
        let follows = follows_another_heading(block, state);
        if !(is_first || follows) {
            return;
        }
        let paragraph = mutable_copy(original);
        paragraph.setParagraphSpacingBefore(if is_first {
            0.0
        } else {
            (original.paragraphSpacingBefore() * 0.4).round()
        });
        let paragraph = frozen(&paragraph);
        self.apply(Attrs::Pairs(&[(keys::paragraph_style(), paragraph.as_ref())]), block.range, state);
    }

    /// Air belongs to the *logical* paragraph, not to the source lines it
    /// happens to be typed across.
    fn apply_hard_wrap_continuation_spacing(&self, block: &MDBlock, original: &NSParagraphStyle, state: &mut DecorateState) {
        if self.discards_typography {
            return;
        }
        if !matches!(block.content, BlockContent::Paragraph) {
            return;
        }
        if !content_contains_newline(block.content_range, state) {
            return;
        }
        let lines = physical_paragraphs(block, state);
        if lines.len() <= 1 {
            return;
        }
        let count = lines.len();
        for (index, line) in lines.into_iter().enumerate() {
            let paragraph = mutable_copy(original);
            if index > 0 {
                paragraph.setParagraphSpacingBefore(0.0);
            }
            if index < count - 1 {
                paragraph.setParagraphSpacing(0.0);
            }
            let paragraph = frozen(&paragraph);
            self.apply(Attrs::Pairs(&[(keys::paragraph_style(), paragraph.as_ref())]), line, state);
        }
    }

    /// Hangs every code row's wraps off that row's own indentation.
    fn apply_code_row_indents(&mut self, block: &MDBlock, context: BlockContext, state: &mut DecorateState) {
        if self.discards_typography {
            return;
        }
        let base = self.styles.paragraph_style(block, context);
        for line in physical_paragraphs(block, state) {
            let columns = indent_columns_in(&state.text, line);
            if columns <= 0 {
                continue;
            }
            let style = self.styles.code_row_style(&base, columns);
            self.apply(Attrs::Pairs(&[(keys::paragraph_style(), style.as_ref())]), line, state);
        }
    }

    /// Block markers are attributed but never revealed inline (§6.1a); the
    /// gutter rail reads `drGutterMarker`.
    fn apply_block_marker(&self, block: &MDBlock, context: BlockContext, state: &mut DecorateState) {
        let Some(marker) = block.marker_range else { return };
        if marker.length <= 0 {
            return;
        }
        let dimmed = !self.policy.highlights_markers;
        if let Some(text) = DecorationEngine::gutter_text(block, context) {
            let base = self.styles.marker_attributes(dimmed);
            let text = NSString::from_str(&text);
            // `attrs[.drGutterMarker] = text` over the marker attributes.
            let flag = base.objectForKey(attribute_keys::dr_marker()).expect("marker attributes");
            let color = base.objectForKey(keys::foreground_color()).expect("marker attributes");
            self.apply(
                Attrs::Pairs(&[
                    (attribute_keys::dr_marker(), &*flag),
                    (keys::foreground_color(), &*color),
                    (attribute_keys::dr_gutter_marker(), text.as_ref()),
                ]),
                marker,
                state,
            );
        } else {
            self.apply(Attrs::Dict(self.styles.marker_attributes(dimmed)), marker, state);
        }
        if let Some(trailing) = block.trailing_marker_range
            && trailing.length > 0
        {
            self.apply(Attrs::Dict(self.styles.marker_attributes(dimmed)), trailing, state);
        }
    }

    fn apply_inlines_if_leaf(&mut self, block: &MDBlock, context: BlockContext, state: &mut DecorateState) {
        if block.inlines.is_empty() {
            return;
        }
        let block_font = self.styles.font(&block.content);
        self.apply_inlines(&block.inlines, context, &block_font, false, false, state);
    }

    #[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
    fn apply_inlines(
        &self,
        spans: &[InlineSpan],
        context: BlockContext,
        block_font: &NSFont,
        bold: bool,
        italic: bool,
        state: &mut DecorateState,
    ) {
        for span in spans {
            if !(span.range.location < state.target.upper_bound() && state.target.location < span.range.upper_bound()) {
                continue;
            }
            let mut bold = bold;
            let mut italic = italic;

            match &span.kind {
                InlineKind::Strong => {
                    // The trait still has to propagate into the children even
                    // when the face itself is about to be overwritten.
                    bold = true;
                    if !self.discards_typography {
                        let font = self.emphasized(block_font, true, italic);
                        self.apply(Attrs::Pairs(&[(keys::font(), font.as_ref())]), span.content_range, state);
                    }
                }
                InlineKind::Emphasis => {
                    italic = true;
                    if !self.discards_typography {
                        let font = self.emphasized(block_font, bold, true);
                        self.apply(Attrs::Pairs(&[(keys::font(), font.as_ref())]), span.content_range, state);
                    }
                }
                InlineKind::Strikethrough => {
                    let faint = &self.style_sheet.text_faint;
                    self.apply(
                        Attrs::Pairs(&[
                            (keys::strikethrough_style(), self.values.underline_single.as_ref()),
                            (keys::strikethrough_color(), faint.as_ref()),
                            (keys::foreground_color(), faint.as_ref()),
                        ]),
                        span.content_range,
                        state,
                    );
                }
                InlineKind::InlineCode => {
                    // Resolving a resized mono face goes through the font
                    // descriptor, so it is not asked for when about to be
                    // replaced.
                    if self.discards_typography {
                        self.apply(
                            Attrs::Pairs(&[
                                (keys::foreground_color(), self.style_sheet.text.as_ref()),
                                (attribute_keys::dr_inline_code(), self.values.yes.as_ref()),
                            ]),
                            span.content_range,
                            state,
                        );
                    } else {
                        let font = self.mono_font(block_font.pointSize() * 0.94);
                        self.apply(
                            Attrs::Pairs(&[
                                (keys::foreground_color(), self.style_sheet.text.as_ref()),
                                (attribute_keys::dr_inline_code(), self.values.yes.as_ref()),
                                (keys::font(), font.as_ref()),
                            ]),
                            span.content_range,
                            state,
                        );
                    }
                }
                InlineKind::Link { destination, .. } => self.apply_link(destination, span, state),
                InlineKind::Autolink { destination } => self.apply_link(destination, span, state),
                InlineKind::Wikilink { target, .. } => self.apply_link(target, span, state),
                InlineKind::Image { source, .. } => {
                    let source = NSString::from_str(source);
                    self.apply(
                        Attrs::Pairs(&[
                            (attribute_keys::dr_link(), source.as_ref()),
                            (keys::foreground_color(), self.style_sheet.text_secondary.as_ref()),
                        ]),
                        span.content_range,
                        state,
                    );
                }
                InlineKind::InlineMath { latex_range } => {
                    let payload = FragmentPayload::new(
                        FragmentKind::InlineMath,
                        span.range,
                        upleft_core::BlockIdentity::new(9, span.range.location),
                        &state.document.substring(*latex_range),
                    );
                    self.apply(Attrs::Pairs(&[(attribute_keys::dr_fragment(), payload.as_ref())]), span.range, state);
                    state.fragment_count += 1;
                }
                InlineKind::PathToken(token) => {
                    // §8.4: the engine marks it; `drPathExists` starts
                    // optimistic and the view refines it.
                    let token = PathTokenValue::new(token.clone());
                    let yes = self.values.yes.as_ref();
                    if self.discards_typography {
                        self.apply(
                            Attrs::Pairs(&[
                                (attribute_keys::dr_path_token(), token.as_ref()),
                                (attribute_keys::dr_path_exists(), yes),
                                (attribute_keys::dr_inline_code(), yes),
                                (keys::foreground_color(), self.style_sheet.text_secondary.as_ref()),
                            ]),
                            span.range,
                            state,
                        );
                    } else {
                        let font = self.mono_font(block_font.pointSize() * 0.94);
                        self.apply(
                            Attrs::Pairs(&[
                                (attribute_keys::dr_path_token(), token.as_ref()),
                                (attribute_keys::dr_path_exists(), yes),
                                (attribute_keys::dr_inline_code(), yes),
                                (keys::foreground_color(), self.style_sheet.text_secondary.as_ref()),
                                (keys::font(), font.as_ref()),
                            ]),
                            span.range,
                            state,
                        );
                    }
                }
                InlineKind::FootnoteReference { identifier } => {
                    // The raised baseline survives into Source — only the
                    // smaller face is overwritten.
                    let identifier = NSString::from_str(identifier);
                    let offset = NSNumber::new_f64(block_font.xHeight() * 0.42);
                    if self.discards_typography {
                        self.apply(
                            Attrs::Pairs(&[
                                (attribute_keys::dr_reference(), identifier.as_ref()),
                                (keys::foreground_color(), self.style_sheet.accent.as_ref()),
                                (keys::baseline_offset(), offset.as_ref()),
                            ]),
                            span.range,
                            state,
                        );
                    } else {
                        let font = self.resized(block_font, block_font.pointSize() * 0.62);
                        self.apply(
                            Attrs::Pairs(&[
                                (attribute_keys::dr_reference(), identifier.as_ref()),
                                (keys::foreground_color(), self.style_sheet.accent.as_ref()),
                                (keys::baseline_offset(), offset.as_ref()),
                                (keys::font(), font.as_ref()),
                            ]),
                            span.range,
                            state,
                        );
                    }
                }
                InlineKind::InlineHTML => {
                    self.apply(
                        Attrs::Pairs(&[(keys::foreground_color(), self.style_sheet.text_faint.as_ref())]),
                        span.range,
                        state,
                    );
                }
                InlineKind::Text | InlineKind::SoftBreak | InlineKind::LineBreak => {}
            }

            let dimmed = !self.policy.highlights_markers;
            for marker in [span.leading_marker_range, span.trailing_marker_range].into_iter().flatten() {
                if marker.length > 0 {
                    self.apply(Attrs::Dict(self.styles.marker_attributes(dimmed)), marker, state);
                }
            }
            if !span.children.is_empty() {
                self.apply_inlines(&span.children, context, block_font, bold, italic, state);
            }
        }
    }

    fn apply_link(&self, destination: &str, span: &InlineSpan, state: &mut DecorateState) {
        // A link has to be findable without colour (WCAG 1.4.1).
        let destination = NSString::from_str(destination);
        self.apply(
            Attrs::Pairs(&[
                (attribute_keys::dr_link(), destination.as_ref()),
                (keys::link(), destination.as_ref()),
                (keys::foreground_color(), self.style_sheet.link.as_ref()),
                (keys::underline_style(), self.values.underline_single.as_ref()),
                (keys::underline_color(), self.values.link_underline.as_ref()),
            ]),
            if span.content_range.length > 0 { span.content_range } else { span.range },
            state,
        );
    }

    /// Applies a conservative native presentation to README-style HTML while
    /// leaving every source character in storage.
    fn apply_safe_html(&self, html: &SafeHTMLDocument, block: &MDBlock, state: &mut DecorateState) {
        let body_font = self.style_sheet.body_font();
        for annotation in &html.annotations {
            for tag_range in &annotation.tag_ranges {
                self.apply(Attrs::Dict(self.styles.marker_attributes(true)), *tag_range, state);
            }
            match &annotation.kind {
                SafeHTMLKind::Paragraph { align } => {
                    let Some(alignment) = align else { continue };
                    if self.discards_typography || annotation.content_range.length <= 0 {
                        continue;
                    }
                    let paragraph = paragraph_with_alignment(state, annotation.content_range.location, *alignment);
                    let physical = state.paragraph_range(annotation.range);
                    let paragraph = frozen(&paragraph);
                    self.apply(Attrs::Pairs(&[(keys::paragraph_style(), paragraph.as_ref())]), physical, state);
                }
                SafeHTMLKind::Heading { level } => {
                    let font = self.style_sheet.heading_font(*level as i64);
                    let color = self.style_sheet.heading_color(*level as i64);
                    let level = NSNumber::new_isize(*level);
                    self.apply(
                        Attrs::Pairs(&[
                            (keys::font(), font.as_ref()),
                            (keys::foreground_color(), color.as_ref()),
                            (attribute_keys::dr_heading(), level.as_ref()),
                        ]),
                        annotation.content_range,
                        state,
                    );
                }
                SafeHTMLKind::Strong => {
                    let font = self.emphasized(&body_font, true, false);
                    self.apply(Attrs::Pairs(&[(keys::font(), font.as_ref())]), annotation.content_range, state);
                }
                SafeHTMLKind::Emphasis => {
                    let font = self.emphasized(&body_font, false, true);
                    self.apply(Attrs::Pairs(&[(keys::font(), font.as_ref())]), annotation.content_range, state);
                }
                SafeHTMLKind::Link { destination, .. } => {
                    let destination = NSString::from_str(destination);
                    self.apply(
                        Attrs::Pairs(&[
                            (attribute_keys::dr_link(), destination.as_ref()),
                            (keys::link(), destination.as_ref()),
                            (keys::foreground_color(), self.style_sheet.link.as_ref()),
                            (keys::underline_style(), self.values.underline_single.as_ref()),
                        ]),
                        annotation.content_range,
                        state,
                    );
                }
                SafeHTMLKind::Image { source, alt } => {
                    let payload = FragmentPayload::new(FragmentKind::Image, annotation.range, block.identity, source);
                    let source = NSString::from_str(source);
                    let alt = NSString::from_str(alt);
                    self.apply(
                        Attrs::Pairs(&[
                            (attribute_keys::dr_fragment(), payload.as_ref()),
                            (attribute_keys::dr_link(), source.as_ref()),
                            (attribute_keys::dr_reference(), alt.as_ref()),
                        ]),
                        annotation.range,
                        state,
                    );
                    state.fragment_count += 1;
                }
                SafeHTMLKind::Summary => {
                    // A stable semantic anchor without inserting a character.
                    let font = self.emphasized(&body_font, true, false);
                    self.apply(
                        Attrs::Pairs(&[
                            (keys::font(), font.as_ref()),
                            (keys::background_color(), self.values.summary_background.as_ref()),
                        ]),
                        annotation.content_range,
                        state,
                    );
                }
                SafeHTMLKind::TableCell { header, align } => {
                    if *header {
                        let font = self.emphasized(&body_font, true, false);
                        self.apply(Attrs::Pairs(&[(keys::font(), font.as_ref())]), annotation.content_range, state);
                    }
                    if !self.discards_typography
                        && let Some(alignment) = align
                        && annotation.content_range.length > 0
                    {
                        // Applied as the mutable style itself, as the Swift does.
                        let paragraph = paragraph_with_alignment(state, annotation.content_range.location, *alignment);
                        self.apply(
                            Attrs::Pairs(&[(keys::paragraph_style(), paragraph.as_ref())]),
                            annotation.content_range,
                            state,
                        );
                    }
                    // A small trailing kern is a visual column gutter that does
                    // not alter the source string.
                    if annotation.content_range.length > 0 {
                        let trailing = NSRange::new(annotation.content_range.upper_bound() - 1, 1);
                        let background = if *header {
                            &self.values.table_header_background
                        } else {
                            &self.values.table_cell_background
                        };
                        self.apply(
                            Attrs::Pairs(&[
                                (keys::kern(), self.values.table_kern.as_ref()),
                                (keys::background_color(), background.as_ref()),
                            ]),
                            trailing,
                            state,
                        );
                        self.apply(
                            Attrs::Pairs(&[(keys::background_color(), background.as_ref())]),
                            annotation.content_range,
                            state,
                        );
                    }
                }
                SafeHTMLKind::Inert => {
                    self.apply(
                        Attrs::Pairs(&[(keys::foreground_color(), self.style_sheet.text_faint.as_ref())]),
                        annotation.range,
                        state,
                    );
                }
                SafeHTMLKind::LineBreak
                | SafeHTMLKind::Details { .. }
                | SafeHTMLKind::DetailsClosing
                | SafeHTMLKind::Table
                | SafeHTMLKind::TableRow => {}
            }
        }
    }

    /// Bold and italic inside a heading must stay at the heading's size, so
    /// traits are derived from the block's own font unless the block is at body
    /// size — where the theme's `emphasisFont` gets to make the call (§11.1).
    fn emphasized(&self, base: &NSFont, bold: bool, italic: bool) -> Retained<NSFont> {
        if !(bold || italic) {
            return base.retain();
        }
        if (base.pointSize() - self.style_sheet.body_font().pointSize()).abs() < 0.5 {
            return self.style_sheet.emphasis_font(bold, italic);
        }
        let key = (base as *const NSFont as usize, bold, italic);
        if let Some((_, font)) = self.fonts.borrow().emphasized.get(&key) {
            return font.clone();
        }
        let descriptor = base.fontDescriptor();
        let mut traits = descriptor.symbolicTraits();
        if bold {
            traits |= NSFontDescriptorSymbolicTraits::TraitBold;
        }
        if italic {
            traits |= NSFontDescriptorSymbolicTraits::TraitItalic;
        }
        let descriptor = descriptor.fontDescriptorWithSymbolicTraits(traits);
        let font = NSFont::fontWithDescriptor_size(&descriptor, base.pointSize()).unwrap_or_else(|| base.retain());
        self.fonts.borrow_mut().emphasized.insert(key, (base.retain(), font.clone()));
        font
    }

    /// `styleSheet.monoFont(size:)`.
    fn mono_font(&self, size: f64) -> Retained<NSFont> {
        if let Some(font) = self.fonts.borrow().mono.get(&size.to_bits()) {
            return font.clone();
        }
        let font = self.style_sheet.mono_font(Some(size));
        self.fonts.borrow_mut().mono.insert(size.to_bits(), font.clone());
        font
    }

    /// `font.withSize(_:)`.
    fn resized(&self, base: &NSFont, size: f64) -> Retained<NSFont> {
        let key = (base as *const NSFont as usize, size.to_bits());
        if let Some((_, font)) = self.fonts.borrow().resized.get(&key) {
            return font.clone();
        }
        let font = base.fontWithSize(size);
        self.fonts.borrow_mut().resized.insert(key, (base.retain(), font.clone()));
        font
    }

    // MARK: - Code blocks (§11.3)

    fn decorate_code_block(
        &mut self,
        block: &MDBlock,
        language: Option<&str>,
        content_range: NSRange,
        context: BlockContext,
        state: &mut DecorateState,
    ) {
        self.apply_base(block, context, state);

        let line_count = line_count(content_range, state.document);
        let collapses = self.policy.collapses_long_code_blocks && line_count > self.collapse_line_count;
        let payload = self.emit_fragment(
            if collapses { FragmentKind::CollapsedCodeBlock } else { FragmentKind::CodeBlock },
            block,
            language.unwrap_or(""),
            state,
        );
        if let Some(payload) = payload {
            payload.set_is_collapsed(collapses);
        }

        // Fences stay in the text; the fragment absorbs them as chrome, so
        // they are styled as markers rather than as code.
        if let Some(marker) = block.marker_range {
            self.apply(Attrs::Dict(self.styles.marker_attributes(true)), marker, state);
        }
        if let Some(trailing) = block.trailing_marker_range {
            self.apply(Attrs::Dict(self.styles.marker_attributes(true)), trailing, state);
        }
        self.apply_code_row_indents(block, context, state);
        if !(content_range.length > 0 && content_range.upper_bound() <= state.storage_length) {
            return;
        }

        let document = state.document;
        let code: &[u16] = if content_range.location >= 0 && content_range.upper_bound() <= document.length {
            &document.utf16[content_range.as_usize_range()]
        } else {
            &[]
        };
        let runs = self.syntax_cache.runs(code, language, self.highlighter.as_ref());
        let is_diff = language.is_some_and(|language| swift_text::str_eq(&swift_text::lowercased(language), "diff"));
        if is_diff {
            let mut cursor = 0isize;
            let length = code.len() as isize;
            while cursor < length {
                let line = code.line_range_for(NSRange::new(cursor, 0));
                if line.length > 0 {
                    let first = code[line.location as usize];
                    if first == 0x2B || first == 0x2D {
                        let background = if first == 0x2B {
                            &self.values.diff_added_background
                        } else {
                            &self.values.diff_removed_background
                        };
                        self.apply(
                            Attrs::Pairs(&[(keys::background_color(), background.as_ref())]),
                            NSRange::new(content_range.location + line.location, line.length),
                            state,
                        );
                    }
                }
                cursor = (cursor + 1).max(line.upper_bound());
            }
        }
        for run in runs.iter() {
            let absolute = NSRange::new(content_range.location + run.range.location as isize, run.range.length as isize);
            let color = self.style_sheet.code_color_ref(run.token);
            // §11.3: ```diff fences get real diff colouring.
            self.apply(Attrs::Pairs(&[(keys::foreground_color(), color.as_ref())]), absolute, state);
        }
    }

    // MARK: - Fragments

    fn emit_fragment(
        &self,
        kind: FragmentKind,
        block: &MDBlock,
        detail: &str,
        state: &mut DecorateState,
    ) -> Option<Retained<FragmentPayload>> {
        if block.range.length <= 0 {
            return None;
        }
        let payload = FragmentPayload::new(kind, block.range, block.identity, detail);
        self.apply(Attrs::Pairs(&[(attribute_keys::dr_fragment(), payload.as_ref())]), block.range, state);
        state.fragment_count += 1;
        Some(payload)
    }

    /// A paragraph that is nothing but an image renders as an object with a
    /// caption (§11.3); an image sitting inside a sentence stays inline.
    fn emit_inline_image_fragment_if_solitary(&self, block: &MDBlock, state: &mut DecorateState) {
        if !(matches!(block.content, BlockContent::Paragraph) && block.inlines.len() == 1) {
            return;
        }
        let InlineKind::Image { source, alt } = &block.inlines[0].kind else { return };
        let payload = FragmentPayload::new(FragmentKind::Image, block.range, block.identity, source);
        payload.set_table_data(None);
        let source = NSString::from_str(source);
        let alt = NSString::from_str(alt);
        self.apply(
            Attrs::Pairs(&[
                (attribute_keys::dr_fragment(), payload.as_ref()),
                (attribute_keys::dr_link(), source.as_ref()),
                (attribute_keys::dr_reference(), alt.as_ref()),
            ]),
            block.range,
            state,
        );
        state.fragment_count += 1;
    }

    // MARK: - Program cache

    fn cached_program(&mut self, block: &MDBlock, context: BlockContext) -> Option<Rc<Vec<AttributeOp>>> {
        if !self.uses_program_cache || block.safe_html.is_some() {
            return None;
        }
        if !(block.subtree_hash != 0 && block.children.is_empty() && block.content.is_leaf_text()) {
            return None;
        }
        // Inline math carries its LaTeX as a payload read out of the document,
        // which the range-shifted recorder cannot resolve.
        if contains_inline_math(&block.inlines) {
            return None;
        }
        let key = ProgramKey {
            hash: block.subtree_hash,
            kind: BlockStyleFactory::kind_code(&block.content),
            list_depth: context.list_depth,
            quote_depth: context.quote_depth,
            length: block.range.length,
            task: context.task,
            plain_typography: self.discards_typography,
        };
        if let Some(hit) = self.program_cache.get(&key) {
            return Some(hit.clone());
        }
        if !self.program_seen.contains(&key) {
            self.program_seen.insert(key);
            return None;
        }
        let program = Rc::new(self.record_program(block, context));
        self.program_cache.insert(key, program.clone());
        Some(program)
    }

    /// Runs the same code paths as the live decorator against a scratch state,
    /// capturing the operations instead of applying them.
    fn record_program(&mut self, block: &MDBlock, context: BlockContext) -> Vec<AttributeOp> {
        let length = block.range.length;
        let spaces = NSString::from_str(&" ".repeat(length.max(0) as usize));
        // SAFETY: `initWithString:` on a freshly allocated NSTextStorage.
        let recorder: Retained<NSTextStorage> = unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*spaces] };
        let origin = block.range.location;
        let mut shifted = MDBlock::new(
            block.content.clone(),
            NSRange::new(0, length),
            shift(block.content_range, -origin),
        );
        shifted.marker_range = block.marker_range.map(|range| shift(range, -origin));
        shifted.trailing_marker_range = block.trailing_marker_range.map(|range| shift(range, -origin));
        shifted.inlines = block.inlines.iter().map(|span| shift_span(span, -origin)).collect();
        shifted.depth = block.depth;
        shifted.quote_depth = block.quote_depth;
        shifted.subtree_hash = block.subtree_hash;
        shifted.identity = block.identity;

        let empty = ParsedDocument::empty();
        let mut scratch = DecorateState::new(&empty, &recorder);
        scratch.target = NSRange::new(0, length);
        recorder.beginEditing();
        self.apply_base(&shifted, context, &mut scratch);
        self.apply_block_marker(&shifted, context, &mut scratch);
        self.apply_inlines_if_leaf(&shifted, context, &mut scratch);
        recorder.endEditing();

        let ops: RefCell<Vec<AttributeOp>> = RefCell::new(Vec::new());
        let block = RcBlock::new(
            |attributes: std::ptr::NonNull<NSDictionary<NSString, AnyObject>>,
             range: objc2_foundation::NSRange,
             _stop: std::ptr::NonNull<objc2::runtime::Bool>| {
                // SAFETY: the enumeration hands over a live dictionary.
                let attributes = unsafe { Retained::retain(attributes.as_ptr()) }.expect("attributes");
                ops.borrow_mut().push(AttributeOp { range: from_ns_range(range), attributes });
            },
        );
        recorder.enumerateAttributesInRange_options_usingBlock(
            ns_range(NSRange::new(0, recorder.length() as isize)),
            NSAttributedStringEnumerationOptions::empty(),
            &block,
        );
        drop(block);
        ops.into_inner()
    }

    fn apply_program(&self, program: &[AttributeOp], origin: isize, state: &mut DecorateState) {
        for op in program {
            self.apply(Attrs::Dict(&op.attributes), shift(op.range, origin), state);
        }
    }

    // MARK: - Hidden ranges (§6.1)

    /// Ranges of syntax markers that must be omitted from the display string,
    /// given the current policy and caret. Ascending, non-overlapping.
    pub fn hidden_ranges(&self, document: &ParsedDocument, caret: Option<isize>, selections: &[NSRange]) -> Vec<NSRange> {
        MarkerPolicy::hidden_ranges(document, self.policy, caret, selections)
    }

    /// `hidden_ranges` in two parts, for a hosted view's incremental base
    /// map: the definitions (before `disjoint`) and what `blocks` produce.
    pub fn definition_hidden_ranges(&self, document: &ParsedDocument) -> Vec<NSRange> {
        if !self.policy.hides_block_markers {
            return Vec::new();
        }
        MarkerPolicy::definition_ranges(document)
    }

    pub fn block_hidden_ranges(&self, document: &ParsedDocument, blocks: &[upleft_core::BlockRef]) -> Vec<NSRange> {
        MarkerPolicy::block_ranges(document, blocks, self.policy, None, &[])
    }

    // MARK: - Gutter markers (§6.1a)

    /// Gutter marker text per block, keyed by the block's start offset.
    pub fn gutter_markers(&self, document: &ParsedDocument) -> Vec<(isize, String, isize)> {
        let mut out = Vec::new();
        collect_gutter_markers(&document.root, BlockContext::ROOT, &mut out);
        out.sort_by_key(|(offset, _, _)| *offset);
        out
    }

    /// What a block writes into the left rail: a heading keeps its level,
    /// everything else draws its own marker (§6.1a).
    pub fn gutter_text(block: &MDBlock, _context: BlockContext) -> Option<String> {
        match block.content {
            BlockContent::Heading { level } => Some(format!("H{}", 1.max(level.min(6)))),
            _ => None,
        }
    }

    fn gutter_level(block: &MDBlock, context: BlockContext) -> isize {
        match block.content {
            BlockContent::Heading { level } => level,
            BlockContent::ListItem { .. } => context.list_depth,
            BlockContent::BlockQuote | BlockContent::Callout { .. } => context.quote_depth,
            _ => 0,
        }
    }
}

fn collect_gutter_markers(block: &MDBlock, context: BlockContext, out: &mut Vec<(isize, String, isize)>) {
    let mut child_context = context;
    match &block.content {
        BlockContent::List { .. } => child_context.list_depth += 1,
        BlockContent::BlockQuote | BlockContent::Callout { .. } => child_context.quote_depth += 1,
        BlockContent::ListItem { ordinal, .. } => child_context.ordinal = *ordinal,
        _ => {}
    }
    if let Some(text) = DecorationEngine::gutter_text(block, context) {
        out.push((block.range.location, text, DecorationEngine::gutter_level(block, context)));
    }
    for child in &block.children {
        collect_gutter_markers(child, child_context, out);
    }
}

// MARK: - Helpers

#[inline]
fn is_typography_key(key: &NSString) -> bool {
    std::ptr::eq(key, keys::font()) || std::ptr::eq(key, keys::paragraph_style())
}

#[inline]
fn pairs_dictionary(pairs: &[(&NSString, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let mut keys_: [&NSString; 8] = [keys::font(); 8];
    let mut values: [&AnyObject; 8] = [pairs[0].1; 8];
    for (index, (key, value)) in pairs.iter().enumerate() {
        keys_[index] = key;
        values[index] = value;
    }
    NSDictionary::from_slices(&keys_[..pairs.len()], &values[..pairs.len()])
}

/// `"\"'“‘".contains(first)` for the first Character of `text`.
fn opens_with_quotation_mark(text: &str) -> bool {
    let Some(first) = swift_text::first(text) else { return false };
    ["\"", "'", "\u{201C}", "\u{2018}"].iter().any(|mark| swift_text::char_eq(mark, first))
}

/// `document.substring(range).contains("\n")`, with the string's provenance
/// (bridged or native) deciding the comparison exactly as in Swift.
fn content_contains_newline(range: NSRange, state: &mut DecorateState) -> bool {
    let document = state.document;
    if !(range.location >= 0 && range.upper_bound() <= document.length) {
        return false;
    }
    let units = &document.utf16[range.as_usize_range()];
    // Nothing but U+000A is, or is canonically equivalent to, "\n".
    if !units.contains(&0x0A) {
        return false;
    }
    let bridged = state.bridged();
    swift_text::contains_with(&document.substring(range), "\n", bridged)
}

/// True when the nearest block above this heading is itself a heading.
fn follows_another_heading(block: &MDBlock, state: &DecorateState) -> bool {
    let headings = &state.document.headings;
    let Some(index) = headings
        .iter()
        .position(|heading| heading.range.location == block.range.location)
    else {
        return false;
    };
    if index == 0 {
        return false;
    }
    let previous = &headings[index - 1];
    // Nothing but whitespace between the two means they are adjacent.
    let between = NSRange::new(
        previous.range.upper_bound(),
        0.max(block.range.location - previous.range.upper_bound()),
    );
    if !(between.length > 0 && between.upper_bound() <= state.document.length) {
        return true;
    }
    swift_text::trim_whitespaces_and_newlines(&state.document.substring(between)).is_empty()
}

/// The physical paragraphs a block is typed across, terminators included.
fn physical_paragraphs(block: &MDBlock, state: &DecorateState) -> Vec<NSRange> {
    if !(block.range.length > 0 && block.range.upper_bound() <= state.storage_length) {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut cursor = block.range.location;
    while cursor < block.range.upper_bound() {
        let line = state.paragraph_range(NSRange::new(cursor, 0));
        if line.length <= 0 {
            break;
        }
        lines.push(line);
        cursor = line.upper_bound().max(cursor + 1);
    }
    lines
}

/// `BlockStyleFactory.indentColumns(of: text.substring(with: line))`, read
/// straight off the storage string.
fn indent_columns_in(text: &NSString, line: NSRange) -> isize {
    let tab = render_metrics::CODE_TAB_COLUMNS as isize;
    let mut columns = 0isize;
    for index in line.location..line.upper_bound() {
        match text.characterAtIndex(index as usize) {
            0x20 => columns += 1,
            0x09 => columns += tab - (columns % tab),
            _ => return columns,
        }
    }
    columns
}

fn paragraph_with_alignment(state: &DecorateState, at: isize, alignment: SafeHTMLAlignment) -> Retained<NSMutableParagraphStyle> {
    // SAFETY: reading an attribute value; the effective range is not asked for.
    let current = unsafe {
        state
            .storage
            .attribute_atIndex_effectiveRange(keys::paragraph_style(), at as usize, std::ptr::null_mut())
    };
    let paragraph = current
        .and_then(|value| value.downcast::<NSParagraphStyle>().ok())
        .map(|style| mutable_copy(&style))
        .unwrap_or_else(NSMutableParagraphStyle::new);
    paragraph.setAlignment(match alignment {
        SafeHTMLAlignment::Left => NSTextAlignment::Left,
        SafeHTMLAlignment::Center => NSTextAlignment::Center,
        SafeHTMLAlignment::Right => NSTextAlignment::Right,
        SafeHTMLAlignment::Justify => NSTextAlignment::Justified,
    });
    paragraph
}

/// Smallest range covering every leaf block that intersects `range`.
/// A fence whose closing marker has not arrived: it runs to the end of the
/// document, with at most whitespace after it.
fn is_open_fence_at_end(block: &MDBlock, document: &ParsedDocument) -> bool {
    if block.marker_range.is_none() || block.trailing_marker_range.is_some() {
        return false;
    }
    let end = block.range.upper_bound().max(0) as usize;
    document.utf16.get(end..).is_some_and(|rest| rest.iter().all(|&unit| matches!(unit, 0x09..=0x0D | 0x20)))
}

/// The range of a Mermaid or math fence still open at the end of the
/// document, if there is one (`MarkdownTextView::set_streaming`).
pub fn open_fence_at_end(document: &ParsedDocument) -> Option<NSRange> {
    let mut found = None;
    document.root.walk(&mut |block| {
        if matches!(block.content, BlockContent::Mermaid { .. } | BlockContent::MathBlock { .. })
            && is_open_fence_at_end(block, document)
        {
            found = Some(block.range);
        }
    });
    found
}

fn block_bounds(range: NSRange, document: &ParsedDocument) -> NSRange {
    let mut bounds = range;
    document.root.walk_pruning(&mut |block| {
        if !(block.range.location < range.upper_bound() && range.location < block.range.upper_bound()) {
            return false;
        }
        if block.children.is_empty() {
            bounds = bounds.union(block.range);
        }
        true
    });
    bounds
}

#[inline]
fn clip(range: NSRange, bounds: NSRange) -> Option<NSRange> {
    let lo = range.location.max(bounds.location);
    let hi = range.upper_bound().min(bounds.upper_bound());
    if hi > lo { Some(NSRange::new(lo, hi - lo)) } else { None }
}

/// The line indices whose full source ranges intersect `target`.
fn line_indices(target: NSRange, line_starts: &[isize]) -> std::ops::Range<usize> {
    if !(target.length > 0 && !line_starts.is_empty()) {
        return 0..0;
    }
    let (mut lower, mut upper) = (0usize, line_starts.len());
    while lower < upper {
        let middle = (lower + upper) / 2;
        if line_starts[middle] <= target.location {
            lower = middle + 1;
        } else {
            upper = middle;
        }
    }
    let first = lower.saturating_sub(1);

    lower = 0;
    upper = line_starts.len();
    while lower < upper {
        let middle = (lower + upper) / 2;
        if line_starts[middle] < target.upper_bound() {
            lower = middle + 1;
        } else {
            upper = middle;
        }
    }
    first..first.max(lower)
}

/// Child blocks are sorted and non-overlapping, so only the contiguous slice
/// that intersects `target` enters the recursive walk.
fn intersecting_child_indices(children: &[upleft_core::BlockRef], target: NSRange) -> std::ops::Range<usize> {
    if !(target.length > 0 && !children.is_empty()) {
        return 0..0;
    }
    let (mut lower, mut upper) = (0usize, children.len());
    while lower < upper {
        let middle = (lower + upper) / 2;
        if children[middle].range.upper_bound() <= target.location {
            lower = middle + 1;
        } else {
            upper = middle;
        }
    }
    let first = lower;

    upper = children.len();
    while lower < upper {
        let middle = (lower + upper) / 2;
        if children[middle].range.location < target.upper_bound() {
            lower = middle + 1;
        } else {
            upper = middle;
        }
    }
    first..first.max(lower)
}

fn line_count(range: NSRange, document: &ParsedDocument) -> isize {
    if range.length <= 0 {
        return 0;
    }
    document.line_at(range.upper_bound() - 1) - document.line_at(range.location) + 1
}

fn contains_inline_math(spans: &[InlineSpan]) -> bool {
    for span in spans {
        if matches!(span.kind, InlineKind::InlineMath { .. }) {
            return true;
        }
        if contains_inline_math(&span.children) {
            return true;
        }
    }
    false
}

#[inline]
fn shift(range: NSRange, delta: isize) -> NSRange {
    NSRange::new(range.location + delta, range.length)
}

fn shift_span(span: &InlineSpan, delta: isize) -> InlineSpan {
    InlineSpan {
        kind: match &span.kind {
            InlineKind::InlineMath { latex_range } => InlineKind::InlineMath { latex_range: shift(*latex_range, delta) },
            other => other.clone(),
        },
        range: shift(span.range, delta),
        content_range: shift(span.content_range, delta),
        leading_marker_range: span.leading_marker_range.map(|range| shift(range, delta)),
        trailing_marker_range: span.trailing_marker_range.map(|range| shift(range, delta)),
        children: span.children.iter().map(|child| shift_span(child, delta)).collect(),
    }
}
