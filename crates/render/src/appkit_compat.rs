//! The Swift overlay's AppKit and CoreGraphics conveniences, reproduced as the
//! calls they compile to.
//!
//! Swift's `CGRect.minY`, `insetBy(dx:dy:)`, `union(_:)`, `intersects(_:)`,
//! `contains(_:)` and friends are the CoreGraphics C functions imported under
//! `CF_SWIFT_NAME`, so the port calls those same functions (they standardise
//! negative sizes and treat the null rect specially, which plain field
//! arithmetic would not). `NSAttributedString.enumerateAttribute(_:in:)` and
//! `attribute(_:at:effectiveRange:)` are wrapped so call sites read like the
//! Swift they port.

use std::ptr::NonNull;

use block2::StackBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGRectContainsPoint, CGRectGetHeight, CGRectGetMaxX, CGRectGetMaxY, CGRectGetMidX, CGRectGetMidY,
    CGRectGetMinX, CGRectGetMinY, CGRectGetWidth, CGRectInset, CGRectIntersection, CGRectIntersectsRect,
    CGRectIsEmpty, CGRectIsNull, CGRectOffset, CGRectUnion,
};
use objc2_foundation::{NSAttributedString, NSAttributedStringEnumerationOptions, NSRange, NSString};

/// Swift's `CGRect(x:y:width:height:)`.
#[inline]
pub fn rect(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat) -> CGRect {
    CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
}

/// `CGRect.zero`.
pub const RECT_ZERO: CGRect = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: 0.0, height: 0.0 } };

/// The `CGRect` members Swift code reaches for, as the C functions they are.
pub trait RectExt: Copy {
    fn min_x(self) -> CGFloat;
    fn mid_x(self) -> CGFloat;
    fn max_x(self) -> CGFloat;
    fn min_y(self) -> CGFloat;
    fn mid_y(self) -> CGFloat;
    fn max_y(self) -> CGFloat;
    fn width(self) -> CGFloat;
    fn height(self) -> CGFloat;
    fn inset_by(self, dx: CGFloat, dy: CGFloat) -> CGRect;
    fn offset_by(self, dx: CGFloat, dy: CGFloat) -> CGRect;
    fn union(self, other: CGRect) -> CGRect;
    fn intersection(self, other: CGRect) -> CGRect;
    fn intersects(self, other: CGRect) -> bool;
    fn contains_point(self, point: CGPoint) -> bool;
    fn is_empty(self) -> bool;
    fn is_null(self) -> bool;
}

impl RectExt for CGRect {
    #[inline]
    fn min_x(self) -> CGFloat {
        CGRectGetMinX(self)
    }
    #[inline]
    fn mid_x(self) -> CGFloat {
        CGRectGetMidX(self)
    }
    #[inline]
    fn max_x(self) -> CGFloat {
        CGRectGetMaxX(self)
    }
    #[inline]
    fn min_y(self) -> CGFloat {
        CGRectGetMinY(self)
    }
    #[inline]
    fn mid_y(self) -> CGFloat {
        CGRectGetMidY(self)
    }
    #[inline]
    fn max_y(self) -> CGFloat {
        CGRectGetMaxY(self)
    }
    #[inline]
    fn width(self) -> CGFloat {
        CGRectGetWidth(self)
    }
    #[inline]
    fn height(self) -> CGFloat {
        CGRectGetHeight(self)
    }
    #[inline]
    fn inset_by(self, dx: CGFloat, dy: CGFloat) -> CGRect {
        CGRectInset(self, dx, dy)
    }
    #[inline]
    fn offset_by(self, dx: CGFloat, dy: CGFloat) -> CGRect {
        CGRectOffset(self, dx, dy)
    }
    #[inline]
    fn union(self, other: CGRect) -> CGRect {
        CGRectUnion(self, other)
    }
    #[inline]
    fn intersection(self, other: CGRect) -> CGRect {
        CGRectIntersection(self, other)
    }
    #[inline]
    fn intersects(self, other: CGRect) -> bool {
        CGRectIntersectsRect(self, other)
    }
    #[inline]
    fn contains_point(self, point: CGPoint) -> bool {
        CGRectContainsPoint(self, point)
    }
    #[inline]
    fn is_empty(self) -> bool {
        CGRectIsEmpty(self)
    }
    #[inline]
    fn is_null(self) -> bool {
        CGRectIsNull(self)
    }
}

/// `NSMakeRange` for the Foundation range the AppKit bindings take.
#[inline]
pub fn ns_range(location: isize, length: isize) -> NSRange {
    NSRange::new(location as usize, length as usize)
}

/// Swift's `NSIntersectionRange` on Foundation ranges.
pub fn intersection_range(a: NSRange, b: NSRange) -> NSRange {
    let max = (a.location + a.length).min(b.location + b.length);
    let location = a.location.max(b.location);
    if max > location { NSRange::new(location, max - location) } else { NSRange::new(0, 0) }
}

/// `attributedString.enumerateAttribute(key, in: range, options: [])`,
/// calling `body(value, range)` for every run; returning `false` stops.
pub fn enumerate_attribute(
    string: &NSAttributedString,
    key: &NSString,
    range: NSRange,
    reverse: bool,
    mut body: impl FnMut(Option<&AnyObject>, NSRange) -> bool,
) {
    let body = std::cell::RefCell::new(&mut body);
    let block = StackBlock::new(|value: *mut AnyObject, range: NSRange, stop: NonNull<Bool>| {
        // SAFETY: AppKit hands the attribute value for the duration of the
        // call, or nil.
        let value = unsafe { value.as_ref() };
        if !(body.borrow_mut())(value, range) {
            // SAFETY: `stop` is AppKit's out parameter.
            unsafe { stop.as_ptr().write(Bool::YES) };
        }
    });
    let options = if reverse {
        NSAttributedStringEnumerationOptions::Reverse
    } else {
        NSAttributedStringEnumerationOptions(0)
    };
    string.enumerateAttribute_inRange_options_usingBlock(key, range, options, &block);
}

/// `attribute(key, at: index, effectiveRange: &range)`.
pub fn attribute_at(string: &NSAttributedString, key: &NSString, index: usize) -> Option<(Retained<AnyObject>, NSRange)> {
    let mut range = NSRange::new(0, 0);
    // SAFETY: `range` is a valid out pointer for the duration of the call;
    // the caller keeps `index` inside the string.
    let value = unsafe { string.attribute_atIndex_effectiveRange(key, index, &mut range) }?;
    Some((value, range))
}

/// `attribute(key, at: index, effectiveRange: nil)`.
pub fn attribute_value(string: &NSAttributedString, key: &NSString, index: usize) -> Option<Retained<AnyObject>> {
    // SAFETY: a null range pointer is allowed; the caller keeps `index`
    // inside the string.
    unsafe { string.attribute_atIndex_effectiveRange(key, index, std::ptr::null_mut()) }
}

/// `NSAttributedString(string:attributes:)` from Swift's dictionary literal.
pub fn attributed_string(
    text: &str,
    attributes: &[(&NSString, &AnyObject)],
) -> Retained<NSAttributedString> {
    let keys: Vec<&NSString> = attributes.iter().map(|(key, _)| *key).collect();
    let values: Vec<&AnyObject> = attributes.iter().map(|(_, value)| *value).collect();
    let dictionary = objc2_foundation::NSDictionary::from_slices(&keys, &values);
    // SAFETY: every value is an Objective-C object of the type its key expects.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&dictionary),
        )
    }
}

/// The AppKit attribute keys, as the `NSAttributedString.Key` statics Swift
/// names them by.
pub mod keys {
    use objc2_foundation::NSString;

    macro_rules! key {
        ($name:ident, $symbol:ident) => {
            #[inline]
            pub fn $name() -> &'static NSString {
                // SAFETY: AppKit exports these keys as immutable globals.
                unsafe { objc2_app_kit::$symbol }
            }
        };
    }

    key!(font, NSFontAttributeName);
    key!(foreground_color, NSForegroundColorAttributeName);
    key!(background_color, NSBackgroundColorAttributeName);
    key!(paragraph_style, NSParagraphStyleAttributeName);
    key!(underline_style, NSUnderlineStyleAttributeName);
    key!(underline_color, NSUnderlineColorAttributeName);
    key!(attachment, NSAttachmentAttributeName);
    key!(link, NSLinkAttributeName);
    key!(ligature, NSLigatureAttributeName);
}
