//! Port of `App/SetupWindowController.swift`: the first-run setup panel.
//!
//! Everything here could be left to the user to find in System Settings, and
//! that is exactly what made the old install bad: an app downloaded from a
//! website and dragged into Applications had no file association, no `down`,
//! and Quick Look extensions the system had never been told to enable. One
//! panel, two checkboxes, one button — and the parts that need no decision
//! (registering the extensions, resetting the icon cache) just happen.
//!
//! It is shown once. "Not now" is an answer, and the panel does not come back
//! uninvited; Settings → General keeps every step available afterwards.
//!
//! `SetupWindowController` is a `define_class!` `NSWindowController`
//! subclass of that name, and its own window's delegate. Swift's nested
//! `SetupWindowController.Step` is [`Step`].
//!
//! Threading: Swift runs the whole setup in a main-actor `Task`. The port
//! keeps that sequence (default handler, `down`, the agent hook, Quick Look,
//! then "Done") and runs the steps that touch the file system or
//! LaunchServices — the default-handler check after the change, the `down`
//! symlinks and the agent's `settings.json` — on a user-initiated global
//! queue, reporting each on the main queue before the next starts
//! (AGENTS.md: never block the main thread). The spinner runs throughout.
//! The move to Applications stays on the main thread, as in Swift: it ends
//! by terminating the process, and `SystemIntegration::move_to_applications`
//! takes the main-thread marker.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject, Sel};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSApplication, NSBackingStoreType, NSBezelStyle, NSButton, NSControlSize,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSImage,
    NSImageScaling, NSImageSymbolConfiguration, NSImageView, NSLayoutAttribute, NSLayoutConstraint,
    NSProgressIndicator, NSProgressIndicatorStyle, NSResponder, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowController, NSWindowDelegate, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSNotification, NSSize, NSString};
use upleft_render::appkit_compat::rect;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use crate::app::themed_window_appearance::ThemedWindowAppearance;
use crate::integrations::agent_integration::{AgentIntegration, Failure};
use crate::support::preferences::Preferences;
use crate::support::system_integration::{CocoaError, CommandLineResult, SystemIntegration};

/// `SetupWindowController.Layout`.
struct Layout;

impl Layout {
    const WINDOW_WIDTH: CGFloat = 520.0;
    const INSET: CGFloat = 34.0;
    /// Width available to a step's wrapped detail line, which hangs under
    /// the checkbox label rather than under its box.
    const DETAIL_WIDTH: CGFloat = Self::WINDOW_WIDTH - Self::INSET * 2.0 - 20.0;
}

/// `SetupWindowController.Step`: a step the user can decline. Quick Look is
/// not one of these: it needs no decision and carries no cost, so it runs
/// regardless and is reported rather than offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    MoveToApplications,
    DefaultApplication,
    CommandLineTool,
    AgentIntegration,
}

impl Step {
    /// `Step.allCases`, in declaration order.
    pub const ALL_CASES: [Step; 4] =
        [Step::MoveToApplications, Step::DefaultApplication, Step::CommandLineTool, Step::AgentIntegration];

    pub fn title(self) -> &'static str {
        match self {
            Step::MoveToApplications => "Move Upleft to your Applications folder",
            Step::DefaultApplication => "Open Markdown files with Upleft",
            Step::CommandLineTool => "Install the down command line tool",
            Step::AgentIntegration => "Open agent edits in Upleft",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Step::MoveToApplications => "arrow.down.app",
            Step::DefaultApplication => "doc.text",
            Step::CommandLineTool => "terminal",
            Step::AgentIntegration => "sparkles",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Step::MoveToApplications => {
                "Upleft is running from a temporary copy. Nothing below can stick until it lives in Applications."
            }
            Step::DefaultApplication => {
                "Double-clicking a .md file opens it here. Other Markdown flavours are left to the apps that own them."
            }
            Step::CommandLineTool => "Opens files from the terminal: down PLAN.md",
            Step::AgentIntegration => {
                "Coding agents like Claude Code show Markdown here as they write it. Adds a hook to ~/.claude/settings.json."
            }
        }
    }

    /// Steps start ticked, because every one of them is a registration this
    /// app is asking to make on its own behalf — except the agent hook, which
    /// edits *another* tool's configuration file. Turning that on by default
    /// would be helping yourself to somebody else's settings.
    pub fn is_preselected(self) -> bool {
        self != Step::AgentIntegration
    }

    /// Whether this step is worth showing at all on this Mac right now.
    ///
    /// Reads the file system, LaunchServices and the agent's settings file;
    /// callable from any thread.
    pub fn is_applicable(self) -> bool {
        match self {
            Step::MoveToApplications => {
                SystemIntegration::is_app_bundle()
                    && !SystemIntegration::is_permanently_installed()
                    && SystemIntegration::applications_destination().is_some()
            }
            Step::DefaultApplication => !SystemIntegration::is_default_markdown_handler(),
            Step::CommandLineTool => {
                SystemIntegration::command_line_tool_is_bundled() && !SystemIntegration::is_command_line_tool_installed()
            }
            // The hook runs `down`, so it is only offerable once the CLI
            // exists — either already installed, or about to be by the step
            // above, which runs first.
            Step::AgentIntegration => {
                (SystemIntegration::is_command_line_tool_installed() || Step::CommandLineTool.is_applicable())
                    && !AgentIntegration::is_installed()
            }
        }
    }
}

/// `var onFinish: (() -> Void)?`.
pub type FinishHandler = Rc<dyn Fn()>;

pub struct SetupWindowControllerIvars {
    /// Called once the panel is finished with, whatever the user chose.
    on_finish: RefCell<Option<FinishHandler>>,
    steps: Vec<Step>,
    checkboxes: RefCell<HashMap<Step, Retained<NSButton>>>,
    status_labels: RefCell<HashMap<Step, Retained<NSTextField>>>,
    quick_look_status: Retained<NSTextField>,
    quick_look_icon: Retained<NSImageView>,
    primary_button: Retained<NSButton>,
    secondary_button: Retained<NSButton>,
    spinner: Retained<NSProgressIndicator>,
    sheet: RefCell<Rc<StyleSheet>>,
    is_working: Cell<bool>,
}

define_class!(
    // SAFETY: `initWithWindow:` is forwarded in `init` after the ivars are
    // set; the action methods and delegate methods keep AppKit's signatures.
    #[unsafe(super(NSWindowController, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "SetupWindowController"]
    #[ivars = SetupWindowControllerIvars]
    pub struct SetupWindowController;

    unsafe impl NSObjectProtocol for SetupWindowController {}

    impl SetupWindowController {
        #[unsafe(method(declineSetup))]
        fn __decline_setup(&self) {
            self.finish();
        }

        #[unsafe(method(runSetup))]
        fn __run_setup(&self) {
            self.run_setup();
        }

        #[unsafe(method(openQuickLookSettings))]
        fn __open_quick_look_settings(&self) {
            SystemIntegration::open_quick_look_settings();
        }

        #[unsafe(method(finishFromButton))]
        fn __finish_from_button(&self) {
            self.finish();
        }
    }

    unsafe impl NSWindowDelegate for SetupWindowController {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            // Closing the panel mid-run would leave half a setup behind with
            // no way to see how it ended.
            !self.ivars().is_working.get()
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            // Covers the red button, which never reaches `finish()`.
            if Preferences::shared().values().has_answered_setup {
                return;
            }
            Preferences::shared().update(|values| values.has_answered_setup = true);
            self.call_on_finish();
        }
    }
);

impl SetupWindowController {
    /// `makeIfNeeded()`: `None` when there is nothing left to set up — the
    /// caller skips the panel entirely rather than showing a card with an
    /// empty checklist.
    pub fn make_if_needed(mtm: MainThreadMarker) -> Option<Retained<SetupWindowController>> {
        Self::make_for(Self::applicable_steps(), mtm)
    }

    /// `Step.allCases.filter(\.isApplicable)`: the checks `makeIfNeeded()`
    /// runs first. They read the file system and LaunchServices, so a caller
    /// may run this off the main thread and hand the result to
    /// [`make_for`](Self::make_for).
    pub fn applicable_steps() -> Vec<Step> {
        Step::ALL_CASES.into_iter().filter(|step| step.is_applicable()).collect()
    }

    /// The rest of `makeIfNeeded()`, for steps computed by
    /// [`applicable_steps`](Self::applicable_steps).
    pub fn make_for(applicable: Vec<Step>, mtm: MainThreadMarker) -> Option<Retained<SetupWindowController>> {
        // A move is never the only reason to interrupt someone: if the app is
        // already where it belongs and the rest is done, there is no panel.
        if applicable.is_empty() {
            return None;
        }
        Some(Self::init(applicable, mtm))
    }

    /// `private init(steps:)`.
    fn init(steps: Vec<Step>, mtm: MainThreadMarker) -> Retained<SetupWindowController> {
        let app = NSApplication::sharedApplication(mtm);
        let quick_look_status = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        let quick_look_icon = NSImageView::new(mtm);
        let primary_button = NSButton::new(mtm);
        let secondary_button = NSButton::new(mtm);
        let spinner = NSProgressIndicator::new(mtm);
        let sheet = StyleSheet::new(ThemeStore::shared().current(), &app.effectiveAppearance(), None);

        // SAFETY: a plain titled window; its controller owns it (AppKit
        // ignores `releasedWhenClosed` for windows owned by a controller).
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, Layout::WINDOW_WIDTH, 400.0),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("Welcome to Upleft"));
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setTitlebarAppearsTransparent(true);
        window.setMovableByWindowBackground(true);
        window.setRestorable(false);

        let this = Self::alloc(mtm).set_ivars(SetupWindowControllerIvars {
            on_finish: RefCell::new(None),
            steps,
            checkboxes: RefCell::new(HashMap::new()),
            status_labels: RefCell::new(HashMap::new()),
            quick_look_status,
            quick_look_icon,
            primary_button,
            secondary_button,
            spinner,
            sheet: RefCell::new(Rc::new(sheet)),
            is_working: Cell::new(false),
        });
        let this: Retained<SetupWindowController> = unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };

        // The card is painted in theme colours but its buttons are drawn by
        // AppKit, so the appearance has to be settled before anything reads a
        // colour — including the sheet the labels below are built from.
        let theme = ThemeStore::shared().current();
        window.apply_theme_appearance(&theme);
        *this.ivars().sheet.borrow_mut() = Rc::new(StyleSheet::new(theme, &window.effectiveAppearance(), None));

        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        let content = this.build_content(mtm);
        window.setContentView(Some(&content));

        // Height follows the checklist. Two steps and no Quick Look line is a
        // shorter panel than three and one, and a fixed height leaves the
        // difference as dead air above the buttons.
        content.layoutSubtreeIfNeeded();
        let size = NSSize::new(Layout::WINDOW_WIDTH, content.fittingSize().height.ceil());
        window.setContentSize(size);
        window.setMinSize(size);
        window.setMaxSize(size);
        let sheet = this.sheet();
        window.setBackgroundColor(Some(&sheet.background));
        window.setInitialFirstResponder(Some(&this.ivars().primary_button));
        window.center();
        this
    }

    /// `onFinish`.
    pub fn on_finish(&self) -> Option<FinishHandler> {
        self.ivars().on_finish.borrow().clone()
    }

    /// `onFinish = …`.
    pub fn set_on_finish(&self, handler: Option<FinishHandler>) {
        *self.ivars().on_finish.borrow_mut() = handler;
    }

    /// The steps this panel offers, in `Step.allCases` order.
    pub fn steps(&self) -> &[Step] {
        &self.ivars().steps
    }

    fn sheet(&self) -> Rc<StyleSheet> {
        self.ivars().sheet.borrow().clone()
    }

    fn call_on_finish(&self) {
        let handler = self.ivars().on_finish.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    // MARK: - Building

    fn build_content(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let sheet = self.sheet();
        let root = NSView::new(mtm);
        // A fixed width, so every wrapping label resolves a real intrinsic
        // height and `fittingSize` can be trusted.
        root.widthAnchor().constraintEqualToConstant(Layout::WINDOW_WIDTH).setActive(true);

        let brand = NSImageView::new(mtm);
        brand.setImage(NSApplication::sharedApplication(mtm).applicationIconImage().as_deref());
        brand.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        brand.setTranslatesAutoresizingMaskIntoConstraints(false);

        let title = NSTextField::labelWithString(&NSString::from_str("Welcome to Upleft"), mtm);
        title.setFont(Some(&NSFont::systemFontOfSize_weight(21.0, unsafe { NSFontWeightSemibold })));
        title.setTextColor(Some(&sheet.text));

        let subtitle = NSTextField::wrappingLabelWithString(&NSString::from_str(self.subtitle_text()), mtm);
        subtitle.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        subtitle.setTextColor(Some(&sheet.text_secondary));

        let heading = stack_view(&[title.as_super().as_super(), subtitle.as_super().as_super()], mtm);
        heading.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        heading.setAlignment(NSLayoutAttribute::Leading);
        heading.setSpacing(3.0);

        let header = stack_view(&[brand.as_super().as_super(), heading.as_super()], mtm);
        header.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        header.setAlignment(NSLayoutAttribute::CenterY);
        header.setSpacing(14.0);

        let rows: Vec<Retained<NSView>> = self.ivars().steps.iter().map(|step| self.make_step_row(*step, mtm)).collect();
        let row_refs: Vec<&NSView> = rows.iter().map(|row| &**row).collect();
        let checklist = stack_view(&row_refs, mtm);
        checklist.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        checklist.setAlignment(NSLayoutAttribute::Leading);
        checklist.setSpacing(16.0);

        let divider = self.make_divider(mtm);
        let stack = stack_view(&[header.as_super(), &divider, checklist.as_super()], mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(20.0);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setCustomSpacing_afterView(18.0, &header);

        // Quick Look gets a status line rather than a checkbox: there is no
        // sensible reason to decline it and no cost to leaving it on, so
        // presenting it as a decision would be a decision made for show.
        if SystemIntegration::quick_look_extensions_are_bundled() {
            stack.addArrangedSubview(&self.make_quick_look_row(mtm));
        }

        let ivars = self.ivars();
        self.configure(&ivars.secondary_button, "Not now", false, sel!(declineSetup));
        self.configure(&ivars.primary_button, "Continue", true, sel!(runSetup));

        ivars.spinner.setStyle(NSProgressIndicatorStyle::Spinning);
        ivars.spinner.setControlSize(NSControlSize::Small);
        ivars.spinner.setDisplayedWhenStopped(false);
        ivars.spinner.setTranslatesAutoresizingMaskIntoConstraints(false);

        let spacer = NSView::new(mtm);
        let buttons = stack_view(
            &[
                ivars.spinner.as_super(),
                &spacer,
                ivars.secondary_button.as_super().as_super(),
                ivars.primary_button.as_super().as_super(),
            ],
            mtm,
        );
        buttons.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        buttons.setAlignment(NSLayoutAttribute::CenterY);
        buttons.setSpacing(10.0);
        buttons.setTranslatesAutoresizingMaskIntoConstraints(false);

        root.addSubview(&stack);
        root.addSubview(&buttons);

        let constraints = NSArray::from_retained_slice(&[
            brand.widthAnchor().constraintEqualToConstant(52.0),
            brand.heightAnchor().constraintEqualToConstant(52.0),
            subtitle.widthAnchor().constraintEqualToConstant(Layout::WINDOW_WIDTH - Layout::INSET * 2.0 - 66.0),
            stack.leadingAnchor().constraintEqualToAnchor_constant(&root.leadingAnchor(), Layout::INSET),
            stack.trailingAnchor().constraintEqualToAnchor_constant(&root.trailingAnchor(), -Layout::INSET),
            stack.topAnchor().constraintEqualToAnchor_constant(&root.topAnchor(), 30.0),
            // An unbroken top-to-bottom chain: without it the root view has
            // no height to fit to and the window falls back to whatever it
            // was created with.
            buttons.topAnchor().constraintEqualToAnchor_constant(&stack.bottomAnchor(), 26.0),
            buttons.leadingAnchor().constraintEqualToAnchor_constant(&root.leadingAnchor(), Layout::INSET),
            buttons.trailingAnchor().constraintEqualToAnchor_constant(&root.trailingAnchor(), -Layout::INSET),
            buttons.bottomAnchor().constraintEqualToAnchor_constant(&root.bottomAnchor(), -24.0),
        ]);
        NSLayoutConstraint::activateConstraints(&constraints);
        root
    }

    fn subtitle_text(&self) -> &'static str {
        if self.ivars().steps.contains(&Step::MoveToApplications) {
            "One thing first — Upleft isn’t installed yet."
        } else {
            "Two quick things and you’re set up."
        }
    }

    fn make_divider(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let sheet = self.sheet();
        let divider = NSView::new(mtm);
        divider.setWantsLayer(true);
        if let Some(layer) = divider.layer() {
            let color = sheet.rule.colorWithAlphaComponent(if sheet.increase_contrast { 0.6 } else { 0.4 });
            layer.setBackgroundColor(Some(&color.CGColor()));
        }
        divider.setTranslatesAutoresizingMaskIntoConstraints(false);
        divider.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
        divider.widthAnchor().constraintEqualToConstant(Layout::WINDOW_WIDTH - Layout::INSET * 2.0).setActive(true);
        divider
    }

    fn make_step_row(&self, step: Step, mtm: MainThreadMarker) -> Retained<NSView> {
        let sheet = self.sheet();
        // SAFETY: no target and no action.
        let checkbox =
            unsafe { NSButton::checkboxWithTitle_target_action(&NSString::from_str(step.title()), None, None, mtm) };
        checkbox.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, unsafe { NSFontWeightMedium })));
        checkbox.setContentTintColor(Some(&sheet.text));
        checkbox.setState(if step.is_preselected() { NSControlStateValueOn } else { NSControlStateValueOff });
        checkbox.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.ivars().checkboxes.borrow_mut().insert(step, checkbox.clone());

        let detail = NSTextField::wrappingLabelWithString(&NSString::from_str(step.detail()), mtm);
        detail.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        detail.setTextColor(Some(&sheet.text_faint));
        detail.setTranslatesAutoresizingMaskIntoConstraints(false);

        // Wrapping, not single-line: some outcomes need a whole sentence, and
        // the one that names a System Settings path needs two.
        let status = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        status.setFont(Some(&NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
        status.setTextColor(Some(&sheet.text_secondary));
        status.setHidden(true);
        status.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.ivars().status_labels.borrow_mut().insert(step, status.clone());

        let column = stack_view(
            &[checkbox.as_super().as_super(), detail.as_super().as_super(), status.as_super().as_super()],
            mtm,
        );
        column.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        column.setAlignment(NSLayoutAttribute::Leading);
        column.setSpacing(2.0);
        column.setCustomSpacing_afterView(4.0, &detail);
        column.setTranslatesAutoresizingMaskIntoConstraints(false);

        // The detail line hangs under the checkbox label, not under its box.
        detail.leadingAnchor().constraintEqualToAnchor_constant(&column.leadingAnchor(), 20.0).setActive(true);
        status.leadingAnchor().constraintEqualToAnchor_constant(&column.leadingAnchor(), 20.0).setActive(true);
        detail.widthAnchor().constraintEqualToConstant(Layout::DETAIL_WIDTH).setActive(true);
        status.widthAnchor().constraintEqualToConstant(Layout::DETAIL_WIDTH).setActive(true);
        Retained::into_super(column)
    }

    fn make_quick_look_row(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let sheet = self.sheet();
        let ivars = self.ivars();
        let icon = &ivars.quick_look_icon;
        icon.setImage(NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("eye"), None).as_deref());
        icon.setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithPointSize_weight(12.0, unsafe {
            NSFontWeightMedium
        })));
        icon.setContentTintColor(Some(&sheet.text_faint));
        icon.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon.setAccessibilityHidden(true);

        let status = &ivars.quick_look_status;
        status.setStringValue(&NSString::from_str(
            "Quick Look previews and Finder icons for Markdown — set up for you.",
        ));
        status.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        status.setTextColor(Some(&sheet.text_faint));
        status.setTranslatesAutoresizingMaskIntoConstraints(false);

        let row = stack_view(&[icon.as_super().as_super(), status.as_super().as_super()], mtm);
        row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        row.setAlignment(NSLayoutAttribute::FirstBaseline);
        row.setSpacing(6.0);
        status.widthAnchor().constraintEqualToConstant(Layout::DETAIL_WIDTH).setActive(true);
        Retained::into_super(row)
    }

    fn configure(&self, button: &NSButton, title: &str, is_default: bool, action: Sel) {
        button.setTitle(&NSString::from_str(title));
        // Swift's `.rounded`, which the SDK now spells `.push` (same value).
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Large);
        // SAFETY: `self` implements `action` and owns the button.
        unsafe {
            button.setTarget(Some(self));
            button.setAction(Some(action));
        }
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        if is_default {
            button.setKeyEquivalent(&NSString::from_str("\r"));
            button.setHasDestructiveAction(false);
        }
    }

    // MARK: - Actions

    fn run_setup(&self) {
        let ivars = self.ivars();
        if ivars.is_working.get() {
            return;
        }

        // A move invalidates every path the other steps would write, so it
        // runs alone: copy, relaunch, and let the fresh instance — which sees
        // an unanswered setup — offer the rest from its permanent home.
        if self.is_checked(Step::MoveToApplications) {
            self.perform_move();
            return;
        }

        ivars.is_working.set(true);
        self.set_controls_enabled(false);
        // SAFETY: a nil sender.
        unsafe { ivars.spinner.startAnimation(None) };
        ivars.primary_button.setTitle(&NSString::from_str("Setting up…"));

        // `Task { @MainActor in … }`: the steps start on a later turn of the
        // main queue, each one after the previous has reported.
        let this = self.retain();
        upleft_render::appkit_compat::main_async(move || Self::setup_default_application(this));
    }

    fn setup_default_application(this: Retained<Self>) {
        if !this.is_checked(Step::DefaultApplication) {
            Self::setup_command_line_tool(this);
            return;
        }
        let mtm = this.mtm();
        SystemIntegration::make_default_markdown_handler(
            move |failure: Option<CocoaError>| {
                let report = move |this: Retained<Self>, is_default: bool, failure: Option<CocoaError>| {
                    this.report(
                        Step::DefaultApplication,
                        failure.is_none() && is_default,
                        "Markdown files now open in Upleft.",
                        &failure.map_or_else(
                            || "macOS declined the change. Finder’s Get Info → Open With can set it.".to_owned(),
                            |failure| format!("Couldn’t set the default: {}", failure.localized_description),
                        ),
                    );
                    Self::setup_command_line_tool(this);
                };
                if failure.is_some() {
                    // `failure == nil && …` never asks LaunchServices.
                    report(this, false, failure);
                    return;
                }
                off_main(SystemIntegration::is_default_markdown_handler, move |is_default| {
                    report(this, is_default, None)
                });
            },
            mtm,
        );
    }

    fn setup_command_line_tool(this: Retained<Self>) {
        if !this.is_checked(Step::CommandLineTool) {
            Self::setup_agent_integration(this);
            return;
        }
        off_main(SystemIntegration::install_command_line_tool, move |result| {
            this.report_command_line_tool(result);
            Self::setup_agent_integration(this);
        });
    }

    /// After the CLI, never before: the hook stores an absolute path to
    /// `down`, and there is nothing to point at until the symlink exists.
    fn setup_agent_integration(this: Retained<Self>) {
        if !this.is_checked(Step::AgentIntegration) {
            Self::register_quick_look(this);
            return;
        }
        off_main(AgentIntegration::install, move |result| {
            this.report_agent_integration(result);
            Self::register_quick_look(this);
        });
    }

    fn register_quick_look(this: Retained<Self>) {
        if !SystemIntegration::quick_look_extensions_are_bundled() {
            this.finish_setup_run();
            return;
        }
        this.ivars()
            .quick_look_status
            .setStringValue(&NSString::from_str("Registering Quick Look previews and Finder icons…"));

        let mtm = this.mtm();
        SystemIntegration::register_with_system(
            true,
            move |enabled| {
                let sheet = this.sheet();
                let ivars = this.ivars();
                if enabled {
                    ivars.quick_look_icon.setImage(
                        NSImage::imageWithSystemSymbolName_accessibilityDescription(
                            &NSString::from_str("checkmark.circle.fill"),
                            None,
                        )
                        .as_deref(),
                    );
                    ivars.quick_look_icon.setContentTintColor(Some(&sheet.accent));
                    ivars.quick_look_status.setStringValue(&NSString::from_str(
                        "Quick Look previews and Finder icons are on. Press space on a .md file to try it.",
                    ));
                } else {
                    // The extension is installed but the system has not
                    // switched it on, which only the user can do — so say
                    // where, precisely.
                    ivars.quick_look_icon.setImage(
                        NSImage::imageWithSystemSymbolName_accessibilityDescription(
                            &NSString::from_str("exclamationmark.circle"),
                            None,
                        )
                        .as_deref(),
                    );
                    ivars.quick_look_icon.setContentTintColor(Some(&sheet.text_secondary));
                    ivars.quick_look_status.setStringValue(&NSString::from_str(
                        "Quick Look needs one switch from you: System Settings → General → Login Items & Extensions → Quick Look.",
                    ));
                    this.show_quick_look_settings_shortcut();
                }
                this.finish_setup_run();
            },
            mtm,
        );
    }

    /// The end of the setup `Task`: its last statements, then its `defer`.
    fn finish_setup_run(&self) {
        let ivars = self.ivars();
        ivars.primary_button.setTitle(&NSString::from_str("Done"));
        // SAFETY: `finishFromButton` is one of this class's methods.
        unsafe { ivars.primary_button.setAction(Some(sel!(finishFromButton))) };
        ivars.secondary_button.setHidden(true);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&ivars.primary_button));
        }

        // `defer`.
        // SAFETY: a nil sender.
        unsafe { ivars.spinner.stopAnimation(None) };
        ivars.is_working.set(false);
        ivars.primary_button.setEnabled(true);
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&ivars.primary_button));
        }
    }

    fn perform_move(&self) {
        let ivars = self.ivars();
        self.set_controls_enabled(false);
        // SAFETY: a nil sender.
        unsafe { ivars.spinner.startAnimation(None) };
        ivars.primary_button.setTitle(&NSString::from_str("Moving…"));
        // Terminates this process once the copy is running; nothing after
        // that point executes.
        let weak_self = objc2::rc::Weak::from(self);
        let moved = SystemIntegration::move_to_applications(
            move |error| {
                if let Some(this) = weak_self.load() {
                    this.report_move_failure(&format!(
                        "Copied to Applications, but the new copy wouldn’t start: {}. Open it from Applications yourself.",
                        error.localized_description
                    ));
                }
            },
            self.mtm(),
        );
        if let Err(error) = moved {
            self.report_move_failure(&format!(
                "Couldn’t move the app: {}. Drag it to Applications yourself, then reopen it.",
                error.localized_description
            ));
        }
    }

    fn report_move_failure(&self, text: &str) {
        let ivars = self.ivars();
        // SAFETY: a nil sender.
        unsafe { ivars.spinner.stopAnimation(None) };
        self.set_controls_enabled(true);
        ivars.primary_button.setTitle(&NSString::from_str("Continue"));
        self.report(Step::MoveToApplications, false, "", text);
        // Unticked so a second Continue proceeds with the steps that can
        // still work from where the app is, rather than retrying the move
        // forever.
        let checkbox = ivars.checkboxes.borrow().get(&Step::MoveToApplications).cloned();
        if let Some(checkbox) = checkbox {
            checkbox.setState(NSControlStateValueOff);
        }
    }

    /// `installCommandLineTool()`'s reporting, for the result of
    /// `SystemIntegration.installCommandLineTool()`.
    fn report_command_line_tool(&self, result: Result<CommandLineResult, CocoaError>) {
        match result {
            Ok(result) => {
                if result.linked.is_empty() {
                    self.report(
                        Step::CommandLineTool,
                        false,
                        "",
                        &format!(
                            "Something else already owns {} in {}.",
                            result.skipped.join(" and "),
                            result.directory.path()
                        ),
                    );
                    return;
                }
                let names = result.linked.join(" and ");
                // Reporting a success the terminal will contradict is worse
                // than reporting the caveat, so an off-PATH directory says so.
                let text = if result.is_on_path {
                    format!("Installed {names} in {}.", result.directory.path())
                } else {
                    format!("Installed in {} — add it to your PATH to use {names}.", result.directory.path())
                };
                self.report(Step::CommandLineTool, result.is_on_path, &text, &text);
            }
            Err(error) => {
                self.report(
                    Step::CommandLineTool,
                    false,
                    "",
                    &format!("Couldn’t install it: {}", error.localized_description),
                );
            }
        }
    }

    /// `installAgentIntegration()`'s reporting, for the result of
    /// `AgentIntegration.install()`.
    fn report_agent_integration(&self, result: Result<bool, Failure>) {
        match result {
            Ok(_) => self.report(Step::AgentIntegration, true, "Agent edits now open in Upleft.", ""),
            // The most likely failure is the CLI step above having failed,
            // which has already reported itself; say what this step needs
            // rather than repeating that.
            Err(error) => self.report(Step::AgentIntegration, false, "", &error.error_description()),
        }
    }

    fn show_quick_look_settings_shortcut(&self) {
        let ivars = self.ivars();
        ivars.secondary_button.setHidden(false);
        ivars.secondary_button.setTitle(&NSString::from_str("Open Settings"));
        // SAFETY: `openQuickLookSettings` is one of this class's methods.
        unsafe { ivars.secondary_button.setAction(Some(sel!(openQuickLookSettings))) };
    }

    // MARK: - Plumbing

    fn is_checked(&self, step: Step) -> bool {
        self.ivars().checkboxes.borrow().get(&step).is_some_and(|checkbox| checkbox.state() == NSControlStateValueOn)
    }

    fn set_controls_enabled(&self, enabled: bool) {
        let ivars = self.ivars();
        ivars.primary_button.setEnabled(enabled);
        ivars.secondary_button.setEnabled(enabled);
        let checkboxes: Vec<Retained<NSButton>> = ivars.checkboxes.borrow().values().cloned().collect();
        for checkbox in checkboxes {
            checkbox.setEnabled(enabled);
        }
    }

    fn report(&self, step: Step, success: bool, success_text: &str, failure_text: &str) {
        let Some(label) = self.ivars().status_labels.borrow().get(&step).cloned() else { return };
        let sheet = self.sheet();
        label.setStringValue(&NSString::from_str(if success { success_text } else { failure_text }));
        label.setTextColor(Some(if success { &sheet.accent } else { &sheet.text_secondary }));
        label.setHidden(false);
    }

    fn finish(&self) {
        Preferences::shared().update(|values| values.has_answered_setup = true);
        self.call_on_finish();
        if let Some(window) = self.window() {
            window.close();
        }
    }
}

/// `NSStackView(views:)`.
fn stack_view(views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
    NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm)
}

/// Runs `work` on the user-initiated global queue and hands its result to
/// `then` on the main queue.
fn off_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, then: impl FnOnce(T) + 'static) {
    let mtm = MainThreadMarker::new().expect("off_main is called on the main thread");
    let then = MainThreadBound::new(Box::new(then) as Box<dyn FnOnce(T)>, mtm);
    DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated)).exec_async(
        move || {
            let value = work();
            DispatchQueue::main().exec_async(move || {
                let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                (then.into_inner(mtm))(value);
            });
        },
    );
}

