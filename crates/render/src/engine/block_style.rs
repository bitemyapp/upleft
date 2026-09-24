//! Port of `Engine/BlockStyle.swift`: `BlockContext`, `WritingDirection`, and
//! `BlockStyleFactory` — fonts, colours, and paragraph styles per block kind.

use std::collections::HashMap;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontWeightRegular, NSLineBreakMode,
    NSMutableParagraphStyle, NSParagraphStyle, NSTextAlignment, NSWritingDirection,
};
use objc2_foundation::{NSArray, NSAttributedString, NSDictionary, NSNumber, NSString};
use upleft_core::{BlockContent, CalloutKind, MDBlock};

use super::keys;
use super::render_metrics::{self, RoundingRule};
use crate::render_contracts::attribute_keys;
use crate::theme::style_sheet::StyleSheet;

/// Where a block sits in the tree. `MDBlock` has no parent pointer, so the
/// decorator carries the ancestry down as it walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockContext {
    pub list_depth: isize,
    pub quote_depth: isize,
    pub callout_kind: Option<CalloutKind>,
    /// Ordinal of the enclosing ordered list item, for the gutter marker.
    pub ordinal: Option<isize>,
    /// True while inside a `- [ ]` task item, so a task's text reserves the
    /// checkbox column, not the bullet column (§11.3).
    pub task: bool,
}

impl BlockContext {
    /// `BlockContext.root`.
    pub const ROOT: BlockContext = BlockContext {
        list_depth: 0,
        quote_depth: 0,
        callout_kind: None,
        ordinal: None,
        task: false,
    };

    pub fn new(
        list_depth: isize,
        quote_depth: isize,
        callout_kind: Option<CalloutKind>,
        ordinal: Option<isize>,
        task: bool,
    ) -> Self {
        BlockContext { list_depth, quote_depth, callout_kind, ordinal, task }
    }
}

/// Resolving one base writing direction for a whole document.
pub struct WritingDirection;

impl WritingDirection {
    /// Ranges whose letters are strongly right-to-left. All are in the BMP.
    fn is_right_to_left(scalar: u32) -> bool {
        matches!(
            scalar,
            0x0590..=0x05FF   // Hebrew
            | 0x0600..=0x06FF // Arabic
            | 0x0700..=0x074F // Syriac
            | 0x0750..=0x077F // Arabic Supplement
            | 0x0780..=0x07BF // Thaana
            | 0x07C0..=0x08FF // NKo, Samaritan, Mandaic, Arabic Extended-A
            | 0xFB1D..=0xFDFF // Hebrew and Arabic presentation forms
            | 0xFE70..=0xFEFF // Arabic presentation forms B
        )
    }

    /// The direction of the document's first strong letter — the same rule
    /// HTML's `dir=auto` uses. Only *letters* vote, and the scan is bounded
    /// to the first `limit` scalars.
    pub fn of(text: &str, limit: usize) -> NSWritingDirection {
        for scalar in text.chars().take(limit) {
            let v = scalar as u32;
            if WritingDirection::is_right_to_left(v) {
                return NSWritingDirection::RightToLeft;
            }
            if (0x41..=0x5A).contains(&v)
                || (0x61..=0x7A).contains(&v)
                || (v >= 128 && upleft_core::swift_text::scalar_is_alphabetic(scalar))
            {
                return NSWritingDirection::LeftToRight;
            }
        }
        NSWritingDirection::LeftToRight
    }
}

/// The attributes every character of a block starts from, in the shape the
/// decorator consumes them: the paragraph style on its own (it is applied
/// paragraph by paragraph) and the rest as a ready dictionary.
#[derive(Debug)]
pub struct BaseAttributes {
    pub font: Retained<NSFont>,
    pub color: Retained<NSColor>,
    pub paragraph_style: Retained<NSParagraphStyle>,
    /// `.drHeading`.
    pub heading: Option<isize>,
    /// `.kern`.
    pub kern: Option<f64>,
    /// `.ligature: 0`.
    pub ligature_off: bool,
    /// Every attribute but `.paragraphStyle`.
    pub without_paragraph: Retained<NSDictionary<NSString, AnyObject>>,
    /// Every attribute.
    pub all: Retained<NSDictionary<NSString, AnyObject>>,
}

impl BaseAttributes {
    fn new(
        font: Retained<NSFont>,
        color: Retained<NSColor>,
        paragraph_style: Retained<NSParagraphStyle>,
        heading: Option<isize>,
        kern: Option<f64>,
        ligature_off: bool,
    ) -> BaseAttributes {
        let mut keys_: Vec<&NSString> = Vec::with_capacity(6);
        let mut values: Vec<Retained<AnyObject>> = Vec::with_capacity(6);
        keys_.push(keys::font());
        values.push(any(font.clone()));
        keys_.push(keys::foreground_color());
        values.push(any(color.clone()));
        if let Some(level) = heading {
            keys_.push(attribute_keys::dr_heading());
            values.push(any(NSNumber::new_isize(level)));
        }
        if let Some(kern) = kern {
            keys_.push(keys::kern());
            values.push(any(NSNumber::new_f64(kern)));
        }
        if ligature_off {
            keys_.push(keys::ligature());
            values.push(any(NSNumber::new_isize(0)));
        }
        let refs: Vec<&AnyObject> = values.iter().map(|v| &**v).collect();
        let without_paragraph = NSDictionary::from_slices(&keys_, &refs);
        keys_.push(keys::paragraph_style());
        let mut refs = refs;
        refs.push(paragraph_style.as_ref());
        let all = NSDictionary::from_slices(&keys_, &refs);
        BaseAttributes { font, color, paragraph_style, heading, kern, ligature_off, without_paragraph, all }
    }
}

/// Any object as `AnyObject`.
fn any<T: objc2::Message>(object: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an `AnyObject`.
    unsafe { Retained::cast_unchecked(object) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CodeRowKey {
    columns: isize,
    /// `CGFloat` hashes and compares by value; the bits are the same thing
    /// for every value a head indent can take.
    head: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleKey {
    pub kind: isize,
    pub level: isize,
    pub list_depth: isize,
    pub quote_depth: isize,
    pub ordinal_digits: isize,
    /// A task's text reserves the checkbox column, a bullet's does not.
    pub task: bool,
    /// A callout reserves a wider column than the plain quote it is built from.
    pub callout: bool,
}

/// Fonts, colours, and paragraph styles per block kind, memoised.
pub struct BlockStyleFactory {
    pub style_sheet: StyleSheet,
    paragraph_cache: HashMap<StyleKey, Retained<NSParagraphStyle>>,
    attribute_cache: HashMap<StyleKey, Rc<BaseAttributes>>,
    grid: f64,
    indent_unit: f64,
    /// How far a wrapped code row hangs past its statement's own left edge.
    code_continuation_indent: f64,
    /// Base writing direction for every block, resolved once for the whole
    /// document (see `WritingDirection::of`).
    base_writing_direction: NSWritingDirection,
    /// Width of one monospace column.
    mono_advance: f64,
    code_row_cache: HashMap<CodeRowKey, Retained<NSParagraphStyle>>,
    /// `markerAttributes(dimmed:)` for `false`, `true`.
    marker_attributes: [Retained<NSDictionary<NSString, AnyObject>>; 2],
}

/// `NSAttributedString(string:attributes: [.font: font]).size().width`.
pub fn string_width(string: &str, font: &NSFont) -> f64 {
    let dictionary = NSDictionary::from_slices(&[keys::font()], &[font.as_ref() as &AnyObject]);
    // SAFETY: the dictionary maps an attribute key to a font.
    let attributed =
        unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(string), &dictionary) };
    attributed.size().width
}

pub(crate) fn frozen(style: &NSMutableParagraphStyle) -> Retained<NSParagraphStyle> {
    // SAFETY: `copy` of a paragraph style is an immutable NSParagraphStyle.
    unsafe { objc2::msg_send![style, copy] }
}

pub(crate) fn mutable_copy(style: &NSParagraphStyle) -> Retained<NSMutableParagraphStyle> {
    // SAFETY: `mutableCopy` of a paragraph style is an NSMutableParagraphStyle.
    unsafe { objc2::msg_send![style, mutableCopy] }
}

impl BlockStyleFactory {
    pub fn new(style_sheet: &StyleSheet) -> BlockStyleFactory {
        let grid = 1f64.max(style_sheet.baseline_grid);
        let indent_unit = render_metrics::indent_unit(style_sheet.body_font().pointSize());
        let mono = style_sheet.mono_font(None);
        let advance = string_width("MM", &mono);
        let code_continuation_indent = (if advance > 1.0 { advance } else { mono.pointSize() }).round();
        let mono_advance = if advance > 1.0 { advance / 2.0 } else { mono.pointSize() / 2.0 };
        let marker = |dimmed: bool| {
            let color = if dimmed { style_sheet.marker.clone() } else { style_sheet.text_secondary.clone() };
            let flag = NSNumber::new_bool(true);
            NSDictionary::from_slices(
                &[attribute_keys::dr_marker(), keys::foreground_color()],
                &[flag.as_ref() as &AnyObject, color.as_ref()],
            )
        };
        BlockStyleFactory {
            style_sheet: style_sheet.clone(),
            paragraph_cache: HashMap::new(),
            attribute_cache: HashMap::new(),
            grid,
            indent_unit,
            code_continuation_indent,
            base_writing_direction: NSWritingDirection::LeftToRight,
            mono_advance,
            code_row_cache: HashMap::new(),
            marker_attributes: [marker(false), marker(true)],
        }
    }

    pub fn base_writing_direction(&self) -> NSWritingDirection {
        self.base_writing_direction
    }

    /// Setting a *different* direction drops the paragraph cache.
    pub fn set_base_writing_direction(&mut self, direction: NSWritingDirection) {
        if direction == self.base_writing_direction {
            return;
        }
        self.base_writing_direction = direction;
        self.paragraph_cache.clear();
    }

    /// Columns a code row's leading whitespace occupies, tabs snapping to the
    /// four-column stops the code paragraph style installs.
    pub fn indent_columns(line: &[u16]) -> isize {
        let tab = render_metrics::CODE_TAB_COLUMNS as isize;
        let mut columns = 0isize;
        for &unit in line {
            match unit {
                0x20 => columns += 1,
                0x09 => columns += tab - (columns % tab),
                _ => return columns,
            }
        }
        columns
    }

    /// The paragraph style for one physical row of a code block, hung off that
    /// row's *own* indentation.
    pub fn code_row_style(&mut self, base: &NSParagraphStyle, columns: isize) -> Retained<NSParagraphStyle> {
        let key = CodeRowKey { columns, head: base.firstLineHeadIndent().to_bits() };
        if let Some(cached) = self.code_row_cache.get(&key) {
            return cached.clone();
        }
        let style = mutable_copy(base);
        style.setHeadIndent(base.firstLineHeadIndent() + columns as f64 * self.mono_advance + self.code_continuation_indent);
        let result = frozen(&style);
        self.code_row_cache.insert(key, result.clone());
        result
    }

    /// Stable discriminator per block kind.
    pub fn kind_code(content: &BlockContent) -> isize {
        match content {
            BlockContent::Document => 0,
            BlockContent::Heading { .. } => 1,
            BlockContent::Paragraph => 2,
            BlockContent::BlockQuote => 3,
            BlockContent::Callout { .. } => 4,
            BlockContent::List { .. } => 5,
            BlockContent::ListItem { .. } => 6,
            BlockContent::CodeBlock { .. } => 7,
            BlockContent::Mermaid { .. } => 8,
            BlockContent::MathBlock { .. } => 9,
            BlockContent::Table(_) => 10,
            BlockContent::ThematicBreak => 11,
            BlockContent::HtmlBlock => 12,
            BlockContent::FrontMatter(_) => 13,
            BlockContent::FootnoteDefinition { .. } => 14,
        }
    }

    fn level(content: &BlockContent) -> isize {
        if let BlockContent::Heading { level } = content { *level } else { 0 }
    }

    pub fn key(&self, block: &MDBlock, context: BlockContext) -> StyleKey {
        StyleKey {
            kind: BlockStyleFactory::kind_code(&block.content),
            level: BlockStyleFactory::level(&block.content),
            list_depth: context.list_depth.min(8),
            quote_depth: context.quote_depth.min(6),
            ordinal_digits: context.ordinal.map_or(0, |ordinal| 1.max(decimal_digits(ordinal))),
            task: context.task,
            callout: context.callout_kind.is_some(),
        }
    }

    pub fn font(&self, content: &BlockContent) -> Retained<NSFont> {
        match content {
            BlockContent::Heading { level } => self.style_sheet.heading_font(*level as i64),
            BlockContent::CodeBlock { .. } | BlockContent::Mermaid { .. } | BlockContent::MathBlock { .. } => {
                self.style_sheet.mono_font(None)
            }
            BlockContent::FrontMatter(_) => {
                self.style_sheet.mono_font(Some(self.style_sheet.body_font().pointSize() * 0.9))
            }
            _ => self.style_sheet.body_font(),
        }
    }

    fn color(&self, content: &BlockContent, context: BlockContext) -> Retained<NSColor> {
        match content {
            BlockContent::Heading { level } => self.style_sheet.heading_color(*level as i64),
            BlockContent::BlockQuote => self.style_sheet.text.clone(),
            BlockContent::Callout { kind, .. } => {
                if context.quote_depth > 0 {
                    self.style_sheet.callout_color(*kind)
                } else {
                    self.style_sheet.text.clone()
                }
            }
            BlockContent::ThematicBreak | BlockContent::FrontMatter(_) => self.style_sheet.text_secondary.clone(),
            _ => self.style_sheet.text.clone(),
        }
    }

    /// Exact, grid-snapped line height, fixed per block kind and independent
    /// of the caret (§6.1a).
    pub fn line_height(&self, content: &BlockContent) -> f64 {
        let f = self.font(content);
        if matches!(content, BlockContent::CodeBlock { .. } | BlockContent::Mermaid { .. }) {
            // Code wants a *tighter* leading than prose; half a grid unit is
            // the finest step that keeps whole blocks landing on the grid.
            let ideal = f.pointSize() * 1.35;
            let snapped = render_metrics::snap(ideal, 1f64.max(self.grid / 2.0), RoundingRule::ToNearestOrAwayFromZero);
            // Never below the glyphs' own extent, however coarse the grid.
            return snapped.max(render_metrics::snap_up(f.ascender() - f.descender(), 1f64.max(self.grid / 2.0)));
        }
        let natural = f.ascender() - f.descender() + f.leading();
        let base = self.style_sheet.line_height.max(natural * 1.02);
        render_metrics::snap_up(base, self.grid)
    }

    /// Blocks that may use the bleed lane past the prose measure's trailing
    /// edge (`RenderMetrics.codeBleed`).
    pub fn is_full_bleed(content: &BlockContent) -> bool {
        matches!(
            content,
            BlockContent::CodeBlock { .. } | BlockContent::Mermaid { .. } | BlockContent::Table(_) | BlockContent::MathBlock { .. }
        )
    }

    pub fn indent(&self, content: &BlockContent, context: BlockContext) -> f64 {
        // The first list level needs only its ornament column.
        let levels = (0.max(context.list_depth - 1) + context.quote_depth) as f64;
        let mut result = levels * self.indent_unit;
        if context.callout_kind.is_some() {
            // A callout consumes one quote level, but its icon needs a wider
            // column than a plain quote rule.
            result += render_metrics::CALLOUT_ICON_INSET_X - self.indent_unit;
        }
        // A fenced block inside a list item sits at the item's content edge.
        if matches!(content, BlockContent::CodeBlock { .. }) && context.list_depth > 0 {
            result += self.marker_column(context, false);
        }
        result
    }

    pub fn paragraph_style(&mut self, block: &MDBlock, context: BlockContext) -> Retained<NSParagraphStyle> {
        let k = self.key(block, context);
        if let Some(cached) = self.paragraph_cache.get(&k) {
            return cached.clone();
        }

        let style = NSMutableParagraphStyle::new();
        let h = self.line_height(&block.content);
        style.setMinimumLineHeight(h);
        style.setMaximumLineHeight(h);
        style.setLineSpacing(0.0);
        style.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        style.setAlignment(NSTextAlignment::Natural);
        // One direction for the document, so a list keeps a single edge.
        style.setBaseWritingDirection(self.base_writing_direction);

        // Prose is held back off the bleed lane; a full-bleed block gets it.
        if !BlockStyleFactory::is_full_bleed(&block.content) {
            style.setTailIndent(-self.style_sheet.code_bleed());
        }

        let indent = self.indent(&block.content, context);
        style.setFirstLineHeadIndent(indent);
        style.setHeadIndent(indent);

        match &block.content {
            BlockContent::Heading { level } => {
                let (before, after) = self.style_sheet.heading_spacing(*level as i64);
                style.setParagraphSpacingBefore(render_metrics::snap_up(before, self.grid));
                style.setParagraphSpacing(render_metrics::snap_up(after, self.grid));
            }
            BlockContent::CodeBlock { .. }
            | BlockContent::Mermaid { .. }
            | BlockContent::MathBlock { .. }
            | BlockContent::FrontMatter(_) => {
                // Vertical air around these lives in the fragment's chrome.
                style.setParagraphSpacingBefore(0.0);
                style.setParagraphSpacing(0.0);
                style.setFirstLineHeadIndent(indent + render_metrics::CODE_INSET_X);
                style.setHeadIndent(indent + render_metrics::CODE_INSET_X);
            }
            BlockContent::ListItem { checkbox, .. } => {
                style.setParagraphSpacingBefore(0.0);
                // Task rows are controls, not compressed prose.
                style.setParagraphSpacing(if checkbox.is_none() { 0.0 } else { self.grid });
                if context.list_depth > 0 {
                    // The ornament is drawn in the hanging column; both the
                    // first line and every wrap start at the content edge.
                    let content_edge = indent + self.marker_column(context, checkbox.is_some());
                    style.setFirstLineHeadIndent(content_edge);
                    style.setHeadIndent(content_edge);
                }
            }
            BlockContent::Table(_) | BlockContent::ThematicBreak => {
                style.setParagraphSpacingBefore(0.0);
                style.setParagraphSpacing(0.0);
            }
            BlockContent::Callout { .. } => {
                let inset = render_metrics::CALLOUT_ICON_INSET_X;
                style.setFirstLineHeadIndent(indent + inset);
                style.setHeadIndent(indent + inset);
                style.setParagraphSpacingBefore(0.0);
                style.setParagraphSpacing(0.0);
            }
            BlockContent::BlockQuote => {
                let inset = render_metrics::CALLOUT_INSET_X;
                style.setFirstLineHeadIndent(indent + inset);
                style.setHeadIndent(indent + inset);
                style.setParagraphSpacingBefore(0.0);
                style.setParagraphSpacing(0.0);
            }
            _ => {
                // Paragraphs inside a list are the body of a list item.
                match self.style_sheet.host.paragraph_spacing {
                    Some(spacing) if context.list_depth == 0 => style.setParagraphSpacing(spacing),
                    _ => style.setParagraphSpacing(render_metrics::snap_up(
                        h * (if context.list_depth > 0 { 0.15 } else { 0.45 }),
                        self.grid,
                    )),
                }
                if context.list_depth > 0 {
                    let content_edge = indent + self.marker_column(context, context.task);
                    style.setFirstLineHeadIndent(content_edge);
                    style.setHeadIndent(content_edge);
                }
            }
        }

        // A host may hyphenate prose (`HostTypography::hyphenation_factor`).
        if let Some(factor) = self.style_sheet.host.hyphenation_factor
            && matches!(
                block.content,
                BlockContent::Paragraph
                    | BlockContent::ListItem { .. }
                    | BlockContent::BlockQuote
                    | BlockContent::Callout { .. }
                    | BlockContent::FootnoteDefinition { .. }
            )
        {
            style.setHyphenationFactor(factor);
        }

        // Code wraps at word boundaries with a continuation indent and
        // deterministic tab stops.
        if matches!(block.content, BlockContent::CodeBlock { .. }) {
            style.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            style.setHeadIndent(style.firstLineHeadIndent() + self.code_continuation_indent);
            style.setTailIndent(-render_metrics::CODE_INSET_X);
            style.setTabStops(Some(&NSArray::new()));
            style.setDefaultTabInterval(render_metrics::CODE_TAB_COLUMNS as f64 * self.mono_advance);
        }

        let result = frozen(&style);
        self.paragraph_cache.insert(k, result.clone());
        result
    }

    /// Attributes every character of the block starts from. Inline spans and
    /// markers layer on top with `addAttributes`.
    pub fn base_attributes(&mut self, block: &MDBlock, context: BlockContext) -> Rc<BaseAttributes> {
        let k = self.key(block, context);
        if let Some(cached) = self.attribute_cache.get(&k) {
            return cached.clone();
        }
        let font = self.font(&block.content);
        let color = self.color(&block.content, context);
        let paragraph_style = self.paragraph_style(block, context);
        let mut heading = None;
        let mut kern = None;
        if let BlockContent::Heading { level } = block.content {
            heading = Some(level);
            let point_size = self.font(&block.content).pointSize();
            if point_size > 28.0 {
                kern = Some(point_size * -0.022);
            } else if point_size >= 20.0 {
                kern = Some(point_size * -0.014);
            } else if level == 5 {
                kern = Some(point_size * 0.04);
            } else if level == 6 {
                kern = Some(point_size * 0.06);
            }
        }
        let ligature_off = !self.style_sheet.theme.typography.mono_ligatures && is_mono(&block.content);
        let attributes = Rc::new(BaseAttributes::new(font, color, paragraph_style, heading, kern, ligature_off));
        self.attribute_cache.insert(k, attributes.clone());
        attributes
    }

    /// Width reserved for the visual list ornament plus a half-em gap. A task
    /// checkbox reserves its own dedicated column (§11.3).
    fn marker_column(&self, context: BlockContext, task: bool) -> f64 {
        if task {
            return render_metrics::task_marker_column();
        }
        let body_size = self.style_sheet.body_font().pointSize();
        let gap = body_size * 0.5;
        let Some(ordinal) = context.ordinal else {
            return (body_size * 1.1).max(body_size * 0.625 + gap);
        };
        let digits = 1.max(decimal_digits(ordinal));
        // SAFETY: AppKit exports the weight as an immutable global.
        let font = NSFont::monospacedDigitSystemFontOfSize_weight(body_size * 0.92, unsafe { NSFontWeightRegular });
        let mut marker = "8".repeat(digits as usize);
        marker.push('.');
        string_width(&marker, &font) + gap
    }

    /// Marker styling: dimmed, same metrics as the text it sits in (§6.1).
    pub fn marker_attributes(&self, dimmed: bool) -> &NSDictionary<NSString, AnyObject> {
        &self.marker_attributes[dimmed as usize]
    }
}

fn is_mono(content: &BlockContent) -> bool {
    matches!(
        content,
        BlockContent::CodeBlock { .. } | BlockContent::Mermaid { .. } | BlockContent::MathBlock { .. } | BlockContent::FrontMatter(_)
    )
}

/// `String(abs(n)).count`.
fn decimal_digits(n: isize) -> isize {
    let mut value = n.unsigned_abs();
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}
