//! The MarkdownCore (`Sources/MarkdownCore`) types the render layer names,
//! re-exported from `upleft-core`.
//!
//! MarkdownRender imports MarkdownCore wholesale; this module keeps the
//! render crate's historical `core_types::…` paths working and adds the one
//! list Swift gets from `CaseIterable` that `upleft-core` does not spell out.

pub use upleft_core::{
    BlockContent, BlockIdentity, BlockRef, CalloutKind, ChangeKind, Checkbox, DirtySet,
    InlineKind, InlineSpan, MDBlock, NSRange, ParsedDocument, PathToken, TableAlignment,
    TableCell, TableData, TableRow,
};

/// `ChangeKind.allCases`, in declaration order.
pub const CHANGE_KINDS: [ChangeKind; 3] = [
    ChangeKind::Inserted,
    ChangeKind::Deleted,
    ChangeKind::Modified,
];
