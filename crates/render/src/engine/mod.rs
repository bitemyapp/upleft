//! Port of `Sources/MarkdownRender/Engine`.

pub mod block_style;
pub mod decoration_engine;
pub mod display_map;
pub mod elision_plan;
pub mod hard_wrap_reflow;
pub mod marker_policy;
pub mod render_metrics;
pub mod syntax_run_cache;

use upleft_core::NSRange;

/// An engine range as Foundation's unsigned `NSRange`, for AppKit calls.
///
/// Every range the engine hands Foundation is non-negative (Swift would trap
/// converting a negative `Int` to `NSUInteger` at the same call).
#[inline]
pub fn ns_range(range: NSRange) -> objc2_foundation::NSRange {
    objc2_foundation::NSRange::new(range.location as usize, range.length as usize)
}

/// A Foundation `NSRange` in the engine's signed coordinates.
#[inline]
pub fn from_ns_range(range: objc2_foundation::NSRange) -> NSRange {
    NSRange::new(range.location as isize, range.length as isize)
}

/// AppKit's `NSAttributedString.Key` constants the engine writes.
pub(crate) mod keys {
    use objc2_app_kit as appkit;
    use objc2_foundation::NSString;

    macro_rules! key {
        ($name:ident, $global:ident) => {
            #[inline]
            pub fn $name() -> &'static NSString {
                // SAFETY: AppKit exports the key as an immutable global.
                unsafe { appkit::$global }
            }
        };
    }

    key!(font, NSFontAttributeName);
    key!(foreground_color, NSForegroundColorAttributeName);
    key!(background_color, NSBackgroundColorAttributeName);
    key!(paragraph_style, NSParagraphStyleAttributeName);
    key!(kern, NSKernAttributeName);
    key!(ligature, NSLigatureAttributeName);
    key!(strikethrough_style, NSStrikethroughStyleAttributeName);
    key!(strikethrough_color, NSStrikethroughColorAttributeName);
    key!(underline_style, NSUnderlineStyleAttributeName);
    key!(underline_color, NSUnderlineColorAttributeName);
    key!(link, NSLinkAttributeName);
    key!(baseline_offset, NSBaselineOffsetAttributeName);
}
