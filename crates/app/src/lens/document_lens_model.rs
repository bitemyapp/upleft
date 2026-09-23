//! Port of `Lens/DocumentLensModel.swift`: the pure, injected data behind the
//! Document Lens panel. It performs no file I/O, no URL resolution and no
//! AppKit work. All item ranges are UTF-16 ranges into
//! `DocumentLensInput.document.text`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use upleft_core::compatibility::compatibility_diagnostics::CompatibilityReport;
use upleft_core::health::document_health::{DocumentHealthDiagnostic, DocumentHealthSeverity};
use upleft_core::{BlockContent, ChangeKind, InlineKind, NSRange, ParsedDocument};
use upleft_swift_text as swift;

use crate::assets::asset_doctor::{AssetDiagnostic, AssetDiagnosticSeverity};
use crate::assets::asset_resolver::AssetReference;

/// The seven views in Document Lens. The order is the order shown in the
/// panel and is part of the keyboard contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentLensTab {
    Structure,
    Health,
    Links,
    Assets,
    Tasks,
    Changes,
    RenderTarget,
}

impl DocumentLensTab {
    /// `allCases`.
    pub const ALL_CASES: [DocumentLensTab; 7] = [
        DocumentLensTab::Structure,
        DocumentLensTab::Health,
        DocumentLensTab::Links,
        DocumentLensTab::Assets,
        DocumentLensTab::Tasks,
        DocumentLensTab::Changes,
        DocumentLensTab::RenderTarget,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            DocumentLensTab::Structure => "structure",
            DocumentLensTab::Health => "health",
            DocumentLensTab::Links => "links",
            DocumentLensTab::Assets => "assets",
            DocumentLensTab::Tasks => "tasks",
            DocumentLensTab::Changes => "changes",
            DocumentLensTab::RenderTarget => "renderTarget",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<DocumentLensTab> {
        DocumentLensTab::ALL_CASES.into_iter().find(|tab| tab.raw_value() == raw)
    }

    /// `id`.
    pub fn id(&self) -> &'static str {
        self.raw_value()
    }

    pub fn title(&self) -> &'static str {
        match self {
            DocumentLensTab::Structure => "Structure",
            DocumentLensTab::Health => "Health",
            DocumentLensTab::Links => "Links",
            DocumentLensTab::Assets => "Assets",
            DocumentLensTab::Tasks => "Tasks",
            DocumentLensTab::Changes => "Changes",
            DocumentLensTab::RenderTarget => "Render Target",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentLensItemKind {
    Heading,
    Block,
    Health,
    Link,
    Image,
    Task,
    Change,
    Compatibility,
}

impl DocumentLensItemKind {
    pub fn raw_value(&self) -> &'static str {
        match self {
            DocumentLensItemKind::Heading => "heading",
            DocumentLensItemKind::Block => "block",
            DocumentLensItemKind::Health => "health",
            DocumentLensItemKind::Link => "link",
            DocumentLensItemKind::Image => "image",
            DocumentLensItemKind::Task => "task",
            DocumentLensItemKind::Change => "change",
            DocumentLensItemKind::Compatibility => "compatibility",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentLensSeverity {
    Info,
    Warning,
    Error,
}

impl DocumentLensSeverity {
    pub fn raw_value(&self) -> &'static str {
        match self {
            DocumentLensSeverity::Info => "info",
            DocumentLensSeverity::Warning => "warning",
            DocumentLensSeverity::Error => "error",
        }
    }
}

/// `DocumentLensItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLensItem {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub range: NSRange,
    pub kind: DocumentLensItemKind,
    pub severity: Option<DocumentLensSeverity>,
    pub is_resolved: Option<bool>,
}

impl DocumentLensItem {
    /// `init(id:title:detail:range:kind:severity:isResolved:)`; Swift defaults
    /// `detail` to `""` and the last two to `nil`.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
        range: NSRange,
        kind: DocumentLensItemKind,
        severity: Option<DocumentLensSeverity>,
        is_resolved: Option<bool>,
    ) -> DocumentLensItem {
        DocumentLensItem { id: id.into(), title: title.into(), detail: detail.into(), range, kind, severity, is_resolved }
    }
}

/// `DocumentLensGroup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLensGroup {
    pub id: String,
    pub title: String,
    pub items: Vec<DocumentLensItem>,
}

impl DocumentLensGroup {
    pub fn new(id: impl Into<String>, title: impl Into<String>, items: Vec<DocumentLensItem>) -> DocumentLensGroup {
        DocumentLensGroup { id: id.into(), title: title.into(), items }
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }
}

/// `DocumentLensSection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLensSection {
    pub tab: DocumentLensTab,
    pub groups: Vec<DocumentLensGroup>,
}

impl DocumentLensSection {
    pub fn new(tab: DocumentLensTab, groups: Vec<DocumentLensGroup>) -> DocumentLensSection {
        DocumentLensSection { tab, groups }
    }

    /// `id`.
    pub fn id(&self) -> &'static str {
        self.tab.id()
    }

    pub fn count(&self) -> usize {
        self.groups.iter().fold(0, |sum, group| sum + group.items.len())
    }

    /// `groups.flatMap(\.items)`.
    pub fn items(&self) -> Vec<DocumentLensItem> {
        self.groups.iter().flat_map(|group| group.items.iter().cloned()).collect()
    }
}

/// A small, `Sendable` representation of a change mark. The app keeps the
/// tracker private; this value is the dependency boundary for the pure Lens
/// builder and also makes tests deterministic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLensChange {
    pub id: String,
    pub kind: ChangeKind,
    pub range: NSRange,
    pub word_ranges: Vec<NSRange>,
}

impl DocumentLensChange {
    /// `init(id:kind:range:wordRanges:)` (`wordRanges` defaults to `[]`).
    pub fn new(id: impl Into<String>, kind: ChangeKind, range: NSRange, word_ranges: Vec<NSRange>) -> DocumentLensChange {
        DocumentLensChange { id: id.into(), kind, range, word_ranges }
    }
}

/// `DocumentLensInput`. `new(document)` gives Swift's defaults (everything
/// else empty or `nil`); set the other fields directly.
#[derive(Clone, Debug)]
pub struct DocumentLensInput {
    pub document: Arc<ParsedDocument>,
    pub health: Vec<DocumentHealthDiagnostic>,
    pub asset_references: Vec<AssetReference>,
    pub assets: Vec<AssetDiagnostic>,
    pub render_target: Option<CompatibilityReport>,
    pub changes: Vec<DocumentLensChange>,
}

impl DocumentLensInput {
    pub fn new(document: Arc<ParsedDocument>) -> DocumentLensInput {
        DocumentLensInput {
            document,
            health: Vec::new(),
            asset_references: Vec::new(),
            assets: Vec::new(),
            render_target: None,
            changes: Vec::new(),
        }
    }
}

/// `DocumentLensModel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLensModel {
    pub sections: Vec<DocumentLensSection>,
}

impl DocumentLensModel {
    /// `init(input:)`.
    pub fn new(input: &DocumentLensInput) -> DocumentLensModel {
        DocumentLensModel {
            sections: vec![
                Self::structure(&input.document),
                Self::health(&input.health),
                Self::links(&input.document),
                Self::assets(&input.asset_references, &input.assets),
                Self::tasks(&input.document),
                Self::changes(&input.changes),
                Self::render_target(input.render_target.as_ref()),
            ],
        }
    }

    /// `section(_:)`.
    pub fn section(&self, tab: DocumentLensTab) -> DocumentLensSection {
        self.sections
            .iter()
            .find(|section| section.tab == tab)
            .cloned()
            .unwrap_or_else(|| DocumentLensSection::new(tab, Vec::new()))
    }

    fn structure(document: &ParsedDocument) -> DocumentLensSection {
        let mut items: Vec<DocumentLensItem> = Vec::new();
        let mut block_ordinal = 0;
        let mut represented_ranges: HashSet<String> = HashSet::new();
        for block in document.root.flattened() {
            if block.content_range.length <= 0 {
                continue;
            }
            let range_key = format!("{}:{}", block.range.location, block.range.length);
            if !represented_ranges.insert(range_key) {
                continue;
            }
            match &block.content {
                BlockContent::Heading { level } => {
                    let title = swift::trim_whitespaces_and_newlines(&document.substring(block.content_range)).to_owned();
                    items.push(DocumentLensItem::new(
                        format!("heading:{}", block.range.location),
                        if title.is_empty() { "Untitled heading".to_owned() } else { title },
                        format!("H{level}"),
                        block.range,
                        DocumentLensItemKind::Heading,
                        None,
                        None,
                    ));
                }
                BlockContent::Document => continue,
                content => {
                    let detail = Self::block_label(content);
                    items.push(DocumentLensItem::new(
                        format!("block:{}:{}", block.range.location, block_ordinal),
                        detail,
                        "",
                        block.range,
                        DocumentLensItemKind::Block,
                        None,
                        None,
                    ));
                    block_ordinal += 1;
                }
            }
        }
        DocumentLensSection::new(DocumentLensTab::Structure, vec![DocumentLensGroup::new("structure", "Document", items)])
    }

    fn health(diagnostics: &[DocumentHealthDiagnostic]) -> DocumentLensSection {
        let groups = Self::grouped(
            diagnostics,
            |diagnostic| swift::capitalized(diagnostic.category.raw_value()),
            |diagnostic| {
                DocumentLensItem::new(
                    format!("health:{}:{}", diagnostic.id, diagnostic.range.location),
                    diagnostic.message.clone(),
                    diagnostic.explanation.clone(),
                    diagnostic.range,
                    DocumentLensItemKind::Health,
                    Some(Self::severity(diagnostic.severity)),
                    None,
                )
            },
        );
        DocumentLensSection::new(
            DocumentLensTab::Health,
            if groups.is_empty() { vec![DocumentLensGroup::new("health", "No issues", Vec::new())] } else { groups },
        )
    }

    fn links(document: &ParsedDocument) -> DocumentLensSection {
        let mut links: Vec<DocumentLensItem> = Vec::new();
        document.root.walk(&mut |block| {
            for inline in &block.inlines {
                inline.walk(&mut |span| {
                    let item = match &span.kind {
                        InlineKind::Link { destination, title } => Some(DocumentLensItem::new(
                            format!("link:{}", span.range.location),
                            destination.clone(),
                            title.clone().unwrap_or_else(|| "Link".to_owned()),
                            span.range,
                            DocumentLensItemKind::Link,
                            None,
                            None,
                        )),
                        InlineKind::Autolink { destination } => Some(DocumentLensItem::new(
                            format!("link:{}", span.range.location),
                            destination.clone(),
                            "Autolink",
                            span.range,
                            DocumentLensItemKind::Link,
                            None,
                            None,
                        )),
                        InlineKind::Wikilink { target, label } => Some(DocumentLensItem::new(
                            format!("link:{}", span.range.location),
                            label.clone().unwrap_or_else(|| target.clone()),
                            target.clone(),
                            span.range,
                            DocumentLensItemKind::Link,
                            None,
                            None,
                        )),
                        InlineKind::Image { source, alt } => Some(DocumentLensItem::new(
                            format!("image:{}", span.range.location),
                            source.clone(),
                            if alt.is_empty() { "No alt text".to_owned() } else { alt.clone() },
                            span.range,
                            DocumentLensItemKind::Image,
                            None,
                            None,
                        )),
                        _ => None,
                    };
                    if let Some(item) = item {
                        links.push(item);
                    }
                });
            }
        });
        let link_items: Vec<DocumentLensItem> =
            links.iter().filter(|item| item.kind == DocumentLensItemKind::Link).cloned().collect();
        let image_items: Vec<DocumentLensItem> =
            links.iter().filter(|item| item.kind == DocumentLensItemKind::Image).cloned().collect();
        let mut groups: Vec<DocumentLensGroup> = Vec::new();
        if !link_items.is_empty() {
            groups.push(DocumentLensGroup::new("links", "Links", link_items));
        }
        if !image_items.is_empty() {
            groups.push(DocumentLensGroup::new("images", "Images", image_items));
        }
        if groups.is_empty() {
            groups = vec![DocumentLensGroup::new("links", "Links", Vec::new())];
        }
        DocumentLensSection::new(DocumentLensTab::Links, groups)
    }

    fn assets(references: &[AssetReference], diagnostics: &[AssetDiagnostic]) -> DocumentLensSection {
        let alt_or_placeholder =
            |reference: &AssetReference| if reference.alt_text.is_empty() { "No alt text".to_owned() } else { reference.alt_text.clone() };
        let mut items: Vec<DocumentLensItem> = references
            .iter()
            .map(|reference| {
                let diagnostic = diagnostics.iter().find(|diagnostic| {
                    diagnostic.reference.image_range == reference.image_range
                        || diagnostic.range.intersection(reference.image_range).is_some()
                });
                let detail = match diagnostic {
                    Some(diagnostic) => format!("{} \u{b7} {}", diagnostic.message, alt_or_placeholder(reference)),
                    None => alt_or_placeholder(reference),
                };
                DocumentLensItem::new(
                    format!("asset:{}", reference.image_range.location),
                    reference.source.clone(),
                    detail,
                    reference.image_range,
                    DocumentLensItemKind::Image,
                    diagnostic.map(|diagnostic| Self::asset_severity(diagnostic.severity)),
                    None,
                )
            })
            .collect();
        if references.is_empty() {
            items = diagnostics
                .iter()
                .map(|diagnostic| {
                    DocumentLensItem::new(
                        diagnostic.id.clone(),
                        diagnostic.message.clone(),
                        diagnostic.reference.source.clone(),
                        diagnostic.range,
                        DocumentLensItemKind::Image,
                        Some(Self::asset_severity(diagnostic.severity)),
                        None,
                    )
                })
                .collect();
        }
        let groups = if items.is_empty() { Vec::new() } else { vec![DocumentLensGroup::new("assets", "Images", items)] };
        DocumentLensSection::new(
            DocumentLensTab::Assets,
            if groups.is_empty() { vec![DocumentLensGroup::new("assets", "No asset issues", Vec::new())] } else { groups },
        )
    }

    fn tasks(document: &ParsedDocument) -> DocumentLensSection {
        let items: Vec<DocumentLensItem> = document
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| {
                DocumentLensItem::new(
                    format!("task:{}:{}", task.mark_range.location, index),
                    task.text.clone(),
                    if task.is_checked { "Done" } else { "Open" },
                    task.content_range,
                    DocumentLensItemKind::Task,
                    None,
                    Some(task.is_checked),
                )
            })
            .collect();
        DocumentLensSection::new(DocumentLensTab::Tasks, vec![DocumentLensGroup::new("tasks", "Tasks", items)])
    }

    fn changes(changes: &[DocumentLensChange]) -> DocumentLensSection {
        let items: Vec<DocumentLensItem> = changes
            .iter()
            .map(|change| {
                DocumentLensItem::new(
                    format!("change:{}", change.id),
                    swift::capitalized(change.kind.raw_value()),
                    if change.word_ranges.is_empty() { "Changed source" } else { "Changed words" },
                    change.range,
                    DocumentLensItemKind::Change,
                    None,
                    None,
                )
            })
            .collect();
        let title = if items.is_empty() { "No recent changes" } else { "Recent changes" };
        DocumentLensSection::new(DocumentLensTab::Changes, vec![DocumentLensGroup::new("changes", title, items)])
    }

    fn render_target(report: Option<&CompatibilityReport>) -> DocumentLensSection {
        let Some(report) = report else {
            return DocumentLensSection::new(
                DocumentLensTab::RenderTarget,
                vec![DocumentLensGroup::new("target", "No target selected", Vec::new())],
            );
        };
        let items: Vec<DocumentLensItem> = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                DocumentLensItem::new(
                    format!("compatibility:{}", diagnostic.id),
                    diagnostic.title.clone(),
                    diagnostic.explanation.clone(),
                    diagnostic.range,
                    DocumentLensItemKind::Compatibility,
                    Some(DocumentLensSeverity::Warning),
                    None,
                )
            })
            .collect();
        DocumentLensSection::new(
            DocumentLensTab::RenderTarget,
            vec![DocumentLensGroup::new("target", report.profile.name.clone(), items)],
        )
    }

    /// `grouped(_:key:item:)`: groups in first-seen key order. The keys are a
    /// Swift `Dictionary<String, …>`, so equivalent spellings share a bucket.
    fn grouped<T>(
        values: &[T],
        key: impl Fn(&T) -> String,
        item: impl Fn(&T) -> DocumentLensItem,
    ) -> Vec<DocumentLensGroup> {
        let mut order: Vec<String> = Vec::new();
        let mut buckets: HashMap<String, Vec<DocumentLensItem>> = HashMap::new();
        for value in values {
            let name = key(value);
            let slot = if buckets.contains_key(&name) {
                name
            } else if let Some(existing) = buckets.keys().find(|existing| swift::str_eq(existing, &name)).cloned() {
                existing
            } else {
                order.push(name.clone());
                name
            };
            buckets.entry(slot).or_default().push(item(value));
        }
        order
            .into_iter()
            .map(|name| {
                let items = swift::dict_get(&buckets, &name).cloned().unwrap_or_default();
                DocumentLensGroup::new(name.clone(), name, items)
            })
            .collect()
    }

    fn severity(severity: DocumentHealthSeverity) -> DocumentLensSeverity {
        match severity {
            DocumentHealthSeverity::Info => DocumentLensSeverity::Info,
            DocumentHealthSeverity::Warning => DocumentLensSeverity::Warning,
            DocumentHealthSeverity::Error => DocumentLensSeverity::Error,
        }
    }

    fn asset_severity(severity: AssetDiagnosticSeverity) -> DocumentLensSeverity {
        match severity {
            AssetDiagnosticSeverity::Info => DocumentLensSeverity::Info,
            AssetDiagnosticSeverity::Warning => DocumentLensSeverity::Warning,
            AssetDiagnosticSeverity::Error => DocumentLensSeverity::Error,
        }
    }

    fn block_label(content: &BlockContent) -> String {
        match content {
            BlockContent::Paragraph => "Paragraph".to_owned(),
            BlockContent::BlockQuote => "Block quote".to_owned(),
            BlockContent::Callout { kind, .. } => format!("Callout \u{b7} {}", swift::capitalized(kind.raw_value())),
            BlockContent::List { .. } => "List".to_owned(),
            BlockContent::ListItem { .. } => "List item".to_owned(),
            BlockContent::CodeBlock { language, .. } => {
                format!("Code{}", language.as_ref().map(|language| format!(" \u{b7} {language}")).unwrap_or_default())
            }
            BlockContent::Mermaid { .. } => "Mermaid diagram".to_owned(),
            BlockContent::MathBlock { .. } => "Math block".to_owned(),
            BlockContent::Table(_) => "Table".to_owned(),
            BlockContent::ThematicBreak => "Divider".to_owned(),
            BlockContent::HtmlBlock => "HTML block".to_owned(),
            BlockContent::FrontMatter(_) => "Front matter".to_owned(),
            BlockContent::FootnoteDefinition { identifier } => format!("Footnote \u{b7} {identifier}"),
            BlockContent::Document | BlockContent::Heading { .. } => "Block".to_owned(),
        }
    }
}
