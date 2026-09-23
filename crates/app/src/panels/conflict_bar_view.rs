//! Port of `Panels/ConflictBarView.swift`: the dirty-buffer conflict bar
//! (§8.1). **Never clobber, never interrupt.**

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSButton, NSResponder, NSView};
use objc2_foundation::NSRect;
use upleft_render::core_types::ChangeKind;
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{ns_string, set_help, set_label};
use super::panel_chrome::{MessageBarInit, MessageBarView};

/// `ConflictBarDelegate`.
pub trait ConflictBarDelegate {
    fn conflict_bar_did_request_review(&self, bar: &ConflictBarView);
    fn conflict_bar_did_request_keep_mine(&self, bar: &ConflictBarView);
    fn conflict_bar_did_request_take_theirs(&self, bar: &ConflictBarView);
    fn conflict_bar_did_request_dismiss(&self, bar: &ConflictBarView);
}

#[derive(Default)]
pub struct ConflictBarViewIvars {
    delegate: RefCell<Option<Weak<dyn ConflictBarDelegate>>>,
}

define_class!(
    /// `ConflictBarView`, a `MessageBarView`.
    // SAFETY: `initWithFrame:` is forwarded to `MessageBarView` in `new`
    // after the ivars are set and the base's arguments are staged.
    #[unsafe(super(MessageBarView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ConflictBarView"]
    #[ivars = ConflictBarViewIvars]
    pub struct ConflictBarView;

    unsafe impl NSObjectProtocol for ConflictBarView {}

    impl ConflictBarView {
        #[unsafe(method(applyStyle))]
        fn __apply_style(&self) {
            let color = self.style_sheet().change_color(ChangeKind::Modified);
            self.set_stripe_color(color);
            let _: () = unsafe { msg_send![super(self), applyStyle] };
        }
    }
);

impl ConflictBarView {
    /// `ConflictBarView()`: hosts build panels before they have a theme in
    /// hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<ConflictBarView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<ConflictBarView> {
        let stripe_color = style_sheet.change_color(ChangeKind::Modified);
        let this = Self::alloc(mtm).set_ivars(ConflictBarViewIvars::default());
        MessageBarView::stage_init(MessageBarInit { style_sheet, stripe_color });
        let this: Retained<ConflictBarView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        this.set_message("Changed on disk while you were editing");

        // Both resolutions throw one version away, and neither verb says
        // which, so the help text names the consequence.
        let weak: ObjcWeak<ConflictBarView> = ObjcWeak::from(&*this);
        let review = this.add_action("Review", move || {
            if let Some(this) = weak.load()
                && let Some(delegate) = this.delegate()
            {
                delegate.conflict_bar_did_request_review(&this);
            }
        });
        Self::describe(&review, "Compare your version with the one on disk. Changes nothing.");

        let weak: ObjcWeak<ConflictBarView> = ObjcWeak::from(&*this);
        let keep_mine = this.add_action("Keep Mine", move || {
            if let Some(this) = weak.load()
                && let Some(delegate) = this.delegate()
            {
                delegate.conflict_bar_did_request_keep_mine(&this);
            }
        });
        keep_mine.setHasDestructiveAction(true);
        Self::describe(&keep_mine, "Save your version over the file, discarding the change on disk.");

        let weak: ObjcWeak<ConflictBarView> = ObjcWeak::from(&*this);
        let take_theirs = this.add_action("Take Theirs", move || {
            if let Some(this) = weak.load()
                && let Some(delegate) = this.delegate()
            {
                delegate.conflict_bar_did_request_take_theirs(&this);
            }
        });
        take_theirs.setHasDestructiveAction(true);
        Self::describe(&take_theirs, "Load the version on disk, discarding your unsaved edits.");

        let weak: ObjcWeak<ConflictBarView> = ObjcWeak::from(&*this);
        this.set_on_dismiss(Some(Rc::new(move || {
            if let Some(this) = weak.load()
                && let Some(delegate) = this.delegate()
            {
                delegate.conflict_bar_did_request_dismiss(&this);
            }
        })));

        set_label(
            &*this,
            &("File changed on disk while you were editing. ".to_owned()
                + "Review compares the two versions. Keep Mine saves yours over the file. "
                + "Take Theirs loads the file and discards your unsaved edits."),
        );
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn ConflictBarDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn ConflictBarDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    /// Attaches a consequence to a button for both pointer and VoiceOver.
    fn describe(button: &NSButton, consequence: &str) {
        button.setToolTip(Some(&ns_string(consequence)));
        set_help(button, consequence);
    }
}
