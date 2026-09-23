//! Port of `View/MarkdownSmartPaste.swift`: clipboard flavour policy for the
//! text surface, kept independent of `NSPasteboard` where it can be.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAttributedStringDocumentAttributeKey, NSAttributedStringDocumentReadingOptionKey, NSDocumentTypeDocumentAttribute,
    NSDocumentTypeDocumentOption, NSHTMLTextDocumentType, NSPasteboard, NSPasteboardType, NSPasteboardTypeFileURL,
    NSPasteboardTypeHTML, NSPasteboardTypePNG, NSPasteboardTypeRTF, NSPasteboardTypeRTFD, NSPasteboardTypeString,
    NSPasteboardTypeTIFF, NSPasteboardTypeURL, NSRTFDTextDocumentType, NSRTFTextDocumentType,
};
use objc2_foundation::{
    NSAttributedString, NSData, NSDictionary, NSPropertyListSerialization, NSString, NSStringEncoding,
};
use upleft_core::smart_paste::SmartPaste;
use upleft_core::{BlockContent, MDBlock, ParsedDocument};

use crate::core_types::{InlineKind, NSRange};
use crate::render_contracts::RenderMode;

/// The context in which a paste is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownPasteContext {
    Markdown,
    Code,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownPasteMode {
    Smart,
    Markdown,
    MatchStyle,
}

/// A clipboard payload independent of `NSPasteboard`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownPastePayload {
    Markdown(String),
    Url(String),
    Html(String, String),
    RichText(String),
    File(String),
    Image,
    Text(String),
}

/// `NSPasteboard.PasteboardType.downrightMarkdown`.
pub fn downright_markdown_type() -> Retained<NSString> {
    NSString::from_str("com.ezzy.downright.markdown")
}

/// `NSPasteboard.PasteboardType.webArchive`.
pub fn web_archive_type() -> Retained<NSString> {
    NSString::from_str("Apple Web Archive pasteboard type")
}

/// `NSPasteboard.PasteboardType.appleWebArchive`.
pub fn apple_web_archive_type() -> Retained<NSString> {
    NSString::from_str("com.apple.webarchive")
}

const MAXIMUM_RICH_PAYLOAD_BYTES: usize = 8 * 1_024 * 1_024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavour {
    DownrightMarkdown,
    String,
    Url,
    FileUrl,
    Html,
    Rtf,
    Rtfd,
    WebArchive,
    AppleWebArchive,
    Tiff,
    Png,
}

impl Flavour {
    fn pasteboard_type(self) -> Retained<NSPasteboardType> {
        // SAFETY: AppKit exports the pasteboard types as immutable globals.
        unsafe {
            match self {
                Flavour::DownrightMarkdown => downright_markdown_type(),
                Flavour::String => NSPasteboardTypeString.retain_static(),
                Flavour::Url => NSPasteboardTypeURL.retain_static(),
                Flavour::FileUrl => NSPasteboardTypeFileURL.retain_static(),
                Flavour::Html => NSPasteboardTypeHTML.retain_static(),
                Flavour::Rtf => NSPasteboardTypeRTF.retain_static(),
                Flavour::Rtfd => NSPasteboardTypeRTFD.retain_static(),
                Flavour::WebArchive => web_archive_type(),
                Flavour::AppleWebArchive => apple_web_archive_type(),
                Flavour::Tiff => NSPasteboardTypeTIFF.retain_static(),
                Flavour::Png => NSPasteboardTypePNG.retain_static(),
            }
        }
    }
}

trait RetainStatic {
    fn retain_static(&self) -> Retained<NSString>;
}

impl RetainStatic for NSString {
    fn retain_static(&self) -> Retained<NSString> {
        use objc2::Message;
        self.retain()
    }
}

pub struct MarkdownSmartPaste;

impl MarkdownSmartPaste {
    /// Ordinary paste is conservative: lossless Downright Markdown wins, then
    /// the producer's visible plain text. Rich conversion belongs to Paste as
    /// Markdown; Match Style also prefers visible text.
    pub fn payload(pasteboard: &NSPasteboard, mode: MarkdownPasteMode) -> Option<MarkdownPastePayload> {
        let rich = [Flavour::Html, Flavour::Rtf, Flavour::Rtfd, Flavour::WebArchive, Flavour::AppleWebArchive];
        let mut ordered: Vec<Flavour> = Vec::new();
        match mode {
            MarkdownPasteMode::Smart => {
                ordered.extend([Flavour::DownrightMarkdown, Flavour::String, Flavour::Url, Flavour::FileUrl]);
                ordered.extend(rich);
                ordered.extend([Flavour::Tiff, Flavour::Png]);
            }
            MarkdownPasteMode::Markdown => {
                ordered.push(Flavour::DownrightMarkdown);
                ordered.extend(rich);
                ordered.extend([Flavour::Url, Flavour::FileUrl, Flavour::String, Flavour::Tiff, Flavour::Png]);
            }
            MarkdownPasteMode::MatchStyle => {
                ordered.extend([Flavour::String, Flavour::DownrightMarkdown, Flavour::Url, Flavour::FileUrl]);
                ordered.extend(rich);
                ordered.extend([Flavour::Tiff, Flavour::Png]);
            }
        }
        let available = pasteboard.types();
        let string_for = |flavour: Flavour| -> Option<String> {
            pasteboard.stringForType(&flavour.pasteboard_type()).map(|string| string.to_string())
        };
        for flavour in ordered {
            let pasteboard_type = flavour.pasteboard_type();
            if !available.as_ref().is_some_and(|types| types.containsObject(&pasteboard_type)) {
                continue;
            }
            match flavour {
                Flavour::DownrightMarkdown => {
                    if let Some(markdown) = string_for(flavour) {
                        return Some(MarkdownPastePayload::Markdown(markdown));
                    }
                }
                Flavour::Url => {
                    if let Some(url) = string_for(flavour)
                        && !url.is_empty()
                    {
                        return Some(MarkdownPastePayload::Url(url));
                    }
                }
                Flavour::Html => {
                    let Some(html) = string_for(flavour) else { continue };
                    return Some(MarkdownPastePayload::Html(html, string_for(Flavour::String).unwrap_or_default()));
                }
                Flavour::Rtf | Flavour::Rtfd => {
                    let Some(data) = pasteboard.dataForType(&pasteboard_type) else { continue };
                    if data.length() > MAXIMUM_RICH_PAYLOAD_BYTES {
                        continue;
                    }
                    let Some(attributed) = read_rich_text(&data, flavour == Flavour::Rtf) else { continue };
                    if let Some(html) = html_of(&attributed) {
                        return Some(MarkdownPastePayload::Html(html, attributed.string().to_string()));
                    }
                    return Some(MarkdownPastePayload::RichText(attributed.string().to_string()));
                }
                Flavour::WebArchive | Flavour::AppleWebArchive => {
                    if let Some(html) = web_archive_html(pasteboard.dataForType(&pasteboard_type).as_deref()) {
                        return Some(MarkdownPastePayload::Html(html, string_for(Flavour::String).unwrap_or_default()));
                    }
                }
                Flavour::FileUrl => {
                    if let Some(raw) = string_for(flavour)
                        && let Some(path) = file_url_path(&raw)
                    {
                        return Some(MarkdownPastePayload::File(path));
                    }
                }
                Flavour::Tiff | Flavour::Png => {
                    // Embedding requires a document-owned file name.
                    return Some(MarkdownPastePayload::Image);
                }
                Flavour::String => {
                    if let Some(text) = string_for(flavour) {
                        return Some(MarkdownPastePayload::Text(text));
                    }
                }
            }
        }
        None
    }

    pub fn replacement(
        payload: &MarkdownPastePayload,
        selection: &str,
        context: MarkdownPasteContext,
        mode: MarkdownPasteMode,
    ) -> String {
        if mode == MarkdownPasteMode::MatchStyle {
            return Self::plain_text(payload);
        }
        if mode == MarkdownPasteMode::Smart && context != MarkdownPasteContext::Markdown {
            if let MarkdownPastePayload::Html(html, fallback) = payload {
                return if fallback.is_empty() { html.clone() } else { fallback.clone() };
            }
            return Self::plain_text(payload);
        }
        match payload {
            MarkdownPastePayload::Markdown(markdown) => markdown.clone(),
            MarkdownPastePayload::Url(url) => SmartPaste::linkified(selection, url).unwrap_or_else(|| url.clone()),
            MarkdownPastePayload::Html(html, fallback) => {
                let markdown = SmartPaste::markdown_for_html(html);
                if markdown.is_empty() { fallback.clone() } else { markdown }
            }
            MarkdownPastePayload::RichText(text) => text.clone(),
            MarkdownPastePayload::File(path) => path.clone(),
            MarkdownPastePayload::Image => String::new(),
            MarkdownPastePayload::Text(text) => {
                SmartPaste::markdown_table_for_tab_separated(text).unwrap_or_else(|| text.clone())
            }
        }
    }

    fn plain_text(payload: &MarkdownPastePayload) -> String {
        match payload {
            MarkdownPastePayload::Markdown(markdown) => markdown.clone(),
            MarkdownPastePayload::Url(url) => url.clone(),
            MarkdownPastePayload::Html(html, fallback) => {
                if fallback.is_empty() {
                    SmartPaste::plain_text_for_html(html)
                } else {
                    fallback.clone()
                }
            }
            MarkdownPastePayload::RichText(text) => text.clone(),
            MarkdownPastePayload::File(path) => path.clone(),
            MarkdownPastePayload::Image => String::new(),
            MarkdownPastePayload::Text(text) => text.clone(),
        }
    }

    /// A range is literal when it intersects a code block or an inline
    /// literal; a caret at a block boundary belongs to the following block.
    pub fn context(range: NSRange, document: &ParsedDocument, mode: RenderMode) -> MarkdownPasteContext {
        if mode == RenderMode::Source {
            return MarkdownPasteContext::Plain;
        }
        let flattened = document.root.flattened();
        let mut literal_inline_ranges: Vec<NSRange> = Vec::new();
        for block in &flattened {
            for inline in &block.inlines {
                inline.walk(&mut |span| {
                    if matches!(span.kind, InlineKind::InlineCode | InlineKind::InlineMath { .. }) {
                        literal_inline_ranges.push(span.range);
                    }
                });
            }
        }
        let intersects_literal_inline = literal_inline_ranges.iter().any(|literal| {
            if range.length == 0 {
                literal.contains(range.location)
            } else {
                literal.location < range.upper_bound() && range.location < literal.upper_bound()
            }
        });
        if intersects_literal_inline {
            return MarkdownPasteContext::Plain;
        }

        let blocks: Vec<&std::sync::Arc<MDBlock>> = flattened
            .iter()
            .filter(|block| block.range.length > 0 && !matches!(block.content, BlockContent::Document))
            .collect();
        let relevant: Vec<&std::sync::Arc<MDBlock>> = if range.length == 0 {
            let containing: Vec<&&std::sync::Arc<MDBlock>> =
                blocks.iter().filter(|block| block.range.contains(range.location)).collect();
            // `min(by:)` keeps the first of equal minima.
            let smallest = containing.iter().fold(None::<&&std::sync::Arc<MDBlock>>, |best, block| match best {
                Some(best) if !(block.range.length < best.range.length) => Some(best),
                _ => Some(block),
            });
            let chosen = smallest.copied().or_else(|| {
                blocks
                    .iter()
                    .filter(|block| block.range.location >= range.location)
                    .fold(None::<&&std::sync::Arc<MDBlock>>, |best, block| match best {
                        Some(best) if !(block.range.location < best.range.location) => Some(best),
                        _ => Some(block),
                    })
            });
            chosen.into_iter().copied().collect()
        } else {
            blocks
                .iter()
                .filter(|block| block.range.location < range.upper_bound() && range.location < block.range.upper_bound())
                .copied()
                .collect()
        };
        let mut context = MarkdownPasteContext::Markdown;
        for block in relevant {
            match block.content {
                BlockContent::CodeBlock { .. } => return MarkdownPasteContext::Code,
                BlockContent::HtmlBlock
                | BlockContent::FrontMatter(_)
                | BlockContent::Mermaid { .. }
                | BlockContent::MathBlock { .. } => context = MarkdownPasteContext::Plain,
                _ => {}
            }
        }
        context
    }
}

/// `NSAttributedString(data:options:[.documentType: rtf|rtfd])`.
fn read_rich_text(data: &NSData, rtf: bool) -> Option<Retained<NSAttributedString>> {
    // SAFETY: AppKit exports the option keys and document types as globals.
    let (key, value) = unsafe {
        let key: &NSAttributedStringDocumentReadingOptionKey = NSDocumentTypeDocumentOption;
        let value: &NSString = if rtf { NSRTFTextDocumentType } else { NSRTFDTextDocumentType };
        (key, value)
    };
    let options = NSDictionary::<NSAttributedStringDocumentReadingOptionKey, AnyObject>::from_slices(&[key], &[value.as_ref()]);
    // SAFETY: the options dictionary holds the documented key and value types.
    unsafe {
        use objc2_app_kit::NSAttributedStringDocumentFormats;
        NSAttributedString::initWithData_options_documentAttributes_error(
            NSAttributedString::alloc(),
            data,
            &options,
            std::ptr::null_mut(),
        )
    }
    .ok()
}

/// `attributed.data(from:documentAttributes: [.documentType: .html])` as UTF-8.
fn html_of(attributed: &NSAttributedString) -> Option<String> {
    use objc2_app_kit::NSAttributedStringDocumentFormats;
    // SAFETY: AppKit exports the attribute key and document type as globals.
    let (key, value) = unsafe {
        let key: &NSAttributedStringDocumentAttributeKey = NSDocumentTypeDocumentAttribute;
        let value: &NSString = NSHTMLTextDocumentType;
        (key, value)
    };
    let attributes = NSDictionary::<NSAttributedStringDocumentAttributeKey, AnyObject>::from_slices(&[key], &[value.as_ref()]);
    let data = attributed
        .dataFromRange_documentAttributes_error(objc2_foundation::NSRange::new(0, attributed.length()), &attributes)
        .ok()?;
    String::from_utf8(data.to_vec()).ok()
}

/// `WebMainResource.WebResourceData` out of a web archive, UTF-8 then UTF-16.
fn web_archive_html(data: Option<&NSData>) -> Option<String> {
    let data = data?;
    if data.length() > MAXIMUM_RICH_PAYLOAD_BYTES {
        return None;
    }
    // SAFETY: a null format pointer is allowed.
    let plist = unsafe {
        NSPropertyListSerialization::propertyListWithData_options_format_error(
            data,
            objc2_foundation::NSPropertyListMutabilityOptions(0),
            std::ptr::null_mut(),
        )
    }
    .ok()?;
    let plist = plist.downcast::<NSDictionary>().ok()?;
    let resource = plist.objectForKey(&NSString::from_str("WebMainResource"))?.downcast::<NSDictionary>().ok()?;
    let html_data = resource.objectForKey(&NSString::from_str("WebResourceData"))?.downcast::<NSData>().ok()?;
    let decode = |encoding: NSStringEncoding| -> Option<String> {
        NSString::initWithData_encoding(NSString::alloc(), &html_data, encoding).map(|string| string.to_string())
    };
    decode(objc2_foundation::NSUTF8StringEncoding).or_else(|| decode(objc2_foundation::NSUTF16StringEncoding))
}

/// `URL(string: raw)` that `isFileURL`, as its `path`.
fn file_url_path(raw: &str) -> Option<String> {
    let url = objc2_foundation::NSURL::URLWithString(&NSString::from_str(raw))?;
    if !url.isFileURL() {
        return None;
    }
    url.path().map(|path| path.to_string())
}
