//! Port of `Assets/DroppedAsset.swift`: turning something dropped on the
//! document surface into files on disk and Markdown in the source.
//!
//! The render layer resolves *where* a drop lands (`DocumentDrop`); this
//! decides *what* it becomes, as pure functions over a pasteboard, a document
//! URL and an "is this name taken?" closure. The window controller does the
//! writing. A destination has to satisfy the renderer (`LocalAssetPolicy`, no
//! percent-decoding), the Asset Doctor's parser (ends a bare destination at a
//! space or tab, truncates at `#` or `?`, percent-decodes, understands
//! `<…>`), the Asset Doctor's diagnostics and the link classifier. Hence: a
//! bare relative path, else the angle-bracket form, else a `file:` URL, and
//! never percent-encoding.
//!
//! `URL` is [`FileUrl`]; the `LocalAssetPolicy` calls go through
//! `upleft-render`'s port, which works on `NSURL`.

use std::sync::LazyLock;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::ClassType;
use objc2_app_kit::{NSPasteboard, NSPasteboardURLReadingFileURLsOnlyKey};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};
use upleft_core::inlines::StringSet;
use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;
use upleft_render::fragments::local_asset_policy::LocalAssetPolicy;
use upleft_swift_text::{self as swift, CharSet, ns::foundation};

use crate::assets::asset_resolver::AssetResolutionContext;
use crate::assets::captured_image::{self, CapturedImage, InsertionEdit, deleting_path_extension};

/// `DroppedAsset.Payload`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Real files: Finder, Preview, an attachment dragged out of Mail.
    Files(Vec<FileUrl>),
    /// Bytes with no file behind them: an image dragged straight out of a
    /// browser or a preview window.
    ImageData(captured_image::Payload),
}

/// The three forms a destination may take, in preference order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// `assets/diagram.png`: portable, and the only form the renderer draws
    /// without a trust prompt.
    Relative(String),
    /// `<meeting notes.png>`: still relative and still trusted, and the
    /// CommonMark-sanctioned way to carry a space.
    Angled(String),
    /// `file:///Users/…`: not portable and not trusted by default, but
    /// unambiguous. Used only when there is nothing to be relative *to*.
    Absolute(FileUrl),
}

impl Destination {
    /// `markdownText`.
    pub fn markdown_text(&self) -> String {
        match self {
            Destination::Relative(path) => path.clone(),
            Destination::Angled(path) => format!("<{path}>"),
            // `absoluteString` percent-encodes exactly what a URL needs, and
            // `URL(string:)` decodes it again: here percent-encoding
            // round-trips.
            Destination::Absolute(url) => url.absolute_string(),
        }
    }

    /// `isRelative`: relative to the document's own folder, which is what
    /// makes it portable and prompt-free.
    pub fn is_relative(&self) -> bool {
        match self {
            Destination::Relative(_) | Destination::Angled(_) => true,
            Destination::Absolute(_) => false,
        }
    }
}

/// `DroppedAsset.Write.Contents`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteContents {
    CopyOf(FileUrl),
    Data(Vec<u8>),
}

/// `DroppedAsset.Write`: a file the drop has to create before its reference
/// means anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub file_name: String,
    pub contents: WriteContents,
}

impl Write {
    pub fn new(file_name: impl Into<String>, contents: WriteContents) -> Write {
        Write { file_name: file_name.into(), contents }
    }
}

/// `DroppedAsset.Insertion`: one dropped item, resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertion {
    /// `None` when the reference points at a file that is already in place.
    pub write: Option<Write>,
    pub markdown: String,
    /// Images are padded out into a paragraph of their own; a link is inline
    /// text and lands exactly where the pointer was.
    pub is_block: bool,
}

impl Insertion {
    pub fn new(write: Option<Write>, markdown: impl Into<String>, is_block: bool) -> Insertion {
        Insertion { write, markdown: markdown.into(), is_block }
    }
}

/// `DroppedAsset`.
pub struct DroppedAsset;

/// `bareHazards`: Characters that end or re-point a *bare* destination.
const BARE_HAZARDS: [char; 7] = [' ', '\t', '#', '?', '%', '(', ')'];
/// `angledHazards`: Characters the angle-bracket form cannot carry either.
const ANGLED_HAZARDS: [char; 4] = ['<', '>', '\n', '\r'];

/// `relative.contains(where: hazards.contains)` over a `Set<Character>`.
fn contains_hazard(relative: &str, hazards: &[char]) -> bool {
    swift::graphemes(relative).any(|character| hazards.iter().any(|&hazard| swift::char_is(character, hazard)))
}

impl DroppedAsset {
    /// `payload(from:)`. Files are read first on purpose: a file drag also
    /// exposes an image representation, and taking the bytes would copy an
    /// image the reader already has.
    pub fn payload(pasteboard: &NSPasteboard) -> Option<Payload> {
        let existing: Vec<FileUrl> = objc2::rc::autoreleasepool(|_| {
            let classes: Retained<NSArray<AnyClass>> = NSArray::from_slice(&[NSURL::class()]);
            let yes = NSNumber::new_bool(true);
            // SAFETY: an AppKit constant key.
            let key: &NSString = unsafe { NSPasteboardURLReadingFileURLsOnlyKey };
            let options: Retained<NSDictionary<NSString, AnyObject>> =
                NSDictionary::from_slices(&[key], &[yes.as_ref() as &AnyObject]);
            // SAFETY: an array of classes and a reading-options dictionary.
            let objects = unsafe { pasteboard.readObjectsForClasses_options(&classes, Some(&options)) };
            // `as? [URL] ?? []`: every element must be a URL, or none is used.
            let urls: Vec<Retained<NSURL>> = match objects {
                Some(objects) => {
                    let mut urls = Vec::new();
                    for object in objects.iter() {
                        match object.downcast::<NSURL>() {
                            Ok(url) => urls.push(url),
                            Err(_) => return Vec::new(),
                        }
                    }
                    urls
                }
                None => Vec::new(),
            };
            urls.iter()
                .filter(|url| {
                    let path = url.path().map(|path| foundation::to_string(&path)).unwrap_or_default();
                    foundation_io::file_exists(&path)
                })
                .filter_map(|url| FileUrl::from_nsurl(url))
                .collect()
        });
        if !existing.is_empty() {
            return Some(Payload::Files(existing));
        }
        if let Some(image) = CapturedImage::payload(pasteboard) {
            return Some(Payload::ImageData(image));
        }
        None
    }

    /// `destination(for:relativeTo:)`: the best destination for a file that
    /// already exists on disk. A file inside the document's folder is
    /// referenced where it stands and never copied.
    pub fn destination(file: &FileUrl, directory: Option<&FileUrl>) -> Destination {
        let Some(relative) = directory.and_then(|directory| Self::relative_path(file, directory)) else {
            return Destination::Absolute(file.standardized_file_url());
        };
        if !contains_hazard(&relative, &BARE_HAZARDS) {
            return Destination::Relative(relative);
        }
        if !contains_hazard(&relative, &ANGLED_HAZARDS) {
            return Destination::Angled(relative);
        }
        Destination::Absolute(file.standardized_file_url())
    }

    /// `relativePath(of:in:)`: `file`'s path relative to `directory`, or
    /// `None` when it is not inside it. Both sides are canonicalised through
    /// `LocalAssetPolicy`, as the renderer will when it resolves the
    /// reference back. Never produces a `..` path.
    pub fn relative_path(file: &FileUrl, directory: &FileUrl) -> Option<String> {
        objc2::rc::autoreleasepool(|_| {
            let child = LocalAssetPolicy::canonical_file_url(&file.to_nsurl())?;
            let root = LocalAssetPolicy::canonical_file_url(&directory.to_nsurl())?;
            if !LocalAssetPolicy::is_within(&child, &root) {
                return None;
            }
            let child_parts = child.pathComponents()?;
            let root_count = root.pathComponents().map_or(0, |parts| parts.count());
            let components: Vec<String> =
                child_parts.iter().skip(root_count).map(|part| foundation::to_string(&part)).collect();
            if components.is_empty() {
                return None;
            }
            Some(components.join("/"))
        })
    }

    /// `isImage(_:)`: formats Upleft can draw, the Asset Doctor's own set, so
    /// a dropped file becomes an `![…]` only when it would not be diagnosed as
    /// an unsupported format.
    pub fn is_image(url: &FileUrl) -> bool {
        static IMAGE_EXTENSIONS: LazyLock<StringSet> =
            LazyLock::new(|| AssetResolutionContext::default().supported_extensions);
        IMAGE_EXTENSIONS.contains(&swift::lowercased(&url.path_extension()))
    }

    /// `plan(for:documentDirectory:documentBaseName:isTaken:)`: everything a
    /// drop turns into, in order. Without a document folder, dropped files
    /// get `file:` destinations and dropped bytes are refused.
    pub fn plan(
        payload: &Payload,
        document_directory: Option<&FileUrl>,
        document_base_name: &str,
        is_taken: impl Fn(&str) -> bool,
    ) -> Vec<Insertion> {
        let mut taken = StringSet::default();
        let mut claim = |stem: &str, file_extension: &str| -> String {
            let name = CapturedImage::unique_file_name(stem, file_extension, |candidate| {
                taken.contains(candidate) || is_taken(candidate)
            });
            taken.insert(name.clone());
            name
        };

        match payload {
            Payload::ImageData(image) => {
                if document_directory.is_none() {
                    return Vec::new();
                }
                let base = CapturedImage::slug(document_base_name);
                let stem = if base.is_empty() { "image".to_owned() } else { base + "-image" };
                let name = claim(&stem, &image.file_extension);
                vec![Insertion::new(
                    Some(Write::new(name.clone(), WriteContents::Data(image.data.clone()))),
                    Self::image_markdown(&Self::alt_text(&name), &Destination::Relative(name)),
                    true,
                )]
            }
            Payload::Files(urls) => urls
                .iter()
                .map(|url| {
                    let placement = Self::destination(url, document_directory);
                    if !Self::is_image(url) {
                        // A link is a *reference*, never an embed: a non-image
                        // is linked where it stands.
                        return Insertion::new(
                            None,
                            Self::link_markdown(&Self::alt_text(&url.last_path_component()), &placement),
                            false,
                        );
                    }
                    if placement.is_relative() || document_directory.is_none() {
                        return Insertion::new(
                            None,
                            Self::image_markdown(&Self::alt_text(&url.last_path_component()), &placement),
                            true,
                        );
                    }
                    // An image from outside the folder is copied in.
                    let stem = CapturedImage::slug(&url.deleting_path_extension().last_path_component());
                    let name = claim(if stem.is_empty() { "image" } else { &stem }, &swift::lowercased(&url.path_extension()));
                    Insertion::new(
                        Some(Write::new(name.clone(), WriteContents::CopyOf(url.clone()))),
                        // The alt text keeps the file's real name even though
                        // the copy is slugged.
                        Self::image_markdown(&Self::alt_text(&url.last_path_component()), &Destination::Relative(name)),
                        true,
                    )
                })
                .collect(),
        }
    }

    /// `edit(insertions:in:at:)`: the one source edit a whole drop becomes. A
    /// drop of more than one item is laid out as blocks; a single link stays
    /// inline at the drop point.
    pub fn edit(insertions: &[Insertion], source: &str, offset: isize) -> Option<InsertionEdit> {
        if insertions.is_empty() {
            return None;
        }
        let is_block = insertions.len() > 1 || insertions.iter().any(|insertion| insertion.is_block);
        let body = insertions
            .iter()
            .map(|insertion| insertion.markdown.as_str())
            .collect::<Vec<&str>>()
            .join(if is_block { "\n\n" } else { " " });
        if !is_block {
            let length = swift::utf16_count(source);
            let position = 0.max(offset).min(length);
            return Some(InsertionEdit { caret: position + swift::utf16_count(&body), replacement: body, origin: position });
        }
        Some(CapturedImage::insertion_block(&body, source, offset))
    }

    /// `imageMarkdown(alt:destination:)`.
    pub fn image_markdown(alt: &str, destination: &Destination) -> String {
        format!("![{alt}]({})", destination.markdown_text())
    }

    /// `linkMarkdown(label:destination:)`.
    pub fn link_markdown(label: &str, destination: &Destination) -> String {
        format!("[{label}]({})", destination.markdown_text())
    }

    /// `altText(for:)`: a label safe between `[` and `]`. Brackets and
    /// backslashes are escaped, newlines become spaces, and it is never empty.
    pub fn alt_text(file_name: &str) -> String {
        let stem = deleting_path_extension(file_name);
        let base = if stem.is_empty() { file_name } else { stem.as_str() };
        let mut out = String::new();
        for character in swift::graphemes(base) {
            if swift::char_is(character, '\\') || swift::char_is(character, '[') || swift::char_is(character, ']') {
                out.push('\\');
            }
            if swift::is_newline(character) {
                out.push(' ');
                continue;
            }
            out.push_str(character);
        }
        let trimmed = swift::trimming(&out, CharSet::Whitespaces);
        if trimmed.is_empty() { "Image".to_owned() } else { trimmed.to_owned() }
    }
}
