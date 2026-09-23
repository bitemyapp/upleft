//! Port of `Assets/AssetResolver.swift`: finding the image destinations of a
//! parsed document and resolving them to file URLs, without touching the disk
//! (file access is injected through [`AssetProbe`]).
//!
//! How the Swift maps:
//!
//! * `URL` is [`FileUrl`] wherever the value is a file URL, which is every URL
//!   this file produces: `resolve` returns a URL only for the three local
//!   kinds, always `standardizedFileURL`.
//! * `URL(string:)` (classification of `data:`, `http(s):` and `file:`
//!   destinations) is `-[NSURL URLWithString:]`. Probed on macOS 26 (Swift
//!   6.4, both binaries stamped with the SDK): over 400,000 random
//!   destinations built from URL-hostile pieces, `URL(string:)` and
//!   `NSURL(string:)` agree on `nil`, on `host == nil`, on `path.isEmpty` and,
//!   for `file://`, on `standardizedFileURL.path` and `hasDirectoryPath` once
//!   the `NSURL`'s path goes through `URL(fileURLWithPath:isDirectory:)`. (Their
//!   only difference is `path` on opaque URLs, `nil` from `NSURL` and the
//!   opaque part from `URL`, which `classify` never reads.)
//! * `String.removingPercentEncoding` is `-[NSString
//!   stringByRemovingPercentEncoding]` (300,000 random probes, no
//!   difference).
//! * `NSString` searches (`range(of: "]:")`, `range(of: "]")`) are
//!   Foundation's non-literal search, called through objc2 when the text is
//!   not ASCII; on ASCII text they are the literal search they reduce to.
//! * `source[..<colon].contains("/")` is Foundation's search when `source` is
//!   bridged from the document's `NSString` (a non-ASCII document) and
//!   Character-wise when it is a native string (probed: `"a/\u{200D}b"`
//!   contains `"/"` bridged, not native). A destination parsed at the use site
//!   is a substring of the document; an image's own source is the parser's
//!   native string; a link reference definition's destination is native
//!   unless it is the definition's untrimmed body (see
//!   `definition_is_untrimmed_body`).

use std::sync::Arc;

use objc2::rc::Retained;
use objc2_foundation::{NSString, NSStringCompareOptions, NSURL};
use upleft_core::inlines::StringSet;
use upleft_core::{InlineKind, LinkReference, NS_NOT_FOUND, NSRange, ParsedDocument};
use upleft_foundation::url::{FileUrl, expanding_tilde_in_path};
use upleft_swift_text::{self as swift, ns::NSStringExt, ns::foundation};

/// The source form of a Markdown image destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetReferenceKind {
    RelativeLocal,
    AbsoluteLocal,
    FileUrl,
    RemoteHttp,
    DataUrl,
    Unsafe,
    Malformed,
}

impl AssetReferenceKind {
    pub fn raw_value(&self) -> &'static str {
        match self {
            AssetReferenceKind::RelativeLocal => "relativeLocal",
            AssetReferenceKind::AbsoluteLocal => "absoluteLocal",
            AssetReferenceKind::FileUrl => "fileURL",
            AssetReferenceKind::RemoteHttp => "remoteHTTP",
            AssetReferenceKind::DataUrl => "dataURL",
            AssetReferenceKind::Unsafe => "unsafe",
            AssetReferenceKind::Malformed => "malformed",
        }
    }
}

/// `AssetReference`.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetReference {
    pub source: String,
    /// Exact source characters covered by `destination_range`. For a
    /// reference image this is the definition destination, not its use label.
    pub source_text: String,
    pub destination_range: NSRange,
    pub image_range: NSRange,
    pub alt_text: String,
    pub title: Option<String>,
    pub kind: AssetReferenceKind,
    pub url: Option<FileUrl>,
    pub line: isize,
}

/// `AssetMetadata`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetMetadata {
    pub exists: bool,
    pub is_directory: bool,
    pub byte_size: Option<i64>,
    pub file_extension: Option<String>,
    pub content_identity: Option<String>,
}

impl AssetMetadata {
    /// `init(exists:isDirectory:byteSize:fileExtension:)` (`contentIdentity`
    /// defaults to `nil`).
    pub fn new(exists: bool, is_directory: bool, byte_size: Option<i64>, file_extension: Option<String>) -> AssetMetadata {
        AssetMetadata { exists, is_directory, byte_size, file_extension, content_identity: None }
    }

    /// `init(…, contentIdentity:)`.
    pub fn with_content_identity(mut self, content_identity: Option<String>) -> AssetMetadata {
        self.content_identity = content_identity;
        self
    }
}

/// `AssetProbe.metadata`'s type (`@Sendable (URL) -> AssetMetadata?`).
pub type AssetMetadataProvider = dyn Fn(&FileUrl) -> Option<AssetMetadata> + Send + Sync;

/// `AssetProbe`: file access is injected. Asset analysis itself does not
/// touch the disk.
#[derive(Clone)]
pub struct AssetProbe {
    metadata: Arc<AssetMetadataProvider>,
}

impl AssetProbe {
    /// `AssetProbe(metadata:)`.
    pub fn new(metadata: impl Fn(&FileUrl) -> Option<AssetMetadata> + Send + Sync + 'static) -> AssetProbe {
        AssetProbe { metadata: Arc::new(metadata) }
    }

    /// `probe.metadata(url)`.
    pub fn metadata(&self, url: &FileUrl) -> Option<AssetMetadata> {
        (self.metadata)(url)
    }
}

impl std::fmt::Debug for AssetProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AssetProbe")
    }
}

/// `AssetResolutionContext`.
#[derive(Clone, Debug)]
pub struct AssetResolutionContext {
    pub document_url: Option<FileUrl>,
    /// Standardized when the context is made.
    pub workspace_root: Option<FileUrl>,
    pub maximum_bytes: i64,
    /// A Swift `Set<String>`: membership by canonical equivalence.
    pub supported_extensions: StringSet,
}

impl AssetResolutionContext {
    /// `10 * 1024 * 1024`.
    pub const DEFAULT_MAXIMUM_BYTES: i64 = 10 * 1024 * 1024;
    pub const DEFAULT_SUPPORTED_EXTENSIONS: [&'static str; 8] = ["png", "jpg", "jpeg", "gif", "svg", "webp", "heic", "avif"];

    /// `init(documentURL:workspaceRoot:)` with the default limit and formats.
    pub fn new(document_url: Option<FileUrl>, workspace_root: Option<FileUrl>) -> AssetResolutionContext {
        Self::with_limits(
            document_url,
            workspace_root,
            Self::DEFAULT_MAXIMUM_BYTES,
            Self::DEFAULT_SUPPORTED_EXTENSIONS.iter().map(|extension| (*extension).to_owned()).collect(),
        )
    }

    /// `init(documentURL:workspaceRoot:maximumBytes:supportedExtensions:)`.
    pub fn with_limits(
        document_url: Option<FileUrl>,
        workspace_root: Option<FileUrl>,
        maximum_bytes: i64,
        supported_extensions: StringSet,
    ) -> AssetResolutionContext {
        AssetResolutionContext {
            document_url,
            workspace_root: workspace_root.map(|root| root.standardized_file_url()),
            maximum_bytes,
            supported_extensions,
        }
    }

    /// `init(documentURL:workspaceRoot:maximumBytes:)` with the default formats.
    pub fn with_maximum_bytes(
        document_url: Option<FileUrl>,
        workspace_root: Option<FileUrl>,
        maximum_bytes: i64,
    ) -> AssetResolutionContext {
        let mut context = Self::new(document_url, workspace_root);
        context.maximum_bytes = maximum_bytes;
        context
    }
}

impl Default for AssetResolutionContext {
    /// `AssetResolutionContext()`.
    fn default() -> AssetResolutionContext {
        AssetResolutionContext::new(None, None)
    }
}

/// `AssetReferenceParser`.
pub struct AssetReferenceParser;

/// `AssetReferenceParser.Destination`.
struct Destination {
    source: String,
    range: NSRange,
    title: Option<String>,
    reference_identifier: Option<String>,
    /// Whether `source` is bridged from the document's `NSString` (see the
    /// module notes); decides how `classify` searches it.
    bridged: bool,
}

impl AssetReferenceParser {
    /// `AssetReferenceParser.references(in:context:)`.
    pub fn references(document: &ParsedDocument, context: &AssetResolutionContext) -> Vec<AssetReference> {
        let mut output: Vec<AssetReference> = Vec::new();
        let text = document.utf16.as_slice();
        // Substrings of the document are bridged exactly when it holds a
        // non-ASCII character (see the module notes).
        let bridged = swift::bridges_substrings(text);
        document.root.walk(&mut |block| {
            for inline in &block.inlines {
                inline.walk(&mut |span| {
                    let InlineKind::Image { source, alt } = &span.kind else { return };
                    let Some(parsed) = Self::parse_destination(text, span.range, source, bridged) else { return };
                    let resolved = Self::resolved_destination(parsed, document, bridged);
                    let kind = Self::classify_with(&resolved.source, resolved.bridged);
                    output.push(AssetReference {
                        source_text: text.substring(resolved.range),
                        destination_range: resolved.range,
                        image_range: span.range,
                        alt_text: alt.clone(),
                        title: resolved.title,
                        kind,
                        url: Self::resolve(&resolved.source, kind, context),
                        source: resolved.source,
                        line: document.line_at(span.range.location),
                    });
                });
            }
        });
        output
    }

    fn resolved_destination(parsed: Destination, document: &ParsedDocument, document_bridges: bool) -> Destination {
        let Some(identifier) = parsed.reference_identifier.as_deref() else { return parsed };
        let Some(definition) = swift::dict_get(&document.link_references, &swift::lowercased(identifier)) else {
            return parsed;
        };
        let Some(definition_range) = Self::definition_destination_range(definition, document.utf16.as_slice()) else {
            return parsed;
        };
        Destination {
            source: definition.destination.clone(),
            range: definition_range,
            title: definition.title.clone(),
            reference_identifier: Some(identifier.to_owned()),
            bridged: document_bridges && Self::definition_is_untrimmed_body(definition, document.utf16.as_slice()),
        }
    }

    /// Whether MarkdownCore's `destinationAndTitle(_:)` handed back the
    /// definition's body itself, a substring of the document's `NSString`
    /// (so bridged when the document is not ASCII). It does exactly when
    /// nothing was trimmed and no title followed, that is when the destination
    /// is the whole rest of the line after `]:`. Otherwise its `String(…)` and
    /// trimming made a native string.
    fn definition_is_untrimmed_body(definition: &LinkReference, text: &[u16]) -> bool {
        let destination = swift::ns::utf16(&definition.destination);
        let upper = definition.range.upper_bound();
        let start = upper - destination.len() as isize;
        start - 2 >= definition.range.location
            && upper <= text.length()
            && text[start as usize..upper as usize] == destination[..]
            && text.character_at(start - 1) == 0x3A
            && text.character_at(start - 2) == 0x5D
    }

    fn definition_destination_range(definition: &LinkReference, text: &[u16]) -> Option<NSRange> {
        let line = &text[definition.range.as_usize_range()];
        let close = range_of(line, "]:", NSRange::new(0, line.length()));
        if close.location == NS_NOT_FOUND {
            return None;
        }
        let mut start = close.location + close.length;
        while start < line.length() && (line.character_at(start) == 0x20 || line.character_at(start) == 0x09) {
            start += 1;
        }
        if start >= line.length() {
            return None;
        }
        let mut end;
        if line.character_at(start) == 0x3C {
            start += 1;
            end = start;
            while end < line.length() && line.character_at(end) != 0x3E {
                end += 1;
            }
            if end >= line.length() {
                return None;
            }
        } else {
            end = start;
            while end < line.length() {
                let character = line.character_at(end);
                if character == 0x20 || character == 0x09 {
                    break;
                }
                end += 1;
            }
        }
        if end <= start {
            return None;
        }
        Some(NSRange::new(definition.range.location + start, end - start))
    }

    fn parse_destination(text: &[u16], image_range: NSRange, fallback_source: &str, bridged: bool) -> Option<Destination> {
        let raw = &text[image_range.as_usize_range()];
        // `rawString.hasPrefix("![")` is Character-wise. Whether a Character
        // boundary follows the `[` depends only on the next scalar, which the
        // first four units hold.
        if !swift::has_prefix(&swift::ns::string_from_utf16(&raw[..raw.len().min(4)]), "![") {
            return None;
        }
        let mut index: isize = 2;
        let mut depth = 1;
        while index < raw.length() {
            let character = raw.character_at(index);
            if character == 0x5C {
                index += 2;
                continue;
            }
            if character == 0x5B {
                depth += 1;
            }
            if character == 0x5D {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            index += 1;
        }
        if index >= raw.length() {
            return None;
        }
        let mut cursor = index + 1;
        while cursor < raw.length() && raw.character_at(cursor) == 0x20 {
            cursor += 1;
        }
        if !(cursor < raw.length() && raw.character_at(cursor) == 0x28) {
            // Reference images do not carry a destination at the use site.
            // Keep the label until `resolved_destination` maps it to the exact
            // destination in the shared definition.
            let label_range = range_of(raw, "]", NSRange::new(cursor, raw.length() - cursor));
            if cursor < raw.length() && raw.character_at(cursor) == 0x5B && label_range.location != NS_NOT_FOUND {
                let range = NSRange::new(image_range.location + cursor, label_range.location + 1 - cursor);
                let identifier = raw.substring(NSRange::new(cursor + 1, label_range.location - cursor - 1));
                return Some(Destination {
                    source: fallback_source.to_owned(),
                    range,
                    title: None,
                    reference_identifier: Some(identifier),
                    // The image's own source comes from the parser.
                    bridged: false,
                });
            }
            return None;
        }
        cursor += 1;
        while cursor < raw.length() && raw.character_at(cursor) == 0x20 {
            cursor += 1;
        }
        let destination_start = cursor;
        if cursor < raw.length() && raw.character_at(cursor) == 0x3C {
            cursor += 1;
            let angle_destination_start = cursor;
            while cursor < raw.length() && raw.character_at(cursor) != 0x3E {
                cursor += 1;
            }
            let destination_end = cursor;
            let source = raw.substring(NSRange::new(angle_destination_start, destination_end - angle_destination_start));
            if source.is_empty() {
                return None;
            }
            return Some(Destination {
                source,
                range: NSRange::new(image_range.location + angle_destination_start, destination_end - angle_destination_start),
                title: Self::parse_title(raw, cursor + 1),
                reference_identifier: None,
                bridged,
            });
        }
        let mut nesting = 0;
        while cursor < raw.length() {
            let character = raw.character_at(cursor);
            if character == 0x5C {
                cursor += 2;
                continue;
            }
            if character == 0x28 {
                nesting += 1;
            }
            if character == 0x29 {
                if nesting == 0 {
                    break;
                }
                nesting -= 1;
            }
            if nesting == 0 && (character == 0x20 || character == 0x09) {
                break;
            }
            cursor += 1;
        }
        let destination_end = cursor;
        let source = raw.substring(NSRange::new(destination_start, destination_end - destination_start));
        if source.is_empty() {
            return None;
        }
        let destination_range = NSRange::new(image_range.location + destination_start, destination_end - destination_start);
        Some(Destination {
            source,
            range: destination_range,
            title: Self::parse_title(raw, cursor),
            reference_identifier: None,
            bridged,
        })
    }

    fn parse_title(raw: &[u16], cursor: isize) -> Option<String> {
        let mut index = cursor;
        while index < raw.length() && (raw.character_at(index) == 0x20 || raw.character_at(index) == 0x09) {
            index += 1;
        }
        if index >= raw.length() {
            return None;
        }
        let quote = raw.character_at(index);
        if !(quote == 0x22 || quote == 0x27) {
            return None;
        }
        index += 1;
        let start = index;
        while index < raw.length() && raw.character_at(index) != quote {
            if raw.character_at(index) == 0x5C {
                index += 2;
            } else {
                index += 1;
            }
        }
        if index >= raw.length() {
            return None;
        }
        Some(raw.substring(NSRange::new(start, index - start)))
    }

    /// `AssetReferenceParser.classify(_:)` for a native Swift string.
    pub fn classify(source: &str) -> AssetReferenceKind {
        Self::classify_with(source, false)
    }

    /// `classify(_:)`; `bridged` says whether `source` is bridged from an
    /// `NSString`, which changes how `contains` searches it.
    fn classify_with(source: &str, bridged: bool) -> AssetReferenceKind {
        if source.is_empty() || source.chars().any(|scalar| (scalar as u32) < 0x20 || scalar as u32 == 0x7F) {
            return AssetReferenceKind::Malformed;
        }
        let lower = swift::lowercased(source);
        if swift::has_prefix(&lower, "data:") {
            return if url_with_string(source).is_none() { AssetReferenceKind::Malformed } else { AssetReferenceKind::DataUrl };
        }
        if swift::has_prefix(&lower, "http://") || swift::has_prefix(&lower, "https://") {
            let Some(url) = url_with_string(source) else { return AssetReferenceKind::Malformed };
            if url.host().is_none() {
                return AssetReferenceKind::Malformed;
            }
            return AssetReferenceKind::RemoteHttp;
        }
        if swift::has_prefix(&lower, "file://") {
            let Some(url) = url_with_string(source) else { return AssetReferenceKind::Malformed };
            // `url.path.isEmpty == false`; `NSURL.path` is nil where `URL.path`
            // is empty.
            if url.path().is_none_or(|path| path.length() == 0) {
                return AssetReferenceKind::Malformed;
            }
            return AssetReferenceKind::FileUrl;
        }
        if let Some(colon) = swift::first_index_of(source, ':') {
            let before = &source[..colon];
            if !swift::contains_with(before, "/", bridged) && !swift::contains_with(before, "\\", bridged) {
                return AssetReferenceKind::Unsafe;
            }
        }
        if swift::has_prefix(source, "/") || swift::has_prefix(source, "~") {
            return AssetReferenceKind::AbsoluteLocal;
        }
        if swift::contains_with(source, "\0", bridged) || swift::str_eq(source, ".") || swift::str_eq(source, "..") {
            return AssetReferenceKind::Unsafe;
        }
        AssetReferenceKind::RelativeLocal
    }

    fn resolve(source: &str, kind: AssetReferenceKind, context: &AssetResolutionContext) -> Option<FileUrl> {
        match kind {
            AssetReferenceKind::RelativeLocal => {
                let document_url = context.document_url.as_ref()?;
                let path = Self::local_path(source);
                Some(document_url.deleting_last_path_component().appending_path_component(&path).standardized_file_url())
            }
            AssetReferenceKind::AbsoluteLocal => {
                let path = Self::local_path(&expanding_tilde_in_path(source));
                Some(FileUrl::from_path(&path).standardized_file_url())
            }
            AssetReferenceKind::FileUrl => {
                // `URL(string: source)?.standardizedFileURL`: a file URL of the
                // standardized path that keeps `hasDirectoryPath` (host, query
                // and fragment are dropped).
                let url = url_with_string(source)?;
                let path = url.path().map(|path| foundation::to_string(&path)).unwrap_or_default();
                if path.is_empty() {
                    // `standardizedFileURL` returns such a URL unchanged; it is
                    // never produced, since `classify` calls it malformed.
                    return None;
                }
                Some(FileUrl::from_path_is_directory(&path, url.hasDirectoryPath()).standardized_file_url())
            }
            _ => None,
        }
    }

    fn local_path(source: &str) -> String {
        let end = swift::first_index_where(source, |character| character == "?" || character == "#").unwrap_or(source.len());
        let path = &source[..end];
        removing_percent_encoding(path).unwrap_or_else(|| path.to_owned())
    }
}

/// `s as NSString`, keeping a leading U+FEFF (`NSString::from_str` drops it).
pub(crate) fn ns_string(s: &str) -> Retained<NSString> {
    if s.is_ascii() { NSString::from_str(s) } else { foundation::ns_from_utf16(&swift::ns::utf16(s)) }
}

/// `URL(string:)`, as `-[NSURL URLWithString:]` (see the module notes).
pub(crate) fn url_with_string(source: &str) -> Option<Retained<NSURL>> {
    objc2::rc::autoreleasepool(|_| NSURL::URLWithString(&ns_string(source)))
}

/// `String.removingPercentEncoding`.
pub(crate) fn removing_percent_encoding(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_owned());
    }
    objc2::rc::autoreleasepool(|_| ns_string(s).stringByRemovingPercentEncoding().map(|decoded| foundation::to_string(&decoded)))
}

/// `NSString.range(of:options: [], range:)` over a UTF-16 buffer: the literal
/// search on ASCII text (where the two agree for these needles), Foundation's
/// otherwise.
fn range_of(units: &[u16], needle: &str, range: NSRange) -> NSRange {
    if units.iter().all(|&unit| unit < 0x80) {
        return units.range_of_literal(&swift::ns::utf16(needle), range);
    }
    objc2::rc::autoreleasepool(|_| {
        foundation::range_of(&foundation::ns_from_utf16(units), needle, NSStringCompareOptions::empty(), range)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(source: &str) -> Option<String> {
        AssetReferenceParser::resolve(source, AssetReferenceKind::FileUrl, &AssetResolutionContext::default())
            .map(|url| url.url_path().to_owned())
    }

    /// Recorded from Swift 6.4 on macOS 26:
    /// `URL(string: s)!.standardizedFileURL` (`path`, plus `/` when
    /// `hasDirectoryPath`).
    #[test]
    fn file_urls_standardize_as_swift_does() {
        assert_eq!(file("file:///tmp/a.png").as_deref(), Some("/tmp/a.png"));
        assert_eq!(file("file://host/a.png").as_deref(), Some("/a.png"));
        assert_eq!(file("file:///tmp/a%20b.png").as_deref(), Some("/tmp/a b.png"));
        assert_eq!(file("file:///tmp/../x/./y.png").as_deref(), Some("/x/y.png"));
        assert_eq!(file("file:///tmp/a.png?x=1#f").as_deref(), Some("/tmp/a.png"));
        assert_eq!(file("file:///tmp/x/..").as_deref(), Some("/tmp/"));
        assert_eq!(file("file:///tmp//double//y.png").as_deref(), Some("/tmp/double/y.png"));
        assert_eq!(file("file:///private/tmp/../tmp").as_deref(), Some("/tmp"));
        assert_eq!(file("file:///tmp/a%2Fb.png").as_deref(), Some("/tmp/a%2Fb.png"));
        assert_eq!(file("file:///tmp/%").as_deref(), Some("/tmp/%"));
        assert_eq!(file("file:///tmp/\u{e9}.png").as_deref(), Some("/tmp/e\u{301}.png"));
    }

    /// Recorded from Swift 6.4 on macOS 26.
    #[test]
    fn classification_matches_swift() {
        use AssetReferenceKind::*;
        let cases = [
            ("https://example.com/a.png", RemoteHttp),
            ("https://", Malformed),
            ("http:///path", Malformed),
            ("https://exa mple.com/a", Malformed),
            ("https://user@/x", RemoteHttp),
            ("HTTPS://Example.com", RemoteHttp),
            ("data:image/png;base64,AA==", DataUrl),
            ("data:a b", DataUrl),
            ("file://", Malformed),
            ("file:///", FileUrl),
            ("file://host/a.png", FileUrl),
            ("javascript:alert(1)", Unsafe),
            ("a/b:c", RelativeLocal),
            ("/a.png", AbsoluteLocal),
            ("~/a.png", AbsoluteLocal),
            (".", Unsafe),
            ("..", Unsafe),
            ("a\tb", Malformed),
            ("", Malformed),
        ];
        for (source, kind) in cases {
            assert_eq!(AssetReferenceParser::classify(source), kind, "{source:?}");
        }
        // Bridged and native `contains` disagree across a ZWJ.
        assert_eq!(AssetReferenceParser::classify_with("a/\u{200D}b:c", false), Unsafe);
        assert_eq!(AssetReferenceParser::classify_with("a/\u{200D}b:c", true), RelativeLocal);
    }

    /// Recorded from Swift 6.4 on macOS 26 (`String.removingPercentEncoding`).
    #[test]
    fn percent_decoding_matches_swift() {
        assert_eq!(removing_percent_encoding("a%20b").as_deref(), Some("a b"));
        assert_eq!(removing_percent_encoding("a%zzb"), None);
        assert_eq!(removing_percent_encoding("%E9"), None);
        assert_eq!(removing_percent_encoding("%C3%A9").as_deref(), Some("\u{e9}"));
        assert_eq!(removing_percent_encoding("a+b").as_deref(), Some("a+b"));
    }
}
