//! Health/DocumentHealth.swift — the document-health pass.
//!
//! The pass never mutates a document; a fix is only suggested when its edit is
//! local and unambiguous. Findings are deterministic: `id` is a rule
//! identifier, and the output is sorted by range, then id.
//!
//! The regular expressions run on Foundation's `NSRegularExpression` itself
//! (ICU), through objc2, so the match semantics are Downright's exactly. The
//! compiled expressions are cached per process (Swift compiles one per call;
//! `NSRegularExpression` is immutable and thread-safe, so the results are the
//! same).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType, msg_send};
use objc2_foundation::{
    NSMatchingOptions, NSRange as FRange, NSRegularExpression, NSRegularExpressionOptions, NSString, NSTextCheckingResult,
};
use unicode_normalization::UnicodeNormalization;

use crate::contracts::TextEdit;
use crate::model::{BlockContent, HeadingNode, InlineKind, InlineSpan, ParsedDocument};
use crate::ns_range::NSRange;
use crate::parser::MarkdownParser;
use crate::swift_text::{
    self, CharSet,
    ns::{NSStringExt, foundation::ns_from_utf16},
};

/// Severity used by the document-health pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentHealthSeverity {
    Info,
    Warning,
    Error,
}

impl DocumentHealthSeverity {
    pub const ALL_CASES: [DocumentHealthSeverity; 3] =
        [DocumentHealthSeverity::Info, DocumentHealthSeverity::Warning, DocumentHealthSeverity::Error];

    pub fn raw_value(&self) -> &'static str {
        match self {
            DocumentHealthSeverity::Info => "info",
            DocumentHealthSeverity::Warning => "warning",
            DocumentHealthSeverity::Error => "error",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<DocumentHealthSeverity> {
        DocumentHealthSeverity::ALL_CASES.into_iter().find(|severity| severity.raw_value() == raw)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentHealthCategory {
    Structure,
    Accessibility,
    References,
    Links,
    Media,
    Syntax,
    Prose,
}

impl DocumentHealthCategory {
    pub const ALL_CASES: [DocumentHealthCategory; 7] = [
        DocumentHealthCategory::Structure,
        DocumentHealthCategory::Accessibility,
        DocumentHealthCategory::References,
        DocumentHealthCategory::Links,
        DocumentHealthCategory::Media,
        DocumentHealthCategory::Syntax,
        DocumentHealthCategory::Prose,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            DocumentHealthCategory::Structure => "structure",
            DocumentHealthCategory::Accessibility => "accessibility",
            DocumentHealthCategory::References => "references",
            DocumentHealthCategory::Links => "links",
            DocumentHealthCategory::Media => "media",
            DocumentHealthCategory::Syntax => "syntax",
            DocumentHealthCategory::Prose => "prose",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<DocumentHealthCategory> {
        DocumentHealthCategory::ALL_CASES.into_iter().find(|category| category.raw_value() == raw)
    }
}

/// A deterministic finding. `id` is a rule identifier, not a UUID, so the
/// same source produces the same findings on every pass.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHealthDiagnostic {
    pub id: String,
    pub severity: DocumentHealthSeverity,
    pub category: DocumentHealthCategory,
    /// UTF-16 source range in the analyzed document.
    pub range: NSRange,
    pub message: String,
    pub explanation: String,
    pub fix: Option<TextEdit>,
}

impl DocumentHealthDiagnostic {
    pub fn new(
        id: impl Into<String>,
        severity: DocumentHealthSeverity,
        category: DocumentHealthCategory,
        range: NSRange,
        message: impl Into<String>,
        explanation: impl Into<String>,
        fix: Option<TextEdit>,
    ) -> DocumentHealthDiagnostic {
        DocumentHealthDiagnostic {
            id: id.into(),
            severity,
            category,
            range,
            message: message.into(),
            explanation: explanation.into(),
            fix,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentHealthOptions {
    pub max_section_words: isize,
    pub max_sentence_words: isize,
    pub max_paragraph_words: isize,
}

impl DocumentHealthOptions {
    /// `init(maxSectionWords:maxSentenceWords:maxParagraphWords:)`: each
    /// limit is at least 1.
    pub const fn new(max_section_words: isize, max_sentence_words: isize, max_paragraph_words: isize) -> DocumentHealthOptions {
        const fn at_least_one(value: isize) -> isize {
            if value > 1 { value } else { 1 }
        }
        DocumentHealthOptions {
            max_section_words: at_least_one(max_section_words),
            max_sentence_words: at_least_one(max_sentence_words),
            max_paragraph_words: at_least_one(max_paragraph_words),
        }
    }

    pub const DEFAULT: DocumentHealthOptions = DocumentHealthOptions::new(500, 35, 120);
}

impl Default for DocumentHealthOptions {
    fn default() -> Self {
        DocumentHealthOptions::DEFAULT
    }
}

/// Resolver injected by the app layer; MarkdownCore performs no file I/O.
/// Returns `true` when the path is known to exist.
#[derive(Clone)]
pub struct DocumentHealthResolver {
    pub exists: Arc<dyn Fn(&str) -> bool + Send + Sync>,
}

impl DocumentHealthResolver {
    pub fn new(exists: impl Fn(&str) -> bool + Send + Sync + 'static) -> DocumentHealthResolver {
        DocumentHealthResolver { exists: Arc::new(exists) }
    }
}

impl std::fmt::Debug for DocumentHealthResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DocumentHealthResolver")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHealthReport {
    pub diagnostics: Vec<DocumentHealthDiagnostic>,
}

impl DocumentHealthReport {
    pub fn new(diagnostics: Vec<DocumentHealthDiagnostic>) -> DocumentHealthReport {
        DocumentHealthReport { diagnostics }
    }
}

/// `DocumentHealth`. Swift's `analyze` overloads map to `analyze` (defaults),
/// `analyze_with`, `analyze_document` and `analyze_document_with`; the
/// closure-taking overloads are `DocumentHealthResolver::new(closure)`.
pub struct DocumentHealth;

impl DocumentHealth {
    pub fn analyze(text: &str) -> Vec<DocumentHealthDiagnostic> {
        Self::analyze_with(text, DocumentHealthOptions::DEFAULT, None)
    }

    pub fn analyze_with(
        text: &str,
        options: DocumentHealthOptions,
        resolver: Option<&DocumentHealthResolver>,
    ) -> Vec<DocumentHealthDiagnostic> {
        Self::analyze_document_with(&MarkdownParser::parse(text), options, resolver)
    }

    pub fn analyze_document(document: &ParsedDocument) -> Vec<DocumentHealthDiagnostic> {
        Self::analyze_document_with(document, DocumentHealthOptions::DEFAULT, None)
    }

    pub fn analyze_document_with(
        document: &ParsedDocument,
        options: DocumentHealthOptions,
        resolver: Option<&DocumentHealthResolver>,
    ) -> Vec<DocumentHealthDiagnostic> {
        HealthPass::new(document, options, resolver).run()
    }

    pub fn report(text: &str) -> DocumentHealthReport {
        Self::report_with(text, DocumentHealthOptions::DEFAULT, None)
    }

    pub fn report_with(text: &str, options: DocumentHealthOptions, resolver: Option<&DocumentHealthResolver>) -> DocumentHealthReport {
        DocumentHealthReport::new(Self::analyze_with(text, options, resolver))
    }
}

// MARK: - The pass

struct Line {
    range: NSRange,
    text: String,
}

struct Definition {
    label: String,
    range: NSRange,
}

struct HealthPass<'a> {
    document: &'a ParsedDocument,
    options: DocumentHealthOptions,
    resolver: Option<&'a DocumentHealthResolver>,
    /// `document.text as NSString`, as UTF-16.
    source: &'a [u16],
    ignored: IntervalIndex,
    inline_code: IntervalIndex,
    /// `lines()`: a pure function of the document, computed once.
    lines: Vec<Line>,
}

impl<'a> HealthPass<'a> {
    fn new(document: &'a ParsedDocument, options: DocumentHealthOptions, resolver: Option<&'a DocumentHealthResolver>) -> HealthPass<'a> {
        let mut ignored: Vec<NSRange> = Vec::new();
        let mut inline_code: Vec<NSRange> = Vec::new();
        document.root.walk(&mut |block| {
            if let BlockContent::FrontMatter(_) | BlockContent::CodeBlock { .. } | BlockContent::Mermaid { .. } | BlockContent::MathBlock { .. } =
                block.content
            {
                ignored.push(block.range);
            }
            for inline in &block.inlines {
                inline.walk(&mut |span| {
                    if let InlineKind::InlineCode = span.kind {
                        inline_code.push(span.range);
                    }
                });
            }
        });
        let source = document.utf16.as_slice();
        let lines = document
            .line_starts
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let range = document.range_of_line(index as isize + 1);
                Line { range, text: source.substring(range) }
            })
            .collect();
        HealthPass {
            document,
            options,
            resolver,
            source,
            // Both sets are disjoint, so a binary-search index keeps the
            // prose/reference passes sub-linear per finding.
            ignored: IntervalIndex::new(ignored),
            inline_code: IntervalIndex::new(inline_code),
            lines,
        }
    }

    fn run(&self) -> Vec<DocumentHealthDiagnostic> {
        let mut findings: Vec<DocumentHealthDiagnostic> = Vec::new();
        self.structure(&mut findings);
        self.references(&mut findings);
        self.links_and_images(&mut findings);
        self.syntax(&mut findings);
        self.prose(&mut findings);
        // Stable, like Swift's `sorted(by:)`.
        findings.sort_by(|a, b| {
            a.range
                .location
                .cmp(&b.range.location)
                .then(a.range.length.cmp(&b.range.length))
                .then_with(|| swift_text::str_cmp(&a.id, &b.id))
        });
        findings
    }

    // MARK: Structure

    fn structure(&self, out: &mut Vec<DocumentHealthDiagnostic>) {
        self.detect_malformed_front_matter(out);
        let mut previous_level: isize = 0;
        let mut h1_seen = false;
        let mut slugs: HashMap<String, isize> = HashMap::new();
        for heading in &self.document.headings {
            if heading.level > previous_level + 1 && previous_level > 0 {
                let target = previous_level + 1;
                let marker = self.marker_range(heading);
                let fix = marker.map(|range| {
                    TextEdit::new(range, format!("{} ", "#".repeat(target as usize)), "Lower heading level", None)
                });
                out.push(diagnostic(
                    "heading.skipped-level",
                    DocumentHealthSeverity::Warning,
                    DocumentHealthCategory::Structure,
                    heading.range,
                    format!("Heading level skips from H{} to H{}", previous_level, heading.level),
                    "Heading levels should increase one level at a time so the outline remains navigable.",
                    fix,
                ));
            }
            if heading.level == 1 {
                if h1_seen {
                    out.push(diagnostic(
                        "heading.multiple-h1",
                        DocumentHealthSeverity::Warning,
                        DocumentHealthCategory::Structure,
                        heading.range,
                        "Document has more than one H1 heading",
                        "Use one document title (H1), then use H2 and deeper headings for sections.",
                        None,
                    ));
                }
                h1_seen = true;
            }
            if swift_text::trim_whitespaces_and_newlines(&heading.title).is_empty() {
                out.push(diagnostic(
                    "heading.empty",
                    DocumentHealthSeverity::Warning,
                    DocumentHealthCategory::Accessibility,
                    heading.range,
                    "Heading has no text",
                    "Empty headings create an unlabeled outline entry and an unusable anchor.",
                    None,
                ));
            }
            let slug = swift_key(&slugify(&heading.title));
            if let Some(count) = slugs.get_mut(&slug) {
                out.push(diagnostic(
                    "heading.duplicate-anchor",
                    DocumentHealthSeverity::Warning,
                    DocumentHealthCategory::Structure,
                    heading.range,
                    "Heading creates a duplicate anchor",
                    "Two headings with the same anchor make links and table-of-contents entries ambiguous.",
                    None,
                ));
                *count += 1;
            } else {
                slugs.insert(slug, 1);
            }
            previous_level = heading.level;
            if heading.word_count > self.options.max_section_words {
                out.push(diagnostic(
                    "section.long",
                    DocumentHealthSeverity::Info,
                    DocumentHealthCategory::Structure,
                    heading.section_range,
                    "Section is unusually long",
                    "Large sections are harder to scan; consider splitting this section into focused subsections.",
                    None,
                ));
            }
        }

        let Some(first_heading) = self.document.headings.first() else {
            return;
        };
        for block in &self.document.root.children {
            if block.range.location < first_heading.range.location && !self.is_ignored(block.range) && !self.is_blank(block.range) {
                out.push(diagnostic(
                    "document.content-before-first-heading",
                    DocumentHealthSeverity::Info,
                    DocumentHealthCategory::Structure,
                    block.range,
                    "Content appears before the first heading",
                    "Put introductory content under a heading, or keep only a short document preamble.",
                    None,
                ));
            }
        }
    }

    /// A line that looks like a front-matter delimiter but is not the exact
    /// `---` opener — a stray character in front of it (`t---`), or an opener
    /// that never closes — leaves the YAML fields to render as prose (§5.1).
    fn detect_malformed_front_matter(&self, out: &mut Vec<DocumentHealthDiagnostic>) {
        if self.document.front_matter.is_some() {
            return;
        }
        let text = self.source;
        if !(text.length() > 0) {
            return;
        }
        let first_line = text.line_range_for(NSRange::new(0, 0));
        let first_text = text.substring(first_line);
        let first = swift_text::trim_whitespaces_and_newlines(&first_text);

        // `t---`, `-x--`, … a stray character in front of an opener. `----`
        // and longer dash runs are thematic breaks and are left alone.
        if swift_text::count(first) >= 4
            && !swift_text::has_prefix(first, "---")
            && swift_text::has_prefix(swift_text::drop_first(first, 1), "---")
        {
            out.push(diagnostic(
                "frontmatter.malformed-delimiter",
                DocumentHealthSeverity::Warning,
                DocumentHealthCategory::Structure,
                NSRange::new(first_line.location, 4),
                "Front-matter opener has a stray character before `---`",
                format!(
                    "A valid YAML front-matter block opens with `---` on the very first line. \
                     `{first}` is not a valid opener, so the metadata below renders as body prose \
                     instead of a metadata card."
                ),
                None,
            ));
            return;
        }

        // `---` opener that never closes, when the lines between look like a
        // field list rather than a plain thematic break.
        if swift_text::str_eq(first, "---") {
            let mut saw_field = false;
            let mut cursor = first_line.upper_bound();
            while cursor < text.length() {
                let line = text.line_range_for(NSRange::new(cursor, 0));
                let line_text = text.substring(line);
                let trimmed = swift_text::trim_whitespaces_and_newlines(&line_text);
                if swift_text::str_eq(trimmed, "---") {
                    return; // closed: valid front matter
                }
                if !trimmed.is_empty() && swift_text::contains(trimmed, ":") {
                    saw_field = true;
                }
                cursor = line.upper_bound();
            }
            if saw_field {
                out.push(diagnostic(
                    "frontmatter.unclosed",
                    DocumentHealthSeverity::Warning,
                    DocumentHealthCategory::Structure,
                    first_line,
                    "Front-matter block is never closed",
                    "A YAML front-matter block needs a closing `---` line; without it the \
                     metadata below renders as ordinary prose.",
                    None,
                ));
            }
        }
    }

    // MARK: References

    fn references(&self, out: &mut Vec<DocumentHealthDiagnostic>) {
        let definitions = self.reference_definitions();
        let mut uses: HashSet<String> = HashSet::new();
        let whole = NSString::from_str(&self.document.text);
        for m in matches(Pattern::ReferenceUse, &whole) {
            if self.is_ignored(m.range) {
                continue;
            }
            let value_text = self.source.substring(m.capture(1));
            let value = swift_text::trim_whitespaces_and_newlines(&value_text);
            let label = if value.is_empty() { self.reference_label_before(m.range) } else { value.to_owned() };
            if label.is_empty() {
                continue;
            }
            let key = swift_key(&swift_text::lowercased(&label));
            let defined = definitions.get(&key).is_some();
            uses.insert(key);
            if !defined {
                out.push(diagnostic(
                    "reference.undefined",
                    DocumentHealthSeverity::Error,
                    DocumentHealthCategory::References,
                    m.range,
                    format!("Reference \u{2018}{label}\u{2019} is undefined"),
                    format!("Add a matching [{label}]: destination definition, or use an inline link."),
                    None,
                ));
            }
        }
        // Swift walks the definitions in Dictionary order (random per
        // process); each has its own line range, so `run`'s sort erases it.
        for (key, definition) in definitions.iter() {
            if !uses.contains(key) {
                out.push(diagnostic(
                    "reference.unused",
                    DocumentHealthSeverity::Info,
                    DocumentHealthCategory::References,
                    definition.range,
                    format!("Reference \u{2018}{}\u{2019} is unused", definition.label),
                    "Remove definitions that have no references to keep the document easy to maintain.",
                    None,
                ));
            }
        }

        let mut footnotes: HashMap<String, NSRange> = HashMap::new();
        for line in &self.lines {
            if self.is_ignored(line.range) {
                continue;
            }
            let Some(m) = first_match(Pattern::FootnoteDefinition, &line_string(self.source, line)) else {
                continue;
            };
            let capture = m.capture(1);
            let id = swift_text::lowercased(&self.source.substring(NSRange::new(line.range.location + capture.location, capture.length)));
            let key = swift_key(&id);
            if footnotes.contains_key(&key) {
                out.push(diagnostic(
                    "footnote.duplicate",
                    DocumentHealthSeverity::Error,
                    DocumentHealthCategory::References,
                    line.range,
                    format!("Footnote \u{2018}{id}\u{2019} is defined more than once"),
                    "Footnote references resolve to one definition; keep a single definition for each identifier.",
                    None,
                ));
            } else {
                footnotes.insert(key, line.range);
            }
        }
    }

    /// Keyed by the lowercased label (Swift `String` key semantics); a later
    /// definition of the same label replaces the earlier one.
    fn reference_definitions(&self) -> OrderedDefinitions {
        let mut result = OrderedDefinitions::default();
        for line in &self.lines {
            if self.is_ignored(line.range) {
                continue;
            }
            let Some(m) = first_match(Pattern::ReferenceDefinition, &line_string(self.source, line)) else {
                continue;
            };
            let capture = m.capture(1);
            let label_range = NSRange::new(line.range.location + capture.location, capture.length);
            let label = self.source.substring(label_range);
            let key = swift_key(&swift_text::lowercased(&label));
            result.insert(key, Definition { label, range: line.range });
        }
        result
    }

    // MARK: Links and media

    fn links_and_images(&self, out: &mut Vec<DocumentHealthDiagnostic>) {
        self.document.root.walk(&mut |block| {
            for inline in &block.inlines {
                self.inspect(inline, out);
            }
        });
        for (_, definition) in self.reference_definitions().iter() {
            let text = NSString::from_str(&self.source.substring(definition.range));
            let Some(m) = first_match(Pattern::DefinitionDestination, &text) else {
                continue;
            };
            // Swift reads the capture, which is relative to the definition's
            // own text, out of the whole source: kept as is.
            self.inspect_url(&self.source.substring(m.capture(1)), definition.range, false, out);
        }
    }

    fn inspect(&self, span: &InlineSpan, out: &mut Vec<DocumentHealthDiagnostic>) {
        match &span.kind {
            InlineKind::Image { source, alt } => {
                if swift_text::trim_whitespaces_and_newlines(alt).is_empty() {
                    out.push(diagnostic(
                        "image.missing-alt",
                        DocumentHealthSeverity::Error,
                        DocumentHealthCategory::Accessibility,
                        span.range,
                        "Image is missing alternative text",
                        "Describe the image's purpose so readers using assistive technology are not left without context.",
                        None,
                    ));
                }
                self.inspect_url(source, span.range, true, out);
            }
            InlineKind::Link { destination, .. } | InlineKind::Autolink { destination } => {
                self.inspect_url(destination, span.range, false, out);
            }
            _ => {}
        }
        for child in &span.children {
            self.inspect(child, out);
        }
    }

    fn inspect_url(&self, value: &str, range: NSRange, is_image: bool, out: &mut Vec<DocumentHealthDiagnostic>) {
        let destination = swift_text::trimming(swift_text::trim_whitespaces_and_newlines(value), CharSet::Chars("<>\"'"));
        if destination.is_empty() {
            return;
        }
        if swift_text::has_prefix(destination, "/") && !swift_text::has_prefix(destination, "//") {
            out.push(diagnostic(
                if is_image { "asset.absolute-path" } else { "link.absolute-path" },
                DocumentHealthSeverity::Warning,
                if is_image { DocumentHealthCategory::Media } else { DocumentHealthCategory::Links },
                range,
                "Local path is absolute",
                "Use a project-relative path so the document works on another machine.",
                None,
            ));
        }
        if let Some(colon) = swift_text::first_index_of(destination, ':') {
            let scheme = swift_text::lowercased(&destination[..colon]);
            let colon_width = swift_text::first(&destination[colon..]).map_or(1, str::len);
            let after = &destination[colon + colon_width..];
            if scheme.is_empty() || swift_text::has_prefix(after, "//") {
                // `://` and empty schemes are malformed below.
            } else if ["http", "https"].iter().any(|known| swift_text::str_eq(known, &scheme)) {
                out.push(diagnostic(
                    "url.malformed",
                    DocumentHealthSeverity::Error,
                    DocumentHealthCategory::Links,
                    range,
                    "URL has a malformed scheme",
                    "Use a valid absolute URL such as https://example.com or a relative path.",
                    None,
                ));
                return;
            } else if !["http", "https", "mailto"].iter().any(|known| swift_text::str_eq(known, &scheme)) {
                out.push(diagnostic(
                    "url.unsafe-scheme",
                    DocumentHealthSeverity::Error,
                    DocumentHealthCategory::Links,
                    range,
                    format!("URL uses unsafe scheme \u{2018}{scheme}:\u{2019}"),
                    "Only web and mail links are accepted by the health pass; unsafe schemes can execute code or expose local data.",
                    None,
                ));
                return;
            }
        }
        if swift_text::contains(destination, "://")
            && !swift_text::has_prefix(destination, "http://")
            && !swift_text::has_prefix(destination, "https://")
        {
            out.push(diagnostic(
                "url.malformed",
                DocumentHealthSeverity::Error,
                DocumentHealthCategory::Links,
                range,
                "URL has a malformed scheme",
                "Use a valid absolute URL such as https://example.com or a relative path.",
                None,
            ));
            return;
        }
        let Some(resolver) = self.resolver.filter(|_| is_local(destination)) else {
            return;
        };
        let path = swift_text::split(destination, '#', 1, false).first().copied().unwrap_or(destination);
        if path.is_empty() || (resolver.exists)(path) {
            return;
        }
        out.push(diagnostic(
            if is_image { "asset.missing" } else { "link.missing" },
            DocumentHealthSeverity::Warning,
            if is_image { DocumentHealthCategory::Media } else { DocumentHealthCategory::Links },
            range,
            if is_image { "Local image asset was not found" } else { "Local link target was not found" },
            "Check the relative path or add the referenced file to the document's project.",
            None,
        ));
    }

    // MARK: Syntax

    fn syntax(&self, out: &mut Vec<DocumentHealthDiagnostic>) {
        // (marker character, run length, line range)
        let mut open: Option<(u8, usize, NSRange)> = None;
        for line in &self.lines {
            // Front matter is metadata, not a fence. Code ranges are not
            // skipped: an unclosed fence is itself the code range and must
            // remain observable to this source-level check.
            if let Some(front_matter) = &self.document.front_matter
                && front_matter.range.intersection(line.range) == Some(line.range)
            {
                continue;
            }
            let trimmed = swift_text::trim_whitespaces(&line.text);
            if !(swift_text::indent_columns(swift_text::leading_indent(&line.text)) < 4) {
                continue;
            }
            // `trimmed.prefix { $0 == "`" || $0 == "~" }`, each Character
            // named by the ASCII marker it equals.
            let mut run: Vec<u8> = Vec::new();
            for g in swift_text::graphemes(trimmed) {
                if swift_text::char_is(g, '`') {
                    run.push(b'`');
                } else if swift_text::char_is(g, '~') {
                    run.push(b'~');
                } else {
                    break;
                }
            }
            if run.len() >= 3 {
                let marker = run[0];
                if let Some((current, current_run, _)) = open
                    && marker == current
                    && run.len() >= current_run
                {
                    open = None;
                } else if open.is_none() {
                    open = Some((marker, run.len(), line.range));
                }
            }
        }
        if let Some((_, _, range)) = open {
            out.push(diagnostic(
                "fence.unclosed",
                DocumentHealthSeverity::Error,
                DocumentHealthCategory::Syntax,
                range,
                "Code fence is not closed",
                "Add a closing fence with the same marker character.",
                None,
            ));
        }

        for block in self.document.root.flattened() {
            let BlockContent::Table(table) = &block.content else {
                continue;
            };
            let expected = table.column_count();
            if !(expected > 0) {
                continue;
            }
            for row in &table.rows {
                if row.cells.len() as isize != expected {
                    out.push(diagnostic(
                        "table.invalid-row",
                        DocumentHealthSeverity::Warning,
                        DocumentHealthCategory::Syntax,
                        row.range,
                        "Table row has a different number of cells",
                        "Keep each table row aligned with the header's column count.",
                        None,
                    ));
                }
            }
        }
    }

    // MARK: Prose

    fn prose(&self, out: &mut Vec<DocumentHealthDiagnostic>) {
        self.document.root.walk(&mut |block| {
            if !matches!(block.content, BlockContent::Paragraph) || self.is_ignored(block.range) {
                return;
            }
            let text = self.source.substring(block.range);
            // `text` and `source.substring(with: range) as NSString` are the
            // same string; bridge it once for both expressions.
            let text_ns = NSString::from_str(&text);
            let words = word_count(&text);
            if words > self.options.max_paragraph_words {
                out.push(diagnostic(
                    "paragraph.dense",
                    DocumentHealthSeverity::Info,
                    DocumentHealthCategory::Prose,
                    block.range,
                    format!("Paragraph is dense ({words} words)"),
                    "Break long paragraphs into smaller units so readers can scan the document.",
                    None,
                ));
            }
            for sentence in self.sentence_ranges(block.range, &text_ns) {
                if !self.inline_code.contains_range(sentence)
                    && word_count(&self.source.substring(sentence)) > self.options.max_sentence_words
                {
                    out.push(diagnostic(
                        "sentence.long",
                        DocumentHealthSeverity::Info,
                        DocumentHealthCategory::Prose,
                        sentence,
                        "Sentence is unusually long",
                        "Shorter sentences are easier to understand and translate.",
                        None,
                    ));
                }
            }
            for m in matches(Pattern::RepeatedWord, &text_ns) {
                let range = NSRange::new(block.range.location + m.range.location, m.range.length);
                if self.inline_code.contains_offset(range.location) {
                    continue;
                }
                out.push(diagnostic(
                    "prose.repeated-word",
                    DocumentHealthSeverity::Warning,
                    DocumentHealthCategory::Prose,
                    range,
                    "Repeated adjacent word",
                    "Remove the accidental duplicate unless the repetition is intentional.",
                    None,
                ));
            }
        });
    }

    // MARK: Source helpers

    fn is_ignored(&self, range: NSRange) -> bool {
        self.ignored.contains_range(range)
    }

    fn is_blank(&self, range: NSRange) -> bool {
        swift_text::trim_whitespaces_and_newlines(&self.source.substring(range)).is_empty()
    }

    fn marker_range(&self, heading: &HeadingNode) -> Option<NSRange> {
        let prefix = self
            .source
            .substring(NSRange::new(heading.range.location, 0.max(heading.content_range.location - heading.range.location)));
        if !swift_text::has_prefix(swift_text::trim_whitespaces(&prefix), "#") {
            return None;
        }
        Some(NSRange::new(heading.range.location, swift_text::utf16_count(&prefix)))
    }

    /// `sentenceRanges(in:)`; `value` is `source.substring(with: range)`.
    fn sentence_ranges(&self, range: NSRange, value: &NSString) -> Vec<NSRange> {
        matches(Pattern::Sentence, value)
            .into_iter()
            .map(|m| NSRange::new(range.location + m.range.location, m.range.length))
            .collect()
    }

    fn reference_label_before(&self, range: NSRange) -> String {
        let value = self.source.substring(range);
        let Some(m) = first_match(Pattern::CollapsedReference, &NSString::from_str(&value)) else {
            return String::new();
        };
        swift_text::ns::utf16(&value).as_slice().substring(m.capture(1))
    }
}

fn diagnostic(
    id: &str,
    severity: DocumentHealthSeverity,
    category: DocumentHealthCategory,
    range: NSRange,
    message: impl Into<String>,
    explanation: impl Into<String>,
    fix: Option<TextEdit>,
) -> DocumentHealthDiagnostic {
    DocumentHealthDiagnostic::new(id, severity, category, range, message, explanation, fix)
}

/// `line.text` bridged to `NSString`. Line ranges never split a surrogate
/// pair, so the UTF-16 slice is exactly the bridged `String`.
fn line_string(source: &[u16], line: &Line) -> Retained<NSString> {
    ns_from_utf16(&source[line.range.as_usize_range()])
}

fn is_local(value: &str) -> bool {
    if swift_text::has_prefix(value, "#") || swift_text::has_prefix(value, "//") {
        return false;
    }
    !swift_text::contains(value, ":")
}

fn slugify(title: &str) -> String {
    let mut result = String::new();
    for character in swift_text::graphemes(&swift_text::lowercased(title)) {
        if swift_text::is_letter(character) || swift_text::is_number(character) {
            result.push_str(character);
        } else if swift_text::char_is(character, ' ') || swift_text::char_is(character, '-') || swift_text::char_is(character, '_') {
            result.push('-');
        }
    }
    swift_text::trimming(&result, CharSet::Chars("-")).to_owned()
}

/// `text.split { !$0.isLetter && !$0.isNumber }.count`: the number of
/// maximal runs of letter-or-number Characters.
fn word_count(text: &str) -> isize {
    let mut count = 0isize;
    let mut in_word = false;
    for character in swift_text::graphemes(text) {
        if swift_text::is_letter(character) || swift_text::is_number(character) {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    count
}

/// A Swift `String` dictionary key: `String` hashes and compares by canonical
/// equivalence, which is equality of the NFC forms.
fn swift_key(s: &str) -> String {
    if s.is_ascii() { s.to_owned() } else { s.nfc().collect() }
}

/// `[String: Definition]` with a deterministic (insertion) order.
#[derive(Default)]
struct OrderedDefinitions {
    index: HashMap<String, usize>,
    entries: Vec<(String, Definition)>,
}

impl OrderedDefinitions {
    fn insert(&mut self, key: String, definition: Definition) {
        match self.index.get(&key) {
            Some(&at) => self.entries[at].1 = definition,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, definition));
            }
        }
    }

    fn get(&self, key: &str) -> Option<&Definition> {
        self.index.get(key).map(|&at| &self.entries[at].1)
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &Definition)> {
        self.entries.iter().map(|(key, definition)| (key, definition))
    }
}

// MARK: - Regular expressions

#[derive(Clone, Copy)]
enum Pattern {
    ReferenceUse,
    FootnoteDefinition,
    ReferenceDefinition,
    DefinitionDestination,
    Sentence,
    RepeatedWord,
    CollapsedReference,
}

impl Pattern {
    const COUNT: usize = 7;

    fn source(self) -> &'static str {
        match self {
            Pattern::ReferenceUse => r"(?<!\!)\[[^\]\n]+\]\[([^\]\n]*)\]",
            Pattern::FootnoteDefinition => r"^\s*\[\^([^\]\n]+)\]:",
            Pattern::ReferenceDefinition => r"^\s*\[([^\]^\n]+)\]:",
            Pattern::DefinitionDestination => r"^\s*\[[^\]]+\]:\s*(\S+)",
            Pattern::Sentence => r"[^.!?]+(?:[.!?]+|$)",
            Pattern::RepeatedWord => "(?i)\\b([a-z][a-z'\u{2019}-]*)\\s+\\1\\b",
            Pattern::CollapsedReference => r"^\[([^\]]+)\]\[\]$",
        }
    }

    /// `try? NSRegularExpression(pattern:)`, compiled once per process.
    fn regex(self) -> Option<&'static NSRegularExpression> {
        static CACHE: [OnceLock<Option<Retained<NSRegularExpression>>>; Pattern::COUNT] =
            [const { OnceLock::new() }; Pattern::COUNT];
        CACHE[self as usize].get_or_init(|| compile(self.source())).as_deref()
    }
}

fn compile(pattern: &str) -> Option<Retained<NSRegularExpression>> {
    objc2::rc::autoreleasepool(|_| {
        let pattern = NSString::from_str(pattern);
        let error: *mut *mut AnyObject = std::ptr::null_mut();
        // `regularExpressionWithPattern:options:error:` returns nil on a bad
        // pattern, which is what `try?` makes of the throw.
        unsafe {
            msg_send![
                NSRegularExpression::class(),
                regularExpressionWithPattern: &*pattern,
                options: NSRegularExpressionOptions(0),
                error: error
            ]
        }
    })
}

struct Match {
    range: NSRange,
    captures: Vec<NSRange>,
}

impl Match {
    /// Capture indexes follow NSRegularExpression's 1-based convention.
    fn capture(&self, index: usize) -> NSRange {
        if !(index > 0 && index <= self.captures.len()) {
            return NSRange::NOT_FOUND;
        }
        self.captures[index - 1]
    }

    fn from_result(result: &NSTextCheckingResult) -> Match {
        let count = result.numberOfRanges();
        Match { range: from_foundation(result.range()), captures: (1..count).map(|i| from_foundation(result.rangeAtIndex(i))).collect() }
    }
}

#[inline]
fn from_foundation(range: FRange) -> NSRange {
    NSRange::new(range.location as isize, range.length as isize)
}

/// `matches(_:in:)`: every match over the whole string.
fn matches(pattern: Pattern, string: &NSString) -> Vec<Match> {
    let Some(regex) = pattern.regex() else {
        return Vec::new();
    };
    objc2::rc::autoreleasepool(|_| {
        let results = regex.matchesInString_options_range(string, NSMatchingOptions(0), FRange::new(0, string.length()));
        results.iter().map(|result| Match::from_result(&result)).collect()
    })
}

/// `firstMatch(_:in:)` (`matches(…).first`).
fn first_match(pattern: Pattern, string: &NSString) -> Option<Match> {
    let regex = pattern.regex()?;
    objc2::rc::autoreleasepool(|_| {
        regex
            .firstMatchInString_options_range(string, NSMatchingOptions(0), FRange::new(0, string.length()))
            .map(|result| Match::from_result(&result))
    })
}

// MARK: - Interval index

/// A binary-search index over a set of disjoint non-empty ranges. Both
/// queries the pass makes — "does a stored range fully contain this range"
/// and "does a stored range contain this offset" — collapse to one floor
/// lookup because the stored ranges are disjoint.
struct IntervalIndex {
    ranges: Vec<NSRange>,
}

impl IntervalIndex {
    fn new(ranges: Vec<NSRange>) -> IntervalIndex {
        let mut ranges: Vec<NSRange> = ranges.into_iter().filter(|r| r.location >= 0 && r.length > 0).collect();
        ranges.sort_by(|a, b| a.location.cmp(&b.location));
        IntervalIndex { ranges }
    }

    /// The stored range with the greatest start ≤ `offset`.
    fn floor(&self, offset: isize) -> Option<NSRange> {
        let (mut low, mut high) = (0usize, self.ranges.len());
        while low < high {
            let mid = (low + high) / 2;
            if self.ranges[mid].location <= offset {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if !(low > 0) {
            return None;
        }
        Some(self.ranges[low - 1])
    }

    fn contains_offset(&self, offset: isize) -> bool {
        let Some(floor) = self.floor(offset) else {
            return false;
        };
        floor.location <= offset && offset < floor.upper_bound()
    }

    /// True when a stored range fully covers `query`.
    fn contains_range(&self, query: NSRange) -> bool {
        if !(query.length > 0) {
            return false;
        }
        let Some(floor) = self.floor(query.location) else {
            return false;
        };
        floor.upper_bound() >= query.upper_bound()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    fn spans(pattern: Pattern, s: &str) -> Vec<(NSRange, Vec<NSRange>)> {
        matches(pattern, &ns(s)).into_iter().map(|m| (m.range, m.captures)).collect()
    }

    #[test]
    fn every_pattern_compiles() {
        for pattern in [
            Pattern::ReferenceUse,
            Pattern::FootnoteDefinition,
            Pattern::ReferenceDefinition,
            Pattern::DefinitionDestination,
            Pattern::Sentence,
            Pattern::RepeatedWord,
            Pattern::CollapsedReference,
        ] {
            assert!(pattern.regex().is_some(), "{}", pattern.source());
        }
    }

    #[test]
    fn a_bad_pattern_is_nil_like_try() {
        assert!(compile("([").is_none());
    }

    // Expectations recorded from NSRegularExpression in Swift 6.4 (probe in
    // the port notes).
    #[test]
    fn regex_ranges_match_foundation() {
        assert_eq!(
            spans(Pattern::ReferenceUse, "é [x][missing] ![i][img] [y][]"),
            vec![
                (NSRange::new(2, 12), vec![NSRange::new(6, 7)]),
                (NSRange::new(25, 5), vec![NSRange::new(29, 0)]),
            ]
        );
        assert_eq!(spans(Pattern::FootnoteDefinition, "  [^a]: one"), vec![(NSRange::new(0, 7), vec![NSRange::new(4, 1)])]);
        assert!(spans(Pattern::ReferenceDefinition, "[^a]: one").is_empty());
        assert_eq!(
            spans(Pattern::DefinitionDestination, "[a]:   <http://x> \"t\""),
            vec![(NSRange::new(0, 17), vec![NSRange::new(7, 10)])]
        );
        assert_eq!(
            spans(Pattern::Sentence, "One. Two!? three"),
            vec![(NSRange::new(0, 4), vec![]), (NSRange::new(4, 6), vec![]), (NSRange::new(10, 6), vec![])]
        );
        assert_eq!(
            spans(Pattern::RepeatedWord, "The the 🎉 don’t don’t x-y x-y"),
            vec![
                (NSRange::new(0, 7), vec![NSRange::new(0, 3)]),
                (NSRange::new(11, 11), vec![NSRange::new(11, 5)]),
                (NSRange::new(23, 7), vec![NSRange::new(23, 3)]),
            ]
        );
        assert_eq!(spans(Pattern::CollapsedReference, "[Label][]"), vec![(NSRange::new(0, 9), vec![NSRange::new(1, 5)])]);
    }

    #[test]
    fn interval_index_answers_floor_queries() {
        let index = IntervalIndex::new(vec![NSRange::new(10, 5), NSRange::new(0, 3), NSRange::new(20, 0), NSRange::new(-1, 4)]);
        assert!(index.contains_offset(0));
        assert!(!index.contains_offset(3));
        assert!(index.contains_offset(14));
        assert!(!index.contains_offset(15));
        assert!(!index.contains_offset(20));
        assert!(index.contains_range(NSRange::new(10, 5)));
        assert!(index.contains_range(NSRange::new(11, 2)));
        assert!(!index.contains_range(NSRange::new(9, 2)));
        assert!(!index.contains_range(NSRange::new(12, 0)));
    }

    #[test]
    fn slug_and_word_count_follow_characters() {
        assert_eq!(slugify("  Hello, World_x - Café! "), "hello-world-x---café");
        assert_eq!(slugify("--Title--"), "title");
        assert_eq!(word_count("don't stop—now 42x"), 5);
        assert_eq!(word_count("e\u{301}t\u{e9} 日本"), 2);
        assert!(is_local("docs/a.md"));
        assert!(!is_local("#anchor"));
        assert!(!is_local("//cdn"));
        assert!(!is_local("mailto:x"));
    }
}
