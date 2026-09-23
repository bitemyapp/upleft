//! Port of `Sources/DownrightApp/Support/QuickOpenProviders.swift`.
//!
//! The Quick Open side of the command palette: the query grammar, the result
//! type, and the providers that list headings, tasks, links, assets,
//! footnotes, workspace files and recent files. Providers only list; the
//! palette model ranks everything with one function.
//!
//! The workspace provider takes plain file URLs and prebuilt symbol results,
//! the same shape `DocumentWindowController+Workspace.swift` hands it from a
//! `WorkspaceIndex` snapshot, so it does not depend on the workspace port.

use std::sync::Arc;

use upleft_core::model::{InlineKind, InlineSpan, ParsedDocument};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift_text, NSRange};

use super::commands::Command;

/// `QuickOpenProviderKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuickOpenProviderKind {
    Command,
    Heading,
    Link,
    Footnote,
    Task,
    Asset,
    RecentFile,
    WorkspaceFile,
    Symbol,
}

impl QuickOpenProviderKind {
    pub const ALL_CASES: [QuickOpenProviderKind; 9] = [
        QuickOpenProviderKind::Command,
        QuickOpenProviderKind::Heading,
        QuickOpenProviderKind::Link,
        QuickOpenProviderKind::Footnote,
        QuickOpenProviderKind::Task,
        QuickOpenProviderKind::Asset,
        QuickOpenProviderKind::RecentFile,
        QuickOpenProviderKind::WorkspaceFile,
        QuickOpenProviderKind::Symbol,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            QuickOpenProviderKind::Command => "command",
            QuickOpenProviderKind::Heading => "heading",
            QuickOpenProviderKind::Link => "link",
            QuickOpenProviderKind::Footnote => "footnote",
            QuickOpenProviderKind::Task => "task",
            QuickOpenProviderKind::Asset => "asset",
            QuickOpenProviderKind::RecentFile => "recentFile",
            QuickOpenProviderKind::WorkspaceFile => "workspaceFile",
            QuickOpenProviderKind::Symbol => "symbol",
        }
    }

    /// `QuickOpenProviderKind.allCases.firstIndex(of:)`.
    pub fn index(self) -> usize {
        QuickOpenProviderKind::ALL_CASES.iter().position(|kind| *kind == self).unwrap_or(usize::MAX)
    }
}

/// `QuickOpenFilter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuickOpenFilter {
    All,
    Commands,
    Headings,
    Tasks,
    Assets,
    Links,
    Files,
    Symbols,
}

impl QuickOpenFilter {
    /// The case name, for dumps.
    pub fn name(self) -> &'static str {
        match self {
            QuickOpenFilter::All => "all",
            QuickOpenFilter::Commands => "commands",
            QuickOpenFilter::Headings => "headings",
            QuickOpenFilter::Tasks => "tasks",
            QuickOpenFilter::Assets => "assets",
            QuickOpenFilter::Links => "links",
            QuickOpenFilter::Files => "files",
            QuickOpenFilter::Symbols => "symbols",
        }
    }
}

/// `QuickOpenQuery`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickOpenQuery {
    pub raw: String,
    pub filter: QuickOpenFilter,
    pub terms: String,
}

/// Prefixes, longest first, because `#task` must win over `#`. One table
/// rather than a chain of `hasPrefix` branches, so a filter cannot be
/// declared and then left unreachable — which is exactly how `.headings`
/// became dead code.
const PREFIXES: [(&str, QuickOpenFilter); 9] = [
    ("#tasks", QuickOpenFilter::Tasks),
    ("#task", QuickOpenFilter::Tasks),
    ("task:", QuickOpenFilter::Tasks),
    ("asset:", QuickOpenFilter::Assets),
    ("file:", QuickOpenFilter::Files),
    ("link:", QuickOpenFilter::Links),
    ("#", QuickOpenFilter::Headings),
    ("@", QuickOpenFilter::Symbols),
    (">", QuickOpenFilter::Commands),
];

impl QuickOpenQuery {
    /// `QuickOpenQuery(_:)`.
    pub fn new(raw: &str) -> QuickOpenQuery {
        let value = swift_text::trim_whitespaces_and_newlines(raw);
        let lowered = swift_text::lowercased(value);
        for (marker, filter) in PREFIXES {
            if swift_text::has_prefix(&lowered, marker) {
                let rest = swift_text::drop_first(value, swift_text::count(marker));
                return QuickOpenQuery {
                    raw: raw.to_owned(),
                    filter,
                    terms: swift_text::trim_whitespaces(rest).to_owned(),
                };
            }
        }
        QuickOpenQuery { raw: raw.to_owned(), filter: QuickOpenFilter::All, terms: value.to_owned() }
    }
}

/// `QuickOpenAction`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickOpenAction {
    Command(Command),
    Select(NSRange),
    Open(FileUrl),
    OpenAt(FileUrl, NSRange),
}

/// `QuickOpenResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickOpenResult {
    pub id: String,
    pub kind: QuickOpenProviderKind,
    pub title: String,
    /// Rendered chrome — "⌘⇧K  ·  Document", "Heading 2". Displayed, never
    /// searched: matching against it made `doc` match most of the app.
    pub subtitle: String,
    /// Extra text the query may match: a file path, a link destination, a
    /// command's synonyms. Searched, never displayed.
    pub search_text: String,
    pub action: QuickOpenAction,
    pub score: isize,
}

impl QuickOpenResult {
    /// `QuickOpenResult(id:kind:title:action:)`, with the defaults for the
    /// rest (empty subtitle and search text, score 0).
    pub fn new(
        id: impl Into<String>,
        kind: QuickOpenProviderKind,
        title: impl Into<String>,
        action: QuickOpenAction,
    ) -> QuickOpenResult {
        QuickOpenResult {
            id: id.into(),
            kind,
            title: title.into(),
            subtitle: String::new(),
            search_text: String::new(),
            action,
            score: 0,
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> QuickOpenResult {
        self.subtitle = subtitle.into();
        self
    }

    pub fn with_search_text(mut self, search_text: impl Into<String>) -> QuickOpenResult {
        self.search_text = search_text.into();
        self
    }

    /// The same result with a rank attached. Scoring lives in the palette
    /// model, so every provider's results are ranked by one function.
    pub fn scored(&self, score: isize) -> QuickOpenResult {
        QuickOpenResult { score, ..self.clone() }
    }
}

/// `protocol QuickOpenProvider`.
pub trait QuickOpenProvider {
    fn id(&self) -> &str;
    fn results(&self, query: &QuickOpenQuery) -> Vec<QuickOpenResult>;
}

/// `CurrentDocumentQuickOpenProvider`.
#[derive(Clone)]
pub struct CurrentDocumentQuickOpenProvider {
    pub document: Arc<ParsedDocument>,
}

impl CurrentDocumentQuickOpenProvider {
    pub fn new(document: Arc<ParsedDocument>) -> CurrentDocumentQuickOpenProvider {
        CurrentDocumentQuickOpenProvider { document }
    }

    fn flatten(span: &InlineSpan, out: &mut Vec<InlineSpan>) {
        out.push(span.clone());
        for child in &span.children {
            Self::flatten(child, out);
        }
    }
}

impl QuickOpenProvider for CurrentDocumentQuickOpenProvider {
    fn id(&self) -> &str {
        "current-document"
    }

    fn results(&self, query: &QuickOpenQuery) -> Vec<QuickOpenResult> {
        let document = &self.document;
        let mut output = Vec::new();
        let wants_headings = matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Headings | QuickOpenFilter::Symbols);
        if wants_headings {
            output.extend(document.headings.iter().map(|heading| {
                QuickOpenResult::new(
                    format!("heading:{}", heading.range.location),
                    QuickOpenProviderKind::Heading,
                    heading.title.clone(),
                    QuickOpenAction::Select(heading.range),
                )
                .with_subtitle(format!("Heading {}", heading.level))
            }));
        }
        let wants_tasks = matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Tasks);
        if wants_tasks {
            output.extend(document.tasks.iter().map(|task| {
                QuickOpenResult::new(
                    format!("task:{}", task.mark_range.location),
                    QuickOpenProviderKind::Task,
                    task.text.clone(),
                    QuickOpenAction::Select(task.content_range),
                )
                .with_subtitle(if task.is_checked { "Done" } else { "Task" })
            }));
        }
        if query.filter == QuickOpenFilter::All {
            let identifiers =
                swift_text::sort::sorted_by(document.footnotes.keys().cloned(), |a, b| swift_text::str_less(a, b));
            for identifier in identifiers {
                let Some(block) = swift_text::dict_get(&document.footnotes, &identifier) else { continue };
                output.push(
                    QuickOpenResult::new(
                        format!("footnote:{identifier}"),
                        QuickOpenProviderKind::Footnote,
                        format!("Footnote {identifier}"),
                        QuickOpenAction::Select(block.range),
                    )
                    .with_subtitle("Definition"),
                );
            }
        }

        let mut spans = Vec::new();
        for block in document.root.flattened() {
            for span in &block.inlines {
                Self::flatten(span, &mut spans);
            }
        }
        for (index, span) in spans.iter().enumerate() {
            match &span.kind {
                InlineKind::Link { destination, .. } | InlineKind::Autolink { destination } => {
                    if !matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Links) {
                        continue;
                    }
                    let label = document.substring(span.content_range);
                    output.push(
                        QuickOpenResult::new(
                            format!("link:{}:{index}", span.range.location),
                            QuickOpenProviderKind::Link,
                            if label.is_empty() { destination.clone() } else { label },
                            QuickOpenAction::Select(span.range),
                        )
                        .with_subtitle(destination.clone())
                        .with_search_text(destination.clone()),
                    );
                }
                InlineKind::Image { source, alt } => {
                    if !matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Assets) {
                        continue;
                    }
                    output.push(
                        QuickOpenResult::new(
                            format!("asset:{}:{index}", span.range.location),
                            QuickOpenProviderKind::Asset,
                            if alt.is_empty() { source.clone() } else { alt.clone() },
                            QuickOpenAction::Select(span.range),
                        )
                        .with_subtitle(source.clone())
                        .with_search_text(source.clone()),
                    );
                }
                InlineKind::FootnoteReference { identifier } => {
                    if query.filter != QuickOpenFilter::All {
                        continue;
                    }
                    output.push(
                        QuickOpenResult::new(
                            format!("footnote-ref:{}:{index}", span.range.location),
                            QuickOpenProviderKind::Footnote,
                            format!("Footnote {identifier}"),
                            QuickOpenAction::Select(span.range),
                        )
                        .with_subtitle("Reference"),
                    );
                }
                _ => continue,
            }
        }
        output
    }
}

/// `WorkspaceQuickOpenProvider`.
#[derive(Clone, Debug)]
pub struct WorkspaceQuickOpenProvider {
    pub files: Vec<FileUrl>,
    pub symbols: Vec<QuickOpenResult>,
}

impl QuickOpenProvider for WorkspaceQuickOpenProvider {
    fn id(&self) -> &str {
        "workspace"
    }

    fn results(&self, query: &QuickOpenQuery) -> Vec<QuickOpenResult> {
        let mut output = Vec::new();
        if matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Files) {
            output.extend(self.files.iter().map(|url| {
                let path = url.path();
                QuickOpenResult::new(
                    format!("file:{path}"),
                    QuickOpenProviderKind::WorkspaceFile,
                    url.deleting_path_extension().last_path_component(),
                    QuickOpenAction::Open(url.clone()),
                )
                .with_subtitle(path.clone())
                .with_search_text(path)
            }));
        }
        if matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Symbols) {
            output.extend(self.symbols.iter().cloned());
        }
        output
    }
}

/// `RecentFilesQuickOpenProvider`.
#[derive(Clone, Debug)]
pub struct RecentFilesQuickOpenProvider {
    pub files: Vec<FileUrl>,
}

impl QuickOpenProvider for RecentFilesQuickOpenProvider {
    fn id(&self) -> &str {
        "recent-files"
    }

    fn results(&self, query: &QuickOpenQuery) -> Vec<QuickOpenResult> {
        if !matches!(query.filter, QuickOpenFilter::All | QuickOpenFilter::Files) {
            return Vec::new();
        }
        self.files
            .iter()
            .map(|url| {
                let path = url.path();
                QuickOpenResult::new(
                    format!("recent:{path}"),
                    QuickOpenProviderKind::RecentFile,
                    url.last_path_component(),
                    QuickOpenAction::Open(url.clone()),
                )
                .with_subtitle(url.deleting_last_path_component().path())
                .with_search_text(path)
            })
            .collect()
    }
}
