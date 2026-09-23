//! The recents half of the start window (`// MARK: - Recents` and
//! `// MARK: - Recent row` of `App/StartWindowController.swift`):
//! `RecentDocumentsPanel`, `RecentEmptyState` and `RecentDocumentButton`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSButton, NSButtonType, NSColor, NSControl, NSCursor, NSEvent, NSFocusRingType, NSFont,
    NSImageSymbolConfiguration, NSImageView, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSLineBreakMode, NSMenu, NSMenuItem, NSResponder,
    NSStackView, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::{CGAffineTransform, CGFloat};
use objc2_foundation::{NSArray, NSPoint, NSString};
use objc2_quartz_core::kCACornerCurveContinuous;
use upleft_render::appkit_compat::{RECT_ZERO, RectExt, main_after, ns_string};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::hero::KeycapBadgeField;
use super::{KeycapFormatter, RecentRowCopy, StartLayout, StartWindowController, configure_passive_label};
use crate::ai::document_state_store::RecentDocument;
use crate::panels::appkit_support::{activate, cg, role, set_help, set_label, set_role, set_value, weight_medium,
    weight_regular, weight_semibold};

/// `CGAffineTransform(scaleX:y:)`.
pub(super) fn scale(x: CGFloat, y: CGFloat) -> CGAffineTransform {
    CGAffineTransform { a: x, b: 0.0, c: 0.0, d: y, tx: 0.0, ty: 0.0 }
}

/// `CGAffineTransform.identity`.
pub(super) const IDENTITY: CGAffineTransform = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

/// `NSStackView(views:)`.
pub(super) fn stack_view(views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
    NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm)
}

/// Swift's `==` on two `[RecentDocument]` (the synthesized `Equatable`:
/// `String` fields by canonical equivalence, `Date` by value).
fn same_recents(a: &[RecentDocument], b: &[RecentDocument]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            swift_text::str_eq(&a.path, &b.path)
                && swift_text::str_eq(&a.display_name, &b.display_name)
                && swift_text::str_eq(&a.first_heading, &b.first_heading)
                && a.last_opened == b.last_opened
                && a.word_count == b.word_count
        })
}

// MARK: - RecentDocumentsPanel

pub(super) struct RecentDocumentsPanelIvars {
    owner: RefCell<Weak<StartWindowController>>,
    header_label: Retained<NSTextField>,
    count_label: Retained<NSTextField>,
    divider: Retained<NSView>,
    list: Retained<NSStackView>,
    row_views: RefCell<Vec<Retained<RecentDocumentButton>>>,
    displayed_recents: RefCell<Vec<RecentDocument>>,
    sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    /// The recents list: a quiet header (label, shortcut hint) over
    /// equal-height rows. Uniform row height and tight spacing make the list
    /// read as one gestalt group; the title carries the weight, while the
    /// timestamp and ordinal keycap stay in a stable trailing rail.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "RecentDocumentsPanel"]
    #[ivars = RecentDocumentsPanelIvars]
    pub(super) struct RecentDocumentsPanel;

    unsafe impl NSObjectProtocol for RecentDocumentsPanel {}
);

impl RecentDocumentsPanel {
    /// `init(recents:owner:sheet:)`.
    pub(super) fn new(
        recents: &[RecentDocument],
        owner: &StartWindowController,
        sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<RecentDocumentsPanel> {
        let this = Self::alloc(mtm).set_ivars(RecentDocumentsPanelIvars {
            owner: RefCell::new(Weak::from(owner)),
            header_label: NSTextField::labelWithString(&NSString::from_str("Recent files"), mtm),
            count_label: NSTextField::labelWithString(&NSString::from_str(""), mtm),
            divider: NSView::new(mtm),
            list: NSStackView::new(mtm),
            row_views: RefCell::new(Vec::new()),
            displayed_recents: RefCell::new(Vec::new()),
            sheet: RefCell::new(sheet),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        // SAFETY: the menu's items target the controller, which outlives
        // the panel's window.
        unsafe { this.setMenu(Some(&Self::make_context_menu(owner, None, mtm))) };

        // A hairline rule separates the decide phase (hero) from the continue
        // phase (recents); two kinds of work should not share one silent gap.
        ivars.divider.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.divider.setWantsLayer(true);
        this.addSubview(&ivars.divider);

        ivars.header_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.header_label.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, weight_semibold())));
        configure_passive_label(&ivars.header_label);

        ivars.count_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.count_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.5, weight_regular())));
        configure_passive_label(&ivars.count_label);

        let header = stack_view(&[&ivars.header_label, &ivars.count_label], mtm);
        header.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        header.setAlignment(NSLayoutAttribute::CenterY);
        header.setSpacing(7.0);
        header.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&header);

        ivars.list.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.list.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        ivars.list.setAlignment(NSLayoutAttribute::Width);
        ivars.list.setSpacing(StartLayout::ROW_SPACING);
        this.addSubview(&ivars.list);

        activate(&[
            ivars.divider.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            ivars.divider.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            ivars.divider.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            ivars.divider.heightAnchor().constraintEqualToConstant(1.0),
            // Keep the section label on the same vertical grid as each file
            // title; the document glyph lives in the small leading gutter.
            header.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 34.0),
            header.topAnchor().constraintEqualToAnchor_constant(&ivars.divider.bottomAnchor(), 12.0),
            ivars.list.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            ivars.list.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            ivars.list.topAnchor().constraintEqualToAnchor_constant(&header.bottomAnchor(), 8.0),
            ivars.list.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            this.widthAnchor().constraintEqualToConstant(StartLayout::CONTENT_WIDTH),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Recent Markdown documents");
        let sheet = this.sheet();
        this.apply(sheet);
        this.rebuild(recents);
        this
    }

    fn sheet(&self) -> Rc<StyleSheet> {
        self.ivars().sheet.borrow().clone()
    }

    /// `reload(recents:owner:)`.
    pub(super) fn reload(&self, recents: &[RecentDocument], owner: &StartWindowController) {
        *self.ivars().owner.borrow_mut() = Weak::from(owner);
        if same_recents(recents, &self.ivars().displayed_recents.borrow()) {
            return;
        }
        self.rebuild(recents);
    }

    /// `apply(sheet:)`.
    pub(super) fn apply(&self, sheet: Rc<StyleSheet>) {
        *self.ivars().sheet.borrow_mut() = sheet.clone();
        let ivars = self.ivars();
        if let Some(layer) = ivars.divider.layer() {
            let color = sheet.rule.colorWithAlphaComponent(if sheet.increase_contrast { 0.6 } else { 0.4 });
            layer.setBackgroundColor(Some(&cg(&color)));
        }
        ivars.header_label.setTextColor(Some(&sheet.text_secondary));
        ivars.count_label.setTextColor(Some(&sheet.text_faint));
        let rows = self.row_buttons();
        for row in rows {
            row.apply(sheet.clone());
        }
    }

    /// `rowButtons`: the recent rows, in display order. StartView walks this
    /// for the arrow-key focus navigation.
    pub(super) fn row_buttons(&self) -> Vec<Retained<RecentDocumentButton>> {
        self.ivars().row_views.borrow().clone()
    }

    /// `revealRows()`.
    pub(super) fn reveal_rows(&self) {
        let reduce = self.sheet().reduce_motion;
        let rows = self.row_buttons();
        if reduce || rows.is_empty() {
            return;
        }
        for (index, row) in rows.iter().enumerate() {
            row.setWantsLayer(true);
            if let Some(layer) = row.layer() {
                layer.setOpacity(0.0);
            }
            let delay = 0.05 + index as f64 * 0.04;
            let weak_row: Weak<RecentDocumentButton> = Weak::from(&**row);
            main_after(delay, move || {
                let Some(row) = weak_row.load() else { return };
                motion::run(
                    false,
                    motion::STANDARD,
                    Curve::EaseOut,
                    move |_| {
                        if let Some(layer) = row.layer() {
                            layer.setOpacity(1.0);
                        }
                    },
                    None,
                );
            });
        }
    }

    /// `rebuild(recents:)`.
    fn rebuild(&self, recents: &[RecentDocument]) {
        let mtm = self.mtm();
        let ivars = self.ivars();
        for view in ivars.list.arrangedSubviews().iter() {
            ivars.list.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        ivars.row_views.borrow_mut().clear();
        *ivars.displayed_recents.borrow_mut() = recents.to_vec();

        let Some(owner) = ivars.owner.borrow().load() else { return };
        ivars.count_label.setStringValue(&NSString::from_str(""));
        ivars.count_label.setHidden(true);
        let sheet = self.sheet();
        if recents.is_empty() {
            let empty = RecentEmptyState::new(&sheet, mtm);
            empty.setTranslatesAutoresizingMaskIntoConstraints(false);
            ivars.list.addArrangedSubview(&empty);
            empty.widthAnchor().constraintEqualToAnchor(&ivars.list.widthAnchor()).setActive(true);
            // Exactly the height a full list would occupy, so the window is
            // the right size for both compositions and neither leaves a hole
            // under it. Derived rather than guessed: the old fixed 150 was
            // already 100pt short of six rows, and drifted further the moment
            // a row grew its second line.
            empty.heightAnchor().constraintEqualToConstant(StartLayout::populated_list_height()).setActive(true);
        } else {
            let titles = RecentRowCopy::disambiguated_titles(recents);
            for (index, recent) in recents.iter().take(StartWindowController::RECENT_DISPLAY_LIMIT).enumerate() {
                let row = RecentDocumentButton::new(recent, &titles[index], index as isize + 1, &owner, sheet.clone(), mtm);
                row.setTranslatesAutoresizingMaskIntoConstraints(false);
                row.heightAnchor().constraintEqualToConstant(StartLayout::ROW_HEIGHT).setActive(true);
                ivars.list.addArrangedSubview(&row);
                row.widthAnchor().constraintEqualToAnchor(&ivars.list.widthAnchor()).setActive(true);
                ivars.row_views.borrow_mut().push(row);
            }
        }
    }

    /// `makeContextMenu(owner:recentPath:)`: the panel's own menu carries the
    /// one global action; a row adds the three that need to know *which*
    /// file was clicked. Right-clicking a single entry and being offered
    /// nothing but "clear them all" answers a question nobody asked, with the
    /// most destructive verb in the list.
    pub(super) fn make_context_menu(
        owner: &StartWindowController,
        recent_path: Option<&str>,
        mtm: MainThreadMarker,
    ) -> Retained<NSMenu> {
        let menu = NSMenu::new(mtm);
        if let Some(recent_path) = recent_path {
            let row_actions: [(&str, Sel); 3] = [
                ("Show in Finder", sel!(showRecentInFinder:)),
                ("Copy Path", sel!(copyRecentPath:)),
                ("Remove from Recents", sel!(removeRecent:)),
            ];
            for (title, action) in row_actions {
                let item = menu_item(title, action, mtm);
                // SAFETY: the controller implements every row action; menu
                // items hold their target weakly, as in Swift.
                unsafe {
                    item.setTarget(Some(owner));
                    item.setRepresentedObject(Some(&ns_string(recent_path)));
                }
                menu.addItem(&item);
            }
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        let clear = menu_item("Clear Recent Files…", sel!(clearRecents:), mtm);
        // SAFETY: as above.
        unsafe { clear.setTarget(Some(owner)) };
        menu.addItem(&clear);
        menu
    }
}

/// `NSMenuItem(title:action:keyEquivalent: "")`.
fn menu_item(title: &str, action: Sel, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    // SAFETY: the action is sent to the item's target, which implements it.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &ns_string(title),
            Some(action),
            &NSString::from_str(""),
        )
    }
}

// MARK: - RecentEmptyState

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new`; no ivars.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "RecentEmptyState"]
    pub(super) struct RecentEmptyState;

    unsafe impl NSObjectProtocol for RecentEmptyState {}
);

impl RecentEmptyState {
    /// `init(sheet:)`.
    fn new(sheet: &StyleSheet, mtm: MainThreadMarker) -> Retained<RecentEmptyState> {
        let this: Retained<Self> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: RECT_ZERO] };
        this.setWantsLayer(true);
        let contrast = sheet.increase_contrast;
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(12.0);
            // SAFETY: Core Animation's corner-curve constant.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.04 } else { 0.02 }))));
            layer.setBorderWidth(1.0);
            layer.setBorderColor(Some(&cg(&sheet.rule.colorWithAlphaComponent(if contrast { 0.6 } else { 0.35 }))));
        }

        let icon_well = NSView::new(mtm);
        icon_well.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon_well.setWantsLayer(true);
        if let Some(layer) = icon_well.layer() {
            layer.setCornerRadius(24.0);
            // SAFETY: as above.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.08 } else { 0.04 }))));
        }

        let icon = NSImageView::new(mtm);
        icon.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon.setImage(
            objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("doc.text"), None)
                .as_deref(),
        );
        icon.setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithPointSize_weight(20.0, weight_regular())));
        icon.setContentTintColor(Some(&sheet.text_secondary));
        icon_well.addSubview(&icon);

        let title = NSTextField::labelWithString(&NSString::from_str("No recent files"), mtm);
        title.setFont(Some(&NSFont::systemFontOfSize_weight(13.5, weight_semibold())));
        title.setTextColor(Some(&sheet.text));
        title.setTranslatesAutoresizingMaskIntoConstraints(false);

        let detail = NSTextField::wrappingLabelWithString(&NSString::from_str("Open a Markdown file to see it here."), mtm);
        detail.setFont(Some(&NSFont::systemFontOfSize_weight(12.0, weight_regular())));
        detail.setTextColor(Some(&sheet.text_secondary));
        detail.setAlignment(NSTextAlignment::Center);
        detail.setMaximumNumberOfLines(2);
        detail.setTranslatesAutoresizingMaskIntoConstraints(false);
        configure_passive_label(&detail);
        configure_passive_label(&title);

        let stack = stack_view(&[&icon_well, &title, &detail], mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::CenterX);
        stack.setSpacing(8.0);
        stack.setCustomSpacing_afterView(10.0, &icon_well);
        stack.setCustomSpacing_afterView(3.0, &title);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&stack);

        activate(&[
            icon_well.widthAnchor().constraintEqualToConstant(48.0),
            icon_well.heightAnchor().constraintEqualToConstant(48.0),
            icon.centerXAnchor().constraintEqualToAnchor(&icon_well.centerXAnchor()),
            icon.centerYAnchor().constraintEqualToAnchor(&icon_well.centerYAnchor()),
            detail.widthAnchor().constraintLessThanOrEqualToConstant(240.0),
            stack.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()),
            stack.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
        ]);
        this
    }
}

// MARK: - RecentDocumentButton

pub(super) struct RecentDocumentButtonIvars {
    document_path: String,
    ordinal: isize,
    shell: Retained<NSView>,
    document_icon: Retained<NSImageView>,
    title_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
    detail_label: Retained<NSTextField>,
    shortcut_label: Retained<KeycapBadgeField>,
    is_hovered: Cell<bool>,
    is_pressed: Cell<bool>,
    sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // the overrides keep AppKit's signatures.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "RecentDocumentButton"]
    #[ivars = RecentDocumentButtonIvars]
    pub(super) struct RecentDocumentButton;

    unsafe impl NSObjectProtocol for RecentDocumentButton {}

    impl RecentDocumentButton {
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn __mouse_down_can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            self.bounds().contains_point(local).then(|| Retained::into_super(Retained::into_super(Retained::into_super(self.retain()))))
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            if !self.isEnabled() {
                return;
            }
            // Keep "selection = first responder" coherent: a click that does
            // not complete a handoff leaves arrow navigation pointing at this
            // row.
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            self.ivars().is_pressed.set(true);
            self.ivars().is_hovered.set(true);
            self.update_surface(false);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            let inside = self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            self.ivars().is_hovered.set(inside);
            self.ivars().is_pressed.set(inside);
            self.update_surface(false);
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            let inside = self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            self.ivars().is_pressed.set(false);
            self.ivars().is_hovered.set(inside);
            self.update_surface(true);
            if inside {
                // SAFETY: the button's own action and target.
                unsafe { self.sendAction_to(self.action(), self.target().as_deref()) };
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            if self.ivars().is_pressed.get() {
                return;
            }
            self.ivars().is_hovered.set(true);
            self.update_surface(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            if self.ivars().is_pressed.get() {
                return;
            }
            self.ivars().is_hovered.set(false);
            self.update_surface(true);
        }

        /// Custom tracking owns pressed/hover so AppKit highlight can't
        /// desync.
        #[unsafe(method(highlight:))]
        fn __highlight(&self, _flag: bool) {}

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn __become_first_responder(&self) -> bool {
            let result: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            self.update_surface(true);
            result
        }

        #[unsafe(method(resignFirstResponder))]
        fn __resign_first_responder(&self) -> bool {
            let result: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            self.update_surface(false);
            result
        }
    }
);

impl RecentDocumentButton {
    /// `init(recent:title:ordinal:target:sheet:)`.
    fn new(
        recent: &RecentDocument,
        title: &str,
        ordinal: isize,
        target: &StartWindowController,
        sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<RecentDocumentButton> {
        let shell = NSView::new(mtm);
        let document_icon = NSImageView::new(mtm);
        let shortcut_label = KeycapBadgeField::new(mtm);
        let title_label = NSTextField::labelWithString(&ns_string(title), mtm);
        let detail_label = NSTextField::labelWithString(&ns_string(&RecentRowCopy::timestamp(recent)), mtm);
        let subtitle_label = NSTextField::labelWithString(&ns_string(&RecentRowCopy::subtitle(recent, title)), mtm);
        let this = Self::alloc(mtm).set_ivars(RecentDocumentButtonIvars {
            document_path: recent.path.clone(),
            ordinal,
            shell,
            document_icon,
            title_label,
            subtitle_label,
            detail_label,
            shortcut_label,
            is_hovered: Cell::new(false),
            is_pressed: Cell::new(false),
            sheet: RefCell::new(sheet),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        // SAFETY: the controller implements `openRecent:`; a control holds
        // its target weakly, as in Swift.
        unsafe {
            this.setTarget(Some(target));
            this.setAction(Some(sel!(openRecent:)));
        }
        this.setIdentifier(Some(&ns_string(&recent.path)));
        this.setButtonType(NSButtonType::MomentaryChange);
        this.setBordered(false);
        this.setTitle(&NSString::from_str(""));
        this.setFocusRingType(NSFocusRingType::None);
        set_role(&*this, role::button());
        set_label(&*this, &format!("Open {title}"));
        set_value(
            &*this,
            &format!(
                "{}, {}",
                swift_text::ns::foundation::to_string(&ivars.subtitle_label.stringValue()),
                swift_text::ns::foundation::to_string(&ivars.detail_label.stringValue())
            ),
        );
        if ordinal <= 9 {
            set_help(&*this, &format!("Press ⌘{ordinal} to open"));
        } else {
            this.setAccessibilityHelp(None);
        }
        this.setWantsLayer(true);
        this.setToolTip(Some(&ns_string(&recent.path)));

        ivars.shell.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.shell.setWantsLayer(true);
        if let Some(layer) = ivars.shell.layer() {
            layer.setCornerRadius(StartLayout::CORNER_RADIUS);
            // SAFETY: Core Animation's corner-curve constant.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
        this.addSubview(&ivars.shell);

        ivars.document_icon.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.document_icon.setImage(
            objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("doc.text"),
                Some(&NSString::from_str("Markdown document")),
            )
            .as_deref(),
        );
        ivars
            .document_icon
            .setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithPointSize_weight(14.0, weight_regular())));
        set_label(&*ivars.document_icon, "Markdown document");
        ivars.shell.addSubview(&ivars.document_icon);

        ivars.title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.title_label.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, weight_medium())));
        ivars.title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        ivars.title_label.setMaximumNumberOfLines(1);
        configure_passive_label(&ivars.title_label);

        // What the document is actually about. A folder full of agent output
        // is a folder of interchangeable names; the first heading is the
        // only thing that tells them apart, and showing it here is the app's
        // whole argument made in one line before the user has read any copy.
        ivars.subtitle_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.subtitle_label.setFont(Some(&NSFont::systemFontOfSize_weight(11.5, weight_regular())));
        ivars.subtitle_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        ivars.subtitle_label.setMaximumNumberOfLines(1);
        configure_passive_label(&ivars.subtitle_label);

        ivars.detail_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.detail_label.setFont(Some(&NSFont::systemFontOfSize_weight(11.5, weight_regular())));
        ivars.detail_label.setAlignment(NSTextAlignment::Right);
        ivars.detail_label.setUsesSingleLineMode(true);
        ivars.detail_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        ivars.detail_label.setMaximumNumberOfLines(1);
        configure_passive_label(&ivars.detail_label);

        ivars.shortcut_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.shortcut_label.setStringValue(&ns_string(&if ordinal <= 9 { format!("⌘{ordinal}") } else { String::new() }));
        ivars.shortcut_label.set_min_width(28.0);
        ivars.shortcut_label.setHidden(ordinal > 9);
        set_label(
            &*ivars.shortcut_label,
            &format!("Keyboard shortcut {}", swift_text::ns::foundation::to_string(&ivars.shortcut_label.stringValue())),
        );

        let trailing_stack = stack_view(&[&ivars.detail_label, &ivars.shortcut_label], mtm);
        trailing_stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        trailing_stack.setAlignment(NSLayoutAttribute::CenterY);
        trailing_stack.setSpacing(7.0);
        trailing_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.shell.addSubview(&trailing_stack);

        // SAFETY: the menu's items target the controller.
        unsafe { this.setMenu(Some(&RecentDocumentsPanel::make_context_menu(target, Some(&recent.path), mtm))) };

        let text = stack_view(&[&ivars.title_label, &ivars.subtitle_label], mtm);
        text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        text.setAlignment(NSLayoutAttribute::Leading);
        text.setSpacing(1.0);
        text.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.shell.addSubview(&text);

        activate(&[
            ivars.shell.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            ivars.shell.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            ivars.shell.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            ivars.shell.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            ivars.document_icon.leadingAnchor().constraintEqualToAnchor_constant(&ivars.shell.leadingAnchor(), 12.0),
            ivars.document_icon.centerYAnchor().constraintEqualToAnchor(&ivars.shell.centerYAnchor()),
            ivars.document_icon.widthAnchor().constraintEqualToConstant(18.0),
            ivars.document_icon.heightAnchor().constraintEqualToConstant(18.0),
            text.leadingAnchor().constraintEqualToAnchor_constant(&ivars.document_icon.trailingAnchor(), 7.0),
            text.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&trailing_stack.leadingAnchor(), -12.0),
            text.centerYAnchor().constraintEqualToAnchor(&ivars.shell.centerYAnchor()),
            trailing_stack.trailingAnchor().constraintEqualToAnchor_constant(&ivars.shell.trailingAnchor(), -12.0),
            trailing_stack.centerYAnchor().constraintEqualToAnchor(&ivars.shell.centerYAnchor()),
        ]);

        let horizontal = NSLayoutConstraintOrientation::Horizontal;
        let (low, required) = (NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired);
        ivars.title_label.setContentCompressionResistancePriority_forOrientation(low, horizontal);
        ivars.subtitle_label.setContentCompressionResistancePriority_forOrientation(low, horizontal);
        text.setContentCompressionResistancePriority_forOrientation(low, horizontal);
        ivars.detail_label.setContentCompressionResistancePriority_forOrientation(required, horizontal);
        ivars.shortcut_label.setContentHuggingPriority_forOrientation(required, horizontal);
        ivars.shortcut_label.setContentCompressionResistancePriority_forOrientation(required, horizontal);

        // SAFETY: the owner is the view itself.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                RECT_ZERO,
                NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&*this as &AnyObject),
                None,
            )
        };
        this.addTrackingArea(&area);
        this.update_surface(false);
        this
    }

    /// `documentPath`.
    pub(super) fn document_path(&self) -> &str {
        &self.ivars().document_path
    }

    /// `apply(sheet:)`.
    pub(super) fn apply(&self, sheet: Rc<StyleSheet>) {
        *self.ivars().sheet.borrow_mut() = sheet;
        self.update_surface(false);
    }

    /// `window?.firstResponder === self && window?.isKeyWindow == true`.
    fn is_focused(&self) -> bool {
        let Some(window) = self.window() else { return false };
        let is_first_responder = window
            .firstResponder()
            .is_some_and(|responder| std::ptr::eq(Retained::as_ptr(&responder).cast::<AnyObject>(), (self as *const Self).cast()));
        is_first_responder && window.isKeyWindow()
    }

    /// `updateSurface(animated:)`.
    fn update_surface(&self, animated: bool) {
        let sheet = self.ivars().sheet.borrow().clone();
        let contrast = sheet.increase_contrast;
        // Keyboard focus and selection are one thing: the row under the first
        // responder draws an accent ring, so arrow-key navigation has a clear
        // and native-feeling destination.
        let is_focused = self.is_focused();
        let is_hovered = self.ivars().is_hovered.get();
        let is_pressed = self.ivars().is_pressed.get();
        let engaged = is_hovered || is_pressed || is_focused;
        let fill: Retained<NSColor> = if is_pressed {
            sheet.accent.colorWithAlphaComponent(if contrast { 0.28 } else { 0.18 })
        } else if is_hovered {
            sheet.text.colorWithAlphaComponent(if contrast { 0.12 } else { 0.07 })
        } else if is_focused {
            // Focus is a selection state, not a warning. Keep the row quiet
            // and let the blue keycap carry the keyboard affordance.
            sheet.text.colorWithAlphaComponent(if contrast { 0.12 } else { 0.075 })
        } else {
            NSColor::clearColor()
        };
        let this = self.retain();
        let reduce_motion = sheet.reduce_motion;
        let apply = move || {
            let ivars = this.ivars();
            if let Some(layer) = ivars.shell.layer() {
                layer.setBackgroundColor(Some(&cg(&fill)));
                layer.setBorderWidth(if is_focused { 1.5 } else { 0.0 });
                layer.setBorderColor(Some(&cg(&sheet.accent)));
            }
            ivars.document_icon.setContentTintColor(Some(if engaged { &sheet.text } else { &sheet.text_secondary }));
            ivars.title_label.setTextColor(Some(&sheet.text));
            ivars.subtitle_label.setTextColor(Some(if engaged { &sheet.text_secondary } else { &sheet.text_faint }));
            ivars.detail_label.setTextColor(Some(if engaged { &sheet.text_secondary } else { &sheet.text_faint }));
            let shortcut_is_active = engaged;
            let shortcut_color = if shortcut_is_active { &sheet.accent } else { &sheet.text_secondary };
            ivars
                .shortcut_label
                .setAttributedStringValue(&KeycapFormatter::format(&format!("⌘{}", ivars.ordinal), shortcut_color));
            if let Some(layer) = ivars.shortcut_label.layer() {
                if shortcut_is_active {
                    layer.setBackgroundColor(Some(&cg(&sheet.accent.colorWithAlphaComponent(if contrast { 0.24 } else { 0.15 }))));
                    layer.setBorderWidth(1.0);
                    layer.setBorderColor(Some(&cg(&sheet.accent.colorWithAlphaComponent(if contrast { 0.72 } else { 0.52 }))));
                } else {
                    layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.08 } else { 0.045 }))));
                    layer.setBorderWidth(1.0);
                    layer.setBorderColor(Some(&cg(&sheet.rule.colorWithAlphaComponent(if contrast { 0.50 } else { 0.30 }))));
                }
            }
            if let Some(layer) = ivars.shell.layer() {
                let is_pressed = ivars.is_pressed.get();
                let is_hovered = ivars.is_hovered.get();
                layer.setAffineTransform(if is_pressed {
                    scale(0.992, 0.992)
                } else if is_hovered {
                    scale(1.004, 1.004)
                } else {
                    IDENTITY
                });
            }
        };
        if animated && !reduce_motion {
            motion::run(false, motion::QUICK, Curve::EaseOut, move |_| apply(), None);
        } else {
            apply();
        }
    }
}
