//! Objective-C boxes for the Swift value types Downright stores as attribute
//! values.
//!
//! Swift wraps a struct it hands to Objective-C (`.drBlock: block.identity`,
//! `.drPathToken: token`) in a `__SwiftValue` box whose `isEqual:` and `hash`
//! defer to the struct's `Hashable` conformance. `NSTextStorage` coalesces
//! adjacent attribute runs by `isEqual:`, so the boxes here compare by value
//! the same way, which keeps run boundaries identical to Downright's.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use upleft_core::{BlockIdentity, PathToken};

define_class!(
    /// `BlockIdentity` boxed as an attribute value (`.drBlock`).
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - The ivar is immutable plain data.
    // - The class does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = AllocAnyThread]
    #[name = "UpleftBlockIdentityValue"]
    #[ivars = BlockIdentity]
    pub struct BlockIdentityValue;

    unsafe impl NSObjectProtocol for BlockIdentityValue {}

    impl BlockIdentityValue {
        #[unsafe(method(isEqual:))]
        fn __is_equal(&self, other: Option<&AnyObject>) -> bool {
            other
                .and_then(|other| other.downcast_ref::<BlockIdentityValue>())
                .is_some_and(|other| other.ivars() == self.ivars())
        }

        #[unsafe(method(hash))]
        fn __hash(&self) -> usize {
            let identity = self.ivars();
            (identity.kind as usize).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(identity.ordinal as usize)
        }
    }
);

impl BlockIdentityValue {
    pub fn new(identity: BlockIdentity) -> Retained<Self> {
        let this = Self::alloc().set_ivars(identity);
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn identity(&self) -> BlockIdentity {
        *self.ivars()
    }
}

define_class!(
    /// `PathToken` boxed as an attribute value (`.drPathToken`).
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - The ivar is immutable plain data.
    // - The class does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = AllocAnyThread]
    #[name = "UpleftPathTokenValue"]
    #[ivars = PathToken]
    pub struct PathTokenValue;

    unsafe impl NSObjectProtocol for PathTokenValue {}

    impl PathTokenValue {
        #[unsafe(method(isEqual:))]
        fn __is_equal(&self, other: Option<&AnyObject>) -> bool {
            other
                .and_then(|other| other.downcast_ref::<PathTokenValue>())
                .is_some_and(|other| {
                    // Swift's synthesised `==`: `String ==` is canonical
                    // equivalence.
                    let (a, b) = (self.ivars(), other.ivars());
                    upleft_core::swift_text::str_eq(&a.raw_path, &b.raw_path)
                        && a.line == b.line
                        && a.column == b.column
                })
        }

        #[unsafe(method(hash))]
        fn __hash(&self) -> usize {
            // Consistent with the canonical-equivalence `isEqual:` above, which
            // a byte hash of the path would not be.
            let token = self.ivars();
            (token.line.unwrap_or(-1) as usize)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(token.column.unwrap_or(-1) as usize)
        }
    }
);

impl PathTokenValue {
    pub fn new(token: PathToken) -> Retained<Self> {
        let this = Self::alloc().set_ivars(token);
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn token(&self) -> &PathToken {
        self.ivars()
    }
}
