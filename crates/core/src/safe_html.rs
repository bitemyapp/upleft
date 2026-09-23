//! SafeHTML.swift — the conservative README-style HTML annotator.

use crate::ns_range::NSRange;

/// The deliberately small HTML vocabulary Downright may present as content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafeHTMLKind {
    Paragraph { align: Option<SafeHTMLAlignment> },
    Heading { level: isize },
    Strong,
    Emphasis,
    Link { destination: String, title: Option<String> },
    Image { source: String, alt: String },
    /// A recognized but deliberately non-rendered tag, such as a remote image.
    Inert,
    LineBreak,
    Details { open: bool },
    /// A closing `</details>` emitted in a separate Markdown HTML block.
    DetailsClosing,
    Summary,
    Table,
    TableRow,
    TableCell { header: bool, align: Option<SafeHTMLAlignment> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SafeHTMLAlignment {
    Left,
    Center,
    Right,
    Justify,
}

impl SafeHTMLAlignment {
    pub fn raw_value(&self) -> &'static str {
        match self {
            SafeHTMLAlignment::Left => "left",
            SafeHTMLAlignment::Center => "center",
            SafeHTMLAlignment::Right => "right",
            SafeHTMLAlignment::Justify => "justify",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<SafeHTMLAlignment> {
        match raw {
            "left" => Some(SafeHTMLAlignment::Left),
            "center" => Some(SafeHTMLAlignment::Center),
            "right" => Some(SafeHTMLAlignment::Right),
            "justify" => Some(SafeHTMLAlignment::Justify),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeHTMLAnnotation {
    pub kind: SafeHTMLKind,
    /// The complete element, including its opening and closing tags.
    pub range: NSRange,
    /// The content between an element's tags.
    pub content_range: NSRange,
    /// Opening and closing tag source ranges.
    pub tag_ranges: Vec<NSRange>,
}

impl SafeHTMLAnnotation {
    pub fn new(kind: SafeHTMLKind, range: NSRange, content_range: NSRange, tag_ranges: Vec<NSRange>) -> Self {
        SafeHTMLAnnotation { kind, range, content_range, tag_ranges }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeHTMLDocument {
    pub range: NSRange,
    pub annotations: Vec<SafeHTMLAnnotation>,
    /// `false` means the complete source range must remain literal.
    pub is_safe: bool,
}

impl SafeHTMLDocument {
    pub fn new(range: NSRange, annotations: Vec<SafeHTMLAnnotation>, is_safe: bool) -> SafeHTMLDocument {
        SafeHTMLDocument { range, annotations, is_safe }
    }

    pub fn tag_ranges(&self) -> Vec<NSRange> {
        let mut ranges: Vec<NSRange> = self.annotations.iter().flat_map(|a| a.tag_ranges.iter().copied()).collect();
        ranges.sort_by(|a, b| a.location.cmp(&b.location));
        ranges
    }

    pub fn hidden_tag_ranges(&self) -> Vec<NSRange> {
        self.tag_ranges()
    }
}

// SafeHTMLParser: ported in the next step.
