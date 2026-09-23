//! Port of `View/MarkdownContainerView.swift`: the single view the app
//! installs. Owns the scroll view, the text view, the left gutter rail
//! (§6.1a) and the footnote margin, and leaves seams for a leading contents
//! map, a trailing accessory and a top accessory.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyClass, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSControlSize, NSResponder, NSScrollView, NSScroller, NSScrollerStyle,
    NSTextStorage, NSView, NSViewBoundsDidChangeNotification, NSWindowOrderingMode,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSEdgeInsets, NSNotification, NSNotificationCenter, NSOperationQueue, NSRect, NSSize};

use objc2_app_kit::NSAppearanceCustomization;
use crate::appkit_compat::{RECT_ZERO, RectExt, rect, rect_fill};
use crate::engine::render_metrics;
use crate::render_contracts::RenderMode;
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::footnote_margin_view::FootnoteMarginView;
use crate::view::gutter_rail_view::GutterRailView;
use crate::view::markdown_text_view::MarkdownTextView;

/// `DensityGutterView.width`, for the leading/trailing density map lane.
const DENSITY_GUTTER_WIDTH: CGFloat = 72.0;

pub struct MarkdownContainerViewIvars {
    text_view: Retained<MarkdownTextView>,
    scroll_view: Retained<NSScrollView>,
    gutter: Retained<GutterRailView>,
    footnote_margin: Retained<FootnoteMarginView>,
    gutter_width: Cell<CGFloat>,
    leading_accessory: RefCell<Option<Retained<NSView>>>,
    trailing_accessory: RefCell<Option<Retained<NSView>>>,
    top_accessory: RefCell<Option<Retained<NSView>>>,
    top_accessory_overlays_content: Cell<bool>,
    scroll_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Drop for MarkdownContainerViewIvars {
    fn drop(&mut self) {
        if let Some(observer) = self.scroll_observer.get_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
    }
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // overrides keep AppKit's signatures. Drop is on the ivars only.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarkdownContainerView"]
    #[ivars = MarkdownContainerViewIvars]
    pub struct MarkdownContainerView;

    unsafe impl NSObjectProtocol for MarkdownContainerView {}

    impl MarkdownContainerView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        /// The container paints the page colour itself.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty_rect: NSRect) {
            self.ivars().text_view.style_sheet().background.setFill();
            rect_fill(dirty_rect.intersection(self.bounds()));
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.layout_subviews();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            let text_view = &self.ivars().text_view;
            let style_sheet = text_view.style_sheet();
            text_view.set_style_sheet(Rc::new(StyleSheet::new(
                style_sheet.theme.clone(),
                &self.effectiveAppearance(),
                None,
            )));
            self.ivars().scroll_view.setBackgroundColor(&text_view.style_sheet().background);
            self.ivars().gutter.reload();
            self.setNeedsLayout(true);
        }
    }
);

impl MarkdownContainerView {
    /// `init(storage:)`, with the fallback style sheet.
    pub fn with_storage(storage: &NSTextStorage, mtm: MainThreadMarker) -> Retained<MarkdownContainerView> {
        Self::new(storage, MarkdownTextView::fallback_style_sheet(), mtm)
    }

    /// `init(storage:styleSheet:)`.
    pub fn new(storage: &NSTextStorage, style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<MarkdownContainerView> {
        let text_view = MarkdownTextView::new(rect(0.0, 0.0, style_sheet.measure_width, 100.0), storage, style_sheet.clone(), mtm);
        let scroll_view = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), RECT_ZERO);
        let gutter = GutterRailView::new(&text_view, mtm);
        let footnote_margin = FootnoteMarginView::new(&text_view, mtm);
        let this = Self::alloc(mtm).set_ivars(MarkdownContainerViewIvars {
            text_view: text_view.clone(),
            scroll_view: scroll_view.clone(),
            gutter: gutter.clone(),
            footnote_margin: footnote_margin.clone(),
            gutter_width: Cell::new(render_metrics::GUTTER_WIDTH),
            leading_accessory: RefCell::new(None),
            trailing_accessory: RefCell::new(None),
            top_accessory: RefCell::new(None),
            top_accessory_overlays_content: Cell::new(false),
            scroll_observer: RefCell::new(None),
        });
        let this: Retained<MarkdownContainerView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };

        scroll_view.setDocumentView(Some(&text_view));
        scroll_view.setHasVerticalScroller(true);
        scroll_view.setHasHorizontalScroller(false);
        scroll_view.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll_view.setAutomaticallyAdjustsContentInsets(false);
        text_view.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
        scroll_view.setDrawsBackground(true);
        scroll_view.setBackgroundColor(&style_sheet.background);
        scroll_view.setContentInsets(NSEdgeInsets { top: 0.0, left: 0.0, bottom: 0.0, right: 0.0 });

        this.addSubview(&scroll_view);
        this.addSubview(&gutter);
        this.addSubview_positioned_relativeTo(&footnote_margin, NSWindowOrderingMode::Above, Some(&scroll_view));
        let clip = scroll_view.contentView();
        clip.setPostsBoundsChangedNotifications(true);
        let weak_margin: ObjcWeak<FootnoteMarginView> = ObjcWeak::from(&*footnote_margin);
        let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
            if let Some(margin) = weak_margin.load() {
                margin.setNeedsDisplay(true);
            }
        });
        // Swift registers a selector observer with no queue, so it runs
        // synchronously on the posting thread; a nil queue does the same.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip),
                None::<&NSOperationQueue>,
                &block,
            )
        };
        *this.ivars().scroll_observer.borrow_mut() = Some(observer);
        gutter.reload();
        this
    }

    pub fn text_view(&self) -> &Retained<MarkdownTextView> {
        &self.ivars().text_view
    }

    pub fn scroll_view(&self) -> &Retained<NSScrollView> {
        &self.ivars().scroll_view
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().text_view.style_sheet()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        self.ivars().text_view.set_style_sheet(style_sheet.clone());
        self.ivars().scroll_view.setBackgroundColor(&style_sheet.background);
        self.setNeedsDisplay(true);
        self.setNeedsLayout(true);
    }

    /// Width reserved on the left for block markers in Live mode (§6.1a).
    pub fn gutter_width(&self) -> CGFloat {
        self.ivars().gutter_width.get()
    }

    pub fn leading_accessory(&self) -> Option<Retained<NSView>> {
        self.ivars().leading_accessory.borrow().clone()
    }

    pub fn set_leading_accessory(&self, accessory: Option<Retained<NSView>>) {
        let old = std::mem::replace(&mut *self.ivars().leading_accessory.borrow_mut(), accessory.clone());
        if let Some(old) = old {
            old.removeFromSuperview();
        }
        if let Some(accessory) = accessory {
            accessory.setTranslatesAutoresizingMaskIntoConstraints(true);
            self.addSubview_positioned_relativeTo(&accessory, NSWindowOrderingMode::Above, None);
        }
        self.setNeedsLayout(true);
    }

    pub fn trailing_accessory(&self) -> Option<Retained<NSView>> {
        self.ivars().trailing_accessory.borrow().clone()
    }

    pub fn set_trailing_accessory(&self, accessory: Option<Retained<NSView>>) {
        let old = std::mem::replace(&mut *self.ivars().trailing_accessory.borrow_mut(), accessory.clone());
        if let Some(old) = old {
            old.removeFromSuperview();
        }
        if let Some(accessory) = accessory {
            accessory.setTranslatesAutoresizingMaskIntoConstraints(true);
            self.addSubview(&accessory);
        }
        self.setNeedsLayout(true);
    }

    pub fn top_accessory(&self) -> Option<Retained<NSView>> {
        self.ivars().top_accessory.borrow().clone()
    }

    pub fn set_top_accessory(&self, accessory: Option<Retained<NSView>>) {
        let old = std::mem::replace(&mut *self.ivars().top_accessory.borrow_mut(), accessory.clone());
        if let Some(old) = old {
            old.removeFromSuperview();
        }
        if let Some(accessory) = accessory {
            accessory.setTranslatesAutoresizingMaskIntoConstraints(true);
            self.addSubview(&accessory);
        }
        self.setNeedsLayout(true);
    }

    pub fn top_accessory_overlays_content(&self) -> bool {
        self.ivars().top_accessory_overlays_content.get()
    }

    pub fn set_top_accessory_overlays_content(&self, overlays: bool) {
        self.ivars().top_accessory_overlays_content.set(overlays);
        self.setNeedsLayout(true);
    }

    pub fn refresh_margin_notes(&self) {
        self.ivars().footnote_margin.setNeedsDisplay(true);
    }

    /// Height of the accessory's own lane above the scrolling content.
    pub fn top_lane_height(&self) -> CGFloat {
        if self.top_accessory_overlays_content() {
            return 0.0;
        }
        let height = self.top_accessory_height();
        if height > 0.0 { height + 8.0 } else { 0.0 }
    }

    fn top_accessory_height(&self) -> CGFloat {
        self.top_accessory().map_or(0.0, |accessory| {
            if accessory.isHidden() {
                return 0.0;
            }
            let height = accessory.fittingSize().height;
            if height > 0.0 { height } else { 24.0 }
        })
    }

    fn density_gutter_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"DensityGutterView")
    }

    fn is_density_map(view: &NSView) -> bool {
        Self::density_gutter_class().is_some_and(|class| view.isKindOfClass(class))
    }

    fn layout_subviews(&self) {
        let ivars = self.ivars();
        let text_view = &ivars.text_view;
        let scroll_view = &ivars.scroll_view;
        let bounds = self.bounds();
        let top_height = self.top_accessory_height();
        let top_lane_height = self.top_lane_height();
        let leading = self.leading_accessory();
        let trailing = self.trailing_accessory();
        let density_map = leading
            .clone()
            .filter(|view| Self::is_density_map(view))
            .or_else(|| trailing.clone().filter(|view| Self::is_density_map(view)));
        let has_leading_density_map = leading.as_ref().is_some_and(|view| Self::is_density_map(view));
        let leading_width = if has_leading_density_map {
            leading.as_ref().map_or(DENSITY_GUTTER_WIDTH, |view| view.fittingSize().width)
        } else {
            leading.as_ref().map_or(0.0, |view| {
                let width = view.fittingSize().width;
                if width > 0.0 { width } else { 24.0 }
            })
        };
        let trailing_width = trailing.as_ref().map_or(0.0, |view| {
            let width = view.fittingSize().width;
            if Self::is_density_map(view) {
                return if width > 0.0 { width } else { DENSITY_GUTTER_WIDTH };
            }
            if width > 0.0 { width } else { 14.0 }
        });
        let content_width = smax(0.0, bounds.width() - leading_width - trailing_width);
        let shows_note_lane = bounds.width() >= 1100.0 && text_view.mode() != RenderMode::Source;
        let note_lane: CGFloat = if shows_note_lane { 230.0 } else { 0.0 };
        let note_gap: CGFloat = if shows_note_lane { 24.0 } else { 0.0 };

        if let Some(leading) = &leading {
            leading.setFrame(rect(0.0, 0.0, leading_width, bounds.height()));
        }
        if let Some(trailing) = &trailing {
            trailing.setFrame(rect(bounds.width() - trailing_width, 0.0, trailing_width, bounds.height()));
        }

        scroll_view.setFrame(rect(leading_width, top_lane_height, content_width, smax(0.0, bounds.height() - top_lane_height)));

        let style_sheet = text_view.style_sheet();
        let rendered_target = smin(72.0, smax(68.0, style_sheet.theme.typography.measure_characters));
        let responsive_characters: CGFloat = if bounds.width() < 900.0 {
            66.0
        } else if bounds.width() > 1200.0 {
            72.0
        } else {
            rendered_target
        };
        let preferred_measure = style_sheet.average_character_width * responsive_characters;
        let overlay_thumb_width = NSScroller::scrollerWidthForControlSize_scrollerStyle(
            NSControlSize::Regular,
            NSScrollerStyle::Overlay,
            self.mtm(),
        );
        let bleed = render_metrics::CODE_BLEED;
        let available = smax(
            render_metrics::MINIMUM_PROSE_WIDTH + bleed,
            content_width - render_metrics::REVEAL_SLACK - overlay_thumb_width,
        );
        let column = smin(preferred_measure + bleed, available);
        let measure = column - bleed;
        text_view.apply_responsive_measure(column);
        let optical_left = smax(0.0, (bounds.width() - measure) / 2.0 - leading_width);
        let text_left = smax(self.gutter_width() + render_metrics::REVEAL_SLACK, optical_left);
        let column_origin = text_left - render_metrics::REVEAL_SLACK;
        text_view.setMinSize(NSSize::new(column + render_metrics::REVEAL_SLACK * 2.0, 0.0));
        scroll_view.setContentInsets(NSEdgeInsets { top: 0.0, left: column_origin, bottom: 0.0, right: 0.0 });

        let scroll_frame = scroll_view.frame();
        let text_origin = scroll_frame.min_x() + text_left;
        let note_x = text_origin + column + note_gap;
        let available_note_width = smax(0.0, bounds.width() - trailing_width - note_x - 16.0);
        let resolved_note_width = smin(note_lane, available_note_width);
        let footnote_margin = &ivars.footnote_margin;
        footnote_margin.setHidden(!shows_note_lane || resolved_note_width < 100.0);
        footnote_margin.setFrame(rect(note_x, scroll_frame.min_y(), resolved_note_width, scroll_frame.height()));
        if let Some(top) = self.top_accessory() {
            top.setFrame(rect(
                text_origin,
                if self.top_accessory_overlays_content() { 8.0 } else { 4.0 },
                smin(measure, smax(0.0, bounds.width() - text_origin - trailing_width)),
                smax(top_height, 0.0),
            ));
            if self.top_accessory_overlays_content() {
                self.addSubview_positioned_relativeTo(&top, NSWindowOrderingMode::Above, None);
            }
        }
        ivars.gutter.setFrame(rect(
            smax(0.0, text_origin - self.gutter_width()),
            top_lane_height,
            self.gutter_width(),
            scroll_frame.height(),
        ));

        if let Some(density_map) = density_map {
            density_map.setFrame(rect(
                if has_leading_density_map { 0.0 } else { bounds.width() - trailing_width },
                0.0,
                if has_leading_density_map { leading_width } else { trailing_width },
                bounds.height(),
            ));
            self.addSubview_positioned_relativeTo(&density_map, NSWindowOrderingMode::Above, None);
            let _: () = unsafe { msg_send![&*density_map, containerGeometryDidChange] };
        }

        if !footnote_margin.isHidden() {
            self.addSubview_positioned_relativeTo(footnote_margin, NSWindowOrderingMode::Above, Some(scroll_view));
            footnote_margin.setNeedsDisplay(true);
        }
    }
}

#[allow(dead_code)]
fn _class_type_in_scope() -> &'static AnyClass {
    NSView::class()
}
