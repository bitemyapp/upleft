//! Port of `Panels/FrontMatterEditorView.swift`: the small, safe front
//! matter editor.  It edits only flat scalar fields.  Nested YAML, comments,
//! anchors, and block scalars stay in Source mode.
//!
//! Objective-C class names: `FrontMatterEditorView`, and Swift's private
//! `FrontMatterFieldHost`, `FrontMatterFieldRow` and
//! `FrontMatterFieldRow.FrontMatterDirtyDot` (`FrontMatterDirtyDot`).
//!
//! `InspectorHostView` reaches this panel by selector: `focusField`
//! (Swift's `focusField()`, the default `named: nil`) and `preferredWidth`.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSButton, NSColor, NSControlTextEditingDelegate, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSLineBreakMode, NSPopUpButton, NSResponder, NSScrollView,
    NSStackView, NSStackViewDistribution, NSTextField, NSTextFieldDelegate, NSTextView, NSUserInterfaceLayoutOrientation,
    NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSComparisonResult, NSNotification, NSPoint, NSRect, NSSize, NSString};
use upleft_core::editing::front_matter_editing::{
    FrontMatterEditOperation, FrontMatterEditing, FrontMatterSourceFallback, FrontMatterValue, swift_double_parses,
};
use upleft_core::model::{CalloutKind, FrontMatter, FrontMatterField, ParsedDocument};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::appkit_support::{RectExt, activate, label, ns_string, rect, role, set_label, set_role};
use super::panel_chrome::{
    ButtonAction, PanelBackdrop, PanelButton, PanelEmptyStateView, PanelFont, PanelList, PanelMetrics, PanelSurface,
    install_backdrop, panel_title,
};
use crate::support::commands::Command;

/// `FrontMatterEditorDelegate`.
pub trait FrontMatterEditorDelegate {
    fn front_matter_editor_did_request(&self, editor: &FrontMatterEditorView, operation: FrontMatterEditOperation);
    fn front_matter_editor_wants_source_mode(&self, editor: &FrontMatterEditorView);
}

pub struct FrontMatterEditorViewIvars {
    delegate: RefCell<Option<Weak<dyn FrontMatterEditorDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    document: RefCell<Arc<ParsedDocument>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    detail_label: Retained<NSTextField>,
    status_label: Retained<NSTextField>,
    field_stack: Retained<NSStackView>,
    /// Flipped so the field list starts at the top of the scroller rather
    /// than its bottom, which is what an unflipped document view would do.
    field_host: Retained<FrontMatterFieldHost>,
    /// `private lazy var fieldScroll`.
    field_scroll: OnceCell<Retained<NSScrollView>>,
    empty_state: Retained<PanelEmptyStateView>,
    add_key_field: Retained<NSTextField>,
    add_value_field: Retained<NSTextField>,
    add_type_popup: Retained<NSPopUpButton>,
    add_button: Retained<NSButton>,
    source_button: Retained<NSButton>,
    add_action: RefCell<Option<Retained<ButtonAction>>>,
    source_action: RefCell<Option<Retained<ButtonAction>>>,
    rows: RefCell<Vec<Retained<FrontMatterFieldRow>>>,
    /// Notices are not errors.  A fallback ("Nested YAML needs Source
    /// Focus") is a warning about what this editor will not touch; a
    /// rejected value is a danger.  They must not share the accent with a
    /// selected task (§11.3).
    status_is_warning: Cell<bool>,
}

define_class!(
    /// `FrontMatterEditorView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FrontMatterEditorView"]
    #[ivars = FrontMatterEditorViewIvars]
    pub struct FrontMatterEditorView;

    unsafe impl NSObjectProtocol for FrontMatterEditorView {}

    impl FrontMatterEditorView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::DETAIL_WIDTH
        }

        /// `focusField()` (Swift's default, `named: nil`), for
        /// `InspectorHostView`.
        #[unsafe(method(focusField))]
        fn __focus_field(&self) {
            self.focus_field(None);
        }
    }
);

impl PanelSurface for FrontMatterEditorView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl FrontMatterEditorView {
    /// `FrontMatterEditorView()`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<FrontMatterEditorView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<FrontMatterEditorView> {
        // Stored-property initial values, in declaration order.
        let title_label = label(&panel_title(Command::FrontMatterEditor), mtm);
        let detail_label = label("", mtm);
        let status_label = label("", mtm);
        let field_stack = NSStackView::new(mtm);
        let field_host = FrontMatterFieldHost::new(mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let add_key_field = NSTextField::new(mtm);
        let add_value_field = NSTextField::new(mtm);
        let add_type_popup = NSPopUpButton::new(mtm);
        // The init body.
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let add_button = PanelButton::text("Add field", &ButtonAction::noop(mtm), false, mtm);
        let source_button = PanelButton::text("Open Source Focus", &ButtonAction::noop(mtm), false, mtm);
        let this = Self::alloc(mtm).set_ivars(FrontMatterEditorViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            document: RefCell::new(ParsedDocument::empty()),
            backdrop,
            title_label,
            detail_label,
            status_label,
            field_stack,
            field_host,
            field_scroll: OnceCell::new(),
            empty_state,
            add_key_field,
            add_value_field,
            add_type_popup,
            add_button,
            source_button,
            add_action: RefCell::new(None),
            source_action: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
            status_is_warning: Cell::new(true),
        });
        let this: Retained<FrontMatterEditorView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        install_backdrop(&this, &this.ivars().backdrop);

        this.build_header();
        this.build_fields();
        this.build_add_form();
        this.apply_style();
        this.reload();

        set_role(&*this, role::group());
        set_label(&*this, "Front matter editor");
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn FrontMatterEditorDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn FrontMatterEditorDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn document(&self) -> Arc<ParsedDocument> {
        self.ivars().document.borrow().clone()
    }

    pub fn set_document(&self, document: Arc<ParsedDocument>) {
        *self.ivars().document.borrow_mut() = document;
        self.reload();
    }

    pub fn rendered_field_count(&self) -> isize {
        self.ivars().rows.borrow().len() as isize
    }

    pub fn shows_source_mode_prompt(&self) -> bool {
        !self.ivars().source_button.isHidden()
    }

    pub fn field_scroll_origin_y_for_testing(&self) -> CGFloat {
        self.field_scroll().contentView().bounds().origin.y
    }

    pub fn focused_field_key_for_testing(&self) -> Option<String> {
        let window = self.window()?;
        let first_responder = window.firstResponder()?;
        let responder_ptr = Retained::as_ptr(&first_responder).cast::<AnyObject>();
        let text_view_delegate: Option<*const AnyObject> =
            super::appkit_support::downcast::<NSTextView>(&first_responder).and_then(|text_view| {
                text_view.delegate().map(|delegate| Retained::as_ptr(&delegate).cast::<AnyObject>())
            });
        self.ivars()
            .rows
            .borrow()
            .iter()
            .find(|row| {
                let editor = Retained::as_ptr(&row.ivars().value_field).cast::<AnyObject>();
                std::ptr::eq(responder_ptr, editor) || text_view_delegate.is_some_and(|delegate| std::ptr::eq(delegate, editor))
            })
            .map(|row| row.field_key())
    }

    pub fn set_field_scroll_origin_y_for_testing(&self, y: CGFloat) {
        let field_scroll = self.field_scroll();
        let content_view = field_scroll.contentView();
        content_view.scrollToPoint(NSPoint::new(0.0, y));
        field_scroll.reflectScrolledClipView(&content_view);
    }

    fn field_scroll(&self) -> Retained<NSScrollView> {
        self.ivars()
            .field_scroll
            .get_or_init(|| PanelList::make_scroll_view(&self.ivars().field_host, self.mtm()))
            .clone()
    }

    fn build_header(&self) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::title()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);

        let detail_label = &ivars.detail_label;
        detail_label.setFont(Some(&PanelFont::secondary()));
        detail_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(detail_label);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            title_label
                .topAnchor()
                .constraintEqualToAnchor_constant(&self.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            detail_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            detail_label.trailingAnchor().constraintEqualToAnchor(&title_label.trailingAnchor()),
            detail_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 2.0),
        ]);
    }

    /// The field list scrolls; the add form and the Source Focus button are
    /// a pinned footer.  Pinning the stack itself let a ten-field block push
    /// both of them off the bottom of the panel with no way to reach either.
    fn build_fields(&self) {
        let ivars = self.ivars();
        let field_stack = &ivars.field_stack;
        field_stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        field_stack.setAlignment(NSLayoutAttribute::Leading);
        field_stack.setSpacing(7.0);
        field_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        // The document view is constrained, not autoresized: its height
        // comes from the field list so the scroller knows when there is more.
        let field_host = &ivars.field_host;
        field_host.setTranslatesAutoresizingMaskIntoConstraints(false);
        field_host.addSubview(field_stack);

        let field_scroll = self.field_scroll();
        self.addSubview(&field_scroll);
        ivars.empty_state.install(self, &field_scroll, 1.0);

        let status_label = &ivars.status_label;
        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setMaximumNumberOfLines(3);
        status_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(status_label);

        activate(&[
            field_stack.leadingAnchor().constraintEqualToAnchor(&field_host.leadingAnchor()),
            field_stack.trailingAnchor().constraintEqualToAnchor(&field_host.trailingAnchor()),
            field_stack.topAnchor().constraintEqualToAnchor(&field_host.topAnchor()),
            field_stack.bottomAnchor().constraintEqualToAnchor(&field_host.bottomAnchor()),
            field_host.widthAnchor().constraintEqualToAnchor(&field_scroll.contentView().widthAnchor()),
            field_scroll.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            field_scroll.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            field_scroll.topAnchor().constraintEqualToAnchor_constant(&ivars.detail_label.bottomAnchor(), 12.0),
            // NSScrollView has no useful vertical intrinsic size. Without an
            // explicit viewport it collapsed to zero: AX exposed every field,
            // while the panel showed only the pinned Add Field footer.
            field_scroll.heightAnchor().constraintEqualToConstant(176.0),
            status_label.leadingAnchor().constraintEqualToAnchor(&field_scroll.leadingAnchor()),
            status_label.trailingAnchor().constraintEqualToAnchor(&field_scroll.trailingAnchor()),
            status_label.topAnchor().constraintEqualToAnchor_constant(&field_scroll.bottomAnchor(), 10.0),
        ]);
    }

    fn build_add_form(&self) {
        let ivars = self.ivars();
        let mtm = self.mtm();
        let add_key_field = &ivars.add_key_field;
        add_key_field.setPlaceholderString(Some(&ns_string("Field name")));
        add_key_field.setFont(Some(&PanelFont::row()));
        set_label(&**add_key_field, "New field name");
        add_key_field.setTranslatesAutoresizingMaskIntoConstraints(false);

        let add_value_field = &ivars.add_value_field;
        add_value_field.setPlaceholderString(Some(&ns_string("Value")));
        add_value_field.setFont(Some(&PanelFont::row()));
        set_label(&**add_value_field, "New field value");
        add_value_field.setTranslatesAutoresizingMaskIntoConstraints(false);

        let add_type_popup = &ivars.add_type_popup;
        add_type_popup.addItemsWithTitles(&kind_titles());
        add_type_popup.selectItemAtIndex(0);
        add_type_popup.setFont(Some(&PanelFont::secondary()));
        set_label(&**add_type_popup, "New field type");
        add_type_popup.setTranslatesAutoresizingMaskIntoConstraints(false);

        let weak: ObjcWeak<FrontMatterEditorView> = ObjcWeak::from(self);
        let action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.add_field();
                }
            },
            mtm,
        );
        *ivars.add_action.borrow_mut() = Some(action.clone());
        let add_button = &ivars.add_button;
        unsafe {
            add_button.setTarget(Some(&action));
            add_button.setAction(Some(ButtonAction::selector()));
        }
        set_label(&**add_button, "Add front matter field");

        let weak: ObjcWeak<FrontMatterEditorView> = ObjcWeak::from(self);
        let source = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.front_matter_editor_wants_source_mode(&this);
                }
            },
            mtm,
        );
        *ivars.source_action.borrow_mut() = Some(source.clone());
        let source_button = &ivars.source_button;
        unsafe {
            source_button.setTarget(Some(&source));
            source_button.setAction(Some(ButtonAction::selector()));
        }
        set_label(&**source_button, "Open Source Focus to edit front matter");

        // Two short lines rather than one long one: a single row of name,
        // value, type, and button needs about 313pt and the inspector can be
        // narrower than that, so the row used to run off the panel.
        let name_row = stack_with(&[as_view(add_key_field), as_view(add_type_popup)], mtm);
        let value_row = stack_with(&[as_view(add_value_field), as_view(add_button)], mtm);
        for row in [&name_row, &value_row] {
            row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
            row.setAlignment(NSLayoutAttribute::CenterY);
            row.setSpacing(5.0);
            row.setDistribution(NSStackViewDistribution::Fill);
        }
        for filler in [add_key_field, add_value_field] {
            filler.setContentHuggingPriority_forOrientation(
                NSLayoutPriorityDefaultLow,
                NSLayoutConstraintOrientation::Horizontal,
            );
            filler.setContentCompressionResistancePriority_forOrientation(
                NSLayoutPriorityDefaultLow,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }
        for fixed in [as_view(add_type_popup), as_view(add_button)] {
            fixed.setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
            fixed.setContentCompressionResistancePriority_forOrientation(
                NSLayoutPriorityRequired,
                NSLayoutConstraintOrientation::Horizontal,
            );
        }

        let form = stack_with(&[Retained::into_super(name_row.clone()), Retained::into_super(value_row.clone())], mtm);
        form.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        form.setAlignment(NSLayoutAttribute::Leading);
        form.setDistribution(NSStackViewDistribution::Fill);
        form.setSpacing(5.0);
        form.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&form);

        source_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(source_button);

        let field_scroll = self.field_scroll();
        activate(&[
            form.leadingAnchor().constraintEqualToAnchor(&field_scroll.leadingAnchor()),
            form.trailingAnchor().constraintEqualToAnchor(&field_scroll.trailingAnchor()),
            form.topAnchor().constraintEqualToAnchor_constant(&ivars.status_label.bottomAnchor(), 10.0),
            name_row.widthAnchor().constraintEqualToAnchor(&form.widthAnchor()),
            value_row.widthAnchor().constraintEqualToAnchor(&form.widthAnchor()),
            add_key_field.widthAnchor().constraintGreaterThanOrEqualToConstant(72.0),
            add_value_field.widthAnchor().constraintGreaterThanOrEqualToConstant(72.0),
            add_type_popup.widthAnchor().constraintEqualToConstant(78.0),
            source_button.leadingAnchor().constraintEqualToAnchor(&form.leadingAnchor()),
            source_button.topAnchor().constraintEqualToAnchor_constant(&form.bottomAnchor(), 8.0),
            source_button.bottomAnchor().constraintEqualToAnchor_constant(&self.bottomAnchor(), -10.0),
        ]);
    }

    pub fn reload(&self) {
        let ivars = self.ivars();
        let old_rows: Vec<Retained<FrontMatterFieldRow>> = ivars.rows.borrow().clone();
        for row in &old_rows {
            ivars.field_stack.removeArrangedSubview(row);
            row.removeFromSuperview();
        }
        ivars.rows.borrow_mut().clear();

        let document = self.document();
        let Some(front) = document.front_matter.as_ref() else {
            ivars.detail_label.setStringValue(&ns_string("No front matter block"));
            ivars.status_label.setStringValue(&ns_string(""));
            ivars.source_button.setHidden(false);
            ivars.add_button.setEnabled(false);
            self.show_empty_state(
                "text.alignleft",
                "No front matter",
                "Add a `---` YAML block at the top of\nthe document in Source Focus.",
            );
            return;
        };

        ivars.detail_label.setStringValue(&ns_string(if front.fields.is_empty() {
            "Empty YAML block"
        } else {
            "Flat fields · changes keep source formatting"
        }));
        let fallback = self.editability_fallback(front);
        let can_edit = fallback.is_none();
        ivars.status_is_warning.set(true);
        ivars.status_label.setStringValue(&ns_string(fallback.map_or("", message)));
        ivars.source_button.setHidden(can_edit);
        ivars.add_button.setEnabled(can_edit);

        for field in &front.fields {
            let row = FrontMatterFieldRow::new(field.clone(), self.style_sheet(), self.mtm());
            let weak_self: ObjcWeak<FrontMatterEditorView> = ObjcWeak::from(self);
            let weak_row: ObjcWeak<FrontMatterFieldRow> = ObjcWeak::from(&*row);
            *row.ivars().on_commit.borrow_mut() = Some(Rc::new(move |key: &str, value: FrontMatterValue| {
                let (Some(this), Some(row)) = (weak_self.load(), weak_row.load()) else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.front_matter_editor_did_request(&this, FrontMatterEditOperation::Set { key: key.to_owned(), value });
                }
                row.clear_error();
            }));
            let weak_self: ObjcWeak<FrontMatterEditorView> = ObjcWeak::from(self);
            *row.ivars().on_remove.borrow_mut() = Some(Rc::new(move |key: &str| {
                let Some(this) = weak_self.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.front_matter_editor_did_request(&this, FrontMatterEditOperation::Remove { key: key.to_owned() });
                }
            }));
            row.set_enabled(can_edit);
            ivars.rows.borrow_mut().push(row.clone());
            ivars.field_stack.addArrangedSubview(&row);
            row.widthAnchor().constraintEqualToAnchor(&ivars.field_stack.widthAnchor()).setActive(true);
        }

        if front.fields.is_empty() {
            self.show_empty_state("text.alignleft", "Empty front matter", "The YAML block has no fields yet.\nAdd one below.");
        } else {
            ivars.empty_state.setHidden(true);
            self.field_scroll().setHidden(false);
        }
        self.apply_style();
    }

    /// Focus the requested rendered field after the trailing panel has been
    /// installed.  Card activation can name a semantic key (title/author),
    /// while an unqualified activation simply lands on the first value.
    ///
    /// Swift's `key.map { … } ?? rows.first`: a key that names no row
    /// yields `nil` (the add form), not the first row.
    pub fn focus_field(&self, key: Option<&str>) {
        let target = {
            let rows = self.ivars().rows.borrow();
            match key {
                Some(wanted) => rows
                    .iter()
                    .find(|row| {
                        ns_string(&row.field_key()).caseInsensitiveCompare(&ns_string(wanted)) == NSComparisonResult::Same
                    })
                    .cloned(),
                None => rows.first().cloned(),
            }
        };
        let Some(target) = target else {
            if let Some(window) = self.window() {
                let responder: &NSResponder = &self.ivars().add_key_field;
                window.makeFirstResponder(Some(responder));
            }
            return;
        };
        self.layoutSubtreeIfNeeded();
        self.ivars().field_host.layoutSubtreeIfNeeded();
        let field_scroll = self.field_scroll();
        field_scroll.contentView().scrollToPoint(NSPoint::new(0.0, 0.0));
        field_scroll.reflectScrolledClipView(&field_scroll.contentView());
        target.scrollRectToVisible(target.bounds());
        if let Some(window) = self.window() {
            let responder: &NSResponder = &target.ivars().value_field;
            window.makeFirstResponder(Some(responder));
        }
        unsafe { target.ivars().value_field.selectText(None) };
    }

    /// A reused inspector remembers its previous scroll position.  Opening
    /// the metadata card must always reveal the existing fields, then place
    /// the keyboard in the field the reader activated.
    pub fn prepare_for_presentation(&self, focus: Option<&str>) {
        self.layoutSubtreeIfNeeded();
        self.ivars().field_host.layoutSubtreeIfNeeded();
        let field_scroll = self.field_scroll();
        field_scroll.contentView().scrollToPoint(NSPoint::new(0.0, 0.0));
        field_scroll.reflectScrolledClipView(&field_scroll.contentView());
        self.focus_field(focus);
    }

    fn show_empty_state(&self, symbol: &str, title: &str, subtitle: &str) {
        let style_sheet = self.style_sheet();
        self.ivars().empty_state.configure(symbol, title, subtitle, &style_sheet);
        self.ivars().empty_state.setHidden(false);
        self.field_scroll().setHidden(true);
    }

    /// The editor asks the core writer to validate the full block.  This
    /// keeps the UI conservative: a complex block always gets a Source mode
    /// path.
    fn editability_fallback(&self, front: &FrontMatter) -> Option<FrontMatterSourceFallback> {
        let field = front.fields.first()?;
        FrontMatterEditing::set(&self.document(), field.key.clone(), FrontMatterValue::Text(field.value.clone())).fallback
    }

    fn add_field(&self) {
        let ivars = self.ivars();
        let key = swift_text::trim_whitespaces_and_newlines(&ivars.add_key_field.stringValue().to_string()).to_owned();
        if key.is_empty() {
            ivars.add_key_field.becomeFirstResponder();
            return;
        }
        let value = FrontMatterFieldRow::value(
            &ivars.add_value_field.stringValue().to_string(),
            ivars.add_type_popup.indexOfSelectedItem(),
        );
        if let Some(delegate) = self.delegate() {
            delegate.front_matter_editor_did_request(self, FrontMatterEditOperation::Add { key, value });
        }
        ivars.add_key_field.setStringValue(&ns_string(""));
        ivars.add_value_field.setStringValue(&ns_string(""));
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        ivars.detail_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.status_label.setTextColor(Some(&style_sheet.callout_color(if ivars.status_is_warning.get() {
            CalloutKind::Warning
        } else {
            CalloutKind::Danger
        })));
        ivars.add_key_field.setTextColor(Some(&style_sheet.text));
        ivars.add_value_field.setTextColor(Some(&style_sheet.text));
        let rows = ivars.rows.borrow().clone();
        for row in rows {
            row.set_style_sheet(style_sheet.clone());
        }
    }

    // MARK: - Test hooks (conformance scenes and tests)

    /// The rendered rows' keys, in order.
    pub fn field_keys_for_testing(&self) -> Vec<String> {
        self.ivars().rows.borrow().iter().map(|row| row.field_key()).collect()
    }
}

fn message(fallback: FrontMatterSourceFallback) -> &'static str {
    match fallback {
        FrontMatterSourceFallback::NestedYAML => "Nested YAML needs Source Focus.",
        FrontMatterSourceFallback::CommentsNotSupported => "Comments need Source Focus.",
        FrontMatterSourceFallback::AnchorsOrAliasesNotSupported => "YAML anchors need Source Focus.",
        FrontMatterSourceFallback::BlockScalarNotSupported => "Block text needs Source Focus.",
        FrontMatterSourceFallback::AmbiguousField => "Duplicate fields need Source Focus.",
        _ => "This block needs Source Focus.",
    }
}

fn as_view(view: &NSView) -> Retained<NSView> {
    view.retain()
}

/// `NSStackView(views: [...])`.
fn stack_with(views: &[Retained<NSView>], mtm: MainThreadMarker) -> Retained<NSStackView> {
    let array: Retained<NSArray<NSView>> = NSArray::from_retained_slice(views);
    NSStackView::stackViewWithViews(&array, mtm)
}

/// `FrontMatterFieldRow.kindTitles`.
const KIND_TITLES: [&str; 4] = ["Text", "Boolean", "Number", "List"];

fn kind_titles() -> Retained<NSArray<NSString>> {
    NSArray::from_retained_slice(&KIND_TITLES.map(ns_string))
}

// MARK: - FrontMatterFieldHost

define_class!(
    /// Flipped host for the scrolling field list.
    // SAFETY: no ivars; AppKit's own initialisers are safe to inherit.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FrontMatterFieldHost"]
    pub struct FrontMatterFieldHost;

    unsafe impl NSObjectProtocol for FrontMatterFieldHost {}

    impl FrontMatterFieldHost {
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }
    }
);

impl FrontMatterFieldHost {
    /// `FrontMatterFieldHost()`.
    fn new(mtm: MainThreadMarker) -> Retained<FrontMatterFieldHost> {
        unsafe { msg_send![FrontMatterFieldHost::alloc(mtm), init] }
    }
}

// MARK: - FrontMatterFieldRow

type CommitHandler = Rc<dyn Fn(&str, FrontMatterValue)>;
type RemoveHandler = Rc<dyn Fn(&str)>;

pub struct FrontMatterFieldRowIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    is_enabled: Cell<bool>,
    on_commit: RefCell<Option<CommitHandler>>,
    on_remove: RefCell<Option<RemoveHandler>>,
    field: FrontMatterField,
    key_field: Retained<NSTextField>,
    value_field: Retained<NSTextField>,
    kind_popup: Retained<NSPopUpButton>,
    dirty_dot: Retained<FrontMatterDirtyDot>,
    remove_button: Retained<NSButton>,
    error_label: Retained<NSTextField>,
    kind_action: RefCell<Option<Retained<ButtonAction>>>,
    remove_action: RefCell<Option<Retained<ButtonAction>>>,
    committed_value: RefCell<String>,
}

define_class!(
    /// Swift's private `FrontMatterFieldRow`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FrontMatterFieldRow"]
    #[ivars = FrontMatterFieldRowIvars]
    pub struct FrontMatterFieldRow;

    unsafe impl NSObjectProtocol for FrontMatterFieldRow {}

    unsafe impl NSControlTextEditingDelegate for FrontMatterFieldRow {
        /// A typed value is visibly unsaved…
        #[unsafe(method(controlTextDidChange:))]
        fn __control_text_did_change(&self, _notification: &NSNotification) {
            let ivars = self.ivars();
            let dirty =
                !swift_text::str_eq(&ivars.value_field.stringValue().to_string(), &ivars.committed_value.borrow());
            ivars.dirty_dot.set_dirty(dirty);
        }

        /// …and clicking away or tabbing out saves it instead of dropping it.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn __control_text_did_end_editing(&self, _notification: &NSNotification) {
            let ivars = self.ivars();
            if swift_text::str_eq(&ivars.value_field.stringValue().to_string(), &ivars.committed_value.borrow()) {
                return;
            }
            self.commit();
        }
    }

    unsafe impl NSTextFieldDelegate for FrontMatterFieldRow {}
);

impl FrontMatterFieldRow {
    /// `init(field:styleSheet:)`.
    fn new(field: FrontMatterField, style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<FrontMatterFieldRow> {
        // Stored-property initial values, in declaration order.
        let kind_popup = NSPopUpButton::new(mtm);
        let dirty_dot = FrontMatterDirtyDot::new(mtm);
        let error_label = label("", mtm);
        // The init body.
        let key_field = NSTextField::textFieldWithString(&ns_string(&field.key), mtm);
        let value_field = NSTextField::textFieldWithString(&ns_string(&field.value), mtm);
        let remove_button = PanelButton::symbol_default("trash", "Remove field", &ButtonAction::noop(mtm), mtm);
        let committed_value = field.value.clone();
        let this = Self::alloc(mtm).set_ivars(FrontMatterFieldRowIvars {
            style_sheet: RefCell::new(style_sheet),
            is_enabled: Cell::new(true),
            on_commit: RefCell::new(None),
            on_remove: RefCell::new(None),
            field,
            key_field,
            value_field,
            kind_popup,
            dirty_dot,
            remove_button,
            error_label,
            kind_action: RefCell::new(None),
            remove_action: RefCell::new(None),
            committed_value: RefCell::new(committed_value),
        });
        let this: Retained<FrontMatterFieldRow> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.finish_init(mtm);
        this
    }

    fn finish_init(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let key = ivars.field.key.clone();
        let key_field = &ivars.key_field;
        key_field.setEditable(false);
        key_field.setBordered(false);
        key_field.setDrawsBackground(false);
        key_field.setFont(Some(&PanelFont::row_emphasised()));
        key_field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        set_label(&**key_field, &format!("Field name: {key}"));
        let value_field = &ivars.value_field;
        value_field.setFont(Some(&PanelFont::row()));
        unsafe { value_field.setDelegate(Some(ProtocolObject::from_ref(self))) };
        set_label(&**value_field, &format!("Value for {key}"));
        let kind_popup = &ivars.kind_popup;
        kind_popup.addItemsWithTitles(&kind_titles());
        kind_popup.selectItemAtIndex(Self::kind_index(&ivars.field.value));
        kind_popup.setFont(Some(&PanelFont::secondary()));
        set_label(&**kind_popup, &format!("Type for {key}"));

        let weak: ObjcWeak<FrontMatterFieldRow> = ObjcWeak::from(self);
        let kind = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.commit();
                }
            },
            mtm,
        );
        *ivars.kind_action.borrow_mut() = Some(kind.clone());
        unsafe {
            kind_popup.setTarget(Some(&kind));
            kind_popup.setAction(Some(ButtonAction::selector()));
        }
        let weak: ObjcWeak<FrontMatterFieldRow> = ObjcWeak::from(self);
        let remove = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                let handler = this.ivars().on_remove.borrow().clone();
                if let Some(handler) = handler {
                    handler(&this.ivars().field.key);
                }
            },
            mtm,
        );
        *ivars.remove_action.borrow_mut() = Some(remove.clone());
        let remove_button = &ivars.remove_button;
        unsafe {
            remove_button.setTarget(Some(&remove));
            remove_button.setAction(Some(ButtonAction::selector()));
        }

        unsafe {
            value_field.setTarget(Some(&kind));
            value_field.setAction(Some(ButtonAction::selector()));
        }

        let dirty_dot = &ivars.dirty_dot;
        let controls = stack_with(
            &[as_view(key_field), as_view(value_field), as_view(kind_popup), as_view(dirty_dot), as_view(remove_button)],
            mtm,
        );
        controls.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        controls.setAlignment(NSLayoutAttribute::CenterY);
        controls.setSpacing(5.0);
        controls.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&controls);

        // The key can give up width before the value does; neither may push
        // the row past the panel.
        key_field.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow - 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        value_field.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        for fixed in [as_view(kind_popup), as_view(dirty_dot), as_view(remove_button)] {
            fixed.setContentCompressionResistancePriority_forOrientation(
                NSLayoutPriorityRequired,
                NSLayoutConstraintOrientation::Horizontal,
            );
            fixed.setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
        }

        let error_label = &ivars.error_label;
        error_label.setFont(Some(&PanelFont::secondary()));
        error_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        error_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(error_label);

        activate(&[
            controls.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            controls.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            controls.topAnchor().constraintEqualToAnchor(&self.topAnchor()),
            key_field.widthAnchor().constraintGreaterThanOrEqualToConstant(54.0),
            value_field.widthAnchor().constraintGreaterThanOrEqualToConstant(60.0),
            kind_popup.widthAnchor().constraintEqualToConstant(74.0),
            dirty_dot.widthAnchor().constraintEqualToConstant(8.0),
            remove_button.widthAnchor().constraintEqualToConstant(28.0),
            error_label.leadingAnchor().constraintEqualToAnchor(&controls.leadingAnchor()),
            error_label.trailingAnchor().constraintEqualToAnchor(&controls.trailingAnchor()),
            error_label.topAnchor().constraintEqualToAnchor_constant(&controls.bottomAnchor(), 2.0),
            error_label.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);
        set_role(self, role::group());
        set_label(self, &format!("Front matter field {key}"));
        self.apply_style();
    }

    fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    fn set_enabled(&self, value: bool) {
        self.ivars().is_enabled.set(value);
        self.set_controls_enabled();
    }

    fn field_key(&self) -> String {
        self.ivars().field.key.clone()
    }

    // MARK: - Editing

    fn commit(&self) {
        let ivars = self.ivars();
        let raw = ivars.value_field.stringValue().to_string();
        let value = Self::value(&raw, ivars.kind_popup.indexOfSelectedItem());
        if matches!(value, FrontMatterValue::Number(_)) && swift_double(&raw).is_none() {
            ivars.error_label.setStringValue(&ns_string("Enter a number."));
            ivars.dirty_dot.set_dirty(true);
            return;
        }
        if matches!(value, FrontMatterValue::Boolean(_)) {
            let lower = swift_text::lowercased(&raw);
            if !["true", "false"].iter().any(|candidate| swift_text::str_eq(candidate, &lower)) {
                ivars.error_label.setStringValue(&ns_string("Enter true or false."));
                ivars.dirty_dot.set_dirty(true);
                return;
            }
        }
        *ivars.committed_value.borrow_mut() = raw;
        ivars.dirty_dot.set_dirty(false);
        let handler = ivars.on_commit.borrow().clone();
        if let Some(handler) = handler {
            handler(&ivars.field.key, value);
        }
    }

    fn clear_error(&self) {
        self.ivars().error_label.setStringValue(&ns_string(""));
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        ivars.key_field.setTextColor(Some(&style_sheet.text_secondary));
        ivars.value_field.setTextColor(Some(&style_sheet.text));
        ivars.error_label.setTextColor(Some(&style_sheet.callout_color(CalloutKind::Danger)));
        ivars.dirty_dot.set_color(style_sheet.callout_color(CalloutKind::Warning));
    }

    fn set_controls_enabled(&self) {
        let ivars = self.ivars();
        let enabled = ivars.is_enabled.get();
        ivars.value_field.setEnabled(enabled);
        ivars.kind_popup.setEnabled(enabled);
        ivars.remove_button.setEnabled(enabled);
    }

    /// `kindIndex(for:)`.
    fn kind_index(value: &str) -> isize {
        let lower = swift_text::lowercased(swift_text::trim_whitespaces_and_newlines(value));
        if swift_text::str_eq(&lower, "true") || swift_text::str_eq(&lower, "false") {
            return 1;
        }
        if swift_double(&lower).is_some() {
            return 2;
        }
        if swift_text::has_prefix(&lower, "[") && swift_text::has_suffix(&lower, "]") {
            return 3;
        }
        0
    }

    /// `value(from:kindIndex:)`.
    fn value(raw: &str, kind_index: isize) -> FrontMatterValue {
        match kind_index {
            1 => FrontMatterValue::Boolean(swift_text::str_eq(
                &swift_text::lowercased(swift_text::trim_whitespaces_and_newlines(raw)),
                "true",
            )),
            2 => FrontMatterValue::Number(swift_double(swift_text::trim_whitespaces_and_newlines(raw)).unwrap_or(0.0)),
            3 => FrontMatterValue::List(Self::parse_list(raw)),
            _ => FrontMatterValue::Text(raw.to_owned()),
        }
    }

    fn parse_list(raw: &str) -> Vec<String> {
        let mut text = swift_text::trim_whitespaces_and_newlines(raw).to_owned();
        if swift_text::has_prefix(&text, "[") && swift_text::has_suffix(&text, "]") {
            text = swift_text::drop_last(swift_text::drop_first(&text, 1), 1).to_owned();
        }
        swift_text::split(&text, ',', usize::MAX, false)
            .into_iter()
            .map(|part| {
                let item = swift_text::trim_whitespaces_and_newlines(part);
                if swift_text::count(item) >= 2
                    && swift_text::first(item).is_some_and(|first| swift_text::char_is(first, '"'))
                    && swift_text::last(item).is_some_and(|last| swift_text::char_is(last, '"'))
                {
                    return swift_text::drop_last(swift_text::drop_first(item, 1), 1).to_owned();
                }
                item.to_owned()
            })
            .collect()
    }
}

/// Swift's `Double(text)`: `nil` unless the whole string (up to a NUL)
/// parses (`swift_double_parses`); the value is the C-locale `strtod`, as
/// the standard library computes it.
fn swift_double(text: &str) -> Option<f64> {
    if !swift_double_parses(text) {
        return None;
    }
    let text = text.split('\0').next().unwrap_or("");
    let unsigned = text.trim_start_matches(['+', '-']);
    if unsigned.eq_ignore_ascii_case("snan") {
        return Some(if text.starts_with('-') { -f64::NAN } else { f64::NAN });
    }
    unsafe extern "C" {
        fn strtod_l(nptr: *const libc::c_char, endptr: *mut *mut libc::c_char, locale: *mut libc::c_void) -> f64;
    }
    let c_text = std::ffi::CString::new(text).ok()?;
    // SAFETY: a NUL-terminated string; a null locale is the C locale.
    Some(unsafe { strtod_l(c_text.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) })
}

// MARK: - FrontMatterDirtyDot

pub struct FrontMatterDirtyDotIvars {
    is_dirty: Cell<bool>,
    color: RefCell<Retained<NSColor>>,
}

define_class!(
    /// A field whose value has been typed but not yet written to source
    /// shows a dot, not a button: the row saves itself when you leave it,
    /// and the dot is there so "unsaved" is visible while you are still in
    /// it (§11.3).
    // SAFETY: `initWithFrame:` sets the ivars.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FrontMatterDirtyDot"]
    #[ivars = FrontMatterDirtyDotIvars]
    pub struct FrontMatterDirtyDot;

    unsafe impl NSObjectProtocol for FrontMatterDirtyDot {}

    impl FrontMatterDirtyDot {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let this = this.set_ivars(FrontMatterDirtyDotIvars {
                is_dirty: Cell::new(false),
                color: RefCell::new(NSColor::clearColor()),
            });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(8.0, 8.0)
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            if !self.ivars().is_dirty.get() {
                return;
            }
            self.ivars().color.borrow().setFill();
            let bounds = self.bounds();
            NSBezierPath::bezierPathWithOvalInRect(rect(bounds.mid_x() - 3.0, bounds.mid_y() - 3.0, 6.0, 6.0)).fill();
        }

        #[unsafe(method_id(accessibilityLabel))]
        fn __accessibility_label(&self) -> Option<Retained<NSString>> {
            if self.ivars().is_dirty.get() { Some(ns_string("Unsaved value")) } else { None }
        }
    }
);

impl FrontMatterDirtyDot {
    /// `FrontMatterDirtyDot()`.
    fn new(mtm: MainThreadMarker) -> Retained<FrontMatterDirtyDot> {
        unsafe { msg_send![FrontMatterDirtyDot::alloc(mtm), init] }
    }

    fn set_dirty(&self, value: bool) {
        self.ivars().is_dirty.set(value);
        self.setNeedsDisplay(true);
    }

    fn set_color(&self, color: Retained<NSColor>) {
        *self.ivars().color.borrow_mut() = color;
        self.setNeedsDisplay(true);
    }
}
