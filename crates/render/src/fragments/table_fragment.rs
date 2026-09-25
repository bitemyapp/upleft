//! Port of `Fragments/TableFragment.swift`: `TableCellPresentation`,
//! `TableLayout` (column geometry computed once per table and shared by its
//! rows) and `TableRowFragment` (one row, §11.3: no gridlines, a rule under
//! the header, zebra on hover, numeric columns right-aligned).

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, Message};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSFont, NSFontDescriptorSymbolicTraits, NSFontWeightSemibold,
    NSKernAttributeName, NSLineBreakMode, NSMutableParagraphStyle, NSTextAlignment, NSTextElement, NSTextElementProvider, NSTextLayoutFragment, NSTextRange,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSAttributedString, NSDictionary, NSMutableAttributedString, NSNumber, NSString};
use upleft_core::model::{InlineSpan, TableAlignment, TableCell, TableData, TableRow};

use crate::appkit_compat::{
    RectExt, attribute_value, attributed_string, attributes_dictionary, enumerate_attribute, keys, ns, ns_string, rect,
    string_bounding_rect,
};
use crate::core_types::NSRange;
use crate::engine::display_map::RangeSet;
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{
    DownrightFragment, FragmentBehavior, FragmentContext, TableLayoutKey, clipped, draw_text, fill_rect,
};
use crate::render_contracts::{FragmentPayload, ThemeAppearance, attribute_keys};
use crate::swift_compat::{is_whitespace_or_newline, smax, smin, split_on_whitespace_characters};
use crate::theme::style_sheet::StyleSheet;

// MARK: - TableCellPresentation

/// `TableCellPresentation`: the semantic content of a Markdown table cell,
/// with the inline marker characters removed and the decoration attributes
/// intact.
pub struct TableCellPresentation;

impl TableCellPresentation {
    pub fn attributed_content(cell: &TableCell, storage: &NSAttributedString) -> Retained<NSAttributedString> {
        if !(cell.content_range.length > 0 && cell.content_range.upper_bound() <= storage.length() as isize) {
            return NSAttributedString::new();
        }
        let content = NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &storage.attributedSubstringFromRange(ns(cell.content_range)),
        );
        let mut markers: Vec<NSRange> = marker_ranges(&cell.inlines)
            .into_iter()
            .filter_map(|marker| {
                let intersection = upleft_core::ns_range::ns_intersection_range(marker, cell.content_range);
                if !(intersection.length > 0) {
                    return None;
                }
                Some(NSRange::new(intersection.location - cell.content_range.location, intersection.length))
            })
            .collect();
        markers.sort_by_key(|marker| std::cmp::Reverse(marker.location));
        for marker in markers {
            if marker.upper_bound() <= content.length() as isize {
                content.deleteCharactersInRange(ns(marker));
            }
        }
        // A cell with no inlines falls back to the cell's whole source range,
        // which for an empty cell reaches over the row's `|`.
        if cell.inlines.is_empty() {
            let husk = |unit: u16| -> bool {
                char::from_u32(unit as u32).is_some_and(|c| is_whitespace_or_newline(c) || c == '|')
            };
            loop {
                let length = content.length();
                if length == 0 {
                    break;
                }
                // `content.string.unicodeScalars.last`: a trailing surrogate
                // is a non-BMP scalar or a repaired U+FFFD, never husk.
                let last = content.string().characterAtIndex(length - 1);
                if !husk(last) {
                    break;
                }
                content.deleteCharactersInRange(objc2_foundation::NSRange::new(length - 1, 1));
            }
            loop {
                if content.length() == 0 {
                    break;
                }
                let first = content.string().characterAtIndex(0);
                if !husk(first) {
                    break;
                }
                content.deleteCharactersInRange(objc2_foundation::NSRange::new(0, 1));
            }
        }
        Retained::into_super(content)
    }

    pub fn plain_text(cell: &TableCell, storage: &NSAttributedString) -> String {
        upleft_swift_text::ns::foundation::to_string(&Self::attributed_content(cell, storage).string())
    }
}

fn marker_ranges(spans: &[InlineSpan]) -> Vec<NSRange> {
    fn collect(span: &InlineSpan, result: &mut Vec<NSRange>) {
        result.extend(span.marker_ranges());
        for child in &span.children {
            collect(child, result);
        }
    }
    let mut result = Vec::new();
    for span in spans {
        collect(span, &mut result);
    }
    RangeSet::normalized(&result)
}

// MARK: - TableLayout

/// Column geometry for one table, computed once and shared by its rows.
#[derive(Debug, Clone, PartialEq)]
pub struct TableLayout {
    pub column_x: Vec<CGFloat>,
    pub column_widths: Vec<CGFloat>,
    pub alignments: Vec<NSTextAlignment>,
    pub row_heights: Vec<CGFloat>,
    pub total_width: CGFloat,
    pub is_stacked: bool,
    pub stacked_label_width: CGFloat,
    /// Cell font size as a fraction of the body font.
    pub body_font_scale: CGFloat,
}

/// `TableLayout.ColumnDemand`.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDemand {
    pub natural: Vec<CGFloat>,
    pub minimums: Vec<CGFloat>,
    pub prose: Vec<Vec<bool>>,
}

/// `[NSAttributedString.Key: Any]` for the measuring passes.
type Attributes = Retained<NSDictionary<NSString, AnyObject>>;

fn attributes(pairs: &[(&NSString, &AnyObject)]) -> Attributes {
    attributes_dictionary(pairs)
}

/// `NSAttributedString(string:attributes:).size()`.
fn measured_size(text: &str, attributes: &Attributes) -> CGSize {
    // SAFETY: every value is an Objective-C object of the type its key expects.
    let string = unsafe {
        NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns_string(text), Some(attributes))
    };
    string.size()
}

/// `(text as NSString).boundingRect(with:options:attributes:)`'s height.
fn bounding_height(text: &str, width: CGFloat, height: CGFloat, attributes: &Attributes) -> CGFloat {
    string_bounding_rect(text, CGSize::new(width, height), attributes).height()
}

/// `widths.reduce(0, +)`.
fn total(values: &[CGFloat]) -> CGFloat {
    values.iter().fold(0.0, |partial, value| partial + value)
}

fn kern_key() -> &'static NSString {
    // SAFETY: AppKit exports the key as an immutable global.
    unsafe { NSKernAttributeName }
}

impl TableLayout {
    /// Floor for `bodyFontScale`.
    pub const MINIMUM_CELL_SCALE: CGFloat = 0.8;

    /// Where a cell stops being a label and becomes prose, in average
    /// characters.
    const PROSE_CHARACTERS: CGFloat = 25.0;

    /// `TableLayout.make(data:storage:width:style:)`.
    pub fn make(data: &TableData, storage: &NSAttributedString, width: CGFloat, style: &StyleSheet) -> TableLayout {
        let columns = data.column_count().max(1) as usize;
        let cell_text: Vec<Vec<String>> = data
            .rows
            .iter()
            .map(|row| row.cells.iter().take(columns).map(|cell| TableCellPresentation::plain_text(cell, storage)).collect())
            .collect();

        let gaps = render_metrics::TABLE_COLUMN_GAP * (columns as CGFloat - 1.0);
        // A table may wrap aggressively but never claims a wider fragment.
        let available = smax(1.0, width - gaps);
        let floor = smin(28.0, available / columns as CGFloat);

        let mut scale: CGFloat = 1.0;
        let mut demand = Self::column_demand(&data.rows, &cell_text, columns, style, scale, None);
        let mut widths: Vec<CGFloat> = demand.minimums.iter().map(|&minimum| smax(floor, minimum)).collect();
        // Label or prose is a property of the cell, not of the size tried.
        let prose = demand.prose.clone();
        // Fields can only be stacked under labels that exist.
        let labels = Self::stacked_labels(
            data.rows.iter().position(|row| row.is_header).map(|index| cell_text[index].as_slice()),
            columns as isize,
        );
        let can_stack = labels.iter().any(Option::is_some);

        if total(&widths) > available {
            let body = style.body_font().pointSize();
            let wanted = available / smax(1.0, total(&widths));
            // Half-point steps.
            let estimate = (body * wanted * 2.0).floor() / 2.0;
            let smallest = (body * Self::MINIMUM_CELL_SCALE).round();
            let mut sizes: Vec<CGFloat> = [smax(estimate, smallest), smallest].into_iter().filter(|&size| size < body).collect();
            if sizes.len() == 2 && sizes[0] == sizes[1] {
                sizes.pop();
            }
            for (position, &size) in sizes.iter().enumerate() {
                let tighter = Self::column_demand(&data.rows, &cell_text, columns, style, size / body, Some(&prose));
                let tighter_widths: Vec<CGFloat> = tighter.minimums.iter().map(|&minimum| smax(floor, minimum)).collect();
                let is_last = position + 1 == sizes.len();
                if !(total(&tighter_widths) <= available || (!can_stack && is_last)) {
                    continue;
                }
                scale = size / body;
                demand = tighter;
                widths = tighter_widths;
                break;
            }
        }

        let minimum_total = total(&widths);
        let mut is_stacked = minimum_total > available && can_stack;
        let degraded = scale < 1.0 || minimum_total > available;
        if is_stacked {
            // Transpose rows into labeled fields on a narrow measure.
            widths = vec![available / columns as CGFloat; columns];
        } else if minimum_total > available {
            // Nothing to stack under: the columns stay and the cells take the
            // loss.
            widths = Self::squeezed(&widths, available, floor);
        } else if total(&demand.natural) > 0.0 {
            // Give remaining space only to columns that can use it.
            let mut remaining = available - minimum_total;
            let mut unmet: Vec<CGFloat> =
                (0..demand.natural.len()).map(|index| smax(0.0, demand.natural[index] - widths[index])).collect();
            while remaining > 0.5 {
                let wanted = total(&unmet);
                if !(wanted > 0.5) {
                    break;
                }
                let budget = remaining;
                for index in 0..widths.len() {
                    if !(unmet[index] > 0.0) {
                        continue;
                    }
                    let addition = smin(unmet[index], budget * (unmet[index] / wanted));
                    widths[index] += addition;
                    unmet[index] -= addition;
                    remaining -= addition;
                }
            }
        } else {
            widths = vec![available / columns as CGFloat; columns];
        }

        // A degraded layout that clips gives way to the stack whenever there
        // are labels to stack under.
        if degraded
            && !is_stacked
            && can_stack
            && !Self::fits_without_clipping(&data.rows, &cell_text, &widths, style, scale)
        {
            is_stacked = true;
            widths = vec![available / columns as CGFloat; columns];
        }

        let mut xs: Vec<CGFloat> = Vec::with_capacity(widths.len());
        let mut cursor: CGFloat = 0.0;
        for &column_width in &widths {
            xs.push(cursor);
            cursor += column_width + render_metrics::TABLE_COLUMN_GAP;
        }

        let alignments = Self::alignments(data, &cell_text, columns);

        let stacked_label_width = smin(150.0, smax(72.0, width * 0.30));
        let body_font = Self::body_font(style, scale);
        let cell_attributes = attributes(&[(keys::font(), &body_font)]);
        let header_attributes = Self::header_attributes(style, scale);
        let mut row_heights: Vec<CGFloat> = Vec::with_capacity(data.rows.len());
        let grid = smax(1.0, style.baseline_grid);
        for (row_index, row) in data.rows.iter().enumerate() {
            if is_stacked {
                if row.is_header {
                    row_heights.push(0.0);
                    continue;
                }
                let cells_height = cell_text[row_index].iter().enumerate().fold(0.0, |partial, (offset, value)| {
                    let has_label = offset < labels.len() && labels[offset].is_some();
                    partial
                        + Self::stacked_cell_height(
                            value,
                            Self::stacked_field(has_label, stacked_label_width, width).1,
                            &style.body_font(),
                            style,
                        )
                });
                row_heights.push(render_metrics::snap_up(cells_height + render_metrics::TABLE_ROW_PADDING * 2.0, grid));
                continue;
            }
            let mut lines: CGFloat = 1.0;
            for (index, value) in cell_text[row_index].iter().enumerate() {
                if index >= widths.len() {
                    continue;
                }
                let height = bounding_height(
                    value,
                    widths[index],
                    style.line_height * 3.0,
                    if row.is_header { &header_attributes } else { &cell_attributes },
                );
                lines = smax(lines, smin(3.0, (height / smax(1.0, style.line_height)).ceil()));
            }
            row_heights.push(render_metrics::snap_up(
                style.line_height * lines + render_metrics::TABLE_ROW_PADDING * 2.0,
                grid,
            ));
        }
        TableLayout {
            column_x: xs,
            column_widths: widths,
            alignments,
            row_heights,
            total_width: smin(width, smax(0.0, cursor - render_metrics::TABLE_COLUMN_GAP)),
            is_stacked,
            stacked_label_width,
            body_font_scale: scale,
        }
    }

    fn body_font(style: &StyleSheet, scale: CGFloat) -> Retained<NSFont> {
        let body = style.body_font();
        if !(scale < 1.0) {
            return body;
        }
        body.fontWithSize(body.pointSize() * scale)
    }

    /// `TableLayout.headerLabel(style:scale:)`: the face and tracking a header
    /// cell is drawn at, so measuring and drawing agree.
    pub fn header_label(style: &StyleSheet, scale: CGFloat) -> (Retained<NSFont>, CGFloat) {
        let body = style.body_font().pointSize();
        (style.emphasis_font(true, false).fontWithSize(body * 0.85 * scale), body * 0.06 * scale)
    }

    fn header_attributes(style: &StyleSheet, scale: CGFloat) -> Attributes {
        let (font, kern) = Self::header_label(style, scale);
        let kern = NSNumber::new_f64(kern);
        attributes(&[(keys::font(), &font), (kern_key(), &kern)])
    }

    /// `TableLayout.columnDemand(rows:cellText:columns:style:scale:prose:)`.
    pub fn column_demand(
        rows: &[TableRow],
        cell_text: &[Vec<String>],
        columns: usize,
        style: &StyleSheet,
        scale: CGFloat,
        prose: Option<&Vec<Vec<bool>>>,
    ) -> ColumnDemand {
        let measurement_safety: CGFloat = 2.0;
        let mut natural = vec![0.0; columns];
        let mut minimums = vec![0.0; columns];
        let mut verdicts: Vec<Vec<bool>> =
            prose.cloned().unwrap_or_else(|| cell_text.iter().map(|row| vec![false; row.len()]).collect());
        let decide = prose.is_none();
        let prose_width = style.average_character_width * Self::PROSE_CHARACTERS;
        let body_font = Self::body_font(style, scale);
        let cell_attributes = attributes(&[(keys::font(), &body_font)]);
        let header_attributes = Self::header_attributes(style, scale);
        for (row_index, row) in rows.iter().enumerate() {
            if row_index >= cell_text.len() {
                continue;
            }
            let attributes = if row.is_header { &header_attributes } else { &cell_attributes };
            for (index, text) in cell_text[row_index].iter().enumerate() {
                if index >= columns {
                    continue;
                }
                // Core Text's fractional advance is not a safe clipping width.
                let measured = measured_size(text, attributes).width.ceil() + measurement_safety;
                natural[index] = smax(natural[index], measured);
                let has_verdict = row_index < verdicts.len() && index < verdicts[row_index].len();
                if decide && has_verdict {
                    verdicts[row_index][index] = measured > prose_width;
                }
                if !(has_verdict && verdicts[row_index][index]) {
                    minimums[index] = smax(minimums[index], measured);
                    continue;
                }
                // Only prose pays for the token scan.
                let longest_token = split_on_whitespace_characters(text)
                    .into_iter()
                    .map(|token| measured_size(token, attributes).width.ceil() + measurement_safety)
                    .reduce(|best, width| if width > best { width } else { best })
                    .unwrap_or(0.0);
                minimums[index] = smax(minimums[index], longest_token);
            }
        }
        ColumnDemand { natural, minimums, prose: verdicts }
    }

    /// `TableLayout.fitsWithoutClipping(rows:cellText:widths:style:scale:)`:
    /// true when every cell fits inside the three-line cap at these widths.
    pub fn fits_without_clipping(
        rows: &[TableRow],
        cell_text: &[Vec<String>],
        widths: &[CGFloat],
        style: &StyleSheet,
        scale: CGFloat,
    ) -> bool {
        let cap = style.line_height * 3.0 + 0.5;
        let body_font = Self::body_font(style, scale);
        let cell_attributes = attributes(&[(keys::font(), &body_font)]);
        let header_attributes = Self::header_attributes(style, scale);
        for (row_index, row) in rows.iter().enumerate() {
            if row_index >= cell_text.len() {
                continue;
            }
            for (index, value) in cell_text[row_index].iter().enumerate() {
                if !(index < widths.len() && !value.is_empty()) {
                    continue;
                }
                let height = bounding_height(
                    value,
                    widths[index],
                    CGFloat::MAX,
                    if row.is_header { &header_attributes } else { &cell_attributes },
                );
                if height > cap {
                    return false;
                }
            }
        }
        true
    }

    /// Shrinks columns in proportion to what each asked for, pinning any that
    /// would fall under the floor.
    fn squeezed(widths: &[CGFloat], available: CGFloat, floor: CGFloat) -> Vec<CGFloat> {
        let mut pinned: BTreeSet<usize> = BTreeSet::new();
        let mut result = widths.to_vec();
        for _ in 0..widths.len() {
            let budget = available - floor * pinned.len() as CGFloat;
            let flexible = (0..widths.len())
                .filter(|index| !pinned.contains(index))
                .fold(0.0, |partial, index| partial + widths[index]);
            if !(flexible > 0.5 && budget > 0.0) {
                break;
            }
            let factor = budget / flexible;
            result = (0..widths.len())
                .map(|index| if pinned.contains(&index) { floor } else { widths[index] * factor })
                .collect();
            let starved: Vec<usize> =
                (0..result.len()).filter(|index| !pinned.contains(index) && result[*index] < floor).collect();
            if starved.is_empty() {
                break;
            }
            pinned.extend(starved);
        }
        result
    }

    /// `TableLayout.stackedLabels(header:columns:)`: header text per column,
    /// `None` where there is no header row or the cell is empty.
    pub fn stacked_labels(header: Option<&[String]>, columns: isize) -> Vec<Option<String>> {
        (0..columns.max(1) as usize)
            .map(|index| {
                let header = header?;
                if index >= header.len() {
                    return None;
                }
                let trimmed = upleft_swift_text::trim_whitespaces(&header[index]);
                if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
            })
            .collect()
    }

    /// `TableLayout.stackedField(hasLabel:labelWidth:in:)`: the value's
    /// offset and width.
    pub fn stacked_field(has_label: bool, label_width: CGFloat, width: CGFloat) -> (CGFloat, CGFloat) {
        if !has_label {
            return (0.0, smax(40.0, width));
        }
        let offset = label_width + render_metrics::TABLE_COLUMN_GAP;
        (offset, smax(40.0, width - offset))
    }

    fn alignments(data: &TableData, cell_text: &[Vec<String>], columns: usize) -> Vec<NSTextAlignment> {
        let mut numeric = vec![0isize; columns];
        let mut counted = vec![0isize; columns];
        for (row_index, row) in data.rows.iter().enumerate() {
            if row.is_header || row_index >= cell_text.len() {
                continue;
            }
            for (index, text) in cell_text[row_index].iter().enumerate() {
                if index >= columns {
                    continue;
                }
                let trimmed = upleft_swift_text::trim_whitespaces(text);
                if trimmed.is_empty() {
                    continue;
                }
                counted[index] += 1;
                if is_numeric(trimmed) {
                    numeric[index] += 1;
                }
            }
        }
        (0..columns)
            .map(|index| {
                let declared = if index < data.alignments.len() { data.alignments[index] } else { TableAlignment::None };
                match declared {
                    TableAlignment::Left => NSTextAlignment::Left,
                    TableAlignment::Center => NSTextAlignment::Center,
                    TableAlignment::Right => NSTextAlignment::Right,
                    TableAlignment::None => {
                        let is_numeric_column = counted[index] > 0 && numeric[index] * 5 >= counted[index] * 3;
                        if is_numeric_column { NSTextAlignment::Right } else { NSTextAlignment::Left }
                    }
                }
            })
            .collect()
    }

    /// `TableLayout.stackedCellHeight(value:width:font:style:)`.
    pub fn stacked_cell_height(value: &str, width: CGFloat, font: &NSFont, style: &StyleSheet) -> CGFloat {
        let height = bounding_height(value, width, style.line_height * 4.0, &attributes(&[(keys::font(), font)]));
        let lines = smin(4.0, smax(1.0, (height / smax(1.0, style.line_height)).ceil()));
        lines * style.line_height + style.baseline_grid
    }
}

/// `TableLayout.isNumeric(_:)`.
fn is_numeric(text: &str) -> bool {
    let mut stripped = text.to_owned();
    for token in ["$", "%", ",", "€", "£", "+"] {
        stripped = upleft_swift_text::replacing_occurrences(&stripped, token, "");
    }
    let stripped = upleft_swift_text::trim_whitespaces(&stripped);
    if stripped.is_empty() {
        return false;
    }
    upleft_core::editing::front_matter_editing::swift_double_parses(stripped)
}

// MARK: - TableRowFragment

/// `TableRowFragment`'s stored properties and hooks.
///
/// Upleft: the table and the row are resolved again from the storage every
/// time (`current`). TextKit lays a row's fragment out again when its
/// element did not change instead of making a new one, so a row laid out
/// after rows were added to the table (a streamed answer) would otherwise
/// measure the table as it was when the fragment was made, and every row
/// shares one layout.
pub struct TableRowFragment {
    data: RefCell<Rc<TableData>>,
    row_index: Cell<isize>,
}

/// `TableRowFragment.make(textElement:range:payload:context:)`: `None` when
/// the payload carries no table geometry.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Option<Retained<NSTextLayoutFragment>> {
    let data = payload.table_data()?;
    // Resolved against the element's own range so a row keeps its identity
    // when the table moves.
    let element_start = element_start(text_element).unwrap_or_else(|| payload.source_range().location);
    let row_index = row_containing(&data, element_start);
    Some(Retained::into_super(DownrightFragment::new(
        c"TableRowFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(TableRowFragment { data: RefCell::new(data), row_index: Cell::new(row_index) }),
    )))
}

impl FragmentBehavior for TableRowFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }

    fn override_height(&self, fragment: &DownrightFragment) -> Option<CGFloat> {
        let (data, row_index) = self.current(fragment);
        if row(&data, row_index).is_none() {
            return Some(0.0);
        }
        let Some(layout) = self.layout(fragment) else { return Some(0.0) };
        if !(row_index < layout.row_heights.len() as isize) {
            return Some(0.0);
        }
        Some(layout.row_heights[row_index as usize])
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        let Some(style) = fragment.style_sheet() else { return };
        let Some(layout) = self.layout(fragment) else { return };
        let (data, row_index) = self.current(fragment);
        let Some(row) = row(&data, row_index) else { return };
        let row_height = if row_index < layout.row_heights.len() as isize {
            layout.row_heights[row_index as usize]
        } else {
            style.line_height
        };
        let frame = rect(point.x, point.y, smax(fragment.content_width(), layout.total_width), row_height);
        let context = fragment.context();

        if row.is_header {
            // A quiet header wash groups labels with their columns.
            fill_rect(cg, frame, &style.surface.colorWithAlphaComponent(0.62), 0.0);
        } else if context.as_ref().is_some_and(|context| context.hovered_table_row.get() == Some(row.range)) {
            let hover_alpha: CGFloat = if style.theme.appearance == ThemeAppearance::Light { 0.038 } else { 0.055 };
            fill_rect(cg, frame, &style.text.colorWithAlphaComponent(hover_alpha), 3.0);
        }

        if layout.is_stacked {
            self.draw_stacked_row(fragment, &data, row, frame, &layout, &style, cg);
            return;
        }

        let storage = context.as_ref().and_then(|context| context.storage());
        for (index, cell) in row.cells.iter().enumerate() {
            if index >= layout.column_x.len() {
                continue;
            }
            let source = match &storage {
                Some(storage) => TableCellPresentation::attributed_content(cell, storage),
                None => NSAttributedString::new(),
            };
            // Header cells keep their author's casing.
            let text = NSMutableAttributedString::initWithAttributedString(NSMutableAttributedString::alloc(), &source);
            if text.length() == 0 {
                continue;
            }
            let paragraph = NSMutableParagraphStyle::new();
            paragraph.setAlignment(layout.alignments[index]);
            paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            paragraph.setFirstLineHeadIndent(0.0);
            paragraph.setHeadIndent(0.0);
            let whole = objc2_foundation::NSRange::new(0, text.length());
            // SAFETY: a paragraph style is the value the key expects.
            unsafe { text.addAttribute_value_range(keys::paragraph_style(), &paragraph, whole) };
            if row.is_header {
                apply_header_treatment(&text, &style, layout.body_font_scale);
            } else if layout.body_font_scale < 1.0 {
                apply_cell_scale(layout.body_font_scale, &text, &style);
            }
            let cell_rect = rect(
                frame.min_x() + layout.column_x[index],
                frame.min_y() + render_metrics::TABLE_ROW_PADDING,
                layout.column_widths[index],
                frame.height() - render_metrics::TABLE_ROW_PADDING,
            );
            // Anything past three lines ends in an ellipsis.
            let visible = clipped(&text, cell_rect.height(), layout.column_widths[index]);
            draw_text(cg, &visible, cell_rect, true);
        }

        // One rule under the header.
        if row.is_header {
            fill_rect(
                cg,
                rect(
                    frame.min_x(),
                    frame.max_y() - render_metrics::TABLE_RULE_WIDTH,
                    frame.width(),
                    render_metrics::TABLE_RULE_WIDTH,
                ),
                &style.rule,
                0.0,
            );
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn element_start(text_element: &NSTextElement) -> Option<isize> {
    let (element_range, manager) = (text_element.elementRange()?, text_element.textContentManager()?);
    Some(manager.offsetFromLocation_toLocation(&manager.documentRange().location(), &element_range.location()))
}

fn row_containing(data: &TableData, offset: isize) -> isize {
    data.rows.iter().position(|row| row.range.contains(offset)).map_or(-1, |index| index as isize)
}

fn row(data: &TableData, index: isize) -> Option<&TableRow> {
    if index >= 0 && index < data.rows.len() as isize { Some(&data.rows[index as usize]) } else { None }
}

impl TableRowFragment {
    /// The table this row belongs to as the storage has it now, and the
    /// row's index in it.
    fn current(&self, fragment: &DownrightFragment) -> (Rc<TableData>, isize) {
        let fresh = fragment.context().and_then(|context| {
            let storage = context.storage()?;
            let element = fragment.textElement()?;
            let start = element_start(&element)?;
            if !(start >= 0 && start < storage.length() as isize) {
                return None;
            }
            let payload = attribute_value(&storage, attribute_keys::dr_fragment(), start as usize)?
                .downcast::<FragmentPayload>()
                .ok()?;
            Some((payload.table_data()?, start))
        });
        if let Some((data, start)) = fresh
            && !Rc::ptr_eq(&data, &self.data.borrow())
        {
            self.row_index.set(row_containing(&data, start));
            *self.data.borrow_mut() = data;
        }
        (self.data.borrow().clone(), self.row_index.get())
    }

    /// The table's shared geometry, from the context's cache.
    pub fn layout(&self, fragment: &DownrightFragment) -> Option<Rc<TableLayout>> {
        let context = fragment.context()?;
        let style = fragment.style_sheet()?;
        let storage = context.storage()?;
        // A table is a full-bleed block: `contentWidth` is the table's to use.
        let width = fragment.content_width();
        let (data, _) = self.current(fragment);
        let key = TableLayoutKey {
            location: data.rows.first().map_or(fragment.payload().source_range().location, |row| row.range.location),
            width: crate::swift_compat::int_truncating((width * 4.0).round()) as isize,
            text_revision: context.text_revision.get(),
        };
        if let Some(cached) = context.table_layouts.borrow().get(&key)
            && let Ok(layout) = cached.clone().downcast::<TableLayout>()
        {
            return Some(layout);
        }
        let made = Rc::new(TableLayout::make(&data, &storage, width, &style));
        context.table_layouts.borrow_mut().insert(key, made.clone() as Rc<dyn Any>);
        Some(made)
    }

    fn draw_stacked_row(
        &self,
        fragment: &DownrightFragment,
        data: &TableData,
        row: &TableRow,
        frame: objc2_core_foundation::CGRect,
        layout: &TableLayout,
        style: &StyleSheet,
        cg: &CGContext,
    ) {
        if row.is_header {
            return;
        }
        let Some(storage) = fragment.context().and_then(|context| context.storage()) else { return };
        let header_text: Option<Vec<String>> = data
            .rows
            .iter()
            .find(|row| row.is_header)
            .map(|header| header.cells.iter().map(|cell| TableCellPresentation::plain_text(cell, &storage)).collect());
        let labels = TableLayout::stacked_labels(header_text.as_deref(), data.column_count());
        let mut y = frame.min_y() + render_metrics::TABLE_ROW_PADDING;

        for (index, cell) in row.cells.iter().enumerate() {
            let label = if index < labels.len() { labels[index].as_deref() } else { None };
            let (value_offset, value_width) =
                TableLayout::stacked_field(label.is_some(), layout.stacked_label_width, frame.width());
            let value_x = frame.min_x() + value_offset;
            let value = NSMutableAttributedString::initWithAttributedString(
                NSMutableAttributedString::alloc(),
                &TableCellPresentation::attributed_content(cell, &storage),
            );
            let value_string = upleft_swift_text::ns::foundation::to_string(&value.string());
            let cell_height = TableLayout::stacked_cell_height(&value_string, value_width, &style.body_font(), style);

            // The author's casing is the label; a missing one is left out.
            if let Some(label) = label {
                let label_paragraph = NSMutableParagraphStyle::new();
                label_paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
                let font = NSFont::systemFontOfSize_weight(
                    smax(10.0, style.body_font().pointSize() * 0.72),
                    unsafe { NSFontWeightSemibold },
                );
                let kern = NSNumber::new_f64(style.body_font().pointSize() * 0.045);
                let label_text = attributed_string(
                    label,
                    &[
                        (keys::font(), &font),
                        (keys::foreground_color(), &style.text_secondary),
                        (kern_key(), &kern),
                        (keys::paragraph_style(), &label_paragraph),
                    ],
                );
                draw_text(
                    cg,
                    &label_text,
                    rect(frame.min_x(), y + 1.0, layout.stacked_label_width, style.line_height),
                    true,
                );
            }

            if value.length() > 0 {
                let paragraph = NSMutableParagraphStyle::new();
                paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
                // SAFETY: a paragraph style is the value the key expects.
                unsafe {
                    value.addAttribute_value_range(
                        keys::paragraph_style(),
                        &paragraph,
                        objc2_foundation::NSRange::new(0, value.length()),
                    )
                };
                // Stacked cells cap at four lines.
                let visible = clipped(&value, cell_height - style.baseline_grid, value_width);
                draw_text(cg, &visible, rect(value_x, y, value_width, cell_height), true);
            }
            y += cell_height;
        }

        fill_rect(
            cg,
            rect(
                frame.min_x(),
                frame.max_y() - render_metrics::TABLE_RULE_WIDTH,
                frame.width(),
                render_metrics::TABLE_RULE_WIDTH,
            ),
            &style.rule.colorWithAlphaComponent(0.65),
            0.0,
        );
    }
}

/// Label treatment for a header cell, applied per font run so inline spans
/// survive.
fn apply_header_treatment(text: &NSMutableAttributedString, style: &StyleSheet, scale: CGFloat) {
    let whole = objc2_foundation::NSRange::new(0, text.length());
    let (label_font, kern) = TableLayout::header_label(style, scale);
    let size = label_font.pointSize();
    let kern = NSNumber::new_f64(kern);
    let added = attributes(&[(keys::foreground_color(), &style.text_secondary), (kern_key(), &kern)]);
    // SAFETY: every value is an Objective-C object of the type its key expects.
    unsafe { text.addAttributes_range(&added, whole) };
    enumerate_attribute(text, keys::font(), whole, false, |value, range| {
        let font = value
            .and_then(|value| value.downcast_ref::<NSFont>())
            .map(|font| font.retain())
            .unwrap_or_else(|| style.body_font());
        let label = if font.isFixedPitch() {
            style.mono_font(Some(size))
        } else {
            let italic = font.fontDescriptor().symbolicTraits().contains(NSFontDescriptorSymbolicTraits::TraitItalic);
            style.emphasis_font(true, italic).fontWithSize(size)
        };
        // SAFETY: a font is the value the key expects.
        unsafe { text.addAttribute_value_range(keys::font(), &label, range) };
        true
    });
}

/// Brings a body cell down to the size its column was measured at, per font
/// run.
fn apply_cell_scale(scale: CGFloat, text: &NSMutableAttributedString, style: &StyleSheet) {
    let whole = objc2_foundation::NSRange::new(0, text.length());
    enumerate_attribute(text, keys::font(), whole, false, |value, range| {
        let font = value
            .and_then(|value| value.downcast_ref::<NSFont>())
            .map(|font| font.retain())
            .unwrap_or_else(|| style.body_font());
        let scaled = font.fontWithSize(font.pointSize() * scale);
        // SAFETY: a font is the value the key expects.
        unsafe { text.addAttribute_value_range(keys::font(), &scaled, range) };
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_on_whitespace_omits_empty_pieces() {
        assert_eq!(split_on_whitespace_characters("  a bc\u{3000}d  "), vec!["a", "bc", "d"]);
        assert_eq!(split_on_whitespace_characters(""), Vec::<&str>::new());
    }

    #[test]
    fn numeric_cells() {
        assert!(is_numeric("$1,200"));
        assert!(is_numeric("12%"));
        assert!(is_numeric("+3.5"));
        assert!(!is_numeric("n/a"));
        assert!(!is_numeric("$"));
    }
}
