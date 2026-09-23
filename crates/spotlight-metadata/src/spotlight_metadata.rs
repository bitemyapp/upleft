//! Port of `Sources/DownrightSpotlightMetadata/SpotlightMetadata.swift`.
//!
//! Stable keys shared by Core Spotlight, the filesystem importer, and tests.
//! The importer emits only local facts derived from the file; it never
//! evaluates code fences or follows links.
//!
//! Parser-backed metadata extraction used by both the running app and the
//! background Spotlight importer. File IO stays at this boundary so the
//! parser remains the source of truth and tests can inject text directly.

use std::ffi::c_void;

use objc2_core_foundation::{CFArray, CFMutableDictionary, CFRetained, CFString};
use upleft_core::ParseOptions;
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::foundation_io::{FoundationError, cocoa_code};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift_text, CharSet};

/// `SpotlightMetadataKey`.
pub struct SpotlightMetadataKey;

impl SpotlightMetadataKey {
    pub const TITLE: &'static str = "kMDItemTitle";
    pub const TEXT_CONTENT: &'static str = "kMDItemTextContent";
    pub const KEYWORDS: &'static str = "kMDItemKeywords";
    pub const KIND: &'static str = "kMDItemKind";
    pub const CONTENT_TYPE: &'static str = "kMDItemContentType";
}

/// A value in [`SpotlightMetadata::attributes`] (`String` or `[String]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    String(String),
    Strings(Vec<String>),
}

impl AttributeValue {
    /// `as? String`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttributeValue::String(text) => Some(text),
            AttributeValue::Strings(_) => None,
        }
    }
}

/// `SpotlightMetadata`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotlightMetadata {
    pub title: String,
    pub text_content: String,
    pub keywords: Vec<String>,
    pub content_type: String,
}

impl SpotlightMetadata {
    pub fn new(title: String, text_content: String, keywords: Vec<String>, content_type: String) -> Self {
        SpotlightMetadata { title, text_content, keywords, content_type }
    }

    /// `attributes`: a `[String: Any]`, here as `(key, value)` pairs in
    /// declaration order.
    pub fn attributes(&self) -> Vec<(&'static str, AttributeValue)> {
        vec![
            (SpotlightMetadataKey::TITLE, AttributeValue::String(self.title.clone())),
            (SpotlightMetadataKey::TEXT_CONTENT, AttributeValue::String(self.text_content.clone())),
            (SpotlightMetadataKey::KEYWORDS, AttributeValue::Strings(self.keywords.clone())),
            (SpotlightMetadataKey::KIND, AttributeValue::String("Markdown document".into())),
            (SpotlightMetadataKey::CONTENT_TYPE, AttributeValue::String(self.content_type.clone())),
        ]
    }

    /// `attributes[key]`.
    pub fn attribute(&self, key: &str) -> Option<AttributeValue> {
        self.attributes().into_iter().find(|(k, _)| *k == key).map(|(_, value)| value)
    }
}

/// `SpotlightMetadataImporter`.
pub struct SpotlightMetadataImporter;

impl SpotlightMetadataImporter {
    pub const MARKDOWN_EXTENSIONS: [&'static str; 8] = ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"];

    /// `metadata(forText:url:)`.
    pub fn metadata_for_text(text: &str, url: &FileUrl) -> SpotlightMetadata {
        let document = MarkdownParser::parse_with(text, ParseOptions::STRUCTURE_ONLY);
        let fields = document.front_matter.as_ref().map(|front| front.fields.as_slice());
        let title = document
            .headings
            .first()
            .map(|heading| heading.title.clone())
            .or_else(|| {
                fields?
                    .iter()
                    .find(|field| swift_text::str_eq(&swift_text::lowercased(&field.key), "title"))
                    .map(|field| field.value.clone())
            })
            .unwrap_or_else(|| url.deleting_path_extension().last_path_component());
        let keywords: Vec<String> = fields
            .unwrap_or(&[])
            .iter()
            .filter(|field| {
                let key = swift_text::lowercased(&field.key);
                ["tag", "tags", "keyword", "keywords"].iter().any(|name| swift_text::str_eq(name, &key))
            })
            .flat_map(|field| {
                swift_text::split_default(swift_text::trimming(&field.value, CharSet::Chars("[]")), ',')
                    .into_iter()
                    .map(|part| swift_text::trimming(part, CharSet::WhitespacesAndNewlines).to_owned())
                    .collect::<Vec<_>>()
            })
            .filter(|keyword| !keyword.is_empty())
            .collect();
        SpotlightMetadata::new(title, text.to_owned(), keywords, Self::content_type(url).to_owned())
    }

    /// `metadata(at:)`: throws `CocoaError(.fileReadUnsupportedScheme)` for a
    /// URL it does not accept and `CocoaError(.fileReadCorruptFile)` when the
    /// head cannot be read (an empty file included).
    pub fn metadata_at(url: &FileUrl) -> Result<SpotlightMetadata, FoundationError> {
        if !Self::accepts(url) {
            return Err(FoundationError::cocoa(cocoa_code::FILE_READ_UNSUPPORTED_SCHEME));
        }
        let Some(head) = DocumentIO::read_head(std::path::Path::new(&url.path()), 2 * 1024 * 1024) else {
            return Err(FoundationError::cocoa(cocoa_code::FILE_READ_CORRUPT_FILE));
        };
        Ok(Self::metadata_for_text(&head, url))
    }

    /// `accepts(_:)` (a [`FileUrl`] is always a file URL).
    pub fn accepts(url: &FileUrl) -> bool {
        let extension = swift_text::lowercased(&url.path_extension());
        Self::MARKDOWN_EXTENSIONS.iter().any(|supported| swift_text::str_eq(supported, &extension))
    }

    /// `contentType(for:)`.
    pub fn content_type(url: &FileUrl) -> &'static str {
        let extension = swift_text::lowercased(&url.path_extension());
        if ["md", "markdown", "mdown", "mkd"].iter().any(|name| swift_text::str_eq(name, &extension)) {
            "net.daringfireball.markdown"
        } else {
            "com.ezzy.downright.markdown"
        }
    }
}

/// C-callable bridge used by the CFPlugIn shim. The importer process has no
/// AppKit or window-server dependency; it fills the dictionary handed to it
/// by Spotlight with values produced by the shared parser.
///
/// `extern bool DownrightSpotlightPopulateMetadata(CFMutableDictionaryRef
/// attributes, CFStringRef contentTypeUTI, CFStringRef pathToFile)`.
///
/// # Safety
///
/// `attributes` must be null or a mutable `CFDictionary` with `CFType`
/// callbacks, and `path_to_file` null or a `CFString`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DownrightSpotlightPopulateMetadata(
    attributes: *mut c_void,
    _content_type_uti: *const c_void,
    path_to_file: *const c_void,
) -> bool {
    if attributes.is_null() || path_to_file.is_null() {
        return false;
    }
    let attributes = unsafe { &*(attributes as *const CFMutableDictionary) };
    let path = unsafe { &*(path_to_file as *const CFString) }.to_string();
    let url = FileUrl::from_path(&path);
    let Ok(metadata) = SpotlightMetadataImporter::metadata_at(&url) else { return false };

    let keywords: Vec<CFRetained<CFString>> = metadata.keywords.iter().map(|keyword| cf_string(keyword)).collect();
    let keywords = CFArray::from_retained_objects(&keywords);
    let title = cf_string(&metadata.title);
    let text_content = cf_string(&metadata.text_content);
    let kind = cf_string("Markdown document");
    let values: [(Option<&'static CFString>, *const c_void); 4] = unsafe {
        [
            (objc2_core_services::kMDItemTitle, &*title as *const CFString as *const c_void),
            (objc2_core_services::kMDItemTextContent, &*text_content as *const CFString as *const c_void),
            (objc2_core_services::kMDItemKeywords, &*keywords as *const CFArray<CFString> as *const c_void),
            (objc2_core_services::kMDItemKind, &*kind as *const CFString as *const c_void),
        ]
    };
    for (key, value) in values {
        let Some(key) = key else { continue };
        unsafe { CFMutableDictionary::set_value(Some(attributes), key as *const CFString as *const c_void, value) };
    }
    true
}

/// `text as CFString`, built from UTF-16 units so that nothing (a leading
/// U+FEFF in particular) is dropped on the way.
fn cf_string(text: &str) -> CFRetained<CFString> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let string = swift_text::ns::foundation::ns_from_utf16(&units);
    // `NSString` is toll-free bridged to `CFString`.
    unsafe { CFRetained::retain(std::ptr::NonNull::from(&*string).cast::<CFString>()) }
}
