//! Port of `RenderContracts.swift`: the render layer's stable surface.
//!
//! §3.2's "one text surface, three modes" is enforced here: a mode is a
//! `DecorationPolicy` value, not a separate code path.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::NSColor;
use objc2_foundation::{NSString, ns_string};

use crate::core_types::{BlockIdentity, InlineSpan, NSRange, TableCell, TableData, TableRow};
use crate::engine::render_metrics;
use crate::swift_compat::{self, json};

// MARK: - Modes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderMode {
    Read,
    Live,
    Source,
}

impl RenderMode {
    pub const ALL_CASES: [RenderMode; 3] = [RenderMode::Read, RenderMode::Live, RenderMode::Source];

    /// Modes exposed by the app. `read` remains decodable so old document
    /// state keeps working, but it migrates to the editable document view.
    pub const USER_FACING_MODES: [RenderMode; 2] = [RenderMode::Live, RenderMode::Source];

    pub const fn raw_value(self) -> &'static str {
        match self {
            RenderMode::Read => "read",
            RenderMode::Live => "live",
            RenderMode::Source => "source",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<RenderMode> {
        RenderMode::ALL_CASES
            .into_iter()
            .find(|mode| mode.raw_value() == raw)
    }

    pub fn normalized_for_editing(self) -> RenderMode {
        if self == RenderMode::Read {
            RenderMode::Live
        } else {
            self
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            RenderMode::Read => "Read",
            RenderMode::Live => "Document",
            RenderMode::Source => "Source",
        }
    }

    pub fn policy(self) -> DecorationPolicy {
        match self {
            RenderMode::Read => {
                DecorationPolicy::new(false, true, true, false, false, false, true, true)
            }
            RenderMode::Live => {
                DecorationPolicy::new(true, true, true, true, true, false, true, false)
            }
            RenderMode::Source => {
                DecorationPolicy::new(true, false, false, false, false, true, false, false)
            }
        }
    }
}

/// Temporary source visibility inside the one editable document surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFocus {
    None,
    Scoped(NSRange),
    Document,
}

impl SourceFocus {
    pub fn range(&self) -> Option<NSRange> {
        match self {
            SourceFocus::Scoped(range) => Some(*range),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecorationPolicy {
    pub shows_insertion_point: bool,
    /// `#`, `>`, `-`, `1.` removed from the text run. In Live mode they
    /// reappear in the gutter (§6.1a), never inline.
    pub hides_block_markers: bool,
    pub hides_inline_markers: bool,
    /// Per-span reveal of inline markers around the caret (§6.1b).
    pub reveals_at_caret: bool,
    /// When enabled, secondary insertion carets reveal their own inline
    /// markers too.
    pub reveals_at_all_cursors: bool,
    pub shows_gutter_markers: bool,
    /// Source mode styles markers instead of hiding them.
    pub highlights_markers: bool,
    /// Math, mermaid, tables, images render as objects rather than source.
    pub renders_fragments: bool,
    pub collapses_long_code_blocks: bool,
}

impl DecorationPolicy {
    /// The Swift memberwise-style initialiser, `revealsAtAllCursors` defaulted
    /// to `false`.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        shows_insertion_point: bool,
        hides_block_markers: bool,
        hides_inline_markers: bool,
        reveals_at_caret: bool,
        shows_gutter_markers: bool,
        highlights_markers: bool,
        renders_fragments: bool,
        collapses_long_code_blocks: bool,
    ) -> Self {
        DecorationPolicy {
            shows_insertion_point,
            hides_block_markers,
            hides_inline_markers,
            reveals_at_caret,
            reveals_at_all_cursors: false,
            shows_gutter_markers,
            highlights_markers,
            renders_fragments,
            collapses_long_code_blocks,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkdownRevealPolicy {
    Never,
    PrimaryCaret,
    AllCursors,
}

impl MarkdownRevealPolicy {
    pub const ALL_CASES: [MarkdownRevealPolicy; 3] = [
        MarkdownRevealPolicy::Never,
        MarkdownRevealPolicy::PrimaryCaret,
        MarkdownRevealPolicy::AllCursors,
    ];

    pub const fn raw_value(self) -> &'static str {
        match self {
            MarkdownRevealPolicy::Never => "never",
            MarkdownRevealPolicy::PrimaryCaret => "primaryCaret",
            MarkdownRevealPolicy::AllCursors => "allCursors",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Self> {
        MarkdownRevealPolicy::ALL_CASES
            .into_iter()
            .find(|policy| policy.raw_value() == raw)
    }
}

/// Bounded renderer-owned controls for app controllers and extensions. The
/// two clamped fields are private so every write goes through the setter
/// that reproduces Swift's `didSet` clamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownRenderConfiguration {
    pub show_invisibles: bool,
    pub reveal_policy: MarkdownRevealPolicy,
    pub typographic_substitution: bool,
    pub typewriter_scrolling: bool,
    /// Presents source-wrapped prose as one visual paragraph without changing
    /// the Markdown bytes.
    pub reflow_hard_wrapped_paragraphs: bool,
    large_file_threshold_megabytes: i64,
    code_collapse_threshold: i64,
}

impl Default for MarkdownRenderConfiguration {
    fn default() -> Self {
        MarkdownRenderConfiguration::new(
            false,
            MarkdownRevealPolicy::PrimaryCaret,
            false,
            false,
            true,
            render_metrics::CODE_COLLAPSE_LINE_COUNT,
            5,
        )
    }
}

impl MarkdownRenderConfiguration {
    pub fn new(
        show_invisibles: bool,
        reveal_policy: MarkdownRevealPolicy,
        typographic_substitution: bool,
        typewriter_scrolling: bool,
        reflow_hard_wrapped_paragraphs: bool,
        code_collapse_threshold: i64,
        large_file_threshold_megabytes: i64,
    ) -> Self {
        MarkdownRenderConfiguration {
            show_invisibles,
            reveal_policy,
            typographic_substitution,
            typewriter_scrolling,
            reflow_hard_wrapped_paragraphs,
            code_collapse_threshold: 10_000.min(1.max(code_collapse_threshold)),
            large_file_threshold_megabytes: 1024.min(1.max(large_file_threshold_megabytes)),
        }
    }

    pub fn large_file_threshold_megabytes(&self) -> i64 {
        self.large_file_threshold_megabytes
    }

    pub fn set_large_file_threshold_megabytes(&mut self, value: i64) {
        self.large_file_threshold_megabytes = 1024.min(1.max(value));
    }

    pub fn code_collapse_threshold(&self) -> i64 {
        self.code_collapse_threshold
    }

    pub fn set_code_collapse_threshold(&mut self, value: i64) {
        self.code_collapse_threshold = 10_000.min(1.max(value));
    }
}

// MARK: - Custom attributes
//
// All decoration is carried as attributes on the text storage. Nothing dynamic
// goes through `NSTextLayoutManager.addRenderingAttribute`.

/// `NSAttributedString.Key` extensions, as compile-time `NSString` constants.
pub mod attribute_keys {
    use super::{NSString, ns_string};

    /// Marks a range as a syntax marker the display string omits.
    pub fn dr_hidden() -> &'static NSString {
        ns_string!("drHidden")
    }
    /// Marks a syntax marker that is currently *shown* so it can be dimmed.
    pub fn dr_marker() -> &'static NSString {
        ns_string!("drMarker")
    }
    /// `FragmentPayload` for ranges that render as an object.
    pub fn dr_fragment() -> &'static NSString {
        ns_string!("drFragment")
    }
    /// `BlockIdentity` of the owning block.
    pub fn dr_block() -> &'static NSString {
        ns_string!("drBlock")
    }
    /// Heading level, for the gutter and the breadcrumb.
    pub fn dr_heading() -> &'static NSString {
        ns_string!("drHeading")
    }
    /// `String` link destination.
    pub fn dr_link() -> &'static NSString {
        ns_string!("drLink")
    }
    /// `PathToken`, with `drPathExists` deciding the styling (§8.4).
    pub fn dr_path_token() -> &'static NSString {
        ns_string!("drPathToken")
    }
    pub fn dr_path_exists() -> &'static NSString {
        ns_string!("drPathExists")
    }
    /// `Bool` checkbox state, for hit testing a click on the glyph.
    pub fn dr_checkbox() -> &'static NSString {
        ns_string!("drCheckbox")
    }
    /// `ChangeKind.rawValue` for change highlighting (§8.1).
    pub fn dr_change() -> &'static NSString {
        ns_string!("drChange")
    }
    /// `String`: text an external write removed, held on the character at
    /// the join point (§8.1).
    pub fn dr_change_ghost() -> &'static NSString {
        ns_string!("drChangeGhost")
    }
    /// Footnote or reference-link identifier, for hover popovers.
    pub fn dr_reference() -> &'static NSString {
        ns_string!("drReference")
    }
    /// Set on ranges elided by structural zoom (§5.2) or folding.
    pub fn dr_elided() -> &'static NSString {
        ns_string!("drElided")
    }
    /// `String` gutter marker text drawn in the left rail in Live mode.
    pub fn dr_gutter_marker() -> &'static NSString {
        ns_string!("drGutterMarker")
    }
    /// Search-hit marking, kept distinct from selection.
    pub fn dr_search_hit() -> &'static NSString {
        ns_string!("drSearchHit")
    }
    pub fn dr_current_search_hit() -> &'static NSString {
        ns_string!("drCurrentSearchHit")
    }
    /// Current word spoken by the system speech synthesizer.
    pub fn dr_speech_highlight() -> &'static NSString {
        ns_string!("drSpeechHighlight")
    }
    /// Inline-code content that receives a padded rounded background.
    pub fn dr_inline_code() -> &'static NSString {
        ns_string!("drInlineCode")
    }
    /// Source range temporarily shown as a flat, monospaced editing region.
    pub fn dr_source_focus() -> &'static NSString {
        ns_string!("drSourceFocus")
    }
    /// Marks spaces and tabs when the host asks the renderer to show them.
    pub fn dr_invisible() -> &'static NSString {
        ns_string!("drInvisible")
    }

    /// Every key, in declaration order, with its raw value.
    pub const NAMES: [&str; 20] = [
        "drHidden",
        "drMarker",
        "drFragment",
        "drBlock",
        "drHeading",
        "drLink",
        "drPathToken",
        "drPathExists",
        "drCheckbox",
        "drChange",
        "drChangeGhost",
        "drReference",
        "drElided",
        "drGutterMarker",
        "drSearchHit",
        "drCurrentSearchHit",
        "drSpeechHighlight",
        "drInlineCode",
        "drSourceFocus",
        "drInvisible",
    ];
}

// MARK: - Fragments

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FragmentKind {
    CodeBlock,
    CollapsedCodeBlock,
    Table,
    InlineMath,
    BlockMath,
    Mermaid,
    Image,
    ThematicBreak,
    FrontMatter,
    Callout,
    ListOrnament,
}

impl FragmentKind {
    pub const ALL_CASES: [FragmentKind; 11] = [
        FragmentKind::CodeBlock,
        FragmentKind::CollapsedCodeBlock,
        FragmentKind::Table,
        FragmentKind::InlineMath,
        FragmentKind::BlockMath,
        FragmentKind::Mermaid,
        FragmentKind::Image,
        FragmentKind::ThematicBreak,
        FragmentKind::FrontMatter,
        FragmentKind::Callout,
        FragmentKind::ListOrnament,
    ];

    pub const fn raw_value(self) -> &'static str {
        match self {
            FragmentKind::CodeBlock => "codeBlock",
            FragmentKind::CollapsedCodeBlock => "collapsedCodeBlock",
            FragmentKind::Table => "table",
            FragmentKind::InlineMath => "inlineMath",
            FragmentKind::BlockMath => "blockMath",
            FragmentKind::Mermaid => "mermaid",
            FragmentKind::Image => "image",
            FragmentKind::ThematicBreak => "thematicBreak",
            FragmentKind::FrontMatter => "frontMatter",
            FragmentKind::Callout => "callout",
            FragmentKind::ListOrnament => "listOrnament",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<FragmentKind> {
        FragmentKind::ALL_CASES
            .into_iter()
            .find(|kind| kind.raw_value() == raw)
    }

    /// True when the fragment draws its own content instead of letting
    /// TextKit draw the element's glyphs.
    pub const fn replaces_glyphs(self) -> bool {
        match self {
            FragmentKind::Table
            | FragmentKind::CollapsedCodeBlock
            | FragmentKind::BlockMath
            | FragmentKind::Mermaid
            | FragmentKind::Image
            | FragmentKind::ThematicBreak
            | FragmentKind::FrontMatter => true,
            // Code keeps its glyphs, and a callout, a list ornament and inline
            // math are chrome drawn *around* real text.
            FragmentKind::CodeBlock
            | FragmentKind::Callout
            | FragmentKind::ListOrnament
            | FragmentKind::InlineMath => false,
        }
    }
}

/// Instance state of `FragmentPayload`. `kind`, `block_identity` and `detail`
/// are `let` in Swift; the rest are `var`.
pub struct FragmentPayloadIvars {
    kind: FragmentKind,
    source_range: Cell<NSRange>,
    block_identity: BlockIdentity,
    detail: String,
    table_data: RefCell<Option<TableData>>,
    is_collapsed: Cell<bool>,
}

define_class!(
    /// Payload attached to a range that draws as an object rather than as
    /// glyphs. A class so it can ride along as an attribute value without
    /// copying.
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - The ivars are plain Rust data with interior mutability; the class is
    //   not `Sync` because of it, so it stays on the thread that uses it
    //   (decoration and drawing both run on the main thread).
    // - FragmentPayload does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = AllocAnyThread]
    #[name = "FragmentPayload"]
    #[ivars = FragmentPayloadIvars]
    pub struct FragmentPayload;
);

impl FragmentPayload {
    pub fn new(
        kind: FragmentKind,
        source_range: NSRange,
        block_identity: BlockIdentity,
        detail: &str,
    ) -> Retained<Self> {
        let this = Self::alloc().set_ivars(FragmentPayloadIvars {
            kind,
            source_range: Cell::new(source_range),
            block_identity,
            detail: detail.to_owned(),
            table_data: RefCell::new(None),
            is_collapsed: Cell::new(false),
        });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn kind(&self) -> FragmentKind {
        self.ivars().kind
    }

    pub fn source_range(&self) -> NSRange {
        self.ivars().source_range.get()
    }

    pub fn set_source_range(&self, range: NSRange) {
        self.ivars().source_range.set(range);
    }

    pub fn block_identity(&self) -> BlockIdentity {
        self.ivars().block_identity
    }

    /// Fence language, mermaid diagram type, image path, LaTeX source …
    pub fn detail(&self) -> &str {
        &self.ivars().detail
    }

    /// Table geometry, populated by the table fragment.
    pub fn table_data(&self) -> std::cell::Ref<'_, Option<TableData>> {
        self.ivars().table_data.borrow()
    }

    pub fn set_table_data(&self, data: Option<TableData>) {
        *self.ivars().table_data.borrow_mut() = data;
    }

    pub fn is_collapsed(&self) -> bool {
        self.ivars().is_collapsed.get()
    }

    pub fn set_is_collapsed(&self, collapsed: bool) {
        self.ivars().is_collapsed.set(collapsed);
    }

    /// Carries absolute parser coordinates across the short interval between
    /// a source edit and the async parse that replaces this payload.
    pub fn project_source_ranges(&self, edit: NSRange, inserted_length: isize) {
        self.set_source_range(project_range(self.source_range(), edit, inserted_length));
        let Some(mut table_data) = self.table_data().clone() else {
            return;
        };
        table_data.delimiter_range =
            project_range(table_data.delimiter_range, edit, inserted_length);
        table_data.rows = table_data
            .rows
            .iter()
            .map(|row| TableRow {
                range: project_range(row.range, edit, inserted_length),
                cells: row
                    .cells
                    .iter()
                    .map(|cell| TableCell {
                        range: project_range(cell.range, edit, inserted_length),
                        content_range: project_range(cell.content_range, edit, inserted_length),
                        inlines: cell
                            .inlines
                            .iter()
                            .map(|span| project_span(span, edit, inserted_length))
                            .collect(),
                    })
                    .collect(),
                is_header: row.is_header,
            })
            .collect();
        self.set_table_data(Some(table_data));
    }
}

fn project_span(span: &InlineSpan, edit: NSRange, inserted_length: isize) -> InlineSpan {
    InlineSpan {
        kind: span.kind.clone(),
        range: project_range(span.range, edit, inserted_length),
        content_range: project_range(span.content_range, edit, inserted_length),
        leading_marker_range: span
            .leading_marker_range
            .map(|range| project_range(range, edit, inserted_length)),
        trailing_marker_range: span
            .trailing_marker_range
            .map(|range| project_range(range, edit, inserted_length)),
        children: span
            .children
            .iter()
            .map(|child| project_span(child, edit, inserted_length))
            .collect(),
    }
}

/// `FragmentPayload.project(_:across:insertedLength:)` on `NSRange`, in
/// Swift's signed `Int` arithmetic.
pub fn project_range(range: NSRange, edit: NSRange, inserted_length: isize) -> NSRange {
    let upper = range.upper_bound();
    let edit_upper = edit.upper_bound();
    let delta = inserted_length - edit.length;
    if upper <= edit.location {
        return range;
    }
    if range.location >= edit_upper {
        return NSRange::new(0.max(range.location + delta), range.length);
    }
    // The edit intersects this semantic range. Preserve the part on each side
    // and let the replacement occupy the intersected interval.
    let prefix = 0.max(edit.location - range.location);
    let suffix = 0.max(upper - edit_upper);
    NSRange::new(
        range.location.min(edit.location),
        prefix + inserted_length + suffix,
    )
}

// MARK: - Theme (§11.2)
//
// JSON, not CSS. Colours may be literal hex or a reference to an `NSColor`
// system colour, so a theme that opts into system colours adapts to light and
// dark, the accent colour, and Increase Contrast for free.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThemeColor {
    pub raw: String,
}

impl ThemeColor {
    pub fn new(raw: &str) -> Self {
        ThemeColor {
            raw: raw.to_owned(),
        }
    }

    /// `#rrggbb`, `#rrggbbaa`, or `system:<name>` naming an `NSColor` class
    /// property. Anything else falls back to `labelColor`.
    pub fn resolved(&self) -> Retained<NSColor> {
        if swift_compat::has_ascii_prefix(&self.raw, "system:") {
            let name = &self.raw["system:".len()..];
            if let Some(color) = system_color_named(name) {
                return color;
            }
            return NSColor::labelColor();
        }
        color_from_hex_string(&self.raw).unwrap_or_else(NSColor::labelColor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeAppearance {
    Light,
    Dark,
    Auto,
}

impl ThemeAppearance {
    pub const fn raw_value(self) -> &'static str {
        match self {
            ThemeAppearance::Light => "light",
            ThemeAppearance::Dark => "dark",
            ThemeAppearance::Auto => "auto",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Self> {
        [
            ThemeAppearance::Light,
            ThemeAppearance::Dark,
            ThemeAppearance::Auto,
        ]
        .into_iter()
        .find(|appearance| appearance.raw_value() == raw)
    }
}

macro_rules! color_struct {
    ($(#[$meta:meta])* $name:ident { $($field:ident : $key:literal),* $(,)? } $(optional { $($ofield:ident : $okey:literal),* })?) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            $(pub $field: ThemeColor,)*
            $($(pub $ofield: Option<ThemeColor>,)*)?
        }

        impl $name {
            /// Property names in declaration order (Swift's `Mirror` order).
            pub const FIELD_NAMES: &[&str] = &[$($key,)* $($($okey,)*)?];

            /// Every stored `ThemeColor`, in declaration order, with its label.
            /// `Mirror` hands an optional colour over as `Optional<ThemeColor>`,
            /// which `as? ThemeColor` unwraps: present, it is listed; `nil`,
            /// it is skipped.
            pub fn colors(&self) -> Vec<(&'static str, &ThemeColor)> {
                let mut out = Vec::new();
                $(out.push(($key, &self.$field));)*
                $($(if let Some(color) = &self.$ofield { out.push(($okey, color)); })*)?
                out
            }

            fn decode(value: &json::Value, path: &str) -> Result<Self, String> {
                let json::Value::Object(_) = value else {
                    return Err(format!("{path}: expected an object, found {}", value.kind()));
                };
                Ok($name {
                    $($field: decode_color(value, $key, path)?,)*
                    $($($ofield: decode_optional_color(value, $okey, path)?,)*)?
                })
            }

            fn encode(&self) -> Vec<(&'static str, EncodedValue)> {
                let mut pairs = Vec::new();
                $(pairs.push(($key, EncodedValue::String(self.$field.raw.clone())));)*
                $($(if let Some(color) = &self.$ofield {
                    pairs.push(($okey, EncodedValue::String(color.raw.clone())));
                })*)?
                pairs
            }
        }
    };
}

color_struct!(
    ThemePalette {
        background: "background",
        surface: "surface",
        text: "text",
        text_secondary: "textSecondary",
        text_faint: "textFaint",
        heading: "heading",
        marker: "marker",
        accent: "accent",
        link: "link",
        rule: "rule",
        selection: "selection",
        code_background: "codeBackground",
        inline_code_background: "inlineCodeBackground",
        code_rule: "codeRule",
        rail_tick: "railTick",
        rail_tick_current: "railTickCurrent",
        quote_rule: "quoteRule",
        change_added: "changeAdded",
        change_removed: "changeRemoved",
        change_modified: "changeModified",
        path_missing: "pathMissing",
        search_hit: "searchHit",
        search_hit_current: "searchHitCurrent",
        callout_note: "calloutNote",
        callout_warning: "calloutWarning",
        callout_success: "calloutSuccess",
        callout_danger: "calloutDanger",
    }
    optional {
        callout_important: "calloutImportant"
    }
);

color_struct!(CodeTheme {
    keyword: "keyword",
    string: "string",
    number: "number",
    comment: "comment",
    r#type: "type",
    function: "function",
    variable: "variable",
    constant: "constant",
    operator: "operator",
    punctuation: "punctuation",
    attribute: "attribute",
    diff_added: "diffAdded",
    diff_removed: "diffRemoved",
    diff_header: "diffHeader",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyPreset {
    /// New York: Apple's serif.
    Reading,
    /// SF Pro Text.
    Working,
}

impl BodyPreset {
    pub const ALL_CASES: [BodyPreset; 2] = [BodyPreset::Reading, BodyPreset::Working];

    pub const fn raw_value(self) -> &'static str {
        match self {
            BodyPreset::Reading => "reading",
            BodyPreset::Working => "working",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Self> {
        BodyPreset::ALL_CASES
            .into_iter()
            .find(|preset| preset.raw_value() == raw)
    }

    pub const fn title(self) -> &'static str {
        match self {
            BodyPreset::Reading => "Reading",
            BodyPreset::Working => "Working",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypographyConfig {
    pub preset: BodyPreset,
    pub body_size: f64,
    /// 1.2 / 1.25 / 1.333: one ratio drives every heading size (§11.1).
    pub scale_ratio: f64,
    pub line_height_multiple: f64,
    /// Capped at 68–72 characters (§11.1).
    pub measure_characters: f64,
    pub mono_family: String,
    /// Code size as a fraction of the body size.
    pub mono_size_adjust: f64,
    pub mono_ligatures: bool,
    /// Hanging punctuation and optical margin alignment (§11.1).
    pub optical_margins: bool,
    /// Math is sized against the body font's x-height (§11.3).
    pub math_scale: f64,
}

impl TypographyConfig {
    /// `TypographyConfig.default`.
    pub fn default_config() -> TypographyConfig {
        TypographyConfig {
            preset: BodyPreset::Reading,
            body_size: 16.0,
            scale_ratio: 1.25,
            line_height_multiple: 1.6,
            measure_characters: 70.0,
            mono_family: "SF Mono".to_owned(),
            mono_size_adjust: 0.88,
            mono_ligatures: false,
            optical_margins: true,
            math_scale: 1.0,
        }
    }

    fn decode(value: &json::Value, path: &str) -> Result<Self, String> {
        let json::Value::Object(_) = value else {
            return Err(format!(
                "{path}: expected an object, found {}",
                value.kind()
            ));
        };
        let preset_raw = decode_string(value, "preset", path)?;
        let preset = BodyPreset::from_raw_value(&preset_raw).ok_or_else(|| {
            format!(
                "{path}.preset: cannot initialize BodyPreset from invalid String value {preset_raw}"
            )
        })?;
        Ok(TypographyConfig {
            preset,
            body_size: decode_number(value, "bodySize", path)?,
            scale_ratio: decode_number(value, "scaleRatio", path)?,
            line_height_multiple: decode_number(value, "lineHeightMultiple", path)?,
            measure_characters: decode_number(value, "measureCharacters", path)?,
            mono_family: decode_string(value, "monoFamily", path)?,
            mono_size_adjust: decode_number(value, "monoSizeAdjust", path)?,
            mono_ligatures: decode_bool(value, "monoLigatures", path)?,
            optical_margins: decode_bool(value, "opticalMargins", path)?,
            math_scale: decode_number(value, "mathScale", path)?,
        })
    }

    fn encode(&self) -> Vec<(&'static str, EncodedValue)> {
        vec![
            (
                "preset",
                EncodedValue::String(self.preset.raw_value().to_owned()),
            ),
            ("bodySize", EncodedValue::Number(self.body_size)),
            ("scaleRatio", EncodedValue::Number(self.scale_ratio)),
            (
                "lineHeightMultiple",
                EncodedValue::Number(self.line_height_multiple),
            ),
            (
                "measureCharacters",
                EncodedValue::Number(self.measure_characters),
            ),
            ("monoFamily", EncodedValue::String(self.mono_family.clone())),
            (
                "monoSizeAdjust",
                EncodedValue::Number(self.mono_size_adjust),
            ),
            ("monoLigatures", EncodedValue::Bool(self.mono_ligatures)),
            ("opticalMargins", EncodedValue::Bool(self.optical_margins)),
            ("mathScale", EncodedValue::Number(self.math_scale)),
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub appearance: ThemeAppearance,
    pub palette: ThemePalette,
    pub code: CodeTheme,
    pub typography: TypographyConfig,
}

impl Theme {
    /// `JSONDecoder().decode(Theme.self, from: data)`.
    pub fn decode_json(data: &[u8]) -> Result<Theme, String> {
        let value = json::parse(data)?;
        Theme::decode(&value)
    }

    fn decode(value: &json::Value) -> Result<Theme, String> {
        let json::Value::Object(_) = value else {
            return Err(format!("expected a Theme object, found {}", value.kind()));
        };
        let name = decode_string(value, "name", "")?;
        let appearance_raw = decode_string(value, "appearance", "")?;
        let appearance = ThemeAppearance::from_raw_value(&appearance_raw).ok_or_else(|| {
            format!("appearance: cannot initialize ThemeAppearance from invalid String value {appearance_raw}")
        })?;
        Ok(Theme {
            name,
            appearance,
            palette: ThemePalette::decode(required(value, "palette", "")?, "palette")?,
            code: CodeTheme::decode(required(value, "code", "")?, "code")?,
            typography: TypographyConfig::decode(required(value, "typography", "")?, "typography")?,
        })
    }

    /// `JSONEncoder` with `[.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]`,
    /// as `ThemeStore.export` configures it.
    pub fn encode_pretty_sorted(&self) -> String {
        let root = EncodedValue::Object(vec![
            ("name", EncodedValue::String(self.name.clone())),
            (
                "appearance",
                EncodedValue::String(self.appearance.raw_value().to_owned()),
            ),
            ("palette", EncodedValue::Object(self.palette.encode())),
            ("code", EncodedValue::Object(self.code.encode())),
            ("typography", EncodedValue::Object(self.typography.encode())),
        ]);
        let mut out = String::new();
        root.write_pretty(&mut out, 0);
        out
    }
}

// MARK: - Codable plumbing (synthesized `Decodable` over `JSONDecoder`)

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

fn required<'a>(object: &'a json::Value, key: &str, path: &str) -> Result<&'a json::Value, String> {
    match object.get(key) {
        None => Err(format!("{}: key not found", join(path, key))),
        Some(json::Value::Null) => Err(format!("{}: value not found (null)", join(path, key))),
        Some(value) => Ok(value),
    }
}

fn decode_string(object: &json::Value, key: &str, path: &str) -> Result<String, String> {
    match required(object, key, path)? {
        json::Value::String(text) => Ok(text.clone()),
        other => Err(format!(
            "{}: expected String, found {}",
            join(path, key),
            other.kind()
        )),
    }
}

fn decode_number(object: &json::Value, key: &str, path: &str) -> Result<f64, String> {
    required(object, key, path)?
        .as_f64()
        .map_err(|error| format!("{}: {error}", join(path, key)))
}

fn decode_bool(object: &json::Value, key: &str, path: &str) -> Result<bool, String> {
    match required(object, key, path)? {
        json::Value::Bool(flag) => Ok(*flag),
        other => Err(format!(
            "{}: expected Bool, found {}",
            join(path, key),
            other.kind()
        )),
    }
}

fn decode_color(object: &json::Value, key: &str, path: &str) -> Result<ThemeColor, String> {
    decode_string(object, key, path).map(|raw| ThemeColor { raw })
}

fn decode_optional_color(
    object: &json::Value,
    key: &str,
    path: &str,
) -> Result<Option<ThemeColor>, String> {
    match object.get(key) {
        None | Some(json::Value::Null) => Ok(None),
        Some(json::Value::String(raw)) => Ok(Some(ThemeColor { raw: raw.clone() })),
        Some(other) => Err(format!(
            "{}: expected String, found {}",
            join(path, key),
            other.kind()
        )),
    }
}

/// A value as `JSONEncoder` writes it.
pub(crate) enum EncodedValue {
    String(String),
    Number(f64),
    Bool(bool),
    Object(Vec<(&'static str, EncodedValue)>),
}

impl EncodedValue {
    fn write_pretty(&self, out: &mut String, indent: usize) {
        match self {
            EncodedValue::String(text) => write_json_string(out, text),
            EncodedValue::Number(value) => out.push_str(&json_encoder_double(*value)),
            EncodedValue::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
            EncodedValue::Object(pairs) => {
                let mut sorted: Vec<&(&str, EncodedValue)> = pairs.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(b.0));
                out.push_str("{\n");
                if sorted.is_empty() {
                    out.push('\n');
                }
                for (index, (key, value)) in sorted.iter().enumerate() {
                    out.push_str(&"  ".repeat(indent + 1));
                    write_json_string(out, key);
                    out.push_str(" : ");
                    value.write_pretty(out, indent + 1);
                    if index + 1 < sorted.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(indent));
                out.push('}');
            }
        }
    }
}

/// `JSONEncoder` string escaping with `.withoutEscapingSlashes`.
fn write_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Swift's `Double.description`: shortest round-trip digits, decimal notation
/// for magnitudes in [1e-4, 2^53], exponential (`1e+16`, `1e-05`) otherwise.
pub fn swift_double_description(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 {
            "-inf".into()
        } else {
            "inf".into()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // `{:e}` prints the shortest round-trip digits: "1.25e16", "-5e-324".
    let scientific = format!("{:e}", value);
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let magnitude = value.abs();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if magnitude <= 9007199254740992.0 && exponent >= -4 {
        if exponent < 0 {
            out.push_str("0.");
            for _ in 0..(-exponent - 1) {
                out.push('0');
            }
            out.push_str(&digits);
        } else {
            let integer_digits = exponent as usize + 1;
            if digits.len() <= integer_digits {
                out.push_str(&digits);
                for _ in digits.len()..integer_digits {
                    out.push('0');
                }
                out.push_str(".0");
            } else {
                out.push_str(&digits[..integer_digits]);
                out.push('.');
                out.push_str(&digits[integer_digits..]);
            }
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
    }
    out
}

/// `JSONEncoder`'s number text: `description` without a trailing `.0`.
pub fn json_encoder_double(value: f64) -> String {
    let text = swift_double_description(value);
    match text.strip_suffix(".0") {
        Some(stripped) => stripped.to_owned(),
        None => text,
    }
}

// MARK: - Colour helpers

/// `NSColor(hexString:)`.
pub fn color_from_hex_string(hex_string: &str) -> Option<Retained<NSColor>> {
    let (r, g, b, a) = hex_components(hex_string)?;
    Some(NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a))
}

/// The component arithmetic of `NSColor(hexString:)`, without the colour.
pub fn hex_components(hex_string: &str) -> Option<(f64, f64, f64, f64)> {
    let mut s = swift_compat::trim_whitespaces(hex_string);
    if swift_compat::has_ascii_prefix(s, "#") {
        s = &s[1..];
    }
    let count = swift_compat::character_count(s);
    if !(count == 6 || count == 8) {
        return None;
    }
    let v = swift_compat::parse_u64_hex(s)?;
    let has_alpha = count == 8;
    let r = ((v >> if has_alpha { 24 } else { 16 }) & 0xFF) as f64 / 255.0;
    let g = ((v >> if has_alpha { 16 } else { 8 }) & 0xFF) as f64 / 255.0;
    let b = ((v >> if has_alpha { 8 } else { 0 }) & 0xFF) as f64 / 255.0;
    let a = if has_alpha {
        (v & 0xFF) as f64 / 255.0
    } else {
        1.0
    };
    Some((r, g, b, a))
}

/// `NSColor.systemColorNamed(_:)`.
pub fn system_color_named(name: &str) -> Option<Retained<NSColor>> {
    Some(match name {
        "label" | "labelColor" => NSColor::labelColor(),
        "secondaryLabel" | "secondaryLabelColor" => NSColor::secondaryLabelColor(),
        "tertiaryLabel" | "tertiaryLabelColor" => NSColor::tertiaryLabelColor(),
        "quaternaryLabel" | "quaternaryLabelColor" => NSColor::quaternaryLabelColor(),
        "textBackground" | "textBackgroundColor" => NSColor::textBackgroundColor(),
        "controlBackground" | "controlBackgroundColor" => NSColor::controlBackgroundColor(),
        "underPageBackground" | "underPageBackgroundColor" => NSColor::underPageBackgroundColor(),
        "accent" | "controlAccentColor" => NSColor::controlAccentColor(),
        "selectedTextBackground" | "selectedTextBackgroundColor" => {
            NSColor::selectedTextBackgroundColor()
        }
        "separator" | "separatorColor" => NSColor::separatorColor(),
        "link" | "linkColor" => NSColor::linkColor(),
        "systemRed" => NSColor::systemRedColor(),
        "systemGreen" => NSColor::systemGreenColor(),
        "systemBlue" => NSColor::systemBlueColor(),
        "systemOrange" => NSColor::systemOrangeColor(),
        "systemYellow" => NSColor::systemYellowColor(),
        "systemPurple" => NSColor::systemPurpleColor(),
        "systemTeal" => NSColor::systemTealColor(),
        "systemPink" => NSColor::systemPinkColor(),
        "systemGray" => NSColor::systemGrayColor(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_description_matches_swift() {
        for (value, text) in [
            (1e15, "1000000000000000.0"),
            (9007199254740992.0, "9007199254740992.0"),
            (9007199254740994.0, "9.007199254740994e+15"),
            (1e16, "1e+16"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (0.00012345, "0.00012345"),
            (1.0 / 3.0, "0.3333333333333333"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (100.0, "100.0"),
            (2.5e-5, "2.5e-05"),
            (123.456, "123.456"),
            (0.1 + 0.2, "0.30000000000000004"),
            (4096.5, "4096.5"),
            (-0.0, "-0.0"),
        ] {
            assert_eq!(swift_double_description(value), text, "{value}");
        }
        assert_eq!(json_encoder_double(16.0), "16");
        assert_eq!(json_encoder_double(-0.0), "-0");
    }

    #[test]
    fn projection_follows_the_swift_arithmetic() {
        let range = NSRange::new(10, 5);
        assert_eq!(project_range(range, NSRange::new(20, 3), 0), range);
        assert_eq!(
            project_range(range, NSRange::new(0, 3), 1),
            NSRange::new(8, 5)
        );
        assert_eq!(
            project_range(range, NSRange::new(12, 2), 5),
            NSRange::new(10, 8)
        );
    }
}
