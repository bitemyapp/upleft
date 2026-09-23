//! Tests for `ThumbnailProvider` (Downright has no Swift tests for the
//! thumbnail extension). They run headless: the reply's drawing block is
//! drawn into a bitmap context, as Quick Look's thumbnail host does, with no
//! window and no Quick Look registration.
//!
//! Quick Look creates `QLFileThumbnailRequest`s itself; the tests use
//! `TestThumbnailRequest`, a subclass that answers `fileURL` and
//! `maximumSize`. The reply's context size and drawing block are read with
//! QLThumbnailReply's own getters (`contextSize`, `drawingBlock`), which the
//! conformance harness uses too.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, Bool, NSObject, NSObjectProtocol};
use objc2::{AnyThread, ClassType, DefinedClass, define_class, msg_send};
use objc2_app_kit::NSGraphicsContext;
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextGetData, CGColorSpace, CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSCocoaErrorDomain, NSError, NSString, NSURL};
use upleft_core::parser::MarkdownParser;
use upleft_thumb::thumbnail_provider::{QLFileThumbnailRequest, QLThumbnailReply, ThumbnailProvider};

// MARK: - Harness

pub struct TestThumbnailRequestIvars {
    url: Retained<NSURL>,
    maximum_size: CGSize,
}

define_class!(
    // SAFETY: the ivars are set before `init`; the overrides keep the
    // superclass's getter signatures.
    #[unsafe(super(QLFileThumbnailRequest, NSObject))]
    #[name = "UpleftTestThumbnailRequest"]
    #[ivars = TestThumbnailRequestIvars]
    struct TestThumbnailRequest;

    impl TestThumbnailRequest {
        #[unsafe(method_id(fileURL))]
        fn file_url(&self) -> Retained<NSURL> {
            self.ivars().url.clone()
        }

        #[unsafe(method(maximumSize))]
        fn maximum_size(&self) -> CGSize {
            self.ivars().maximum_size
        }

        #[unsafe(method(minimumSize))]
        fn minimum_size(&self) -> CGSize {
            CGSize::new(0.0, 0.0)
        }

        #[unsafe(method(scale))]
        fn scale(&self) -> CGFloat {
            2.0
        }
    }
);

fn request(path: &Path, maximum_size: CGSize) -> Retained<QLFileThumbnailRequest> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
    let this = TestThumbnailRequest::alloc().set_ivars(TestThumbnailRequestIvars { url, maximum_size });
    let this: Retained<TestThumbnailRequest> = unsafe { msg_send![super(this), init] };
    Retained::into_super(this)
}

/// What the completion handler received.
struct Outcome {
    reply: Option<Retained<QLThumbnailReply>>,
    error: Option<Retained<NSError>>,
}

fn provide(path: &Path, maximum_size: CGSize) -> Outcome {
    let provider = ThumbnailProvider::new();
    let request = request(path, maximum_size);
    let outcome: std::rc::Rc<RefCell<Option<Outcome>>> = std::rc::Rc::new(RefCell::new(None));
    let sink = outcome.clone();
    let handler = RcBlock::new(move |reply: *mut QLThumbnailReply, error: *mut NSError| {
        let reply = unsafe { Retained::retain(reply) };
        let error = unsafe { Retained::retain(error) };
        *sink.borrow_mut() = Some(Outcome { reply, error });
    });
    // Through the Objective-C method, as Quick Look calls it.
    let handler_ref: &DynBlock<dyn Fn(*mut QLThumbnailReply, *mut NSError)> = &handler;
    let _: () = unsafe { msg_send![&*provider, provideThumbnailForFileRequest: &*request, completionHandler: handler_ref] };
    outcome.borrow_mut().take().expect("the handler is called before provideThumbnail returns")
}

fn context_size(reply: &QLThumbnailReply) -> CGSize {
    unsafe { msg_send![reply, contextSize] }
}

/// Draws `draw` into an sRGB RGBA bitmap of `size` points at `scale`, with
/// an unflipped AppKit context current, and returns the bytes.
fn render(size: CGSize, scale: CGFloat, draw: impl FnOnce() -> bool) -> (bool, usize, usize, Vec<u8>) {
    let width = (size.width * scale).ceil() as usize;
    let height = (size.height * scale).ceil() as usize;
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).unwrap();
    let context = unsafe {
        CGBitmapContextCreate(std::ptr::null_mut(), width, height, 8, 0, Some(&space), CGImageAlphaInfo::PremultipliedLast.0)
    }
    .unwrap();
    CGContext::scale_ctm(Some(&context), scale, scale);
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, false)));
    let drew = draw();
    NSGraphicsContext::setCurrentContext(previous.as_deref());
    let bytes_per_row = objc2_core_graphics::CGBitmapContextGetBytesPerRow(Some(&context));
    let data = CGBitmapContextGetData(Some(&context)) as *const u8;
    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let start = unsafe { data.add(row * bytes_per_row) };
        pixels.extend_from_slice(unsafe { std::slice::from_raw_parts(start, width * 4) });
    }
    (drew, width, height, pixels)
}

fn render_reply(reply: &QLThumbnailReply, scale: CGFloat) -> (bool, usize, usize, Vec<u8>) {
    let size = context_size(reply);
    let block: *mut DynBlock<dyn Fn() -> Bool> = unsafe { msg_send![reply, drawingBlock] };
    assert!(!block.is_null(), "a current-context reply carries its drawing block");
    render(size, scale, || unsafe { &*block }.call(()).as_bool())
}

/// The pixel at (x, y), counted from the top-left, as RGBA.
fn pixel(image: &(bool, usize, usize, Vec<u8>), x: usize, y: usize) -> [u8; 4] {
    let offset = (y * image.1 + x) * 4;
    [image.3[offset], image.3[offset + 1], image.3[offset + 2], image.3[offset + 3]]
}

fn near(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected.iter()).all(|(a, b)| (*a as i32 - *b as i32).abs() <= 2)
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> TemporaryDirectory {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "upleft-thumb-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TemporaryDirectory(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PAPER: [u8; 4] = [250, 248, 244, 255];
const SPINE: [u8; 4] = [36, 64, 95, 255];

// MARK: - Tests

#[test]
fn page_size_is_a_portrait_letter_page_inside_the_bounding_box() {
    let aspect = 8.5 / 11.0;
    let square = ThumbnailProvider::page_size(CGSize::new(256.0, 256.0));
    assert_eq!(square, CGSize::new(256.0 * aspect, 256.0));
    let wide = ThumbnailProvider::page_size(CGSize::new(1024.0, 64.0));
    assert_eq!(wide, CGSize::new(64.0 * aspect, 64.0));
    let narrow = ThumbnailProvider::page_size(CGSize::new(100.0, 400.0));
    assert_eq!(narrow, CGSize::new(100.0, 100.0 / aspect));
    assert_eq!(ThumbnailProvider::page_size(CGSize::new(0.0, 300.0)), CGSize::new(0.0, 300.0));
    assert_eq!(ThumbnailProvider::page_size(CGSize::new(300.0, -1.0)), CGSize::new(300.0, -1.0));
}

#[test]
fn first_prose_line_is_the_first_non_empty_top_level_paragraph() {
    let document = MarkdownParser::parse("# Title\n\n```\ncode\n```\n\n> quoted\n\n   Body text here.  \nsecond line\n\nLater.\n");
    assert_eq!(ThumbnailProvider::first_prose_line(&document).as_deref(), Some("Body text here.  \nsecond line"));
    let none = MarkdownParser::parse("# Only a heading\n\n- a list item\n");
    assert_eq!(ThumbnailProvider::first_prose_line(&none), None);
}

#[test]
fn provider_class_is_registered_under_the_swift_name() {
    // The extension's `main` registers the class before NSExtensionMain
    // looks up `NSExtensionPrincipalClass` by name.
    let registered = ThumbnailProvider::class();
    assert_eq!(registered.name(), c"ThumbnailProvider");
    let class = AnyClass::get(c"ThumbnailProvider").expect("NSClassFromString(\"ThumbnailProvider\")");
    assert!(std::ptr::eq(class, ThumbnailProvider::class()));
    assert_eq!(class.superclass().unwrap().name(), c"QLThumbnailProvider");
    // Quick Look instantiates the principal class from Objective-C.
    let instance: Retained<NSObject> = unsafe { msg_send![class, new] };
    assert!(instance.isKindOfClass(ThumbnailProvider::class()));
}

#[test]
fn missing_file_reports_a_corrupt_file_error_and_no_reply() {
    let directory = TemporaryDirectory::new();
    let outcome = provide(&directory.0.join("absent.md"), CGSize::new(256.0, 256.0));
    assert!(outcome.reply.is_none());
    let error = outcome.error.expect("an error");
    assert_eq!(error.code(), 259);
    assert_eq!(&*error.domain(), unsafe { NSCocoaErrorDomain });
}

#[test]
fn empty_file_reports_a_corrupt_file_error() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("empty.md", "");
    let outcome = provide(&path, CGSize::new(256.0, 256.0));
    assert!(outcome.reply.is_none());
    assert_eq!(outcome.error.expect("an error").code(), 259);
}

#[test]
fn reply_is_page_sized_and_draws_the_page() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("plan.md", "# The plan\n\nFirst we read the code.\n\n- [x] read\n- [ ] write\n");
    let outcome = provide(&path, CGSize::new(256.0, 256.0));
    assert!(outcome.error.is_none());
    let reply = outcome.reply.expect("a reply");
    assert_eq!(context_size(&reply), ThumbnailProvider::page_size(CGSize::new(256.0, 256.0)));
    let image = render_reply(&reply, 2.0);
    assert!(image.0, "the drawing block reports success");
    // The spine runs down the left edge, clipped to the rounded page; paper
    // fills the right-hand margin.
    assert!(near(pixel(&image, 4, image.2 / 2), SPINE), "spine {:?}", pixel(&image, 4, image.2 / 2));
    assert!(near(pixel(&image, image.1 - 12, image.2 / 2), PAPER), "paper {:?}", pixel(&image, image.1 - 12, image.2 / 2));
    // The rounded corner leaves the bitmap's corner transparent.
    assert_eq!(pixel(&image, 0, 0)[3], 0);
}

/// The icon's title is the first heading, else the front matter's `title`,
/// else the file name without its extension: each reply draws exactly what
/// `draw` draws for that title.
#[test]
fn title_falls_back_from_heading_to_front_matter_to_file_name() {
    let directory = TemporaryDirectory::new();
    let size = CGSize::new(256.0, 256.0);
    let page = ThumbnailProvider::page_size(size);
    let cases = [
        ("heading.md", "---\ntitle: From front matter\n---\n\n# From heading\n\nBody.\n", "From heading"),
        ("front.md", "---\ntitle: From front matter\n---\n\nBody.\n", "From front matter"),
        ("summary.notes.md", "Body.\n", "summary.notes"),
    ];
    for (name, text, title) in cases {
        let path = directory.write(name, text);
        let reply = provide(&path, size).reply.expect("a reply");
        let drawn = render_reply(&reply, 2.0);
        let expected = render(page, 2.0, || {
            ThumbnailProvider::draw(title, "Body.", None, page);
            true
        });
        assert!(drawn.3 == expected.3, "{name}: the reply draws the title {title:?}");
    }
}

#[test]
fn task_badge_reflects_completion() {
    let size = CGSize::new(180.0, 240.0);
    let none = render(size, 1.0, || {
        ThumbnailProvider::draw("Tasks", "Body", None, size);
        true
    });
    let half = render(size, 1.0, || {
        ThumbnailProvider::draw("Tasks", "Body", Some((1, 2)), size);
        true
    });
    let all = render(size, 1.0, || {
        ThumbnailProvider::draw("Tasks", "Body", Some((2, 2)), size);
        true
    });
    assert!(none.3 != half.3);
    assert!(half.3 != all.3);
}

#[test]
fn small_sizes_draw_ruled_lines_instead_of_text() {
    // Below 72pt of height the page is ruled, so the title does not matter.
    let size = ThumbnailProvider::page_size(CGSize::new(64.0, 64.0));
    let a = render(size, 2.0, || {
        ThumbnailProvider::draw("One title", "Body", None, size);
        true
    });
    let b = render(size, 2.0, || {
        ThumbnailProvider::draw("Another, much longer title", "Different body", None, size);
        true
    });
    assert!(a.3 == b.3);
    // At legible sizes the title shows.
    let size = ThumbnailProvider::page_size(CGSize::new(256.0, 256.0));
    let c = render(size, 1.0, || {
        ThumbnailProvider::draw("One title", "Body", None, size);
        true
    });
    let d = render(size, 1.0, || {
        ThumbnailProvider::draw("Another, much longer title", "Body", None, size);
        true
    });
    assert!(c.3 != d.3);
}
