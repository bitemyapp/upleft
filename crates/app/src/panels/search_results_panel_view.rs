//! Port of `Panels/SearchResultsPanelView.swift`: cross-file search results
//! (§9.4, `⌘⇧F`).
//!
//! Grouped by file, one context line per hit with the match emphasised.
//! Still no index and no vault (§2): the host hands over hits from a shallow
//! scan of the sibling list, and this view's only job is to make them
//! scannable.
//!
//! Objective-C class names equal the Swift ones: `SearchResultsPanelView`
//! and the private row class `SearchHitRowView`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSControlTextEditingDelegate, NSLayoutConstraintOrientation, NSLayoutPriorityRequired, NSLineBreakMode, NSResponder,
    NSScrollView, NSTableColumn, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextAlignment,
    NSTextField, NSUserInterfaceItemIdentification, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSMutableAttributedString, NSRange as FoundationRange, NSRect, NSString};
use upleft_render::appkit_compat::{attributes_dictionary, keys};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift;

use super::activity_indicator_view::ActivityIndicatorView;
use super::appkit_support::{
    activate, label, ns_string, object, role, set_label, set_role, set_tool_tip, set_value, wrapping_label,
};
use super::panel_chrome::{
    PanelEmptyStateView, PanelFont, PanelGroupRowView, PanelList, PanelMetrics, PanelSurface, PanelTableView,
};
use crate::support::find_engine::SiblingHit;

/// `SearchResultsDelegate`.
pub trait SearchResultsDelegate {
    fn search_results_did_select(&self, view: &SearchResultsPanelView, hit: &SiblingHit);
}

/// `SearchResultsPanelView.Row`.
#[derive(Debug, Clone)]
enum Row {
    Group { name: String, count: usize },
    /// Index into `hits`.
    Hit(usize),
}

pub struct SearchResultsPanelViewIvars {
    delegate: RefCell<Option<Weak<dyn SearchResultsDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    hits: RefCell<Vec<SiblingHit>>,
    query: RefCell<String>,
    searched_file_count: Cell<isize>,
    is_searching: Cell<bool>,
    status_label: Retained<NSTextField>,
    empty_state: Retained<PanelEmptyStateView>,
    spinner: Retained<ActivityIndicatorView>,
    table: Retained<PanelTableView>,
    /// `lazy var scroll`, made on first use.
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    rows: RefCell<Vec<Row>>,
}

define_class!(
    /// `SearchResultsPanelView`, a `PanelSurface`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "SearchResultsPanelView"]
    #[ivars = SearchResultsPanelViewIvars]
    pub struct SearchResultsPanelView;

    unsafe impl NSObjectProtocol for SearchResultsPanelView {}

    impl SearchResultsPanelView {
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            PanelMetrics::DETAIL_WIDTH
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }

        #[unsafe(method(rowClicked:))]
        fn __row_clicked(&self, _sender: Option<&AnyObject>) {
            if !(self.ivars().table.clickedRow() >= 0) {
                return;
            }
            self.activate_selection();
        }
    }

    // MARK: - Table

    unsafe impl NSTableViewDataSource for SearchResultsPanelView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for SearchResultsPanelView {}

    unsafe impl NSTableViewDelegate for SearchResultsPanelView {
        #[unsafe(method(tableView:heightOfRow:))]
        fn __height_of_row(&self, _table_view: &NSTableView, row: isize) -> CGFloat {
            self.height_of_row(row)
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn __is_group_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.is_group_row(row)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn __should_select_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.should_select_row(row)
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }
    }
);

impl PanelSurface for SearchResultsPanelView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl SearchResultsPanelView {
    /// `SearchResultsPanelView()`: hosts build panels before they have a
    /// theme in hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<SearchResultsPanelView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<SearchResultsPanelView> {
        // Stored-property initial values, in declaration order.
        let status_label = label("", mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let spinner = ActivityIndicatorView::new(mtm);
        let table = PanelList::make_table_view("searchResults", mtm);

        let this = Self::alloc(mtm).set_ivars(SearchResultsPanelViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            hits: RefCell::new(Vec::new()),
            query: RefCell::new(String::new()),
            searched_file_count: Cell::new(0),
            is_searching: Cell::new(false),
            status_label: status_label.clone(),
            empty_state: empty_state.clone(),
            spinner: spinner.clone(),
            table: table.clone(),
            scroll: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
        });
        let this: Retained<SearchResultsPanelView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setAlignment(NSTextAlignment::Right);
        status_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&status_label);

        spinner.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&spinner);

        // SAFETY: the panel owns the table and outlives it.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*this)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            table.setTarget(Some(object(&*this)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<SearchResultsPanelView> = ObjcWeak::from(&*this);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&*table, "Search results");
        let scroll = this.scroll();
        this.addSubview(&scroll);
        // Lifted past the geometric centre: with the header row (and, in the
        // inspector, the find bar) pinning the eye to the top, exact centre
        // reads as sunk.
        empty_state.install(&this, &scroll, 0.9);

        activate(&[
            status_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            status_label
                .topAnchor()
                .constraintEqualToAnchor_constant(&this.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            status_label
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            spinner.trailingAnchor().constraintEqualToAnchor_constant(&status_label.leadingAnchor(), -6.0),
            spinner.centerYAnchor().constraintEqualToAnchor(&status_label.centerYAnchor()),
            spinner.widthAnchor().constraintEqualToConstant(14.0),
            spinner.heightAnchor().constraintEqualToConstant(14.0),
            scroll.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&status_label.bottomAnchor(), 6.0),
            scroll.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
        ]);

        this.apply_style();
        set_role(&*this, role::group());
        set_label(&*this, "Search results");
        this
    }

    /// `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self) -> Retained<NSScrollView> {
        if let Some(scroll) = self.ivars().scroll.borrow().clone() {
            return scroll;
        }
        let scroll = PanelList::make_scroll_view(&self.ivars().table, self.mtm());
        *self.ivars().scroll.borrow_mut() = Some(scroll.clone());
        scroll
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn SearchResultsDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn SearchResultsDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    pub fn hits(&self) -> Vec<SiblingHit> {
        self.ivars().hits.borrow().clone()
    }

    pub fn set_hits(&self, hits: Vec<SiblingHit>) {
        *self.ivars().hits.borrow_mut() = hits;
        self.reload();
    }

    /// The query the results answer; the panel echoes it so an empty list
    /// can say *for what* nothing was found.
    pub fn query(&self) -> String {
        self.ivars().query.borrow().clone()
    }

    pub fn set_query(&self, query: &str) {
        *self.ivars().query.borrow_mut() = query.to_owned();
        self.update_status();
    }

    /// How many sibling files the last pass scanned; 0 means the host has
    /// not told us yet.
    pub fn searched_file_count(&self) -> isize {
        self.ivars().searched_file_count.get()
    }

    pub fn set_searched_file_count(&self, count: isize) {
        self.ivars().searched_file_count.set(count);
        self.update_status();
    }

    pub fn is_searching(&self) -> bool {
        self.ivars().is_searching.get()
    }

    pub fn set_is_searching(&self, is_searching: bool) {
        let old_value = self.ivars().is_searching.replace(is_searching);
        if is_searching == old_value {
            return;
        }
        self.update_status();
    }

    // MARK: - Reload

    pub fn reload(&self) {
        self.rebuild_rows();
        self.ivars().table.reloadData();
        self.update_status();
    }

    /// Hits arrive grouped by file already; this preserves that order rather
    /// than re-sorting.
    fn rebuild_rows(&self) {
        let hits = self.ivars().hits.borrow();
        let mut rows = self.ivars().rows.borrow_mut();
        rows.clear();
        let mut index = 0;
        while index < hits.len() {
            let path = hits[index].url.path();
            let mut end = index;
            while end < hits.len() && swift::str_eq(&hits[end].url.path(), &path) {
                end += 1;
            }
            rows.push(Row::Group { name: hits[index].display_name.clone(), count: end - index });
            rows.extend((index..end).map(Row::Hit));
            index = end;
        }
    }

    fn update_status(&self) {
        let ivars = self.ivars();
        let status_label = &ivars.status_label;
        if ivars.is_searching.get() {
            ivars.spinner.begin();
            status_label.setStringValue(&ns_string("Searching…"));
        } else {
            ivars.spinner.end();
            let (count, files) = {
                let hits = ivars.hits.borrow();
                let files: HashSet<String> = hits.iter().map(|hit| swift::string_key(&hit.url.path())).collect();
                (hits.len(), files.len())
            };
            let text = if count == 0 {
                "No matches".to_owned()
            } else {
                format!("{count} in {files} file{}", if files == 1 { "" } else { "s" })
            };
            status_label.setStringValue(&ns_string(&text));
        }
        let status = status_label.stringValue().to_string();
        set_label(&**status_label, &status);
        let has_results = !ivars.rows.borrow().is_empty();
        ivars.table.setHidden(!has_results);
        ivars.empty_state.setHidden(has_results);
        let style_sheet = self.style_sheet();
        ivars.empty_state.configure(
            if ivars.is_searching.get() { "magnifyingglass" } else { "doc.text.magnifyingglass" },
            &self.empty_state_title(),
            &self.empty_state_subtitle(),
            &style_sheet,
        );
        set_value(self, &status);
    }

    // MARK: - Empty-state copy

    /// Three quiet states: a pass in flight, a pass that found nothing, and
    /// nothing searched yet.
    fn empty_state_title(&self) -> String {
        if self.ivars().is_searching.get() {
            return "Searching".to_owned();
        }
        let query = self.ivars().query.borrow();
        if query.is_empty() {
            return "Type to search".to_owned();
        }
        format!("No matches for “{}”", Self::truncated(&query, 48))
    }

    fn empty_state_subtitle(&self) -> String {
        let count = self.ivars().searched_file_count.get();
        let noun = if count == 1 { "file" } else { "files" };
        if self.ivars().is_searching.get() {
            return if count > 0 {
                format!("Searching {count} nearby {noun}…")
            } else {
                "Searching nearby Markdown files…".to_owned()
            };
        }
        if self.ivars().query.borrow().is_empty() {
            return "Matches in this document’s sibling files appear here as you type.".to_owned();
        }
        if count > 0 {
            format!("Nothing in {count} nearby {noun} contains it.")
        } else {
            "No matching files or lines.".to_owned()
        }
    }

    /// A query of any length lands in one centred line.
    fn truncated(query: &str, limit: usize) -> String {
        if !(swift::count(query) > limit) {
            return query.to_owned();
        }
        swift::prefix(query, limit).to_owned() + "…"
    }

    fn apply_style(&self) {
        let style_sheet = self.style_sheet();
        self.ivars().status_label.setTextColor(Some(&style_sheet.text_faint));
        self.ivars().table.reloadData();
    }

    fn activate_selection(&self) {
        let table = &self.ivars().table;
        let row = if table.clickedRow() >= 0 { table.clickedRow() } else { table.selectedRow() };
        let hit = {
            let rows = self.ivars().rows.borrow();
            let hits = self.ivars().hits.borrow();
            if !(row >= 0 && (row as usize) < rows.len()) {
                return;
            }
            let Row::Hit(index) = rows[row as usize] else { return };
            if !(index < hits.len()) {
                return;
            }
            hits[index].clone()
        };
        if let Some(delegate) = self.delegate() {
            delegate.search_results_did_select(self, &hit);
        }
    }

    // MARK: - Table

    fn row(&self, row: isize) -> Option<Row> {
        let rows = self.ivars().rows.borrow();
        if !(row < rows.len() as isize) {
            return None;
        }
        // Swift traps on a negative index; so does this.
        Some(rows[row as usize].clone())
    }

    fn height_of_row(&self, row: isize) -> CGFloat {
        match self.row(row) {
            None => PanelMetrics::DETAIL_ROW_HEIGHT,
            Some(Row::Group { .. }) => PanelMetrics::GROUP_ROW_HEIGHT,
            Some(Row::Hit(_)) => PanelMetrics::DETAIL_ROW_HEIGHT,
        }
    }

    fn is_group_row(&self, row: isize) -> bool {
        matches!(self.row(row), Some(Row::Group { .. }))
    }

    fn should_select_row(&self, row: isize) -> bool {
        match self.row(row) {
            None => false,
            Some(Row::Group { .. }) => false,
            Some(Row::Hit(_)) => true,
        }
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let row = self.row(row)?;
        let mtm = self.mtm();
        let style_sheet = self.style_sheet();
        match row {
            Row::Group { name, count } => {
                let identifier = NSString::from_str("resultsGroup");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| view.downcast::<PanelGroupRowView>().ok())
                    .unwrap_or_else(|| PanelGroupRowView::new(&identifier, mtm));
                cell.configure(&format!("{}  ·  {count}", swift::uppercased(&name)), &style_sheet.text_faint);
                Some(Retained::into_super(cell))
            }
            Row::Hit(index) => {
                let hit = self.ivars().hits.borrow().get(index).cloned()?;
                let identifier = NSString::from_str("resultsRow");
                let cell = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
                    .and_then(|view| view.downcast::<SearchHitRowView>().ok())
                    .unwrap_or_else(|| SearchHitRowView::new(&identifier, mtm));
                cell.configure(&hit, &style_sheet);
                Some(Retained::into_super(cell))
            }
        }
    }

    // MARK: - Testing

    /// The table, for tests and scenes.
    pub fn table_for_testing(&self) -> Retained<PanelTableView> {
        self.ivars().table.clone()
    }

    /// The empty state, for tests and scenes.
    pub fn empty_state_for_testing(&self) -> Retained<PanelEmptyStateView> {
        self.ivars().empty_state.clone()
    }
}

// MARK: - Row

pub struct SearchHitRowViewIvars {
    line_label: Retained<NSTextField>,
    /// A wrapping label: `labelWithString` never sets `cell.wraps`, so the
    /// two-line allowance below used to truncate on line one.
    context_label: Retained<NSTextField>,
}

define_class!(
    /// `SearchHitRowView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "SearchHitRowView"]
    #[ivars = SearchHitRowViewIvars]
    pub struct SearchHitRowView;

    unsafe impl NSObjectProtocol for SearchHitRowView {}
);

impl SearchHitRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<SearchHitRowView> {
        let line_label = label("", mtm);
        let context_label = wrapping_label("", mtm);
        let this = Self::alloc(mtm)
            .set_ivars(SearchHitRowViewIvars { line_label: line_label.clone(), context_label: context_label.clone() });
        let this: Retained<SearchHitRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));

        line_label.setFont(Some(&PanelFont::secondary()));
        line_label.setAlignment(NSTextAlignment::Right);
        line_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        line_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        this.addSubview(&line_label);

        context_label.setFont(Some(&PanelFont::row()));
        context_label.setMaximumNumberOfLines(2);
        context_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        // …and the ellipsis lands on the *last visible* line.
        if let Some(cell) = context_label.cell() {
            cell.setTruncatesLastVisibleLine(true);
        }
        if let Some(cell) = context_label.cell() {
            cell.setUsesSingleLineMode(false);
        }
        context_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&context_label);

        activate(&[
            line_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 6.0),
            line_label.widthAnchor().constraintEqualToConstant(30.0),
            // The number belongs to the context's first line whether the
            // context wraps or not.
            line_label.firstBaselineAnchor().constraintEqualToAnchor(&context_label.firstBaselineAnchor()),
            context_label.leadingAnchor().constraintEqualToAnchor_constant(&line_label.trailingAnchor(), 6.0),
            context_label
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            // Centred in the row.
            context_label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            context_label.topAnchor().constraintGreaterThanOrEqualToAnchor_constant(&this.topAnchor(), 4.0),
            context_label.bottomAnchor().constraintLessThanOrEqualToAnchor_constant(&this.bottomAnchor(), -4.0),
        ]);
        this
    }

    fn configure(&self, hit: &SiblingHit, style_sheet: &StyleSheet) {
        let ivars = self.ivars();
        ivars.line_label.setStringValue(&ns_string(&hit.line_number.to_string()));
        ivars.line_label.setTextColor(Some(&style_sheet.text_faint));

        // Collapse the context to one visual line before highlighting; each
        // newline becomes one space so the match offset stays exact.
        let raw = swift::replacing_occurrences(&hit.context_text, "\n", " ");

        // No paragraphStyle here: the label's own `lineBreakMode` +
        // `maximumNumberOfLines` own that policy.
        let font = PanelFont::row();
        let attributes = attributes_dictionary(&[
            (keys::font(), object(&*font)),
            (keys::foreground_color(), object(&*style_sheet.text_secondary)),
        ]);
        // SAFETY: every value is an Objective-C object of the type its key
        // expects.
        let attributed = unsafe {
            NSMutableAttributedString::initWithString_attributes(
                NSMutableAttributedString::alloc(),
                &ns_string(&raw),
                Some(&attributes),
            )
        };

        let offset = hit.range.location - hit.context_range.location;
        let length = hit.range.length.min(0isize.max(attributed.length() as isize - offset));
        if offset >= 0 && length > 0 {
            let emphasised = PanelFont::row_emphasised();
            let emphasis = attributes_dictionary(&[
                (keys::foreground_color(), object(&*style_sheet.text)),
                (keys::background_color(), object(&*style_sheet.search_hit)),
                (keys::font(), object(&*emphasised)),
            ]);
            // SAFETY: as above; the range lies inside the string.
            unsafe {
                attributed.addAttributes_range(&emphasis, FoundationRange::new(offset as usize, length as usize));
            }
        }
        ivars.context_label.setAttributedStringValue(&attributed);

        let mut label = format!("Line {}: {raw}", hit.line_number);
        label = format!("{}, {label}", hit.display_name);
        if let Some(heading) = &hit.heading_title {
            label += &format!(", in {heading}");
        }
        set_role(self, role::row());
        set_label(self, &label);
        set_tool_tip(self, hit.heading_title.as_deref());
    }
}

#[allow(unused)]
fn _unused(_: &dyn NSAccessibility) {}
