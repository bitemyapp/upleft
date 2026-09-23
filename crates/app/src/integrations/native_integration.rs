//! Port of `Sources/DownrightApp/Integrations/NativeIntegration.swift`.
//!
//! The small command surface shared by Services, App Intents, and future
//! share extensions. Integrations never reach into a window controller: they
//! resolve a file, then hand it to this one owner.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSPasteboardURLReadingFileURLsOnlyKey};
use objc2_foundation::{NSArray, NSDictionary, NSFileManager, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL};
use upleft_foundation::url::{self, FileUrl};
use upleft_swift_text as swift_text;

/// `IntegrationRegistry.OpenHandler`.
pub type OpenHandler = Rc<dyn Fn(&FileUrl)>;

/// `IntegrationRegistry` (main actor).
pub struct IntegrationRegistry {
    open_handler: RefCell<Option<OpenHandler>>,
}

thread_local! {
    static SHARED: Rc<IntegrationRegistry> = Rc::new(IntegrationRegistry::new(None));
}

fn file_exists(path: &str) -> bool {
    NSFileManager::defaultManager().fileExistsAtPath(&NSString::from_str(path))
}

impl IntegrationRegistry {
    /// `IntegrationRegistry.shared`.
    pub fn shared(_mtm: MainThreadMarker) -> Rc<IntegrationRegistry> {
        SHARED.with(Rc::clone)
    }

    /// `IntegrationRegistry(openHandler:)`.
    pub fn new(open_handler: Option<OpenHandler>) -> IntegrationRegistry {
        IntegrationRegistry { open_handler: RefCell::new(open_handler) }
    }

    pub fn open_handler(&self) -> Option<OpenHandler> {
        self.open_handler.borrow().clone()
    }

    pub fn set_open_handler(&self, handler: Option<OpenHandler>) {
        *self.open_handler.borrow_mut() = handler;
    }

    /// `open(_:)`: routes an existing Markdown file to the open handler.
    /// True only when a handler took it.
    pub fn open(&self, url: &FileUrl) -> bool {
        let file_url = url.standardized_file_url();
        if !(NativeIntegrationPolicy::accepts(&file_url) && file_exists(&file_url.path())) {
            return false;
        }
        let handler = self.open_handler();
        if let Some(handler) = &handler {
            handler(&file_url);
        }
        handler.is_some()
    }
}

/// Pure routing rules. Keeping these separate makes extension entry points
/// testable without starting AppKit or touching a user's files.
pub struct NativeIntegrationPolicy;

impl NativeIntegrationPolicy {
    /// `markdownExtensions` (a `Set`; order is not meaningful).
    pub const MARKDOWN_EXTENSIONS: [&'static str; 8] = ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"];

    fn accepts_extension(path_extension: &str) -> bool {
        let lowered = swift_text::lowercased(path_extension);
        Self::MARKDOWN_EXTENSIONS.iter().any(|candidate| swift_text::str_eq(&lowered, candidate))
    }

    /// `accepts(_:)` for a file URL (always `isFileURL`).
    pub fn accepts(url: &FileUrl) -> bool {
        Self::accepts_extension(&url.path_extension())
    }

    /// `accepts(_:)` for any URL: a non-file URL is refused.
    pub fn accepts_url(url: &NSURL) -> bool {
        if !url.isFileURL() {
            return false;
        }
        match FileUrl::from_nsurl(url) {
            Some(file_url) => Self::accepts(&file_url),
            None => false,
        }
    }

    /// `normalizedPath(_:)`: `~` expanded, made absolute and standardized;
    /// `None` unless it names a Markdown file.
    pub fn normalized_path(value: &str) -> Option<FileUrl> {
        let expanded = url::expanding_tilde_in_path(value);
        let file_url = FileUrl::from_path(&expanded).standardized_file_url();
        Self::accepts(&file_url).then_some(file_url)
    }
}

/// Input decoding for an AppKit Services provider. Finder sends file URLs;
/// text editors may send a path string, so support both without guessing at
/// arbitrary URLs or shell commands.
pub struct ServiceInputResolver;

impl ServiceInputResolver {
    pub fn urls(pasteboard: &NSPasteboard) -> Vec<FileUrl> {
        objc2::rc::autoreleasepool(|_| {
            let classes = NSArray::from_slice(&[NSURL::class()]);
            let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
                &[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }],
                &[&*NSNumber::new_bool(true) as &AnyObject],
            );
            let candidates = unsafe { pasteboard.readObjectsForClasses_options(&classes, Some(&options)) };
            let file_urls: Vec<FileUrl> = candidates
                .map(|objects| {
                    objects
                        .iter()
                        .filter_map(|object| object.downcast::<NSURL>().ok())
                        .filter_map(|url| FileUrl::from_nsurl(&url))
                        .filter(|url| NativeIntegrationPolicy::accepts(url) && file_exists(&url.path()))
                        .collect()
                })
                .unwrap_or_default();
            if !file_urls.is_empty() {
                return file_urls;
            }

            let Some(text) = pasteboard.stringForType(unsafe { NSPasteboardTypeString }) else { return Vec::new() };
            let text = swift_text::ns::foundation::to_string(&text);
            split_lines(&text)
                .into_iter()
                .filter_map(|line| NativeIntegrationPolicy::normalized_path(swift_text::trim_whitespaces(line)))
                .filter(|url| file_exists(&url.path()))
                .collect()
        })
    }
}

/// `text.split(whereSeparator: \.isNewline)`: Character-wise, empty lines
/// dropped.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for character in swift_text::graphemes(text) {
        if swift_text::is_newline(character) {
            if start < offset {
                lines.push(&text[start..offset]);
            }
            start = offset + character.len();
        }
        offset += character.len();
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

define_class!(
    /// Principal class for the optional macOS Services registration. The
    /// service itself is declared by the app's Info.plist; this class only
    /// handles the pasteboard and delegates opening to `IntegrationRegistry`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DownrightServicesProvider"]
    pub struct DownrightServicesProvider;

    unsafe impl NSObjectProtocol for DownrightServicesProvider {}

    impl DownrightServicesProvider {
        #[unsafe(method(openMarkdownInDownright:userData:error:))]
        fn open_markdown(&self, pasteboard: &NSPasteboard, _user_data: Option<&NSString>, error: *mut *mut NSString) {
            let urls = ServiceInputResolver::urls(pasteboard);
            if urls.is_empty() {
                set_error(error, "The selection does not contain a Markdown file.");
                return;
            }
            let registry = IntegrationRegistry::shared(self.mtm());
            let opened = urls.iter().filter(|url| registry.open(url)).count();
            if opened == 0 {
                set_error(error, "Upleft is not ready to open this file.");
            }
        }
    }
);

fn set_error(error: *mut *mut NSString, message: &str) {
    if error.is_null() {
        return;
    }
    unsafe { *error = Retained::autorelease_ptr(NSString::from_str(message)) };
}

impl DownrightServicesProvider {
    pub fn new(mtm: MainThreadMarker) -> Retained<DownrightServicesProvider> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }

    /// The class object, for `NSApp.servicesProvider` wiring and tests.
    pub fn class_object() -> &'static AnyClass {
        Self::class()
    }
}
