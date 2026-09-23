//! Port of `View/ParagraphSubstitution.swift`: marker hiding for the
//! physical-paragraph fallback (§6.1).
//!
//! `NSTextContentStorageDelegate.textContentStorage(_:textParagraphWith:)` is
//! the documented substitution hook. Grouped prose is handled by
//! `MarkdownContentStorage`; this delegate keeps marker hiding safe while the
//! custom layout is suspended during edits (§3.1).

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_app_kit::{NSTextContentManagerDelegate, NSTextContentStorage, NSTextContentStorageDelegate, NSTextParagraph};
use objc2_foundation::NSRange;

use crate::appkit_compat::from_ns;
use crate::engine::display_map::DisplayMap;

pub struct ParagraphSubstitutionIvars {
    /// Replaced wholesale whenever the caret moves or the mode changes.
    display_map: RefCell<DisplayMap>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the delegate method
    // keeps AppKit's signature. No Drop impl.
    #[unsafe(super(NSObject))]
    #[name = "ParagraphSubstitution"]
    #[ivars = ParagraphSubstitutionIvars]
    pub struct ParagraphSubstitution;

    unsafe impl NSObjectProtocol for ParagraphSubstitution {}

    unsafe impl NSTextContentManagerDelegate for ParagraphSubstitution {}

    unsafe impl NSTextContentStorageDelegate for ParagraphSubstitution {
        #[unsafe(method_id(textContentStorage:textParagraphWithRange:))]
        fn text_content_storage_text_paragraph_with_range(
            &self,
            text_content_storage: &NSTextContentStorage,
            range: NSRange,
        ) -> Option<Retained<NSTextParagraph>> {
            let display_map = self.ivars().display_map.borrow();
            if display_map.is_identity() {
                return None;
            }
            let storage = unsafe { text_content_storage.textStorage() }?;
            // Safety valve: between a text edit and the map rebuild the two are
            // briefly out of step, so when the shapes disagree fall back to
            // the storage and let the next rebuild fix it.
            if display_map.paragraphs.length != storage.length() as isize {
                return None;
            }
            let range = from_ns(range);
            if display_map.paragraphs.paragraph_range_containing(range.location) != range {
                return None;
            }
            let substituted = display_map.display_string_for_paragraph(range, &storage, true)?;
            Some(NSTextParagraph::initWithAttributedString(NSTextParagraph::alloc(), Some(&substituted)))
        }
    }
);

impl ParagraphSubstitution {
    pub fn new() -> Retained<ParagraphSubstitution> {
        let this = Self::alloc().set_ivars(ParagraphSubstitutionIvars { display_map: RefCell::new(DisplayMap::identity()) });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn display_map(&self) -> DisplayMap {
        self.ivars().display_map.borrow().clone()
    }

    pub fn set_display_map(&self, map: DisplayMap) {
        *self.ivars().display_map.borrow_mut() = map;
    }
}
