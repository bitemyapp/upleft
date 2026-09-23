//! `FlippedStackView`, `ThemePreviewView`, `PreferencesPane` and
//! `ActionHandler` (PreferencesWindowController.swift).

use std::cell::RefCell;
use std::ffi::c_void;
use std::ops::RangeInclusive;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityGroupRole, NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSFont, NSFontWeightSemibold,
    NSLayoutAttribute, NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSPopUpButton,
    NSResponder, NSScrollView, NSStackView, NSStepper, NSTabViewController, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_foundation::{
    NSArray, NSBundle, NSEdgeInsets, NSNotification, NSNotificationCenter, NSNumber, NSNumberFormatter,
    NSNumberFormatterStyle, NSOperationQueue, NSPoint, NSRect, NSSize, NSString,
};
use upleft_cli::markdown_cli::swift_double;
use upleft_render::render_contracts::ThemeAppearance;
use upleft_render::swift_compat::{smax, smin};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_swift_text as swift_text;

use super::preference_row::{ChoiceSelection, PreferenceRow, PreferenceRowFilter};
use super::{PreferenceSearchable, SettingsPane};
use crate::support::preferences::{self, Preferences};

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

// MARK: - FlippedStackView

define_class!(
    /// `private final class FlippedStackView: NSStackView`.
    // SAFETY: no ivars; `init` is inherited.
    #[unsafe(super(NSStackView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FlippedStackView"]
    pub(super) struct FlippedStackView;

    impl FlippedStackView {
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedStackView {
    /// `FlippedStackView()`.
    fn new(mtm: MainThreadMarker) -> Retained<FlippedStackView> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `-[NSStackView init]`, NSView's designated `initWithFrame:`
        // with a zero frame, as Swift's `init()`.
        unsafe { msg_send![super(this), init] }
    }
}

// MARK: - ThemePreviewView

pub(super) struct ThemePreviewViewIvars {
    heading: Retained<NSTextField>,
    body: Retained<NSTextField>,
    accent: Retained<NSTextField>,
    observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Drop for ThemePreviewViewIvars {
    /// `deinit { observer.map(NotificationCenter.default.removeObserver) }`.
    fn drop(&mut self) {
        if let Some(observer) = self.observer.take() {
            // SAFETY: the token `addObserverForName:object:queue:usingBlock:`
            // returned.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
    }
}

define_class!(
    /// `private final class ThemePreviewView: NSView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ThemePreviewView"]
    #[ivars = ThemePreviewViewIvars]
    pub(super) struct ThemePreviewView;

    impl ThemePreviewView {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(520.0, 112.0)
        }
    }
);

impl ThemePreviewView {
    /// `init()`.
    fn new(mtm: MainThreadMarker) -> Retained<ThemePreviewView> {
        let heading = NSTextField::labelWithString(&ns("A clear document"), mtm);
        let body = NSTextField::labelWithString(&ns("Readable prose, a link, and `inline code`."), mtm);
        let accent = NSTextField::labelWithString(&ns("downright.md"), mtm);
        let this = Self::alloc(mtm).set_ivars(ThemePreviewViewIvars {
            heading: heading.clone(),
            body: body.clone(),
            accent: accent.clone(),
            observer: RefCell::new(None),
        });
        // SAFETY: NSView's designated initialiser, `super.init(frame: .zero)`.
        let this: Retained<ThemePreviewView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(8.0);
        }
        if let Some(layer) = this.layer() {
            layer.setBorderWidth(1.0);
        }
        for label in [&heading, &body, &accent] {
            label.setTranslatesAutoresizingMaskIntoConstraints(false);
            this.addSubview(label);
        }
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            heading.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 18.0),
            heading.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 16.0),
            body.leadingAnchor().constraintEqualToAnchor(&heading.leadingAnchor()),
            body.topAnchor().constraintEqualToAnchor_constant(&heading.bottomAnchor(), 10.0),
            accent.leadingAnchor().constraintEqualToAnchor(&heading.leadingAnchor()),
            accent.topAnchor().constraintEqualToAnchor_constant(&body.bottomAnchor(), 8.0),
        ]));
        let weak: Weak<ThemePreviewView> = Weak::from(&*this);
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            if let Some(this) = weak.load() {
                this.refresh();
            }
        });
        // SAFETY: the block runs on the main queue; the token is removed on
        // drop.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(&ns(preferences::DID_CHANGE)),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *this.ivars().observer.borrow_mut() = Some(observer);
        this.refresh();
        // SAFETY: AppKit exports the role as an immutable global.
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityGroupRole }));
        this.setAccessibilityLabel(Some(&ns("Theme preview")));
        this.setAccessibilityElement(true);
        this
    }

    fn refresh(&self) {
        let mtm = MainThreadMarker::from(self);
        let name = Preferences::shared().values().theme_name;
        let Some(theme) = ThemeStore::shared().themes().into_iter().find(|theme| swift_text::str_eq(&theme.name, &name))
        else {
            return;
        };
        // SAFETY: AppKit exports the appearance names as immutable globals.
        let appearance_name = unsafe {
            if theme.appearance == ThemeAppearance::Dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua }
        };
        let appearance: Retained<NSAppearance> = NSAppearance::appearanceNamed(appearance_name)
            .unwrap_or_else(|| NSApplication::sharedApplication(mtm).effectiveAppearance());
        let sheet = StyleSheet::new(theme, &appearance, None);
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&sheet.background.CGColor()));
        }
        if let Some(layer) = self.layer() {
            layer.setBorderColor(Some(&sheet.rule.CGColor()));
        }
        let ivars = self.ivars();
        ivars.heading.setFont(Some(&sheet.heading_font(2)));
        ivars.heading.setTextColor(Some(&sheet.heading_color(2)));
        ivars.body.setFont(Some(&sheet.body_font()));
        ivars.body.setTextColor(Some(&sheet.text));
        ivars.accent.setFont(Some(&sheet.mono_font(None)));
        ivars.accent.setTextColor(Some(&sheet.link));
    }
}

// MARK: - ActionHandler

define_class!(
    /// `final class ActionHandler: NSObject`: a target that runs a closure.
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ActionHandler"]
    #[ivars = Box<dyn Fn()>]
    pub struct ActionHandler;

    unsafe impl NSObjectProtocol for ActionHandler {}

    impl ActionHandler {
        #[unsafe(method(run))]
        fn __run(&self) {
            (self.ivars())();
        }
    }
);

impl ActionHandler {
    /// `ActionHandler(_ block:)`.
    pub fn new(block: impl Fn() + 'static, mtm: MainThreadMarker) -> Retained<ActionHandler> {
        let this = Self::alloc(mtm).set_ivars(Box::new(block) as Box<dyn Fn()>);
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    /// `run()`.
    pub fn run(&self) {
        (self.ivars())();
    }
}

/// `PreferencesPane.handlerKey`: its address is the association key.
static HANDLER_KEY: u8 = 0;

/// `objc_setAssociatedObject(control, &PreferencesPane.handlerKey, handler,
/// .OBJC_ASSOCIATION_RETAIN)`: a control's target is unretained, so the
/// control keeps its handler alive.
fn associate(control: &NSView, handler: &ActionHandler) {
    // SAFETY: both are live Objective-C objects; the key is a static's
    // address.
    unsafe {
        objc2::ffi::objc_setAssociatedObject(
            control as *const NSView as *mut AnyObject,
            &HANDLER_KEY as *const u8 as *const c_void,
            handler as *const ActionHandler as *mut AnyObject,
            objc2::ffi::OBJC_ASSOCIATION_RETAIN,
        );
    }
}

/// `control.target = handler; control.action = #selector(ActionHandler.run)`.
fn target(control: &objc2_app_kit::NSControl, handler: &ActionHandler) {
    // SAFETY: `run` is `ActionHandler`'s action method; the handler is kept
    // alive by `associate`.
    unsafe {
        control.setTarget(Some(handler));
        control.setAction(Some(sel!(run)));
    }
}

// MARK: - PreferencesPane

pub struct PreferencesPaneIvars {
    pane: SettingsPane,
    /// Rows are re-read, not snapshotted. A theme imported since the window
    /// was built, a history folder that has grown, an updater that has
    /// checked since — all of it is stale the moment it is captured.
    make_rows: Rc<dyn Fn() -> Vec<PreferenceRow>>,
    stack: Retained<FlippedStackView>,
    empty_label: Retained<NSTextField>,
    search_query: RefCell<String>,
}

define_class!(
    /// `final class PreferencesPane: NSViewController, PreferenceSearchable`.
    // SAFETY: `initWithNibName:bundle:` is forwarded in `new` after the ivars
    // are set.
    #[unsafe(super(NSViewController, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PreferencesPane"]
    #[ivars = PreferencesPaneIvars]
    pub struct PreferencesPane;

    unsafe impl NSObjectProtocol for PreferencesPane {}

    impl PreferencesPane {
        #[unsafe(method(loadView))]
        fn __load_view(&self) {
            self.load_view();
        }

        #[unsafe(method(viewDidLoad))]
        fn __view_did_load(&self) {
            // SAFETY: the override calls through to NSViewController.
            let () = unsafe { msg_send![super(self), viewDidLoad] };
            if let Some(item) = self.tab_bar_item() {
                let pane = self.ivars().pane;
                item.setImage(
                    objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(
                        &ns(pane.symbol()),
                        Some(&ns(pane.title())),
                    )
                    .as_deref(),
                );
            }
        }

        /// Every appearance rebuilds: the window is long-lived and the values
        /// behind these controls are not.
        #[unsafe(method(viewWillAppear))]
        fn __view_will_appear(&self) {
            // SAFETY: the override calls through to NSViewController.
            let () = unsafe { msg_send![super(self), viewWillAppear] };
            self.rebuild();
        }
    }
);

impl PreferencesPane {
    /// `init(pane:rows:)`.
    pub fn new(
        pane: SettingsPane,
        rows: Rc<dyn Fn() -> Vec<PreferenceRow>>,
        mtm: MainThreadMarker,
    ) -> Retained<PreferencesPane> {
        let stack = FlippedStackView::new(mtm);
        let empty_label = NSTextField::labelWithString(&ns("No settings match your search."), mtm);
        let this = Self::alloc(mtm).set_ivars(PreferencesPaneIvars {
            pane,
            make_rows: rows,
            stack,
            empty_label,
            search_query: RefCell::new(String::new()),
        });
        // SAFETY: NSViewController's designated initialiser, with no nib.
        let this: Retained<PreferencesPane> =
            unsafe { msg_send![super(this), initWithNibName: None::<&NSString>, bundle: None::<&NSBundle>] };
        this.setTitle(Some(&ns(pane.title())));
        this
    }

    /// `init(pane:rows:)` with a plain closure.
    pub fn with_rows(
        pane: SettingsPane,
        rows: impl Fn() -> Vec<PreferenceRow> + 'static,
        mtm: MainThreadMarker,
    ) -> Retained<PreferencesPane> {
        PreferencesPane::new(pane, Rc::new(rows), mtm)
    }

    /// `searchQuery`.
    pub fn search_query(&self) -> String {
        self.ivars().search_query.borrow().clone()
    }

    /// `searchQuery = …` and its `didSet`.
    pub fn set_search_query(&self, query: &str) {
        let old_value = self.ivars().search_query.replace(query.to_owned());
        if swift_text::str_eq(query, &old_value) {
            return;
        }
        if !self.isViewLoaded() {
            return;
        }
        self.rebuild();
        // A filtered pane is a different list. The clip view clamps itself
        // when the rows no longer fill the window, but a query that still
        // overflows keeps the offset the previous list had — and lands the
        // user below every match it just found for them.
        self.scroll_to_top();
    }

    /// `searchMatchCount`.
    pub fn search_match_count(&self) -> isize {
        let rows = (self.ivars().make_rows)();
        let query = self.search_query();
        PreferenceRowFilter::apply(&rows, &query).iter().filter(|row| !row.is_section()).count() as isize
    }

    fn load_view(&self) {
        let mtm = MainThreadMarker::from(self);
        let stack = &self.ivars().stack;
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(10.0);
        stack.setEdgeInsets(NSEdgeInsets { top: 20.0, left: 24.0, bottom: 20.0, right: 24.0 });

        let empty_label = &self.ivars().empty_label;
        empty_label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        empty_label.setTextColor(Some(&NSColor::tertiaryLabelColor()));

        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.contentView().setDrawsBackground(false);
        scroll.setDocumentView(Some(stack));
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            stack.leadingAnchor().constraintEqualToAnchor(&scroll.contentView().leadingAnchor()),
            stack.topAnchor().constraintEqualToAnchor(&scroll.contentView().topAnchor()),
            stack.widthAnchor().constraintEqualToAnchor(&scroll.contentView().widthAnchor()),
            stack.heightAnchor().constraintGreaterThanOrEqualToAnchor(&scroll.contentView().heightAnchor()),
        ]));
        self.setView(&scroll);
        self.rebuild();
    }

    fn rebuild(&self) {
        let mtm = MainThreadMarker::from(self);
        let stack = self.ivars().stack.clone();
        for view in stack.arrangedSubviews().iter() {
            stack.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        let made = (self.ivars().make_rows)();
        let query = self.search_query();
        let rows = PreferenceRowFilter::apply(&made, &query);
        if rows.is_empty() {
            stack.addArrangedSubview(&self.ivars().empty_label);
            return;
        }
        for row in &rows {
            stack.addArrangedSubview(&self.control(row));
        }
        let spacer = NSView::new(mtm);
        spacer.setContentHuggingPriority_forOrientation(NSLayoutPriorityDefaultLow, NSLayoutConstraintOrientation::Vertical);
        spacer.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Vertical,
        );
        stack.addArrangedSubview(&spacer);
        spacer.heightAnchor().constraintGreaterThanOrEqualToConstant(1.0).setActive(true);
    }

    fn scroll_to_top(&self) {
        let Ok(scroll) = self.view().downcast::<NSScrollView>() else { return };
        scroll.contentView().scrollToPoint(NSPoint::ZERO);
        scroll.reflectScrolledClipView(&scroll.contentView());
    }

    /// `tabBarItem`: `(parent as? NSTabViewController)?.tabViewItems.first {
    /// $0.viewController === self }`.
    fn tab_bar_item(&self) -> Option<Retained<objc2_app_kit::NSTabViewItem>> {
        let parent = self.parentViewController()?.downcast::<NSTabViewController>().ok()?;
        parent.tabViewItems().iter().find(|item| {
            item.viewController(MainThreadMarker::from(self)).is_some_and(|controller| {
                std::ptr::eq(
                    &*controller as *const NSViewController,
                    self as *const PreferencesPane as *const NSViewController,
                )
            })
        })
    }

    fn control(&self, row: &PreferenceRow) -> Retained<NSView> {
        let mtm = MainThreadMarker::from(self);
        match row {
            PreferenceRow::Section(title) => {
                let label = NSTextField::labelWithString(&ns(title), mtm);
                // SAFETY: AppKit exports the weight as an immutable global.
                label.setFont(Some(&NSFont::systemFontOfSize_weight(12.0, unsafe { NSFontWeightSemibold })));
                label.setTextColor(Some(&NSColor::secondaryLabelColor()));
                Retained::into_super(Retained::into_super(label))
            }

            PreferenceRow::Note(text) => {
                let label = NSTextField::wrappingLabelWithString(&ns(text), mtm);
                label.setFont(Some(&NSFont::systemFontOfSize(11.0)));
                label.setTextColor(Some(&NSColor::tertiaryLabelColor()));
                label.setPreferredMaxLayoutWidth(520.0);
                Retained::into_super(Retained::into_super(label))
            }

            PreferenceRow::ThemePreview => {
                let preview = ThemePreviewView::new(mtm);
                preview.setTranslatesAutoresizingMaskIntoConstraints(false);
                NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
                    preview.widthAnchor().constraintEqualToConstant(520.0),
                    preview.heightAnchor().constraintEqualToConstant(112.0),
                ]));
                Retained::into_super(preview)
            }

            PreferenceRow::Toggle { title, help, get, set } => {
                // SAFETY: no target or action yet.
                let button = unsafe { NSButton::checkboxWithTitle_target_action(&ns(title), None, None, mtm) };
                button.setState(if get() { NSControlStateValueOn } else { NSControlStateValueOff });
                let handler = {
                    let set = set.clone();
                    let button = button.clone();
                    ActionHandler::new(move || set(button.state() == NSControlStateValueOn), mtm)
                };
                target(&button, &handler);
                associate(&button, &handler);
                self.labelled(Retained::into_super(Retained::into_super(button)), help.as_deref())
            }

            PreferenceRow::Stepper { title, help, range, step, get, set } => {
                // The formatter comes from the row's own step, so a fractional
                // setting can actually be typed: a bare NumberFormatter allows
                // zero fraction digits and quietly rejects "1.55" for line
                // height.
                let formatter = PreferencesPane::number_formatter(range.clone(), *step);
                let field = NSTextField::textFieldWithString(&ns(&format_number(&formatter, get())), mtm);
                field.setFormatter(Some(&formatter));
                field.widthAnchor().constraintEqualToConstant(70.0).setActive(true);
                let stepper = NSStepper::new(mtm);
                stepper.setMinValue(*range.start());
                stepper.setMaxValue(*range.end());
                stepper.setIncrement(*step);
                stepper.setDoubleValue(get());
                let update_value: Rc<dyn Fn(f64)> = {
                    let range = range.clone();
                    let stepper = stepper.clone();
                    let field = field.clone();
                    let formatter = formatter.clone();
                    let set = set.clone();
                    Rc::new(move |value: f64| {
                        let value = smin(*range.end(), smax(*range.start(), value));
                        stepper.setDoubleValue(value);
                        field.setStringValue(&ns(&format_number(&formatter, value)));
                        set(value);
                    })
                };
                let stepper_handler = {
                    let update_value = update_value.clone();
                    let stepper = stepper.clone();
                    ActionHandler::new(move || update_value(stepper.doubleValue()), mtm)
                };
                let field_handler = {
                    let field = field.clone();
                    let stepper = stepper.clone();
                    let formatter = formatter.clone();
                    ActionHandler::new(
                        move || {
                            let Some(value) = swift_double(&field.stringValue().to_string()) else {
                                field.setStringValue(&ns(&format_number(&formatter, stepper.doubleValue())));
                                return;
                            };
                            update_value(value);
                        },
                        mtm,
                    )
                };
                target(&stepper, &stepper_handler);
                target(&field, &field_handler);
                associate(&stepper, &stepper_handler);
                associate(&field, &field_handler);

                let mut controls: Vec<Retained<NSView>> =
                    vec![Retained::into_super(Retained::into_super(field)), Retained::into_super(Retained::into_super(stepper))];
                if let Some(unit) = PreferencesPane::unit(title) {
                    controls.push(Retained::into_super(Retained::into_super(NSTextField::labelWithString(&ns(unit), mtm))));
                }
                let row = self.form_row(title, controls);
                self.labelled(row, help.as_deref())
            }

            PreferenceRow::Choice { title, help, options, get, set } => {
                let popup = NSPopUpButton::new(mtm);
                popup.setAutoenablesItems(false);
                let titles: Vec<Retained<NSString>> = options.iter().map(|option| ns(option)).collect();
                popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
                match get() {
                    ChoiceSelection::Index(index) => {
                        popup.selectItemAtIndex(0.max(index).min(0.max(options.len() as isize - 1)));
                    }
                    ChoiceSelection::Missing(name) => {
                        // Show what is stored, disabled, instead of silently
                        // selecting something else: the setting and the
                        // popup must agree.
                        popup.addItemWithTitle(&ns(&format!("{name} (not available)")));
                        if let Some(item) = popup.lastItem() {
                            item.setEnabled(false);
                        }
                        popup.selectItem(popup.lastItem().as_deref());
                    }
                }
                let handler = {
                    let set = set.clone();
                    let popup = popup.clone();
                    ActionHandler::new(move || set(popup.indexOfSelectedItem()), mtm)
                };
                target(&popup, &handler);
                associate(&popup, &handler);

                popup.widthAnchor().constraintGreaterThanOrEqualToConstant(220.0).setActive(true);
                let row = self.form_row(title, vec![Retained::into_super(Retained::into_super(Retained::into_super(popup)))]);
                self.labelled(row, help.as_deref())
            }

            PreferenceRow::Text { title, help, get, set } => {
                let field = NSTextField::textFieldWithString(&ns(&get()), mtm);
                field.widthAnchor().constraintEqualToConstant(280.0).setActive(true);
                let handler = {
                    let set = set.clone();
                    let field = field.clone();
                    ActionHandler::new(move || set(field.stringValue().to_string()), mtm)
                };
                target(&field, &handler);
                associate(&field, &handler);

                let row = self.form_row(title, vec![Retained::into_super(Retained::into_super(field))]);
                self.labelled(row, help.as_deref())
            }

            PreferenceRow::Button(title, action) => {
                // SAFETY: no target or action yet.
                let button = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), None, None, mtm) };
                let handler = {
                    let action = action.clone();
                    ActionHandler::new(move || action(), mtm)
                };
                target(&button, &handler);
                associate(&button, &handler);
                Retained::into_super(Retained::into_super(button))
            }
        }
    }

    fn form_row(&self, title: &str, controls: Vec<Retained<NSView>>) -> Retained<NSView> {
        let mtm = MainThreadMarker::from(self);
        let label = NSTextField::labelWithString(&ns(title), mtm);
        label.setAlignment(NSTextAlignment::Right);
        label.widthAnchor().constraintEqualToConstant(150.0).setActive(true);
        let mut views: Vec<Retained<NSView>> = vec![Retained::into_super(Retained::into_super(label))];
        views.extend(controls);
        let row = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&views), mtm);
        row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        row.setAlignment(NSLayoutAttribute::FirstBaseline);
        row.setSpacing(8.0);
        Retained::into_super(row)
    }

    /// `unit(for:)`.
    fn unit(title: &str) -> Option<&'static str> {
        match title {
            "Size" | "Text size adjustment" => Some("pt"),
            "Line height" | "Math scale" => Some("×"),
            "Measure (characters)" => Some("characters"),
            "Large-file threshold" | "Maximum size" => Some("MB"),
            "Keep versions for" => Some("days"),
            _ => None,
        }
    }

    /// `numberFormatter(range:step:)`: decimal places implied by a step: 1 →
    /// none, 0.05 → two.
    pub fn number_formatter(range: RangeInclusive<f64>, step: f64) -> Retained<NSNumberFormatter> {
        // Swift's `max(0, Int(ceil(-log10(step))))`.
        let digits: isize = if step > 0.0 { 0.max((-step.log10()).ceil() as isize) } else { 0 };
        let formatter = NSNumberFormatter::new();
        formatter.setNumberStyle(NSNumberFormatterStyle::DecimalStyle);
        formatter.setUsesGroupingSeparator(false);
        formatter.setAllowsFloats(digits > 0);
        formatter.setMinimumFractionDigits(0);
        formatter.setMaximumFractionDigits(digits as usize);
        formatter.setMinimum(Some(&NSNumber::numberWithDouble(*range.start())));
        formatter.setMaximum(Some(&NSNumber::numberWithDouble(*range.end())));
        formatter
    }

    fn labelled(&self, control: Retained<NSView>, help: Option<&str>) -> Retained<NSView> {
        let Some(help) = help else { return control };
        let mtm = MainThreadMarker::from(self);
        let hint = NSTextField::wrappingLabelWithString(&ns(help), mtm);
        hint.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        hint.setTextColor(Some(&NSColor::tertiaryLabelColor()));
        hint.setPreferredMaxLayoutWidth(520.0);
        let stack = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[control, Retained::into_super(Retained::into_super(hint))]),
            mtm,
        );
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(2.0);
        Retained::into_super(stack)
    }
}

/// `formatter.string(from: NSNumber(value: value)) ?? ""`.
fn format_number(formatter: &NSNumberFormatter, value: f64) -> String {
    formatter.stringFromNumber(&NSNumber::numberWithDouble(value)).map(|string| string.to_string()).unwrap_or_default()
}

impl PreferenceSearchable for PreferencesPane {
    fn search_query(&self) -> String {
        PreferencesPane::search_query(self)
    }

    fn set_search_query(&self, query: &str) {
        PreferencesPane::set_search_query(self, query);
    }

    fn search_match_count(&self) -> isize {
        PreferencesPane::search_match_count(self)
    }
}
