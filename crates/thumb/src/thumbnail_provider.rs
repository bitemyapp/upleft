//! Port of `Sources/DownrightThumb/ThumbnailProvider.swift`.
//!
//! Real Finder icons for markdown files (§10).
//!
//! Nobody does this, and on a folder full of agent output it is
//! transformative: twelve files called `plan.md`, `output.md`, and
//! `summary.md` become twelve distinguishable documents, because the icon
//! shows the first heading.
//!
//! Runs under the same memory ceiling as the preview extension, so it parses
//! with `ParseOptions::STRUCTURE_ONLY` and never touches math, mermaid, or
//! images.
//!
//! `ThumbnailProvider` is a `define_class!` subclass of QuickLookThumbnailing's
//! `QLThumbnailProvider`, named as in Swift. There is no objc2 binding for
//! QuickLookThumbnailing, so the three classes it uses are declared here.

// Framework bindings keep the Objective-C selector names.
#![allow(non_snake_case)]

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{AnyThread, define_class, extern_class, extern_methods, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSFont, NSFontWeightMedium, NSFontWeightSemibold, NSGraphicsContext,
    NSLayoutManager, NSLineBreakMode, NSMutableParagraphStyle, NSStringDrawing, NSTextContainer, NSTextStorage,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSCocoaErrorDomain, NSDictionary, NSError, NSString, NSURL};
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_core::{BlockContent, ParseOptions, ParsedDocument};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{RectExt, keys, ns_string, rect};
use upleft_swift_text as swift_text;

// QuickLookThumbnailing defines `QLThumbnailProvider` and the request and
// reply classes; the superclass must be loaded before `ThumbnailProvider`
// registers.
#[link(name = "QuickLookThumbnailing", kind = "framework")]
unsafe extern "C" {}

extern_class!(
    /// `QLThumbnailProvider`.
    #[unsafe(super(NSObject))]
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct QLThumbnailProvider;
);

extern_class!(
    /// `QLFileThumbnailRequest`.
    #[unsafe(super(NSObject))]
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct QLFileThumbnailRequest;
);

impl QLFileThumbnailRequest {
    extern_methods!(
        #[unsafe(method(maximumSize))]
        #[unsafe(method_family = none)]
        pub fn maximumSize(&self) -> CGSize;

        #[unsafe(method(minimumSize))]
        #[unsafe(method_family = none)]
        pub fn minimumSize(&self) -> CGSize;

        #[unsafe(method(scale))]
        #[unsafe(method_family = none)]
        pub fn scale(&self) -> CGFloat;

        #[unsafe(method(fileURL))]
        #[unsafe(method_family = none)]
        pub fn fileURL(&self) -> Retained<NSURL>;
    );
}

extern_class!(
    /// `QLThumbnailReply`.
    #[unsafe(super(NSObject))]
    #[derive(Debug, PartialEq, Eq, Hash)]
    pub struct QLThumbnailReply;
);

impl QLThumbnailReply {
    extern_methods!(
        /// `QLThumbnailReply(contextSize:currentContextDrawing:)`.
        #[unsafe(method(replyWithContextSize:currentContextDrawingBlock:))]
        #[unsafe(method_family = none)]
        pub fn replyWithContextSize_currentContextDrawingBlock(
            context_size: CGSize,
            drawing_block: &DynBlock<dyn Fn() -> Bool>,
        ) -> Retained<Self>;
    );
}

/// `CocoaError.Code.fileReadCorruptFile`.
const FILE_READ_CORRUPT_FILE: isize = 259;

define_class!(
    /// `final class ThumbnailProvider: QLThumbnailProvider`.
    // SAFETY: no ivars; the override keeps QLThumbnailProvider's signature.
    #[unsafe(super(QLThumbnailProvider, NSObject))]
    #[name = "ThumbnailProvider"]
    pub struct ThumbnailProvider;

    unsafe impl NSObjectProtocol for ThumbnailProvider {}

    impl ThumbnailProvider {
        #[unsafe(method(provideThumbnailForFileRequest:completionHandler:))]
        fn __provide_thumbnail(
            &self,
            request: &QLFileThumbnailRequest,
            handler: &DynBlock<dyn Fn(*mut QLThumbnailReply, *mut NSError)>,
        ) {
            self.provide_thumbnail(request, handler);
        }
    }
);

/// A task tally, `(done: Int, total: Int)`.
pub type Tasks = (isize, isize);

impl ThumbnailProvider {
    /// `ThumbnailProvider()`.
    pub fn new() -> Retained<ThumbnailProvider> {
        // SAFETY: NSObject's initialiser; the class has no ivars.
        unsafe { msg_send![ThumbnailProvider::alloc(), init] }
    }

    /// `provideThumbnail(for:_:)`.
    pub fn provide_thumbnail(
        &self,
        request: &QLFileThumbnailRequest,
        handler: &DynBlock<dyn Fn(*mut QLThumbnailReply, *mut NSError)>,
    ) {
        let file_url = request.fileURL();
        // Only the head of the file matters for a thumbnail, and reading
        // 64KB instead of a 4MB agent transcript is the difference between an
        // icon that appears and one that doesn't.
        let head = file_url
            .path()
            .and_then(|path| DocumentIO::read_head(std::path::Path::new(&path.to_string()), 64 * 1024));
        let Some(head) = head else {
            // SAFETY: `NSCocoaErrorDomain` is an immutable Foundation global.
            let error = unsafe { NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, FILE_READ_CORRUPT_FILE, None) };
            handler.call((std::ptr::null_mut(), Retained::as_ptr(&error) as *mut NSError));
            return;
        };

        let document = MarkdownParser::parse_with(&head, ParseOptions::STRUCTURE_ONLY);
        let title = document
            .headings
            .first()
            .map(|heading| heading.title.clone())
            .or_else(|| document.front_matter.as_ref().and_then(|front| front.get("title")).map(str::to_owned))
            .unwrap_or_else(|| {
                FileUrl::from_nsurl(&file_url)
                    .map(|url| url.deleting_path_extension().last_path_component())
                    .unwrap_or_default()
            });
        let subtitle = ThumbnailProvider::first_prose_line(&document).unwrap_or_default();
        let task_count = document.tasks.len() as isize;
        let done_count = document.tasks.iter().filter(|task| task.is_checked).count() as isize;

        // A page, not a square. `maximumSize` is a bounding box, and handing
        // it back unchanged is what made these icons squares sitting among
        // every other app's portrait documents — the single thing that made a
        // folder of them look wrong before you had read a word.
        let size = ThumbnailProvider::page_size(request.maximumSize());

        // This renderer uses AppKit text and paths, so request Quick Look's
        // AppKit current-context variant. The Core Graphics overload applies
        // the request's Retina scale before our NSGraphicsContext bridge;
        // bridging it again made Finder show only the bottom-left quarter.
        let tasks = if task_count > 0 { Some((done_count, task_count)) } else { None };
        let drawing = RcBlock::new(move || -> Bool {
            ThumbnailProvider::draw(&title, &subtitle, tasks, size);
            Bool::YES
        });
        let reply = QLThumbnailReply::replyWithContextSize_currentContextDrawingBlock(size, &drawing);
        handler.call((Retained::as_ptr(&reply) as *mut QLThumbnailReply, std::ptr::null_mut()));
    }

    // MARK: - Geometry

    /// US Letter, near enough. Every document icon on the system is this
    /// shape and an icon that isn't reads as a mistake long before it reads
    /// as a file.
    const PAGE_ASPECT: CGFloat = 8.5 / 11.0;

    /// `pageSize(fitting:)`.
    pub fn page_size(maximum: CGSize) -> CGSize {
        if !(maximum.width > 0.0 && maximum.height > 0.0) {
            return maximum;
        }
        let tall = CGSize::new(maximum.height * Self::PAGE_ASPECT, maximum.height);
        if !(tall.width > maximum.width) {
            return tall;
        }
        CGSize::new(maximum.width, maximum.width / Self::PAGE_ASPECT)
    }

    /// Below this the page is a few points wide per line of type and real
    /// text is a grey smudge, so the icon switches to ruled lines that at
    /// least read as a document. Finder's list and column views live down
    /// here; icon view and Cover Flow live above it.
    const LEGIBLE_TEXT_HEIGHT: CGFloat = 72.0;

    // MARK: - Palette
    //
    // Fixed sRGB, never `labelColor` or `textBackgroundColor`. A thumbnail is
    // drawn once by a background process with no appearance context and then
    // cached by the system for months; a dynamic colour resolves to whatever
    // that process happened to be and bakes it in, so a light icon can end
    // up permanently stuck in a dark Finder or the reverse. Document icons
    // are artwork — Pages and TextEdit are white pages in both appearances
    // too.

    fn srgb(red: CGFloat, green: CGFloat, blue: CGFloat, alpha: CGFloat) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, alpha)
    }

    /// The warm paper of the app icon, not a clinical white.
    fn paper() -> Retained<NSColor> {
        Self::srgb(0.980, 0.973, 0.957, 1.0)
    }
    fn page_edge() -> Retained<NSColor> {
        Self::srgb(0.0, 0.0, 0.0, 0.12)
    }
    fn spine() -> Retained<NSColor> {
        Self::srgb(0.141, 0.251, 0.373, 1.0)
    }
    fn title_ink() -> Retained<NSColor> {
        Self::srgb(0.106, 0.153, 0.200, 1.0)
    }
    fn body_ink() -> Retained<NSColor> {
        Self::srgb(0.431, 0.463, 0.506, 1.0)
    }
    fn faint_ink() -> Retained<NSColor> {
        Self::srgb(0.604, 0.631, 0.667, 1.0)
    }
    fn accent() -> Retained<NSColor> {
        Self::srgb(0.290, 0.498, 0.757, 1.0)
    }

    // MARK: - Drawing

    /// `draw(title:subtitle:tasks:size:)`, into the current graphics context.
    pub fn draw(title: &str, subtitle: &str, tasks: Option<Tasks>, size: CGSize) {
        NSGraphicsContext::saveGraphicsState_class();
        Self::draw_page(title, subtitle, tasks, size);
        // `defer { NSGraphicsContext.restoreGraphicsState() }`.
        NSGraphicsContext::restoreGraphicsState_class();
    }

    fn draw_page(title: &str, subtitle: &str, tasks: Option<Tasks>, size: CGSize) {
        let radius = size.width * 0.055;
        let page = rect(0.0, 0.0, size.width, size.height).inset_by(0.5, 0.5);
        let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(page, radius, radius);
        Self::paper().setFill();
        outline.fill();

        // The spine is clipped to the page so it follows the rounded corners
        // instead of squaring them off, which the old full-height rect did.
        NSGraphicsContext::saveGraphicsState_class();
        outline.addClip();
        Self::spine().setFill();
        NSBezierPath::bezierPathWithRect(rect(
            page.min_x(),
            page.min_y(),
            smax(1.5, size.width * 0.035),
            page.height(),
        ))
        .fill();
        NSGraphicsContext::restoreGraphicsState_class();

        Self::page_edge().setStroke();
        outline.setLineWidth(1.0);
        outline.stroke();

        let left_inset = smax(3.0, size.width * 0.035) + size.width * 0.085;
        let right_inset = size.width * 0.085;
        let content = rect(
            page.min_x() + left_inset,
            page.min_y() + size.height * 0.075,
            page.width() - left_inset - right_inset,
            page.height() - size.height * 0.15,
        );

        if !(size.height >= Self::LEGIBLE_TEXT_HEIGHT) {
            Self::draw_ruled_lines(content, tasks, size);
            return;
        }

        let mut cursor = content.max_y();
        let title_size = smax(7.0, size.height * 0.098);
        // Each block advances the cursor by the height it actually used, so a
        // one-line title no longer leaves a three-line hole above the body.
        // SAFETY: `NSFontWeightSemibold` is an immutable AppKit global.
        let title_height = Self::draw_text(
            title,
            &NSFont::systemFontOfSize_weight(title_size, unsafe { NSFontWeightSemibold }),
            &Self::title_ink(),
            rect(content.min_x(), content.min_y(), content.width(), cursor - content.min_y()),
            cursor,
            3,
        );
        cursor -= title_height;

        let badge_reserve = if tasks.is_none() { 0.0 } else { size.height * 0.11 };
        if !subtitle.is_empty() {
            cursor -= size.height * 0.045;
            let available = cursor - content.min_y() - badge_reserve;
            let subtitle_size = smax(6.0, size.height * 0.058);
            if available >= subtitle_size * 1.2 {
                // `.systemFont(ofSize:)`: the regular weight.
                let _ = Self::draw_text(
                    subtitle,
                    &NSFont::systemFontOfSize(subtitle_size),
                    &Self::body_ink(),
                    rect(content.min_x(), content.min_y() + badge_reserve, content.width(), available),
                    cursor,
                    4,
                );
            }
        }

        // A plan's completion state is the single most useful thing an icon
        // can say about agent output (§8.5), and a bar says it before the eye
        // has resolved the digits.
        if let Some(tasks) = tasks {
            Self::draw_task_badge(tasks, content, size);
        }
    }

    /// Draws `text` with its top edge at `top_at` and returns the height used.
    fn draw_text(text: &str, font: &NSFont, color: &NSColor, bounds: CGRect, top_at: CGFloat, lines: usize) -> CGFloat {
        if !(bounds.height() > 0.0) {
            return 0.0;
        }
        // Word wrapping on the paragraph, truncation on the container. A
        // paragraph style set to `.byTruncatingTail` never wraps at all — it
        // lays the whole string on one line and clips it — which is why these
        // icons only ever showed one line of title and one line of body no
        // matter how much page was left underneath.
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        paragraph.setMaximumLineHeight(font.pointSize() * 1.26);
        paragraph.setLineSpacing(0.0);

        let attributes = NSDictionary::<NSString, AnyObject>::from_slices(
            &[keys::font(), keys::foreground_color(), keys::paragraph_style()],
            &[font as &AnyObject, color as &AnyObject, &*paragraph as &AnyObject],
        );
        // SAFETY: every value is an object of the type its key expects.
        let storage: Retained<NSTextStorage> = unsafe {
            msg_send![NSTextStorage::alloc(), initWithString: &*ns_string(text), attributes: Some(&*attributes)]
        };
        let container = NSTextContainer::initWithSize(NSTextContainer::alloc(), CGSize::new(bounds.width(), bounds.height()));
        container.setMaximumNumberOfLines(lines);
        container.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        container.setLineFragmentPadding(0.0);
        let layout = NSLayoutManager::new();
        layout.addTextContainer(&container);
        storage.addLayoutManager(&layout);

        let glyphs = layout.glyphRangeForTextContainer(&container);
        let used = layout.usedRectForTextContainer(&container);

        // NSLayoutManager lays text out top-down — line 0 sits at the
        // smallest y of its own coordinate space — so drawing it straight
        // into this unflipped page puts the first line at the bottom and the
        // paragraph reads backwards. One line hides the fault entirely, which
        // is how it survived; give the glyphs a flipped context of their own.
        let previous = NSGraphicsContext::currentContext();
        let Some(cg_context) = previous.as_ref().map(|previous| previous.CGContext()) else {
            return used.size.height;
        };
        let context: Option<&CGContext> = Some(&cg_context);
        CGContext::save_g_state(context);
        CGContext::translate_ctm(context, 0.0, top_at);
        CGContext::scale_ctm(context, 1.0, -1.0);
        NSGraphicsContext::setCurrentContext(Some(&NSGraphicsContext::graphicsContextWithCGContext_flipped(
            &cg_context,
            true,
        )));
        layout.drawGlyphsForGlyphRange_atPoint(glyphs, CGPoint::new(bounds.min_x(), 0.0));
        NSGraphicsContext::setCurrentContext(previous.as_deref());
        CGContext::restore_g_state(context);

        used.size.height
    }

    /// The small-size fallback: proportioned rules that read as a page of
    /// text.
    fn draw_ruled_lines(content: CGRect, tasks: Option<Tasks>, size: CGSize) {
        let rule = smax(1.0, size.height * 0.055);
        let gap = rule * 0.85;
        let mut y = content.max_y() - rule;

        Self::spine().setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            rect(content.min_x(), y, content.width() * 0.82, rule),
            rule / 2.0,
            rule / 2.0,
        )
        .fill();

        // Ragged widths so it reads as prose rather than a barcode.
        let widths: [CGFloat; 4] = [0.95, 0.72, 0.88, 0.6];
        Self::faint_ink().setFill();
        for width in widths {
            y -= rule + gap;
            if !(y >= content.min_y()) {
                break;
            }
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                rect(content.min_x(), y, content.width() * width, rule),
                rule / 2.0,
                rule / 2.0,
            )
            .fill();
        }

        let Some(tasks) = tasks else { return };
        if !(tasks.1 > 0) {
            return;
        }
        Self::accent().setFill();
        let bar_height = smax(1.0, size.height * 0.04);
        let fraction = tasks.0 as CGFloat / tasks.1 as CGFloat;
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            rect(content.min_x(), content.min_y(), content.width() * fraction, bar_height),
            bar_height / 2.0,
            bar_height / 2.0,
        )
        .fill();
    }

    fn draw_task_badge(tasks: Tasks, content: CGRect, size: CGSize) {
        if !(tasks.1 > 0) {
            return;
        }
        let bar_height = smax(1.5, size.height * 0.022);
        let bar_width = content.width() * 0.5;
        let bar = rect(content.min_x(), content.min_y(), bar_width, bar_height);

        Self::faint_ink().colorWithAlphaComponent(0.35).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bar, bar_height / 2.0, bar_height / 2.0).fill();

        let fraction = smin(1.0, tasks.0 as CGFloat / tasks.1 as CGFloat);
        Self::accent().setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            rect(bar.min_x(), bar.min_y(), smax(bar_height, bar_width * fraction), bar_height),
            bar_height / 2.0,
            bar_height / 2.0,
        )
        .fill();

        let label = ns_string(&format!("{}/{}", tasks.0, tasks.1));
        // SAFETY: `NSFontWeightMedium` is an immutable AppKit global.
        let font = NSFont::systemFontOfSize_weight(smax(5.0, size.height * 0.05), unsafe { NSFontWeightMedium });
        let faint_ink = Self::faint_ink();
        let attributes = NSDictionary::<NSString, AnyObject>::from_slices(
            &[keys::font(), keys::foreground_color()],
            &[&*font as &AnyObject, &*faint_ink as &AnyObject],
        );
        // SAFETY: every value is an object of the type its key expects.
        let width = unsafe { label.sizeWithAttributes(Some(&attributes)) }.width;
        // SAFETY: as above.
        unsafe {
            label.drawAtPoint_withAttributes(
                CGPoint::new(
                    smin(bar.max_x() + size.width * 0.04, content.max_x() - width),
                    bar.min_y() - font.pointSize() * 0.36,
                ),
                Some(&attributes),
            )
        };
    }

    // MARK: - Reading

    /// `firstProseLine(in:)`.
    pub fn first_prose_line(document: &ParsedDocument) -> Option<String> {
        for block in &document.root.children {
            if !matches!(block.content, BlockContent::Paragraph) {
                continue;
            }
            let text = document.substring(block.content_range);
            let text = swift_text::trim_whitespaces_and_newlines(&text);
            if !text.is_empty() {
                return Some(text.to_owned());
            }
        }
        None
    }
}

/// `Swift.min(x, y)` for floating point: `y < x ? y : x`.
fn smin(x: CGFloat, y: CGFloat) -> CGFloat {
    if y < x { y } else { x }
}

/// `Swift.max(x, y)` for floating point: `y >= x ? y : x`.
fn smax(x: CGFloat, y: CGFloat) -> CGFloat {
    if y >= x { y } else { x }
}
