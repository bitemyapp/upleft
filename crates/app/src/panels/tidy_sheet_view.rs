//! Port of `Panels/TidySheetView.swift`: Tidy Document's accept/reject sheet
//! (§9.1).
//!
//! "Shows a rendered diff before applying, with per-change accept/reject."
//! Grouping by `TidyRule` is what makes that practical: you almost always
//! want all of one rule and none of another — every renumbered list, but none
//! of the guessed fence languages — and a flat list of forty checkboxes would
//! make that a chore instead of two clicks.
//!
//! Objective-C class names: `TidySheetView`, and Swift's private
//! `TidyGroupRowView` and `TidyProposalRowView`.
//!
//! Reproduced as Swift behaves: `TidyProposalRowView.configure` measures
//! whether a diff is truncated at its *current* width (`bounds.width`), which
//! is zero for a freshly made row (so the 200pt floor applies) and the
//! previous row's width for a reused one; and it replaces the expand button's
//! configured symbol image with an unconfigured one.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAttributedStringNSExtendedStringDrawing, NSButton, NSControlTextEditingDelegate, NSFontAttributeName, NSForegroundColorAttributeName,
    NSLayoutAttribute, NSLayoutConstraintOrientation, NSLineBreakMode, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSResponder, NSScrollView, NSStackView, NSStackViewDistribution,
    NSStringDrawingOptions, NSStringDrawing, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{
    NSAttributedString, NSDictionary, NSMutableIndexSet, NSPoint, NSRange as FoundationRange, NSRect, NSSize,
    NSString, NSStringCompareOptions,
};
use upleft_core::contracts::{ChangeKind, TextEdit, TidyRule, Uuid};
use upleft_render::engine::render_metrics;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::appkit_support::{
    activate, label, ns_string, rect, role, set_label, set_role, set_tool_tip, smax, smin, system_symbol,
};
use super::panel_chrome::{
    ButtonAction, CheckState, PanelBackdrop, PanelButton, PanelCheckbox, PanelEmptyStateView, PanelFont, PanelList,
    PanelMetrics, PanelTableView, install_backdrop, panel_title,
};
use crate::support::commands::Command;

/// `TidySheetDelegate`.
pub trait TidySheetDelegate {
    fn tidy_sheet_did_apply(&self, sheet: &TidySheetView, edits: &[TextEdit]);
    fn tidy_sheet_did_cancel(&self, sheet: &TidySheetView);
}

/// One element of Swift's `proposals`: `(edit:, before:, after:)`.
#[derive(Clone, Debug)]
pub struct TidyProposal {
    pub edit: TextEdit,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Copy, Debug)]
enum Row {
    Group(Option<TidyRule>),
    /// Index into `proposals`.
    Proposal(usize),
}

/// A `[NSAttributedString.Key: Any]` dictionary.
type Attributes = Retained<NSDictionary<NSString, AnyObject>>;

/// `checkboxInset` (declared, unused in Swift too).
#[allow(dead_code)]
const CHECKBOX_INSET: CGFloat = 14.0;
const TEXT_INSET: CGFloat = 38.0;

pub struct TidySheetViewIvars {
    delegate: RefCell<Option<Weak<dyn TidySheetDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    /// Each edit with the before/after text to show.
    proposals: RefCell<Vec<TidyProposal>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
    table: Retained<PanelTableView>,
    /// `private lazy var scroll`.
    scroll: OnceCell<Retained<NSScrollView>>,
    footer: Retained<NSStackView>,
    apply_button: RefCell<Option<Retained<NSButton>>>,
    actions: RefCell<Vec<Retained<ButtonAction>>>,
    rows: RefCell<Vec<Row>>,
    accepted: RefCell<HashSet<Uuid>>,
    /// Proposals whose diff the reader has asked to see in full.
    expanded: RefCell<HashSet<Uuid>>,
    height_cache: RefCell<HashMap<usize, CGFloat>>,
    cached_width: Cell<CGFloat>,
    empty_state: Retained<PanelEmptyStateView>,
}

define_class!(
    /// `TidySheetView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TidySheetView"]
    #[ivars = TidySheetViewIvars]
    pub struct TidySheetView;

    unsafe impl NSObjectProtocol for TidySheetView {}

    impl TidySheetView {
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.layout_body();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }

    unsafe impl NSTableViewDataSource for TidySheetView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for TidySheetView {}

    unsafe impl NSTableViewDelegate for TidySheetView {
        #[unsafe(method(tableView:isGroupRow:))]
        fn __is_group_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.is_group_row(row)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn __should_select_row(&self, _table_view: &NSTableView, _row: isize) -> bool {
            false
        }

        #[unsafe(method(tableView:heightOfRow:))]
        fn __height_of_row(&self, _table_view: &NSTableView, row: isize) -> CGFloat {
            self.height_of_row(row)
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&objc2_app_kit::NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }
    }
);

impl TidySheetView {
    /// `TidySheetView()`: hosts build panels before they have a theme in
    /// hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<TidySheetView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<TidySheetView> {
        // Stored-property initial values, in declaration order.
        let title_label = label(&panel_title(Command::TidyDocument), mtm);
        let subtitle_label = label("", mtm);
        let table = PanelList::make_table_view("tidy", mtm);
        let footer = NSStackView::new(mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        // The init body.
        let backdrop = PanelBackdrop::new(
            style_sheet.clone(),
            NSVisualEffectMaterial::WindowBackground,
            NSVisualEffectBlendingMode::BehindWindow,
            mtm,
        );
        let this = Self::alloc(mtm).set_ivars(TidySheetViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            proposals: RefCell::new(Vec::new()),
            backdrop,
            title_label,
            subtitle_label,
            table,
            scroll: OnceCell::new(),
            footer,
            apply_button: RefCell::new(None),
            actions: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
            accepted: RefCell::new(HashSet::new()),
            expanded: RefCell::new(HashSet::new()),
            height_cache: RefCell::new(HashMap::new()),
            cached_width: Cell::new(0.0),
            empty_state,
        });
        let this: Retained<TidySheetView> =
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, 620.0, 460.0)] };

        install_backdrop(&this, &this.ivars().backdrop);

        this.build_header();
        this.build_footer(mtm);
        this.build_table();
        this.apply_style();

        set_role(&*this, role::group());
        set_label(&*this, "Tidy Document changes");
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn TidySheetDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn TidySheetDelegate>>) {
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

    pub fn proposals(&self) -> Vec<TidyProposal> {
        self.ivars().proposals.borrow().clone()
    }

    pub fn set_proposals(&self, proposals: Vec<TidyProposal>) {
        let accepted: HashSet<Uuid> = proposals.iter().map(|proposal| proposal.edit.id).collect();
        *self.ivars().proposals.borrow_mut() = proposals;
        *self.ivars().accepted.borrow_mut() = accepted;
        self.reload();
    }

    fn scroll(&self) -> Retained<NSScrollView> {
        self.ivars().scroll.get_or_init(|| PanelList::make_scroll_view(&self.ivars().table, self.mtm())).clone()
    }

    fn build_header(&self) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::title()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(title_label);

        let subtitle_label = &ivars.subtitle_label;
        subtitle_label.setFont(Some(&PanelFont::secondary()));
        subtitle_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(subtitle_label);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 14.0),
            subtitle_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            subtitle_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 2.0),
        ]);
    }

    fn build_footer(&self, mtm: MainThreadMarker) {
        let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
        let select_all = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.set_all(true);
                }
            },
            mtm,
        );
        let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
        let select_none = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.set_all(false);
                }
            },
            mtm,
        );
        let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
        let cancel = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.tidy_sheet_did_cancel(&this);
                }
            },
            mtm,
        );
        let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
        let apply = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    let edits = this.accepted_edits();
                    delegate.tidy_sheet_did_apply(&this, &edits);
                }
            },
            mtm,
        );
        self.ivars().actions.borrow_mut().extend([select_all.clone(), select_none.clone(), cancel.clone(), apply.clone()]);

        let cancel_button = PanelButton::text("Cancel", &cancel, false, mtm);
        cancel_button.setKeyEquivalent(&ns_string("\u{1b}"));
        let apply_button = PanelButton::text("Apply", &apply, true, mtm);
        *self.ivars().apply_button.borrow_mut() = Some(apply_button.clone());

        let spacer = NSView::new(mtm);
        spacer.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);

        let footer = &self.ivars().footer;
        footer.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        footer.setSpacing(8.0);
        footer.setAlignment(NSLayoutAttribute::CenterY);
        footer.setDistribution(NSStackViewDistribution::Fill);
        footer.setTranslatesAutoresizingMaskIntoConstraints(false);
        footer.addArrangedSubview(&PanelButton::text("Select All", &select_all, false, mtm));
        footer.addArrangedSubview(&PanelButton::text("Select None", &select_none, false, mtm));
        footer.addArrangedSubview(&spacer);
        footer.addArrangedSubview(&cancel_button);
        footer.addArrangedSubview(&apply_button);
        self.addSubview(footer);

        activate(&[
            footer.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), 16.0),
            footer.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -16.0),
            footer.bottomAnchor().constraintEqualToAnchor_constant(&self.bottomAnchor(), -14.0),
        ]);
    }

    fn build_table(&self) {
        let ivars = self.ivars();
        let table = &ivars.table;
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        set_label(&**table, "Proposed changes");

        let scroll = self.scroll();
        self.addSubview(&scroll);
        ivars.empty_state.install(self, &scroll, 1.0);
        activate(&[
            scroll.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&ivars.subtitle_label.bottomAnchor(), 10.0),
            scroll.bottomAnchor().constraintEqualToAnchor_constant(&ivars.footer.topAnchor(), -10.0),
        ]);
    }

    // MARK: - Reload

    pub fn reload(&self) {
        self.rebuild_rows();
        self.ivars().height_cache.borrow_mut().clear();
        self.ivars().table.reloadData();
        self.update_footer();
        self.update_empty_state();
    }

    fn update_empty_state(&self) {
        let ivars = self.ivars();
        let scroll = self.scroll();
        if !ivars.proposals.borrow().is_empty() {
            ivars.empty_state.setHidden(true);
            scroll.setHidden(false);
            return;
        }
        let style_sheet = self.style_sheet();
        ivars.empty_state.configure(
            "checkmark.seal",
            "Nothing to tidy",
            "This document already follows every\ntidy rule.",
            &style_sheet,
        );
        ivars.empty_state.setHidden(false);
        scroll.setHidden(true);
    }

    fn toggle_expansion(&self, index: usize) {
        let ivars = self.ivars();
        let Some(id) = ivars.proposals.borrow().get(index).map(|proposal| proposal.edit.id) else { return };
        {
            let mut expanded = ivars.expanded.borrow_mut();
            if expanded.contains(&id) {
                expanded.remove(&id);
            } else {
                expanded.insert(id);
            }
        }
        ivars.height_cache.borrow_mut().remove(&index);
        let indexes = NSMutableIndexSet::new();
        for (position, row) in ivars.rows.borrow().iter().enumerate() {
            if let Row::Proposal(candidate) = row
                && *candidate == index
            {
                indexes.addIndex(position);
            }
        }
        ivars.table.noteHeightOfRowsWithIndexesChanged(&indexes);
        ivars.table.reloadData();
    }

    /// Rules keep `TidyRule.allCases` order so the sheet reads the same way
    /// every time, with unruled edits last.
    fn rebuild_rows(&self) {
        let mut by_rule: HashMap<Option<TidyRule>, Vec<usize>> = HashMap::new();
        for (index, proposal) in self.ivars().proposals.borrow().iter().enumerate() {
            by_rule.entry(proposal.edit.rule).or_default().push(index);
        }
        let mut rows = self.ivars().rows.borrow_mut();
        rows.clear();
        for rule in TidyRule::ALL_CASES {
            let Some(indices) = by_rule.get(&Some(rule)) else { continue };
            if indices.is_empty() {
                continue;
            }
            rows.push(Row::Group(Some(rule)));
            rows.extend(indices.iter().map(|index| Row::Proposal(*index)));
        }
        if let Some(others) = by_rule.get(&None)
            && !others.is_empty()
        {
            rows.push(Row::Group(None));
            rows.extend(others.iter().map(|index| Row::Proposal(*index)));
        }
    }

    fn accepted_edits(&self) -> Vec<TextEdit> {
        let accepted = self.ivars().accepted.borrow();
        self.ivars()
            .proposals
            .borrow()
            .iter()
            .filter(|proposal| accepted.contains(&proposal.edit.id))
            .map(|proposal| proposal.edit.clone())
            .collect()
    }

    fn set_all(&self, value: bool) {
        let accepted: HashSet<Uuid> = if value {
            self.ivars().proposals.borrow().iter().map(|proposal| proposal.edit.id).collect()
        } else {
            HashSet::new()
        };
        *self.ivars().accepted.borrow_mut() = accepted;
        self.ivars().table.reloadData();
        self.update_footer();
    }

    fn set_accepted_for_group(&self, value: bool, rule: Option<TidyRule>) {
        {
            let proposals = self.ivars().proposals.borrow();
            let mut accepted = self.ivars().accepted.borrow_mut();
            for proposal in proposals.iter().filter(|proposal| proposal.edit.rule == rule) {
                if value {
                    accepted.insert(proposal.edit.id);
                } else {
                    accepted.remove(&proposal.edit.id);
                }
            }
        }
        self.ivars().table.reloadData();
        self.update_footer();
    }

    fn toggle(&self, index: usize) {
        let Some(id) = self.ivars().proposals.borrow().get(index).map(|proposal| proposal.edit.id) else { return };
        {
            let mut accepted = self.ivars().accepted.borrow_mut();
            if accepted.contains(&id) {
                accepted.remove(&id);
            } else {
                accepted.insert(id);
            }
        }
        self.ivars().table.reloadData();
        self.update_footer();
    }

    fn update_footer(&self) {
        let ivars = self.ivars();
        let count = ivars.accepted.borrow().len();
        let apply_button = ivars.apply_button.borrow().clone().expect("applyButton");
        apply_button.setTitle(&ns_string(&if count == 1 {
            "Apply 1 Change".to_owned()
        } else {
            format!("Apply {count} Changes")
        }));
        apply_button.setEnabled(count > 0);
        let total = ivars.proposals.borrow().len();
        ivars.subtitle_label.setStringValue(&ns_string(&if total == 0 {
            String::new()
        } else {
            format!("{count} of {total} selected")
        }));
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        ivars.subtitle_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.height_cache.borrow_mut().clear();
        ivars.table.reloadData();
        self.update_empty_state();
    }

    fn layout_body(&self) {
        let ivars = self.ivars();
        let width = self.scroll().bounds().size.width;
        if !((width - ivars.cached_width.get()).abs() > 0.5) {
            return;
        }
        ivars.cached_width.set(width);
        ivars.height_cache.borrow_mut().clear();
        let count = ivars.rows.borrow().len();
        if count == 0 {
            return;
        }
        let indexes = objc2_foundation::NSIndexSet::indexSetWithIndexesInRange(FoundationRange::new(0, count));
        ivars.table.noteHeightOfRowsWithIndexesChanged(&indexes);
    }

    // MARK: - Diff text

    /// Whitespace-only edits are half of what Tidy does, and "" → "" tells
    /// the reader nothing, so those get described rather than shown.
    pub fn display_text(text: &str) -> String {
        if text.is_empty() {
            return "(nothing)".to_owned();
        }
        if swift_text::trim_whitespaces_and_newlines(text).is_empty() {
            let newlines = swift_text::count(&swift_text::filter(text, swift_text::is_newline));
            if newlines > 0 {
                return format!("({newlines} blank line{})", if newlines == 1 { "" } else { "s" });
            }
            let count = swift_text::count(text);
            return format!("({count} space{})", if count == 1 { "" } else { "s" });
        }
        // Trailing whitespace is invisible and is exactly what one rule
        // trims, so make it visible where it occurs.
        swift_text::split(text, '\n', usize::MAX, false)
            .into_iter()
            .map(|line| {
                let trimmed = replacing_trailing_blanks(line);
                let dots = 0isize.max(swift_text::count(line) as isize - swift_text::count(&trimmed) as isize);
                trimmed + &"·".repeat(dots as usize)
            })
            .collect::<Vec<String>>()
            .join("\n")
    }

    fn diff_attributes(&self, kind: ChangeKind) -> Attributes {
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        let style_sheet = self.style_sheet();
        let font = style_sheet.mono_font(Some(11.0));
        let color = style_sheet.change_color(kind);
        // SAFETY: AppKit exports the keys as immutable globals.
        let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName, NSParagraphStyleAttributeName] };
        let values: [&AnyObject; 3] = [font.as_ref(), color.as_ref(), paragraph.as_ref()];
        NSDictionary::from_slices(&keys, &values)
    }

    // MARK: - Table

    fn is_group_row(&self, row: isize) -> bool {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return false;
        }
        matches!(rows[row as usize], Row::Group(_))
    }

    fn height_of_row(&self, row: isize) -> CGFloat {
        let entry = {
            let rows = self.ivars().rows.borrow();
            if !(row < rows.len() as isize) {
                return PanelMetrics::LIST_ROW_HEIGHT;
            }
            rows[row as usize]
        };
        match entry {
            Row::Group(_) => PanelMetrics::GROUP_ROW_HEIGHT + 4.0,
            Row::Proposal(index) => {
                if let Some(cached) = self.ivars().height_cache.borrow().get(&index) {
                    return *cached;
                }
                let cached_width = self.ivars().cached_width.get();
                let width =
                    smax(200.0, (if cached_width > 0.0 { cached_width } else { self.bounds().size.width }) - TEXT_INSET - 16.0);
                let (before, after, is_expanded) = {
                    let proposals = self.ivars().proposals.borrow();
                    let proposal = &proposals[index];
                    (
                        Self::display_text(&proposal.before),
                        Self::display_text(&proposal.after),
                        self.ivars().expanded.borrow().contains(&proposal.edit.id),
                    )
                };
                let height = TidyProposalRowView::height(
                    &before,
                    &after,
                    &self.diff_attributes(ChangeKind::Deleted),
                    width,
                    is_expanded,
                );
                self.ivars().height_cache.borrow_mut().insert(index, height);
                height
            }
        }
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let entry = {
            let rows = self.ivars().rows.borrow();
            if !(row < rows.len() as isize) {
                return None;
            }
            rows[row as usize]
        };
        let owner: &AnyObject = self.as_ref();
        match entry {
            Row::Group(rule) => {
                let identifier = ns_string("tidyGroup");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(owner)) }
                    .and_then(|view| super::appkit_support::downcast::<TidyGroupRowView>(&view))
                    .unwrap_or_else(|| TidyGroupRowView::new(&identifier, self.mtm()));
                let (count, checked_count) = {
                    let proposals = self.ivars().proposals.borrow();
                    let accepted = self.ivars().accepted.borrow();
                    let members: Vec<&TidyProposal> =
                        proposals.iter().filter(|proposal| proposal.edit.rule == rule).collect();
                    let checked = members.iter().filter(|proposal| accepted.contains(&proposal.edit.id)).count();
                    (members.len(), checked)
                };
                let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
                *cell.ivars().on_toggle.borrow_mut() = Some(Rc::new(move |value| {
                    if let Some(this) = weak.load() {
                        this.set_accepted_for_group(value, rule);
                    }
                }));
                cell.configure(
                    rule.map_or("Other changes", |rule| rule.title()),
                    count,
                    checked_count,
                    self.style_sheet(),
                );
                Some(Retained::into_super(cell))
            }
            Row::Proposal(index) => {
                let (summary, before, after, is_accepted, is_expanded) = {
                    let proposals = self.ivars().proposals.borrow();
                    let proposal = proposals.get(index)?;
                    (
                        proposal.edit.summary.clone(),
                        Self::display_text(&proposal.before),
                        Self::display_text(&proposal.after),
                        self.ivars().accepted.borrow().contains(&proposal.edit.id),
                        self.ivars().expanded.borrow().contains(&proposal.edit.id),
                    )
                };
                let identifier = ns_string("tidyProposal");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(owner)) }
                    .and_then(|view| super::appkit_support::downcast::<TidyProposalRowView>(&view))
                    .unwrap_or_else(|| TidyProposalRowView::new(&identifier, self.mtm()));
                let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
                *cell.ivars().on_toggle.borrow_mut() = Some(Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.toggle(index);
                    }
                }));
                let weak: ObjcWeak<TidySheetView> = ObjcWeak::from(self);
                *cell.ivars().on_toggle_expansion.borrow_mut() = Some(Rc::new(move || {
                    if let Some(this) = weak.load() {
                        this.toggle_expansion(index);
                    }
                }));
                cell.configure(
                    &summary,
                    &before,
                    &after,
                    is_accepted,
                    is_expanded,
                    self.style_sheet(),
                    &self.diff_attributes(ChangeKind::Deleted),
                    &self.diff_attributes(ChangeKind::Inserted),
                );
                Some(Retained::into_super(cell))
            }
        }
    }

    // MARK: - Test hooks (conformance scenes and tests)

    pub fn row_count_for_testing(&self) -> isize {
        self.ivars().rows.borrow().len() as isize
    }

    pub fn accepted_count_for_testing(&self) -> isize {
        self.ivars().accepted.borrow().len() as isize
    }

    pub fn table_for_testing(&self) -> Retained<PanelTableView> {
        self.ivars().table.clone()
    }
}

/// `line.replacingOccurrences(of: "[ \t]+$", with: "", options:
/// .regularExpression)`, through Foundation (ICU's `$` also matches before a
/// final line terminator).
fn replacing_trailing_blanks(line: &str) -> String {
    let string = ns_string(line);
    let result = string.stringByReplacingOccurrencesOfString_withString_options_range(
        &ns_string("[ \t]+$"),
        &ns_string(""),
        NSStringCompareOptions::RegularExpressionSearch,
        FoundationRange::new(0, string.length()),
    );
    result.to_string()
}

// MARK: - Group row

pub struct TidyGroupRowViewIvars {
    on_toggle: RefCell<Option<Rc<dyn Fn(bool)>>>,
    /// The same drawn checkbox the task panel uses, in its mixed state when
    /// a rule is partly accepted — one checkbox style in the app, not two.
    checkbox: Retained<PanelCheckbox>,
    title_label: Retained<NSTextField>,
}

define_class!(
    /// Swift's private `TidyGroupRowView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TidyGroupRowView"]
    #[ivars = TidyGroupRowViewIvars]
    pub struct TidyGroupRowView;

    unsafe impl NSObjectProtocol for TidyGroupRowView {}
);

impl TidyGroupRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<TidyGroupRowView> {
        let checkbox = PanelCheckbox::new_default(mtm);
        let title_label = label("", mtm);
        let this = Self::alloc(mtm).set_ivars(TidyGroupRowViewIvars { on_toggle: RefCell::new(None), checkbox, title_label });
        let this: Retained<TidyGroupRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));

        let ivars = this.ivars();
        ivars.checkbox.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&ivars.checkbox);

        ivars.title_label.setFont(Some(&PanelFont::group()));
        ivars.title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        ivars.title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&ivars.title_label);

        let checkbox = &ivars.checkbox;
        let title_label = &ivars.title_label;
        activate(&[
            checkbox.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 14.0),
            checkbox.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            checkbox.widthAnchor().constraintEqualToConstant(render_metrics::TASK_BOX_SIDE),
            checkbox.heightAnchor().constraintEqualToConstant(render_metrics::TASK_BOX_SIDE),
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&checkbox.trailingAnchor(), 8.0),
            title_label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            title_label.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&this.trailingAnchor(), -14.0),
        ]);
        this
    }

    fn configure(&self, title: &str, count: usize, checked_count: usize, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        ivars.title_label.setStringValue(&ns_string(&format!("{title}  ({count})")));
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.checkbox.set_style_sheet(style_sheet);
        ivars.checkbox.set_state(
            if checked_count == 0 {
                CheckState::Off
            } else if checked_count == count {
                CheckState::On
            } else {
                CheckState::Mixed
            },
            false,
        );
        let next = checked_count != count;
        let weak: ObjcWeak<TidyGroupRowView> = ObjcWeak::from(self);
        ivars.checkbox.set_on_toggle(Some(Rc::new(move || {
            let Some(this) = weak.load() else { return };
            let handler = this.ivars().on_toggle.borrow().clone();
            if let Some(handler) = handler {
                handler(next);
            }
        })));
        set_label(&*ivars.checkbox, &format!("{title}, {checked_count} of {count} selected"));
    }
}

// MARK: - Proposal row

pub struct TidyProposalRowViewIvars {
    on_toggle: RefCell<Option<Rc<dyn Fn()>>>,
    on_toggle_expansion: RefCell<Option<Rc<dyn Fn()>>>,
    checkbox: Retained<PanelCheckbox>,
    expand_button: Retained<NSButton>,
    expand_action: RefCell<Option<Retained<ButtonAction>>>,
    summary: RefCell<String>,
    before: RefCell<Retained<NSAttributedString>>,
    after: RefCell<Retained<NSAttributedString>>,
    style_sheet: RefCell<Option<Rc<StyleSheet>>>,
    is_expanded: Cell<bool>,
}

define_class!(
    /// Swift's private `TidyProposalRowView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TidyProposalRowView"]
    #[ivars = TidyProposalRowViewIvars]
    pub struct TidyProposalRowView;

    unsafe impl NSObjectProtocol for TidyProposalRowView {}

    impl TidyProposalRowView {
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            self.draw();
        }
    }
);

impl TidyProposalRowView {
    const SUMMARY_HEIGHT: CGFloat = 18.0;
    const VERTICAL_PADDING: CGFloat = 8.0;
    /// About six lines of 11pt mono.  The old 48pt cap hid most of a
    /// multi-line change behind nothing at all.
    const COLLAPSED_DIFF_HEIGHT: CGFloat = 96.0;
    const EXPANDED_DIFF_HEIGHT: CGFloat = 320.0;
    const TEXT_INSET: CGFloat = 38.0;

    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<TidyProposalRowView> {
        let checkbox = PanelCheckbox::new_default(mtm);
        let before = NSAttributedString::new();
        let after = NSAttributedString::new();
        let expand_button = PanelButton::symbol_default("chevron.down", "Show the whole change", &ButtonAction::noop(mtm), mtm);
        let this = Self::alloc(mtm).set_ivars(TidyProposalRowViewIvars {
            on_toggle: RefCell::new(None),
            on_toggle_expansion: RefCell::new(None),
            checkbox,
            expand_button,
            expand_action: RefCell::new(None),
            summary: RefCell::new(String::new()),
            before: RefCell::new(before),
            after: RefCell::new(after),
            style_sheet: RefCell::new(None),
            is_expanded: Cell::new(false),
        });
        let this: Retained<TidyProposalRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));

        let ivars = this.ivars();
        ivars.checkbox.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&ivars.checkbox);

        let weak: ObjcWeak<TidyProposalRowView> = ObjcWeak::from(&*this);
        let expand = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                let handler = this.ivars().on_toggle_expansion.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
            },
            mtm,
        );
        *ivars.expand_action.borrow_mut() = Some(expand.clone());
        let expand_button = &ivars.expand_button;
        unsafe {
            expand_button.setTarget(Some(&expand));
            expand_button.setAction(Some(ButtonAction::selector()));
        }
        expand_button.setHidden(true);
        this.addSubview(expand_button);

        let checkbox = &ivars.checkbox;
        activate(&[
            checkbox.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 16.0),
            checkbox.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 8.0),
            checkbox.widthAnchor().constraintEqualToConstant(render_metrics::TASK_BOX_SIDE),
            checkbox.heightAnchor().constraintEqualToConstant(render_metrics::TASK_BOX_SIDE),
            expand_button.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -8.0),
            expand_button.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 4.0),
            expand_button.widthAnchor().constraintEqualToConstant(24.0),
        ]);
        this
    }

    fn cap(is_expanded: bool) -> CGFloat {
        if is_expanded { Self::EXPANDED_DIFF_HEIGHT } else { Self::COLLAPSED_DIFF_HEIGHT }
    }

    fn natural_height(text: &str, attributes: &NSDictionary<NSString, AnyObject>, width: CGFloat) -> CGFloat {
        let string = unsafe {
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns_string(text), Some(attributes))
        };
        bounding_height(&string, width).ceil()
    }

    fn height(
        before: &str,
        after: &str,
        attributes: &NSDictionary<NSString, AnyObject>,
        width: CGFloat,
        is_expanded: bool,
    ) -> CGFloat {
        let limit = Self::cap(is_expanded);
        let measure = |text: &str| -> CGFloat { smin(limit, Self::natural_height(text, attributes, width)) };
        Self::SUMMARY_HEIGHT + measure(before) + measure(after) + Self::VERTICAL_PADDING * 2.0 + 4.0
    }

    #[allow(clippy::too_many_arguments)]
    fn configure(
        &self,
        summary: &str,
        before: &str,
        after: &str,
        is_accepted: bool,
        is_expanded: bool,
        style_sheet: Rc<StyleSheet>,
        before_attributes: &NSDictionary<NSString, AnyObject>,
        after_attributes: &NSDictionary<NSString, AnyObject>,
    ) {
        let ivars = self.ivars();
        *ivars.summary.borrow_mut() = summary.to_owned();
        *ivars.style_sheet.borrow_mut() = Some(style_sheet.clone());
        ivars.is_expanded.set(is_expanded);
        *ivars.before.borrow_mut() = unsafe {
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns_string(before), Some(before_attributes))
        };
        *ivars.after.borrow_mut() = unsafe {
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns_string(after), Some(after_attributes))
        };

        ivars.checkbox.set_style_sheet(style_sheet.clone());
        ivars.checkbox.set_state(if is_accepted { CheckState::On } else { CheckState::Off }, false);
        let weak: ObjcWeak<TidyProposalRowView> = ObjcWeak::from(self);
        ivars.checkbox.set_on_toggle(Some(Rc::new(move || {
            let Some(this) = weak.load() else { return };
            let handler = this.ivars().on_toggle.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
        })));
        set_label(&*ivars.checkbox, summary);

        // The affordance only appears where there is something hidden.
        let width = smax(200.0, self.bounds().size.width - Self::TEXT_INSET - 16.0);
        let limit = Self::cap(false);
        let is_truncated =
            [before, after].iter().any(|text| Self::natural_height(text, before_attributes, width) > limit);
        let expand_button = &ivars.expand_button;
        expand_button.setHidden(!is_truncated);
        expand_button.setImage(
            system_symbol(
                if is_expanded { "chevron.up" } else { "chevron.down" },
                Some(if is_expanded { "Show less" } else { "Show the whole change" }),
            )
            .as_deref(),
        );
        expand_button.setToolTip(Some(&ns_string(if is_expanded { "Show less" } else { "Show the whole change" })));
        expand_button.setContentTintColor(Some(&style_sheet.text_faint));
        set_tool_tip(self, if is_truncated { Some(format!("− {before}\n+ {after}")) } else { None }.as_deref());

        set_label(self, &format!("{summary}. Before: {before}. After: {after}"));
        self.setNeedsDisplay(true);
    }

    fn draw(&self) {
        let ivars = self.ivars();
        let Some(style_sheet) = ivars.style_sheet.borrow().clone() else { return };
        let width = self.bounds().size.width - Self::TEXT_INSET - 16.0;
        if !(width > 0.0) {
            return;
        }

        let mut y = Self::VERTICAL_PADDING;
        let summary = ns_string(&ivars.summary.borrow());
        let font = PanelFont::row();
        // SAFETY: AppKit exports the keys as immutable globals.
        let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
        let values: [&AnyObject; 2] = [font.as_ref(), style_sheet.text.as_ref()];
        let attributes: Attributes = NSDictionary::from_slices(&keys, &values);
        unsafe { summary.drawAtPoint_withAttributes(NSPoint::new(Self::TEXT_INSET, y), Some(&attributes)) };
        y += Self::SUMMARY_HEIGHT;

        let limit = Self::cap(ivars.is_expanded.get());
        let texts = [ivars.before.borrow().clone(), ivars.after.borrow().clone()];
        for text in texts {
            let height = smin(limit, bounding_height(&text, width).ceil());
            // The diff attributes carry `.byTruncatingTail`, so a line that
            // runs past the box ends in an ellipsis rather than a cut glyph.
            text.drawWithRect_options_context(
                rect(Self::TEXT_INSET, y, width, height),
                NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
                None,
            );
            y += height + 2.0;
        }
    }
}

/// `text.boundingRect(with: NSSize(width: width, height: 4000), options:
/// [.usesLineFragmentOrigin, .usesFontLeading]).height`.
fn bounding_height(text: &NSAttributedString, width: CGFloat) -> CGFloat {
    text.boundingRectWithSize_options_context(
        NSSize::new(width, 4000.0),
        NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
        None,
    )
    .size
    .height
}
