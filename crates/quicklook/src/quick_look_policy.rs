//! Port of `Sources/DownrightQL/QuickLookPolicy.swift`.
//!
//! Resource limits shared by the preview controller and its tests. Keeping
//! the decision pure prevents a large file from accidentally taking the
//! extension down before AppKit has a chance to show a fallback.

/// `QuickLookPolicy`.
pub struct QuickLookPolicy;

impl QuickLookPolicy {
    pub const MEMORY_CEILING_BYTES: isize = 60 * 1024 * 1024;
    pub const LARGE_FILE_THRESHOLD_BYTES: isize = 2 * 1024 * 1024;
    /// Full previews use the threshold as a hard cap, even if the file grows
    /// after Quick Look's initial resource-value lookup.
    pub const FULL_READ_LIMIT_BYTES: isize = Self::LARGE_FILE_THRESHOLD_BYTES;
    pub const PREFIX_BLOCK_COUNT: isize = 60;
    /// Bounded head read for oversized files — enough bytes to render the
    /// first `PREFIX_BLOCK_COUNT` blocks without ever loading the whole file.
    pub const PREFIX_READ_LIMIT_BYTES: isize = 8 * 1024 * 1024;
    /// Hard cap for the text handed to TextKit after the block prefix is
    /// found. A single Markdown block can be arbitrarily large, so the block
    /// count is not a sufficient memory bound on its own.
    pub const PREFIX_RENDER_LIMIT_BYTES: isize = 512 * 1024;
    pub const PREFIX_RENDER_LIMIT_UTF16: isize = 512 * 1024;
    /// Below this width the 72pt document map leaves too little useful measure.
    pub const MINIMUM_DENSITY_GUTTER_WIDTH: f64 = 520.0;

    /// `presentation(forByteCount:)`.
    pub fn presentation(byte_count: isize) -> Presentation {
        if byte_count > Self::LARGE_FILE_THRESHOLD_BYTES {
            Presentation::Prefix { block_count: Self::PREFIX_BLOCK_COUNT }
        } else {
            Presentation::Full
        }
    }
}

/// `QuickLookPolicy.Presentation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    Full,
    Prefix { block_count: isize },
}
