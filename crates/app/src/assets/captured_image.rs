//! Port of `Assets/CapturedImage.swift`: turning a Continuity Camera capture
//! into a file next to the document and a Markdown reference to it.
//!
//! Everything here is a pure function over a pasteboard, a directory listing,
//! or a source string: the window controller decides *when* an image arrives,
//! this decides *what* gets written and *what text* the document gains. The
//! naming rules satisfy three consumers at once: `LocalAssetPolicy` (a bare
//! relative filename next to the document draws without a trust prompt),
//! `AssetReferenceParser` (a destination ends at a space, `#` or `?` and is
//! percent-decoded) and `AssetDoctor` (no absolute paths, supported formats,
//! alt text present).

use objc2::{AnyThread, Message};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPasteboard, NSPasteboardType, NSPasteboardTypePNG,
};
use objc2_foundation::{NSArray, NSDictionary, NSString};
use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift, ns::foundation};

use crate::assets::asset_resolver::ns_string;

/// Image bytes plus the extension they should be written under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payload {
    pub data: Vec<u8>,
    pub file_extension: String,
}

impl Payload {
    pub fn new(data: Vec<u8>, file_extension: impl Into<String>) -> Payload {
        Payload { data, file_extension: file_extension.into() }
    }
}

/// The text to insert, the zero-length source range to insert it at, and
/// where the caret lands afterwards (Swift's `(replacement:origin:caret:)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertionEdit {
    pub replacement: String,
    pub origin: isize,
    pub caret: isize,
}

/// `CapturedImage`.
pub struct CapturedImage;

impl CapturedImage {
    /// `passthroughTypes`: types taken verbatim, in preference order, as
    /// (pasteboard type, file extension). These three are exactly the
    /// camera-shaped formats in `AssetResolutionContext.supportedExtensions`.
    pub fn passthrough_types() -> [(Retained<NSPasteboardType>, &'static str); 3] {
        [
            // SAFETY: an AppKit constant string.
            (unsafe { NSPasteboardTypePNG }.retain(), "png"),
            (NSString::from_str("public.jpeg"), "jpg"),
            (NSString::from_str("public.heic"), "heic"),
        ]
    }

    /// `acceptsReturnType(_:)`: true for the return types this app can turn
    /// into a Markdown image. Used both to advertise the responder to AppKit
    /// and to decode.
    pub fn accepts_return_type(pasteboard_type: &str) -> bool {
        // `NSImage.imageTypes.contains(type.rawValue)`: Swift `String ==`.
        NSImage::imageTypes().iter().any(|image_type| swift::str_eq(&foundation::to_string(&image_type), pasteboard_type))
    }

    /// `payload(from:)`.
    pub fn payload(pasteboard: &NSPasteboard) -> Option<Payload> {
        objc2::rc::autoreleasepool(|_| {
            for (candidate_type, file_extension) in Self::passthrough_types() {
                if pasteboard.availableTypeFromArray(&NSArray::from_slice(&[&*candidate_type])).is_none() {
                    continue;
                }
                let Some(data) = pasteboard.dataForType(&candidate_type) else { continue };
                if data.length() == 0 {
                    continue;
                }
                return Some(Payload::new(data.to_vec(), file_extension));
            }
            // Anything else (TIFF from an older capture path, a PDF page from
            // a scan) is re-encoded to PNG rather than written under its own
            // extension: `.tiff` is not a supported format and no browser opens
            // a `.pdf` from an `![…]()`.
            let image = NSImage::initWithPasteboard(NSImage::alloc(), pasteboard)?;
            let tiff = image.TIFFRepresentation()?;
            let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)?;
            // SAFETY: an empty properties dictionary.
            let png = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
            Some(Payload::new(png.to_vec(), "png"))
        })
    }

    /// `slug(_:)`: a filename component that survives both the filesystem and
    /// `AssetReferenceParser`. Whitespace becomes `-`, the characters that
    /// would end or re-point the destination are dropped, non-ASCII letters
    /// are kept.
    pub fn slug(name: &str) -> String {
        let mut out = String::new();
        let mut last_was_dash = false;
        for scalar in name.chars() {
            let mut buffer = [0u8; 4];
            let character: &str = scalar.encode_utf8(&mut buffer);
            if swift::is_whitespace(character) || scalar == '_' {
                if !last_was_dash && !out.is_empty() {
                    out.push('-');
                    last_was_dash = true;
                }
                continue;
            }
            // `/ : \0` break the write; `# ? %` break the parse; the rest are
            // ordinary shell and Markdown hazards not worth carrying.
            if "/\\:#?%()[]<>|\"'*\u{0}".contains(scalar) {
                continue;
            }
            // `generalCategory == .control` (Cc).
            if scalar.is_control() {
                continue;
            }
            out.push(scalar);
            last_was_dash = false;
        }
        // `hasSuffix`, `removeLast()`, `hasPrefix` and `removeFirst()` work on
        // Characters.
        while swift::has_suffix(&out, "-") || swift::has_suffix(&out, ".") {
            out = swift::drop_last(&out, 1).to_owned();
        }
        while swift::has_prefix(&out, "-") || swift::has_prefix(&out, ".") {
            out = swift::drop_first(&out, 1).to_owned();
        }
        out
    }

    /// `uniqueFileName(stem:fileExtension:exists:)`: the first free
    /// `<stem>[-n].<ext>`. Numbered rather than timestamped so the name is
    /// deterministic, never reused so a second write cannot overwrite the
    /// first; past 999 the name carries a UUID. Shared with the drop path.
    pub fn unique_file_name(stem: &str, file_extension: &str, mut exists: impl FnMut(&str) -> bool) -> String {
        for index in 1..=999 {
            let candidate =
                if index == 1 { format!("{stem}.{file_extension}") } else { format!("{stem}-{index}.{file_extension}") };
            if !exists(&candidate) {
                return candidate;
            }
        }
        format!("{stem}-{}.{file_extension}", foundation_io::uuid_string())
    }

    /// `uniqueFileName(documentBaseName:fileExtension:exists:)`: the capture's
    /// own naming rule, `<document>-photo[-n].<ext>`.
    pub fn unique_file_name_for_document(
        document_base_name: &str,
        file_extension: &str,
        exists: impl FnMut(&str) -> bool,
    ) -> String {
        let base = Self::slug(document_base_name);
        let stem = if base.is_empty() { "photo".to_owned() } else { base + "-photo" };
        Self::unique_file_name(&stem, file_extension, exists)
    }

    /// `uniqueFileName(in:documentBaseName:fileExtension:)`: asks the file
    /// system whether each candidate exists.
    pub fn unique_file_name_in(directory: &FileUrl, document_base_name: &str, file_extension: &str) -> String {
        Self::unique_file_name_for_document(document_base_name, file_extension, |name| {
            foundation_io::file_exists(&directory.appending_path_component(name).path())
        })
    }

    /// `insertion(destination:altText:in:at:)`: the text to insert, padded
    /// into a paragraph of its own.
    pub fn insertion(destination: &str, alt_text: &str, source: &str, caret: isize) -> InsertionEdit {
        Self::insertion_block(&format!("![{alt_text}]({destination})"), source, caret)
    }

    /// `insertion(block:in:at:)`: the same padding rule for any block of
    /// generated Markdown. It counts the newlines already on each side, so
    /// dropping into an existing blank line adds nothing and dropping
    /// mid-sentence adds exactly two.
    pub fn insertion_block(block: &str, source: &str, caret: isize) -> InsertionEdit {
        let text = swift::ns::utf16(source);
        let length = text.len() as isize;
        let position = 0.max(caret).min(length);

        let mut leading_newlines = 0;
        let mut index = position - 1;
        while index >= 0 && leading_newlines < 2 && text[index as usize] == 0x0A {
            leading_newlines += 1;
            index -= 1;
        }
        let prefix = if position == 0 { String::new() } else { "\n".repeat(2 - leading_newlines) };

        let mut trailing_newlines = 0;
        index = position;
        while index < length && trailing_newlines < 2 && text[index as usize] == 0x0A {
            trailing_newlines += 1;
            index += 1;
        }
        let suffix = if position == length { String::new() } else { "\n".repeat(2 - trailing_newlines) };

        let head = prefix + block;
        InsertionEdit { caret: position + swift::utf16_count(&head), replacement: head + &suffix, origin: position }
    }

    /// `altText(forFileNamed:)`: never empty, so a capture never arrives with
    /// a missing-alt diagnostic.
    pub fn alt_text_for_file_named(file_name: &str) -> String {
        let stem = deleting_path_extension(file_name);
        if stem.is_empty() { "Photo".to_owned() } else { stem }
    }
}

/// `(name as NSString).deletingPathExtension`.
pub(crate) fn deleting_path_extension(name: &str) -> String {
    objc2::rc::autoreleasepool(|_| foundation::to_string(&ns_string(name).stringByDeletingPathExtension()))
}
