//! `upleft-markup`: a Rust port of the parts of Apple's
//! [swift-markdown](https://github.com/apple/swift-markdown) (revision
//! `27b7fc1a19068bcea3d2072db0ce86360d1400ed`) that Downright uses.
//!
//! Downright parses with `Document(parsing: body, options: [.disableSmartOpts])`
//! and walks the resulting `Markup` tree. The port runs the same cmark-gfm
//! (`upleft-cmark-gfm-sys`) with the same options and extensions and
//! converts its node tree the way `CommonMarkConverter.swift` does, so every
//! kind, child order, source range, string and property matches.
//!
//! ```
//! use upleft_markup::{Document, MarkupData, ParseOptions};
//!
//! let document = Document::parse("# Hi *there*\n", ParseOptions::DISABLE_SMART_OPTS);
//! let heading = document.child(0).unwrap();
//! assert_eq!(heading.data(), MarkupData::Heading { level: 1 });
//! let range = heading.range().unwrap();
//! assert_eq!((range.lower_bound.line, range.lower_bound.column), (1, 1));
//! assert_eq!(heading.plain_text().as_deref(), Some("Hi there"));
//! ```
//!
//! Swift → Rust:
//!
//! | swift-markdown | here |
//! |---|---|
//! | `Parser/CommonMarkConverter.swift` | [`parser::common_mark_converter`] |
//! | `Parser/ParseOptions.swift` | [`parser::parse_options`] |
//! | `Base/Document.swift` | [`base::document`] |
//! | `Base/RawMarkup.swift` | [`base::raw_markup`] |
//! | `Base/Markup.swift`, `Base/MarkupData.swift` | [`base::markup`] |
//! | `Base/MarkupChildren.swift` | [`base::markup_children`] |
//! | `plainText` (`PlainTextConvertibleMarkup`, `InlineContainer`, leaves) | [`base::plain_text_convertible_markup`] |
//! | `Infrastructure/SourceLocation.swift` | [`infrastructure::source_location`] |
//! | `Block Nodes/Tables/*.swift` | [`nodes::tables`] |
//! | `Inline Nodes/Inline Containers/Link.swift` (`isAutolink`) | [`nodes::inline_nodes`] |
//! | `Walker/Walkers/MarkupTreeDumper.swift` | [`walker::markup_tree_dumper`] |
//!
//! Not ported, because Downright never reaches them: `BlockDirectiveParser`
//! (`.parseBlockDirectives`, `.parseMinimalDoxygen`), the `source` URL on
//! locations, tree editing and rewriting, visitors, and `format()`.

pub mod base {
    pub mod document;
    pub mod markup;
    pub mod markup_children;
    pub mod plain_text_convertible_markup;
    pub mod raw_markup;
}

pub mod infrastructure {
    pub mod source_location;
}

pub mod nodes {
    pub mod inline_nodes;
    pub mod tables;
}

pub mod parser {
    pub mod common_mark_converter;
    pub mod parse_options;
}

pub mod walker {
    pub mod markup_tree_dumper;
}

pub(crate) mod utility {
    pub(crate) mod swift_string;
}

pub use base::document::Document;
pub use base::markup::Markup;
pub use base::markup_children::MarkupChildren;
pub use base::raw_markup::{Checkbox, MarkupData, NodeId};
pub use infrastructure::source_location::{SourceLocation, SourceRange};
pub use nodes::tables::ColumnAlignment;
pub use parser::parse_options::ParseOptions;

#[cfg(test)]
mod tests;
