//! Port of `Panels/TrustPromptView.swift`: the non-modal, read-only prompt
//! for an external effect.  It reports a typed decision; it never opens a
//! URL, reads a file, or launches an application.
//!
//! The safe answer is the default one.  `⏎` denies and `⎋` denies, so a
//! reader clearing a stack of prompts by holding Return grants nothing
//! (§11.4).  Allow keeps a plain bezel so it never looks like the
//! recommended answer.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSControlSize, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSResponder,
    NSStackView, NSStackViewDistribution, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSRect, NSString};
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{
    activate, ns_string, object, role, set_label, set_role, set_value, weight_semibold, wrapping_label,
};
use super::panel_chrome::{PanelBackdrop, PanelFont, PanelMetrics, PanelSurface, install_backdrop};
use crate::security::document_trust::{TrustEffect, TrustRequest};
use crate::updater::update_metadata::Url;

/// `TrustPromptDecision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustPromptDecision {
    AllowOnce,
    AllowForFile,
    AllowForFolder,
    Deny,
    Revoke,
}

/// `TrustPromptViewDelegate`.
pub trait TrustPromptViewDelegate {
    fn trust_prompt_did_choose(&self, view: &TrustPromptView, decision: TrustPromptDecision, request: &TrustRequest);
}

pub struct TrustPromptViewIvars {
    delegate: RefCell<Option<Weak<dyn TrustPromptViewDelegate>>>,
    request: RefCell<Option<TrustRequest>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    target_label: Retained<NSTextField>,
    consequence_label: Retained<NSTextField>,
    asked_by_label: Retained<NSTextField>,
    deny_button: Retained<NSButton>,
    allow_once_button: Retained<NSButton>,
    allow_file_button: Retained<NSButton>,
    allow_folder_button: Retained<NSButton>,
    revoke_button: Retained<NSButton>,
}

define_class!(
    /// `TrustPromptView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TrustPromptView"]
    #[ivars = TrustPromptViewIvars]
    pub struct TrustPromptView;

    unsafe impl NSObjectProtocol for TrustPromptView {}

    impl TrustPromptView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        /// Esc denies, matching the default button rather than fighting it.
        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.choose(TrustPromptDecision::Deny);
        }

        #[unsafe(method(allowOnce:))]
        fn __allow_once(&self, _sender: Option<&AnyObject>) {
            self.choose(TrustPromptDecision::AllowOnce);
        }

        #[unsafe(method(allowFile:))]
        fn __allow_file(&self, _sender: Option<&AnyObject>) {
            self.choose(TrustPromptDecision::AllowForFile);
        }

        #[unsafe(method(allowFolder:))]
        fn __allow_folder(&self, _sender: Option<&AnyObject>) {
            self.choose(TrustPromptDecision::AllowForFolder);
        }

        #[unsafe(method(deny:))]
        fn __deny(&self, _sender: Option<&AnyObject>) {
            self.choose(TrustPromptDecision::Deny);
        }

        #[unsafe(method(revoke:))]
        fn __revoke(&self, _sender: Option<&AnyObject>) {
            self.choose(TrustPromptDecision::Revoke);
        }
    }
);

impl PanelSurface for TrustPromptView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

fn button(title: &str, mtm: MainThreadMarker) -> Retained<NSButton> {
    unsafe { NSButton::buttonWithTitle_target_action(&ns_string(title), None, None, mtm) }
}

impl TrustPromptView {
    /// `TrustPromptView()`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<TrustPromptView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<TrustPromptView> {
        let title_label = wrapping_label("Permission Needed", mtm);
        let target_label = wrapping_label("", mtm);
        let consequence_label = wrapping_label("", mtm);
        let asked_by_label = wrapping_label("", mtm);
        let deny_button = button("Don’t Allow", mtm);
        let allow_once_button = button("Allow Once", mtm);
        let allow_file_button = button("Always Allow for This File", mtm);
        let allow_folder_button = button("Always Allow for This Folder", mtm);
        let revoke_button = button("Revoke Existing Permission", mtm);
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(TrustPromptViewIvars {
            delegate: RefCell::new(None),
            request: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            backdrop: backdrop.clone(),
            title_label: title_label.clone(),
            target_label: target_label.clone(),
            consequence_label: consequence_label.clone(),
            asked_by_label: asked_by_label.clone(),
            deny_button: deny_button.clone(),
            allow_once_button: allow_once_button.clone(),
            allow_file_button: allow_file_button.clone(),
            allow_folder_button: allow_folder_button.clone(),
            revoke_button: revoke_button.clone(),
        });
        let this: Retained<TrustPromptView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        install_backdrop(&this, &backdrop);

        title_label.setFont(Some(&PanelFont::system(13.0, weight_semibold())));
        title_label.setMaximumNumberOfLines(3);
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&title_label);

        target_label.setFont(Some(&PanelFont::monospaced_regular(11.0)));
        target_label.setMaximumNumberOfLines(4);
        target_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&target_label);

        consequence_label.setFont(Some(&PanelFont::secondary()));
        consequence_label.setMaximumNumberOfLines(4);
        consequence_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&consequence_label);

        asked_by_label.setFont(Some(&PanelFont::secondary()));
        asked_by_label.setMaximumNumberOfLines(2);
        asked_by_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&asked_by_label);

        // Deny is the default and also the Esc key.  Both keys agree, and
        // both agree with the safe answer.
        this.configure(&deny_button, sel!(deny:), "Do not allow this action");
        deny_button.setKeyEquivalent(&NSString::from_str("\r"));
        this.configure(&allow_once_button, sel!(allowOnce:), "Allow this action once");
        this.configure(&allow_file_button, sel!(allowFile:), "Always allow this action for this file");
        this.configure(&allow_folder_button, sel!(allowFolder:), "Always allow this action for this folder");
        this.configure(&revoke_button, sel!(revoke:), "Revoke matching trust");
        revoke_button.setHasDestructiveAction(true);

        let views: [Retained<NSView>; 5] = [
            Retained::into_super(Retained::into_super(deny_button.clone())),
            Retained::into_super(Retained::into_super(allow_once_button.clone())),
            Retained::into_super(Retained::into_super(allow_file_button.clone())),
            Retained::into_super(Retained::into_super(allow_folder_button.clone())),
            Retained::into_super(Retained::into_super(revoke_button.clone())),
        ];
        let buttons = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&views), mtm);
        buttons.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        buttons.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
        buttons.setSpacing(5.0);
        buttons.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&buttons);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            title_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            title_label
                .topAnchor()
                .constraintEqualToAnchor_constant(&this.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            target_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            target_label.trailingAnchor().constraintEqualToAnchor(&title_label.trailingAnchor()),
            target_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 6.0),
            consequence_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            consequence_label.trailingAnchor().constraintEqualToAnchor(&title_label.trailingAnchor()),
            consequence_label.topAnchor().constraintEqualToAnchor_constant(&target_label.bottomAnchor(), 8.0),
            asked_by_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            asked_by_label.trailingAnchor().constraintEqualToAnchor(&title_label.trailingAnchor()),
            asked_by_label.topAnchor().constraintEqualToAnchor_constant(&consequence_label.bottomAnchor(), 4.0),
            buttons.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            buttons.trailingAnchor().constraintEqualToAnchor(&title_label.trailingAnchor()),
            buttons.topAnchor().constraintEqualToAnchor_constant(&asked_by_label.bottomAnchor(), 14.0),
            buttons
                .bottomAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&this.bottomAnchor(), -PanelMetrics::INSET),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Permission Needed");
        this.apply_style();
        this.reload();
        this
    }

    fn configure(&self, button: &NSButton, action: Sel, label: &str) {
        unsafe {
            button.setTarget(Some(object(self)));
            button.setAction(Some(action));
        }
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Small);
        button.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        set_label(button, label);
    }

    pub fn delegate(&self) -> Option<Rc<dyn TrustPromptViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn TrustPromptViewDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn request(&self) -> Option<TrustRequest> {
        self.ivars().request.borrow().clone()
    }

    pub fn set_request(&self, request: Option<TrustRequest>) {
        *self.ivars().request.borrow_mut() = request;
        self.reload();
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.apply_style();
    }

    // MARK: - Copy

    pub fn reload(&self) {
        let ivars = self.ivars();
        let Some(request) = self.request() else {
            ivars.title_label.setStringValue(&ns_string("No pending action"));
            ivars.target_label.setStringValue(&ns_string(""));
            ivars.consequence_label.setStringValue(&ns_string(""));
            ivars.asked_by_label.setStringValue(&ns_string(""));
            for button in self.all_buttons() {
                button.setHidden(true);
            }
            return;
        };
        for button in self.all_buttons() {
            button.setHidden(false);
        }

        // Lead with what is about to happen and to what, not with the word
        // "Permission".
        ivars.title_label.setStringValue(&ns_string(&Self::headline(&request)));
        ivars.target_label.setStringValue(&ns_string(&request.target.display_name));
        ivars.consequence_label.setStringValue(&ns_string(Self::consequence(request.effect)));
        let asked_by = match &request.document_path {
            Some(path) => format!("Requested by {}.", FileUrl::from_path(path).last_path_component()),
            None => "Requested by the current document.".to_owned(),
        };
        ivars.asked_by_label.setStringValue(&ns_string(&asked_by));

        let file_name = Self::file_grant_name(&request);
        ivars.allow_file_button.setHidden(file_name.is_none());
        ivars.allow_file_button.setTitle(&ns_string(&match &file_name {
            Some(name) => format!("Always Allow for {name}"),
            None => "Always Allow for This File".to_owned(),
        }));
        // Name the folder.  "Allow for Folder" without the folder is a grant
        // whose scope the reader cannot see.
        ivars.allow_folder_button.setHidden(self.folder_name().is_none());
        ivars.allow_folder_button.setTitle(&ns_string(&match self.folder_name() {
            Some(name) => format!("Always Allow for “{name}”"),
            None => "Always Allow for This Folder".to_owned(),
        }));

        // The group keeps its name; the specifics are its value.
        set_value(self, &format!("{} {}", ivars.title_label.stringValue(), request.target.display_name));
        for button in [&ivars.allow_file_button, &ivars.allow_folder_button] {
            set_label(&**button, &button.title().to_string());
        }
    }

    fn all_buttons(&self) -> [Retained<NSButton>; 5] {
        let ivars = self.ivars();
        [
            ivars.deny_button.clone(),
            ivars.allow_once_button.clone(),
            ivars.allow_file_button.clone(),
            ivars.allow_folder_button.clone(),
            ivars.revoke_button.clone(),
        ]
    }

    /// The file the policy will actually persist.  Local effects are scoped
    /// to their target; URL effects without a local target are scoped to the
    /// requesting document.  The button must name that same file.
    pub fn file_grant_name(request: &TrustRequest) -> Option<String> {
        let path = request.target.canonical_path.as_ref().or(request.document_path.as_ref());
        path.map(|path| FileUrl::from_path(path).last_path_component())
    }

    /// The folder a "Allow for Folder" grant would actually cover.
    fn folder_name(&self) -> Option<String> {
        let request = self.request()?;
        let path = request.target.canonical_path.or(request.document_path)?;
        let folder = FileUrl::from_path(&path).deleting_last_path_component();
        let last = folder.last_path_component();
        Some(if last.is_empty() { folder.path() } else { last })
    }

    fn headline(request: &TrustRequest) -> String {
        let external = request.target.external_url.as_deref();
        match request.effect {
            TrustEffect::OpenExternalLink => {
                format!("Open {} in your browser?", Self::host(external).unwrap_or_else(|| "an external site".to_owned()))
            }
            TrustEffect::LoadRemoteAsset => {
                format!("Load an image from {}?", Self::host(external).unwrap_or_else(|| "an external site".to_owned()))
            }
            TrustEffect::ReadLocalAsset => "Read a file from outside this document’s folder?".to_owned(),
            TrustEffect::LaunchPathOrEditor => "Open this path in another application?".to_owned(),
            TrustEffect::AutomationAppIntent => "Run an app or automation from this document?".to_owned(),
        }
    }

    fn consequence(effect: TrustEffect) -> &'static str {
        match effect {
            TrustEffect::OpenExternalLink => {
                "Allowing hands the address below to your default browser. Upleft does not load it."
            }
            TrustEffect::LoadRemoteAsset => {
                "Allowing contacts the server below and displays its image. The server can observe your IP address."
            }
            TrustEffect::ReadLocalAsset => {
                "Allowing lets this document display the file below. Upleft reads it; it never writes to it."
            }
            TrustEffect::LaunchPathOrEditor => {
                "Allowing asks macOS to open the path below in the application registered for it."
            }
            TrustEffect::AutomationAppIntent => {
                "Allowing lets this document ask another application to act on your behalf."
            }
        }
    }

    /// `URL(string:)?.host`.
    fn host(url_string: Option<&str>) -> Option<String> {
        Url::from_string(url_string?)?.host()
    }

    // MARK: - Decisions

    pub fn choose_for_testing(&self, decision: TrustPromptDecision) {
        self.choose(decision);
    }

    fn choose(&self, decision: TrustPromptDecision) {
        let Some(request) = self.request() else { return };
        if let Some(delegate) = self.delegate() {
            delegate.trust_prompt_did_choose(self, decision, &request);
        }
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        // The headline is the most important text on the panel, so it uses
        // the primary colour.
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        ivars.target_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.consequence_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.asked_by_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.deny_button.setContentTintColor(Some(&style_sheet.text));
        for button in [&ivars.allow_once_button, &ivars.allow_file_button, &ivars.allow_folder_button, &ivars.revoke_button]
        {
            button.setContentTintColor(Some(&style_sheet.text_secondary));
        }
    }
}

#[allow(unused)]
fn _unused(_: NSStackViewDistribution) {}
