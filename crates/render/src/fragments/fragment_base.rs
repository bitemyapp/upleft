//! Port of `Fragments/FragmentBase.swift`: object rendering (§11.3) without a
//! WebView anywhere (§3.3).
//!
//! Every element that draws as something other than glyphs is an
//! `NSTextLayoutFragment` subclass returned from the layout manager delegate
//! (`FragmentProvider`). Two rules hold across all of them: the characters
//! stay put, and a multi-paragraph block is many fragments — the first draws
//! the whole object, the rest collapse to `ElidedFragment`.
//!
//! # Subclassing `DownrightFragment`
//!
//! Swift subclasses override four hooks (`verticalPadding`, `suppressesText`,
//! `overrideHeight`, `drawObject(at:in:)`) and nothing else. Here a subclass
//! is a [`FragmentBehavior`] plus an Objective-C class name: the
//! `DownrightFragment` class implements the three `NSTextLayoutFragment`
//! overrides once and dispatches the hooks to the behaviour stored in its
//! ivars, and [`DownrightFragment::new`] allocates the instance from a
//! runtime subclass registered under the Swift class's name, so the layout
//! dump reads `CodeBlockFragment`, `TableRowFragment`, … exactly as Swift's
//! `String(describing: type(of:))` does.

use std::any::Any;
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::ffi::CStr;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder};
use objc2::{ClassType, DefinedClass, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAttributedStringNSExtendedStringDrawing, NSAttributedStringNSStringDrawing, NSBezierPath, NSColor,
    NSColorSpace, NSFont, NSFontWeightMedium, NSGraphicsContext, NSImage, NSLineBreakMode, NSMutableParagraphStyle,
    NSParagraphStyle, NSStringDrawingOptions, NSTextElement, NSTextLayoutFragment, NSTextRange, NSTextStorage,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGContext, CGMutablePath, CGPath};
use objc2_foundation::{
    NSAttributedString, NSCharacterSet, NSMutableAttributedString, NSPoint, NSString,
};

use crate::appkit_compat::{RectExt, attribute_value, attributed_string, keys, rect};
use crate::core_types::NSRange;
use crate::engine::display_map::ParagraphIndex;
use crate::engine::elision_plan::ElisionPlan;
use crate::engine::render_metrics;
use crate::fragments::inline_code_pill::{self, INLINE_CODE_PILL_PAD_X};
use crate::render_contracts::{FragmentPayload, RenderMode};
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::markdown_text_view::MarkdownTextView;

// MARK: - Style token

/// Identity of a resolved `StyleSheet`, for caching rendered output
/// (`StyleToken`). Swift folds the values into a per-process seeded
/// `Hasher`; the token is only ever compared within one process, so any
/// stable hash of the same values is equivalent.
pub struct StyleToken;

impl StyleToken {
    pub fn of(style_sheet: &StyleSheet) -> i64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        style_sheet.revision.hash(&mut hasher);
        style_sheet.theme.name.hash(&mut hasher);
        style_sheet.body_font().pointSize().to_bits().hash(&mut hasher);
        style_sheet.increase_contrast.hash(&mut hasher);
        for color in [
            &style_sheet.background,
            &style_sheet.text,
            &style_sheet.accent,
            &style_sheet.code_background,
        ] {
            let resolved = color
                .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                .unwrap_or_else(|| color.clone());
            resolved.redComponent().to_bits().hash(&mut hasher);
            resolved.greenComponent().to_bits().hash(&mut hasher);
            resolved.blueComponent().to_bits().hash(&mut hasher);
        }
        hasher.finish() as i64
    }
}

/// A checkbox toggle that is still confirming itself on screen (§7.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CheckboxPulse {
    /// The list block that holds the box.
    pub source_range: NSRange,
    pub started: f64,
    /// The state the box just entered.
    pub checked: bool,
}

impl CheckboxPulse {
    /// Total animation time.
    pub const DURATION: f64 = 0.4;
}

/// Identity of a table geometry cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TableLayoutKey {
    pub location: isize,
    pub width: isize,
    pub text_revision: isize,
}

/// `CFAbsoluteTimeGetCurrent()`.
pub fn cf_absolute_time_get_current() -> f64 {
    unsafe extern "C" {
        fn CFAbsoluteTimeGetCurrent() -> f64;
    }
    // SAFETY: a pure clock read.
    unsafe { CFAbsoluteTimeGetCurrent() }
}

/// App-side trust hook for local assets outside the document directory.
pub type LocalAssetAuthorizer = Rc<dyn Fn(&str) -> bool>;

/// View-side state the fragments read while drawing (`FragmentContext`).
///
/// Shared as `Rc<FragmentContext>`; fragments hold it weakly so they never
/// keep the view alive. Every field is interior-mutable because Swift's
/// class is mutated in place by the view while fragments hold it.
pub struct FragmentContext {
    text_view: RefCell<ObjcWeak<MarkdownTextView>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    style_token: Cell<i64>,
    pub mode: Cell<RenderMode>,
    /// Explicit block/selection source lens.
    pub source_focus_range: Cell<Option<NSRange>>,
    /// Source range of the fragment under the pointer (§7.1).
    pub hovered_fragment_range: Cell<Option<NSRange>>,
    /// Code block that just copied.
    pub copied_code_range: Cell<Option<NSRange>>,
    /// Row the pointer is over inside a hovered table.
    pub hovered_table_row: Cell<Option<NSRange>>,
    /// Checkbox toggles still confirming themselves (§7.1).
    pub checkbox_pulses: RefCell<Vec<CheckboxPulse>>,
    /// Primary caret in source coordinates, or `None` in Read mode.
    pub caret: Cell<Option<isize>>,
    /// Width of the text column, for objects that fill the measure.
    pub content_width: Cell<CGFloat>,
    /// Directory images and diagrams resolve against (§3.4), as a file path.
    pub document_url: RefCell<Option<String>>,
    /// Existing app trust, if any.
    pub local_asset_authorizer: RefCell<Option<LocalAssetAuthorizer>>,
    /// Paragraph structure of the current text.
    pub paragraph_index: RefCell<ParagraphIndex>,
    /// Zoom + fold + search visibility (§5.2, §7.1, §9.4).
    pub elision: RefCell<ElisionPlan>,
    pub cue_elision: RefCell<ElisionPlan>,
    /// Explicit per-block code collapse, keyed by the block's start offset.
    pub collapse_overrides: RefCell<HashMap<isize, bool>>,
    /// Front matter fields, so the metadata card does not re-parse YAML.
    pub front_matter_fields: RefCell<Vec<(String, String)>>,
    pub document_has_h1: Cell<bool>,
    /// Column geometry per table (`[TableLayoutKey: TableLayout]`). The value
    /// type belongs to the table fragment, so it is stored type-erased.
    pub table_layouts: RefCell<HashMap<TableLayoutKey, Rc<dyn Any>>>,
    /// Bumped every time the text storage changes.
    pub text_revision: Cell<isize>,
}

impl FragmentContext {
    pub fn new(style_sheet: Rc<StyleSheet>) -> Rc<FragmentContext> {
        let token = StyleToken::of(&style_sheet);
        Rc::new(FragmentContext {
            text_view: RefCell::new(ObjcWeak::new(None)),
            style_sheet: RefCell::new(style_sheet),
            style_token: Cell::new(token),
            mode: Cell::new(RenderMode::Read),
            source_focus_range: Cell::new(None),
            hovered_fragment_range: Cell::new(None),
            copied_code_range: Cell::new(None),
            hovered_table_row: Cell::new(None),
            checkbox_pulses: RefCell::new(Vec::new()),
            caret: Cell::new(None),
            content_width: Cell::new(640.0),
            document_url: RefCell::new(None),
            local_asset_authorizer: RefCell::new(None),
            paragraph_index: RefCell::new(ParagraphIndex::empty()),
            elision: RefCell::new(ElisionPlan::none()),
            cue_elision: RefCell::new(ElisionPlan::none()),
            collapse_overrides: RefCell::new(HashMap::new()),
            front_matter_fields: RefCell::new(Vec::new()),
            document_has_h1: Cell::new(false),
            table_layouts: RefCell::new(HashMap::new()),
            text_revision: Cell::new(0),
        })
    }

    pub fn text_view(&self) -> Option<Retained<MarkdownTextView>> {
        self.text_view.borrow().load()
    }

    pub fn set_text_view(&self, view: &MarkdownTextView) {
        *self.text_view.borrow_mut() = ObjcWeak::from(view);
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.style_sheet.borrow().clone()
    }

    /// `styleSheet`'s setter, with its `didSet`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        let token = StyleToken::of(&style_sheet);
        *self.style_sheet.borrow_mut() = style_sheet;
        self.style_token.set(token);
    }

    /// Cache token for rendered images; see `StyleToken`.
    pub fn style_token(&self) -> i64 {
        self.style_token.get()
    }

    pub fn paragraph_index(&self) -> Ref<'_, ParagraphIndex> {
        self.paragraph_index.borrow()
    }

    pub fn invalidate_derived_layout(&self) {
        self.text_revision.set(self.text_revision.get().wrapping_add(1));
        self.table_layouts.borrow_mut().clear();
    }

    /// Register a toggle and drop any pulse that has already finished.
    pub fn begin_checkbox_pulse(&self, range: NSRange, checked: bool) {
        let now = cf_absolute_time_get_current();
        let mut pulses = self.checkbox_pulses.borrow_mut();
        pulses.retain(|pulse| !(now - pulse.started > CheckboxPulse::DURATION));
        pulses.push(CheckboxPulse { source_range: range, started: now, checked });
    }

    /// `textView?.textStorage`.
    pub fn storage(&self) -> Option<Retained<NSTextStorage>> {
        self.text_view().and_then(|view| unsafe { view.textStorage() })
    }

    /// Strictly inside, not merely touching.
    pub fn is_caret_inside(&self, range: NSRange) -> bool {
        let Some(caret) = self.caret.get() else { return false };
        if !(range.length > 0) {
            return false;
        }
        caret > range.location && caret < range.upper_bound()
    }

    pub fn is_source_focused(&self, range: NSRange) -> bool {
        let Some(focus) = self.source_focus_range.get() else { return false };
        upleft_core::ns_range::ns_intersection_range(focus, range).length > 0 || focus.contains(range.location)
    }
}

// MARK: - DownrightFragment

/// The four hooks a `DownrightFragment` subclass overrides.
pub trait FragmentBehavior: 'static {
    /// Extra height this fragment adds above and below its glyph run.
    fn vertical_padding(&self, _fragment: &DownrightFragment) -> (CGFloat, CGFloat) {
        (0.0, 0.0)
    }
    /// When true the fragment's own glyphs are not drawn.
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        false
    }
    /// Fixed height, overriding the glyph run's own. `None` keeps it.
    fn override_height(&self, _fragment: &DownrightFragment) -> Option<CGFloat> {
        None
    }
    /// Drawn beneath the glyphs.
    fn draw_object(&self, _fragment: &DownrightFragment, _point: CGPoint, _cg: &CGContext) {}
    /// For downcasting to the subclass's own state.
    fn as_any(&self) -> &dyn Any;
}

pub struct DownrightFragmentIvars {
    payload: Retained<FragmentPayload>,
    context: Weak<FragmentContext>,
    behavior: Box<dyn FragmentBehavior>,
}

define_class!(
    /// Base for every drawing fragment (`DownrightFragment`).
    // SAFETY: NSTextLayoutFragment's designated initialiser is
    // `initWithTextElement:range:`, which `new` forwards to after setting the
    // ivars. The overrides keep AppKit's signatures. No Drop impl.
    #[unsafe(super(NSTextLayoutFragment))]
    #[name = "DownrightFragment"]
    #[ivars = DownrightFragmentIvars]
    pub struct DownrightFragment;

    impl DownrightFragment {
        #[unsafe(method(layoutFragmentFrame))]
        fn __layout_fragment_frame(&self) -> CGRect {
            let mut frame = self.super_layout_fragment_frame();
            if let Some(height) = self.override_height() {
                frame.size.height = height;
            } else {
                let padding = self.vertical_padding();
                frame.size.height += padding.0 + padding.1;
            }
            frame
        }

        #[unsafe(method(renderingSurfaceBounds))]
        fn __rendering_surface_bounds(&self) -> CGRect {
            let frame = self.layoutFragmentFrame();
            let natural: CGRect = unsafe { msg_send![super(self), renderingSurfaceBounds] };
            let object = rect(
                -render_metrics::REVEAL_SLACK,
                0.0,
                self.content_width() + render_metrics::REVEAL_SLACK * 2.0,
                frame.height(),
            );
            natural.union(object)
        }

        #[unsafe(method(drawAtPoint:inContext:))]
        fn __draw(&self, point: CGPoint, context: &CGContext) {
            self.ivars().behavior.draw_object(self, point, context);
            if self.suppresses_text() {
                return;
            }
            let padding = self.vertical_padding();
            let style_sheet = self.style_sheet();
            if let Some(style_sheet) = &style_sheet {
                inline_code_pill::draw_inline_code_pills(self, point, padding.0, style_sheet, context);
            }
            if padding.0 == 0.0 {
                let _: () = unsafe { msg_send![super(self), drawAtPoint: point, inContext: context] };
            } else {
                let shifted = CGPoint::new(point.x, point.y + padding.0);
                let _: () = unsafe { msg_send![super(self), drawAtPoint: shifted, inContext: context] };
            }
            if let Some(style_sheet) = &style_sheet {
                inline_code_pill::draw_invisible_marks(self, point, padding.0, style_sheet, context);
            }
        }
    }
);

thread_local! {
    static FRAGMENT_CLASSES: RefCell<HashMap<&'static CStr, &'static AnyClass>> = RefCell::new(HashMap::new());
}

/// The runtime subclass of `DownrightFragment` named `name`, registered on
/// first use. It adds no ivars and no methods: its only job is to carry the
/// Swift subclass's name.
pub fn downright_fragment_class(name: &'static CStr) -> &'static AnyClass {
    FRAGMENT_CLASSES.with(|classes| {
        *classes.borrow_mut().entry(name).or_insert_with(|| {
            if let Some(existing) = AnyClass::get(name) {
                return existing;
            }
            ClassBuilder::new(name, DownrightFragment::class())
                .unwrap_or_else(|| panic!("fragment class {name:?} could not be declared"))
                .register()
        })
    })
}

impl DownrightFragment {
    /// `init(textElement:range:payload:context:)` of the subclass named
    /// `class_name`, whose hooks are `behavior`.
    pub fn new(
        class_name: &'static CStr,
        text_element: &NSTextElement,
        range: Option<&NSTextRange>,
        payload: &FragmentPayload,
        context: &Rc<FragmentContext>,
        behavior: Box<dyn FragmentBehavior>,
    ) -> Retained<DownrightFragment> {
        let class = downright_fragment_class(class_name);
        // SAFETY: `class` is a subclass of DownrightFragment with no ivars of
        // its own, so an allocation of it is a valid DownrightFragment
        // allocation whose ivars `set_ivars` initialises.
        let this: Allocated<DownrightFragment> = unsafe { msg_send![class, alloc] };
        let this = this.set_ivars(DownrightFragmentIvars {
            payload: payload.retain(),
            context: Rc::downgrade(context),
            behavior,
        });
        // SAFETY: NSTextLayoutFragment's designated initialiser.
        unsafe { msg_send![super(this), initWithTextElement: text_element, range: range] }
    }

    pub fn payload(&self) -> &FragmentPayload {
        &self.ivars().payload
    }

    pub fn context(&self) -> Option<Rc<FragmentContext>> {
        self.ivars().context.upgrade()
    }

    /// The subclass's own state.
    pub fn behavior(&self) -> &dyn FragmentBehavior {
        &*self.ivars().behavior
    }

    /// `nil` only after the view has gone away mid-relayout.
    pub fn style_sheet(&self) -> Option<Rc<StyleSheet>> {
        self.context().map(|context| context.style_sheet())
    }

    /// Cache token for anything expensive this fragment renders.
    pub fn style_token(&self) -> i64 {
        self.context().map_or(0, |context| context.style_token())
    }

    pub fn vertical_padding(&self) -> (CGFloat, CGFloat) {
        self.ivars().behavior.vertical_padding(self)
    }

    pub fn suppresses_text(&self) -> bool {
        self.ivars().behavior.suppresses_text(self)
    }

    pub fn override_height(&self) -> Option<CGFloat> {
        self.ivars().behavior.override_height(self)
    }

    /// `super.layoutFragmentFrame`, for hooks that need the glyph run's own
    /// geometry.
    pub fn super_layout_fragment_frame(&self) -> CGRect {
        unsafe { msg_send![super(self), layoutFragmentFrame] }
    }

    /// Width of the whole text column — the reading measure plus the
    /// trailing bleed lane.
    pub fn content_width(&self) -> CGFloat {
        if let Some(width) = self.context().map(|context| context.content_width.get())
            && width > 1.0
        {
            return width;
        }
        smax(1.0, self.super_layout_fragment_frame().width())
    }

    /// Width of the reading column alone.
    pub fn prose_content_width(&self) -> CGFloat {
        smax(1.0, self.content_width() - render_metrics::CODE_BLEED)
    }

    /// Rect of this fragment in its own drawing space, anchored at `point`.
    pub fn bounds(&self, point: CGPoint) -> CGRect {
        rect(point.x, point.y, self.content_width(), self.layoutFragmentFrame().height())
    }

    /// Text of the fragment's source range, straight from the storage.
    pub fn source_text(&self, range: NSRange) -> String {
        let Some(storage) = self.context().and_then(|context| context.storage()) else { return String::new() };
        if !(range.upper_bound() <= storage.length() as isize && range.length > 0) {
            return String::new();
        }
        storage.attributedSubstringFromRange(crate::appkit_compat::ns(range)).string().to_string()
    }

    /// Paragraph style the engine put on this block.
    pub fn paragraph_style(&self) -> Option<Retained<NSParagraphStyle>> {
        let range = self.element_source_range();
        let storage = self.context()?.storage()?;
        if !(range.location < storage.length() as isize) {
            return None;
        }
        attribute_value(&storage, keys::paragraph_style(), range.location as usize)?
            .downcast::<NSParagraphStyle>()
            .ok()
    }

    /// Source range of the paragraph this fragment covers.
    pub fn element_source_range(&self) -> NSRange {
        element_source_range(self, || self.payload().source_range())
    }

    /// Compare paragraph identity, not raw starts.
    pub fn is_first_paragraph_of_block(&self) -> bool {
        let Some(context) = self.context() else {
            return self.element_source_range().location <= self.payload().source_range().location;
        };
        let index = context.paragraph_index();
        index.index_containing(self.element_source_range().location)
            == index.index_containing(self.payload().source_range().location)
    }
}

/// `elementSourceRange` for any fragment: the element's range in source
/// offsets, or `fallback()` when the element has no manager.
pub fn element_source_range(fragment: &NSTextLayoutFragment, fallback: impl FnOnce() -> NSRange) -> NSRange {
    let Some(element) = fragment.textElement() else { return fallback() };
    let (Some(range), Some(manager)) = (element.elementRange(), element.textContentManager()) else {
        return fallback();
    };
    let document_start = manager.documentRange().location();
    let location = manager.offsetFromLocation_toLocation(&document_start, &range.location()) as isize;
    let end = manager.offsetFromLocation_toLocation(&document_start, &range.endLocation()) as isize;
    NSRange::new(location, smax_i(0, end - location))
}

#[inline]
fn smax_i(x: isize, y: isize) -> isize {
    if y >= x { y } else { x }
}

// MARK: - ProseFragment

pub struct ProseFragmentIvars {
    context: Weak<FragmentContext>,
}

define_class!(
    /// Ordinary prose: a paragraph TextKit lays out and draws itself, with
    /// the pills and invisibles painted in the same pass (`ProseFragment`).
    // SAFETY: designated initialiser forwarded in `new`; no Drop impl.
    #[unsafe(super(NSTextLayoutFragment))]
    #[name = "ProseFragment"]
    #[ivars = ProseFragmentIvars]
    pub struct ProseFragment;

    impl ProseFragment {
        /// A pill overhangs the code run it bounds.
        #[unsafe(method(renderingSurfaceBounds))]
        fn __rendering_surface_bounds(&self) -> CGRect {
            let natural: CGRect = unsafe { msg_send![super(self), renderingSurfaceBounds] };
            natural.inset_by(-(INLINE_CODE_PILL_PAD_X + 1.0), 0.0)
        }

        #[unsafe(method(drawAtPoint:inContext:))]
        fn __draw(&self, point: CGPoint, context: &CGContext) {
            if let Some(style_sheet) = self.ivars().context.upgrade().map(|context| context.style_sheet()) {
                inline_code_pill::draw_inline_code_pills(self, point, 0.0, &style_sheet, context);
                let _: () = unsafe { msg_send![super(self), drawAtPoint: point, inContext: context] };
                inline_code_pill::draw_invisible_marks(self, point, 0.0, &style_sheet, context);
            } else {
                let _: () = unsafe { msg_send![super(self), drawAtPoint: point, inContext: context] };
            }
        }
    }
);

impl ProseFragment {
    pub fn new(
        text_element: &NSTextElement,
        range: Option<&NSTextRange>,
        context: Option<&Rc<FragmentContext>>,
    ) -> Retained<ProseFragment> {
        let this = Self::alloc().set_ivars(ProseFragmentIvars {
            context: context.map_or_else(Weak::new, Rc::downgrade),
        });
        unsafe { msg_send![super(this), initWithTextElement: text_element, range: range] }
    }
}

// MARK: - ElidedFragment

define_class!(
    /// Zero height, draws nothing (`ElidedFragment`): the one mechanism
    /// behind every collapse in the app.
    // SAFETY: designated initialiser forwarded in `new`; no ivars.
    #[unsafe(super(NSTextLayoutFragment))]
    #[name = "ElidedFragment"]
    pub struct ElidedFragment;

    impl ElidedFragment {
        #[unsafe(method(layoutFragmentFrame))]
        fn __layout_fragment_frame(&self) -> CGRect {
            let mut frame: CGRect = unsafe { msg_send![super(self), layoutFragmentFrame] };
            frame.size.height = 0.0;
            frame
        }

        #[unsafe(method(renderingSurfaceBounds))]
        fn __rendering_surface_bounds(&self) -> CGRect {
            crate::appkit_compat::RECT_ZERO
        }

        #[unsafe(method(drawAtPoint:inContext:))]
        fn __draw(&self, _point: CGPoint, _context: &CGContext) {}
    }
);

impl ElidedFragment {
    pub fn new(text_element: &NSTextElement, range: Option<&NSTextRange>) -> Retained<ElidedFragment> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), initWithTextElement: text_element, range: range] }
    }
}

// MARK: - ElisionCueFragment

pub struct ElisionCueFragmentIvars {
    hidden_range: NSRange,
    context: Weak<FragmentContext>,
}

define_class!(
    /// First row of a structurally hidden run (`ElisionCueFragment`).
    // SAFETY: designated initialiser forwarded in `new`; no Drop impl.
    #[unsafe(super(NSTextLayoutFragment))]
    #[name = "ElisionCueFragment"]
    #[ivars = ElisionCueFragmentIvars]
    pub struct ElisionCueFragment;

    impl ElisionCueFragment {
        #[unsafe(method(layoutFragmentFrame))]
        fn __layout_fragment_frame(&self) -> CGRect {
            let mut frame: CGRect = unsafe { msg_send![super(self), layoutFragmentFrame] };
            frame.size.height = 28.0;
            frame
        }

        #[unsafe(method(renderingSurfaceBounds))]
        fn __rendering_surface_bounds(&self) -> CGRect {
            self.layoutFragmentFrame()
        }

        #[unsafe(method(drawAtPoint:inContext:))]
        fn __draw(&self, point: CGPoint, cg: &CGContext) {
            self.draw_cue(point, cg);
        }
    }
);

impl ElisionCueFragment {
    pub fn new(
        text_element: &NSTextElement,
        range: Option<&NSTextRange>,
        hidden_range: NSRange,
        context: &Rc<FragmentContext>,
    ) -> Retained<ElisionCueFragment> {
        let this = Self::alloc().set_ivars(ElisionCueFragmentIvars { hidden_range, context: Rc::downgrade(context) });
        unsafe { msg_send![super(this), initWithTextElement: text_element, range: range] }
    }

    fn draw_cue(&self, point: CGPoint, cg: &CGContext) {
        let Some(context) = self.ivars().context.upgrade() else { return };
        let Some(storage) = context.storage() else { return };
        let source = storage.string();
        let clamped = upleft_core::ns_range::ns_intersection_range(
            self.ivars().hidden_range,
            NSRange::new(0, source.length() as isize),
        );
        let components = source
            .substringWithRange(crate::appkit_compat::ns(clamped))
            .componentsSeparatedByCharactersInSet(&NSCharacterSet::newlineCharacterSet());
        let count = components.count() as isize;
        let lines = (count - 1).max(1);
        let style = context.style_sheet();
        let font = NSFont::systemFontOfSize_weight(10.5, unsafe { NSFontWeightMedium });
        let label = attributed_string(
            &format!("⋯  {lines} line{}", if lines == 1 { "" } else { "s" }),
            &[(keys::font(), &font), (keys::foreground_color(), &style.text_faint)],
        );
        let frame = self.layoutFragmentFrame();
        let size = label.size();
        let y = point.y + smax(0.0, (frame.height() - size.height) / 2.0);
        let center = point.x + frame.width() / 2.0;
        let gap: CGFloat = 9.0;
        style.rule.setStroke();
        let path = NSBezierPath::bezierPath();
        path.setLineWidth(1.0);
        path.moveToPoint(NSPoint::new(point.x, y + size.height / 2.0));
        path.lineToPoint(NSPoint::new(center - size.width / 2.0 - gap, y + size.height / 2.0));
        path.moveToPoint(NSPoint::new(center + size.width / 2.0 + gap, y + size.height / 2.0));
        path.lineToPoint(NSPoint::new(point.x + frame.width(), y + size.height / 2.0));
        path.stroke();
        let previous = NSGraphicsContext::currentContext();
        NSGraphicsContext::setCurrentContext(Some(&NSGraphicsContext::graphicsContextWithCGContext_flipped(cg, true)));
        label.drawAtPoint(NSPoint::new(center - size.width / 2.0, y));
        NSGraphicsContext::setCurrentContext(previous.as_deref());
    }
}

// MARK: - Clipping

/// `NSAttributedString.clipped(toHeight:width:)`: this string, wrapped at
/// `width` and cut to `max_height`, ending in an ellipsis when anything was
/// dropped.
pub fn clipped(string: &NSAttributedString, max_height: CGFloat, width: CGFloat) -> Retained<NSAttributedString> {
    if !(string.length() > 0 && width > 1.0 && max_height > 0.0) {
        return string.retain();
    }
    let height = |candidate: &NSAttributedString| -> CGFloat {
        candidate
            .boundingRectWithSize_options_context(
                CGSize::new(width, CGFloat::MAX),
                NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
                None,
            )
            .height()
    };
    if !(height(string) > max_height + 0.5) {
        return string.retain();
    }
    let length = string.length();
    let candidate = |prefix: usize| -> Retained<NSAttributedString> {
        let head = NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &string.attributedSubstringFromRange(objc2_foundation::NSRange::new(0, prefix)),
        );
        loop {
            if head.length() == 0 {
                break;
            }
            let text = head.string().to_string();
            let Some(last) = text.chars().next_back() else { break };
            if !crate::swift_compat::is_whitespace_or_newline(last) {
                break;
            }
            head.deleteCharactersInRange(objc2_foundation::NSRange::new(head.length() - 1, 1));
        }
        let index = prefix.min(length.saturating_sub(1));
        // SAFETY: `index` is inside the string.
        let attributes = unsafe { string.attributesAtIndex_effectiveRange(index, std::ptr::null_mut()) };
        let ellipsis = unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str("…"),
                Some(&attributes),
            )
        };
        head.appendAttributedString(&ellipsis);
        Retained::into_super(head)
    };
    let (mut low, mut high) = (0usize, length);
    while low < high {
        let mid = (low + high + 1) / 2;
        if height(&candidate(mid)) <= max_height + 0.5 {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    candidate(low)
}

// MARK: - Failed objects

/// The trust treatment for an object that did not render (§8.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedObject {
    pub label: String,
    pub source: String,
}

impl FailedObject {
    pub const INSET_X: CGFloat = 14.0;
    pub const INSET_Y: CGFloat = 12.0;
    pub const SOURCE_LINE_LIMIT: isize = 12;
}

impl DownrightFragment {
    fn failed_object_text(
        &self,
        object: &FailedObject,
        style: &StyleSheet,
    ) -> (Retained<NSAttributedString>, Retained<NSAttributedString>) {
        let size = style.body_font().pointSize();
        let label_paragraph = NSMutableParagraphStyle::new();
        label_paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        let label_font = style.emphasis_font(true, false).fontWithSize(size * 0.86);
        let label = attributed_string(
            &object.label,
            &[
                (keys::font(), &label_font),
                (keys::foreground_color(), &style.path_missing),
                (keys::paragraph_style(), &label_paragraph),
            ],
        );
        if object.source.is_empty() {
            return (label, NSAttributedString::new());
        }
        let source_paragraph = NSMutableParagraphStyle::new();
        source_paragraph.setLineBreakMode(NSLineBreakMode::ByCharWrapping);
        let source_font = style.mono_font(Some(size * 0.82));
        let source = attributed_string(
            &object.source,
            &[
                (keys::font(), &source_font),
                (keys::foreground_color(), &style.text_secondary),
                (keys::paragraph_style(), &source_paragraph),
            ],
        );
        let clipped = clipped(
            &source,
            style.line_height * FailedObject::SOURCE_LINE_LIMIT as CGFloat,
            smax(80.0, self.content_width() - FailedObject::INSET_X * 2.0),
        );
        (label, clipped)
    }

    fn failed_object_metrics(&self, object: &FailedObject, style: &StyleSheet) -> (CGFloat, CGFloat, CGFloat) {
        let width = smax(80.0, self.content_width() - FailedObject::INSET_X * 2.0);
        let text = self.failed_object_text(object, style);
        let bounds = |string: &NSAttributedString| -> CGFloat {
            if string.length() == 0 {
                return 0.0;
            }
            string
                .boundingRectWithSize_options_context(
                    CGSize::new(width, CGFloat::MAX),
                    NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
                    None,
                )
                .height()
                .ceil()
        };
        (bounds(&text.0), bounds(&text.1), width)
    }

    /// Height the block needs, before any grid snapping the caller applies.
    pub fn failed_object_height(&self, object: &FailedObject, style: &StyleSheet) -> CGFloat {
        let metrics = self.failed_object_metrics(object, style);
        let gap = if metrics.1 > 0.0 { render_metrics::IMAGE_CAPTION_GAP } else { 0.0 };
        metrics.0 + gap + metrics.1 + FailedObject::INSET_Y * 2.0
    }

    pub fn draw_failed_object(&self, object: &FailedObject, target: CGRect, style: &StyleSheet, cg: &CGContext) {
        fill_rect(cg, target, &style.code_background, render_metrics::IMAGE_CORNER_RADIUS);
        let context = Some(cg);
        CGContext::save_g_state(context);
        CGContext::set_stroke_color_with_color(context, Some(&style.path_missing.colorWithAlphaComponent(0.55).CGColor()));
        CGContext::set_line_width(context, 1.0);
        // SAFETY: a null transform is allowed.
        let path = unsafe {
            CGPath::with_rounded_rect(
                target.inset_by(0.5, 0.5),
                render_metrics::IMAGE_CORNER_RADIUS,
                render_metrics::IMAGE_CORNER_RADIUS,
                std::ptr::null(),
            )
        };
        CGContext::add_path(context, Some(&path));
        CGContext::stroke_path(context);
        CGContext::restore_g_state(context);

        let metrics = self.failed_object_metrics(object, style);
        let text = self.failed_object_text(object, style);
        let x = target.min_x() + FailedObject::INSET_X;
        draw_text(cg, &text.0, rect(x, target.min_y() + FailedObject::INSET_Y, metrics.2, metrics.0), true);
        if !(metrics.1 > 0.0) {
            return;
        }
        draw_text(
            cg,
            &text.1,
            rect(
                x,
                target.min_y() + FailedObject::INSET_Y + metrics.0 + render_metrics::IMAGE_CAPTION_GAP,
                metrics.2,
                metrics.1,
            ),
            true,
        );
    }
}

// MARK: - Drawing helpers

/// `NSColor.mixed(with:amount:)`: blend toward `other`.
pub fn mixed(color: &NSColor, other: &NSColor, amount: CGFloat) -> Retained<NSColor> {
    let srgb = NSColorSpace::sRGBColorSpace();
    let a = color.colorUsingColorSpace(&srgb).unwrap_or_else(|| color.retain());
    let b = other.colorUsingColorSpace(&srgb).unwrap_or_else(|| other.retain());
    let t = smax(0.0, smin(1.0, amount));
    NSColor::colorWithSRGBRed_green_blue_alpha(
        a.redComponent() + (b.redComponent() - a.redComponent()) * t,
        a.greenComponent() + (b.greenComponent() - a.greenComponent()) * t,
        a.blueComponent() + (b.blueComponent() - a.blueComponent()) * t,
        a.alphaComponent() + (b.alphaComponent() - a.alphaComponent()) * t,
    )
}

/// `drawNSImage(_:in:in:cornerRadius:)`: draws an image into a flipped
/// fragment context without AppKit's focus stack.
pub fn draw_ns_image(image: &NSImage, target: CGRect, cg: &CGContext, corner_radius: CGFloat) {
    let mut proposed = target;
    let drawing_context = NSGraphicsContext::graphicsContextWithCGContext_flipped(cg, true);
    // SAFETY: `proposed` is a valid in-out rect for the call.
    let Some(cg_image) = (unsafe { image.CGImageForProposedRect_context_hints(&mut proposed, Some(&drawing_context), None) })
    else {
        return;
    };
    let context = Some(cg);
    CGContext::save_g_state(context);
    if corner_radius > 0.0 {
        // SAFETY: a null transform is allowed.
        let path = unsafe { CGPath::with_rounded_rect(target, corner_radius, corner_radius, std::ptr::null()) };
        CGContext::add_path(context, Some(&path));
        CGContext::clip(context);
    }
    CGContext::translate_ctm(context, 0.0, target.mid_y());
    CGContext::scale_ctm(context, 1.0, -1.0);
    CGContext::translate_ctm(context, 0.0, -target.mid_y());
    CGContext::draw_image(context, target, Some(&cg_image));
    CGContext::restore_g_state(context);
}

/// Which corners of a rect to round (`RectCorners`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RectCorners(pub isize);

impl RectCorners {
    pub const TOP_LEFT: RectCorners = RectCorners(1 << 0);
    pub const TOP_RIGHT: RectCorners = RectCorners(1 << 1);
    pub const BOTTOM_LEFT: RectCorners = RectCorners(1 << 2);
    pub const BOTTOM_RIGHT: RectCorners = RectCorners(1 << 3);
    pub const ALL: RectCorners = RectCorners(0b1111);

    pub fn contains(self, other: RectCorners) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for RectCorners {
    type Output = RectCorners;
    fn bitor(self, rhs: RectCorners) -> RectCorners {
        RectCorners(self.0 | rhs.0)
    }
}

/// `CGContext.fillRect(_:color:radius:)`.
pub fn fill_rect(cg: &CGContext, target: CGRect, color: &NSColor, radius: CGFloat) {
    if !(target.size.width > 0.0 && target.size.height > 0.0) {
        return;
    }
    let context = Some(cg);
    CGContext::set_fill_color_with_color(context, Some(&color.CGColor()));
    if radius > 0.0 {
        // SAFETY: a null transform is allowed.
        let path = unsafe { CGPath::with_rounded_rect(target, radius, radius, std::ptr::null()) };
        CGContext::add_path(context, Some(&path));
        CGContext::fill_path(context);
    } else {
        CGContext::fill_rect(context, target);
    }
}

/// `CGContext.fillRect(_:color:radius:corners:)`: only some corners rounded.
pub fn fill_rect_corners(cg: &CGContext, target: CGRect, color: &NSColor, radius: CGFloat, corners: RectCorners) {
    if !(target.size.width > 0.0 && target.size.height > 0.0) {
        return;
    }
    if !(radius > 0.0 && !corners.is_empty()) {
        fill_rect(cg, target, color, 0.0);
        return;
    }
    let context = Some(cg);
    CGContext::set_fill_color_with_color(context, Some(&color.CGColor()));
    let r = smin(radius, smin(target.width(), target.height()) / 2.0);
    let path = CGMutablePath::new();
    let tl = if corners.contains(RectCorners::TOP_LEFT) { r } else { 0.0 };
    let tr = if corners.contains(RectCorners::TOP_RIGHT) { r } else { 0.0 };
    let br = if corners.contains(RectCorners::BOTTOM_RIGHT) { r } else { 0.0 };
    let bl = if corners.contains(RectCorners::BOTTOM_LEFT) { r } else { 0.0 };
    let (min_x, max_x, min_y, max_y) = (target.min_x(), target.max_x(), target.min_y(), target.max_y());
    let pi = std::f64::consts::PI;
    let m = Some(&*path);
    // SAFETY: null transforms are allowed throughout.
    unsafe {
        CGMutablePath::move_to_point(m, std::ptr::null(), min_x + tl, min_y);
        CGMutablePath::add_line_to_point(m, std::ptr::null(), max_x - tr, min_y);
        if tr > 0.0 {
            CGMutablePath::add_arc(m, std::ptr::null(), max_x - tr, min_y + tr, tr, -pi / 2.0, 0.0, false);
        }
        CGMutablePath::add_line_to_point(m, std::ptr::null(), max_x, max_y - br);
        if br > 0.0 {
            CGMutablePath::add_arc(m, std::ptr::null(), max_x - br, max_y - br, br, 0.0, pi / 2.0, false);
        }
        CGMutablePath::add_line_to_point(m, std::ptr::null(), min_x + bl, max_y);
        if bl > 0.0 {
            CGMutablePath::add_arc(m, std::ptr::null(), min_x + bl, max_y - bl, bl, pi / 2.0, pi, false);
        }
        CGMutablePath::add_line_to_point(m, std::ptr::null(), min_x, min_y + tl);
        if tl > 0.0 {
            CGMutablePath::add_arc(m, std::ptr::null(), min_x + tl, min_y + tl, tl, pi, pi * 1.5, false);
        }
    }
    CGMutablePath::close_subpath(m);
    CGContext::add_path(context, Some(&path));
    CGContext::fill_path(context);
}

/// `CGContext.drawText(_:in:flipped:)`.
pub fn draw_text(cg: &CGContext, string: &NSAttributedString, target: CGRect, flipped: bool) {
    if !(string.length() > 0 && target.width() > 1.0) {
        return;
    }
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&NSGraphicsContext::graphicsContextWithCGContext_flipped(cg, flipped)));
    string.drawWithRect_options_context(
        target,
        NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
        None,
    );
    NSGraphicsContext::setCurrentContext(previous.as_deref());
}

#[allow(dead_code)]
fn _unused(_: &AnyObject) {}
