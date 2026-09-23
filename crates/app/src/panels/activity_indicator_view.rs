//! Port of `Panels/ActivityIndicatorView.swift`: an indeterminate activity
//! cue for operations that take more than a moment. Appearing is deferred a
//! second so a fast operation never flashes chrome; hiding is immediate.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAccessibility, NSControlSize, NSProgressIndicator, NSProgressIndicatorStyle, NSResponder, NSView};
use objc2_foundation::NSSize;

use super::appkit_support::{WorkItem, activate, rect, role, set_label, set_role};

pub struct ActivityIndicatorViewIvars {
    spinner: Retained<NSProgressIndicator>,
    reveal_work_item: RefCell<Option<WorkItem>>,
    on_visibility_change: RefCell<Option<Rc<dyn Fn(bool)>>>,
}

define_class!(
    /// `ActivityIndicatorView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ActivityIndicatorView"]
    #[ivars = ActivityIndicatorViewIvars]
    pub struct ActivityIndicatorView;

    unsafe impl NSObjectProtocol for ActivityIndicatorView {}

    impl ActivityIndicatorView {
        /// Hidden toolbar items are still auto-measured by AppKit; 1×1 is
        /// invisible yet unambiguous.
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            if self.isHidden() { NSSize::new(1.0, 1.0) } else { NSSize::new(18.0, 18.0) }
        }
    }
);

impl ActivityIndicatorView {
    /// `init()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<ActivityIndicatorView> {
        let spinner = NSProgressIndicator::new(mtm);
        let this = Self::alloc(mtm).set_ivars(ActivityIndicatorViewIvars {
            spinner: spinner.clone(),
            reveal_work_item: RefCell::new(None),
            on_visibility_change: RefCell::new(None),
        });
        let this: Retained<ActivityIndicatorView> =
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, 18.0, 18.0)] };
        spinner.setStyle(NSProgressIndicatorStyle::Spinning);
        spinner.setControlSize(NSControlSize::Small);
        spinner.setDisplayedWhenStopped(false);
        spinner.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&spinner);
        activate(&[
            spinner.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()),
            spinner.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            spinner.widthAnchor().constraintEqualToConstant(14.0),
            spinner.heightAnchor().constraintEqualToConstant(14.0),
        ]);
        this.setHidden(true);
        this.setAccessibilityElement(true);
        set_role(&*this, role::progress_indicator());
        set_label(&*this, "Working");
        this
    }

    /// Lets the toolbar collapse the item while idle.
    pub fn set_on_visibility_change(&self, handler: Option<Rc<dyn Fn(bool)>>) {
        *self.ivars().on_visibility_change.borrow_mut() = handler;
    }

    /// Show the cue only if the operation is still running a second from now.
    pub fn begin(&self) {
        if let Some(item) = self.ivars().reveal_work_item.borrow().as_ref() {
            item.cancel();
        }
        let weak: ObjcWeak<ActivityIndicatorView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            this.set_visible(true);
            unsafe { this.ivars().spinner.startAnimation(None) };
        });
        *self.ivars().reveal_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(1.0);
    }

    pub fn end(&self) {
        if let Some(item) = self.ivars().reveal_work_item.borrow().as_ref() {
            item.cancel();
        }
        *self.ivars().reveal_work_item.borrow_mut() = None;
        unsafe { self.ivars().spinner.stopAnimation(None) };
        self.set_visible(false);
    }

    fn set_visible(&self, visible: bool) {
        let hide = !visible;
        if self.isHidden() == hide {
            return;
        }
        self.setHidden(hide);
        self.invalidateIntrinsicContentSize();
        let handler = self.ivars().on_visibility_change.borrow().clone();
        if let Some(handler) = handler {
            handler(hide);
        }
    }

    pub fn spinner_for_testing(&self) -> Retained<NSProgressIndicator> {
        self.ivars().spinner.clone()
    }
}

#[allow(unused)]
fn _unused(_: &NSView) {}
