//! Port of `Fragments/LocalAssetPolicy.swift`: resolving a Markdown image
//! destination without touching the filesystem beyond canonical path
//! resolution, shared by the app renderer and Quick Look.
//!
//! Swift's `URL` operations are `NSURL`'s here. One composition matters:
//! Swift standardizes a relative URL after resolving it against its base,
//! while `-[NSURL standardizedURL]` standardizes only the relative part (so
//! `../x` against `/a/b/` would stay inside `b`). `canonical_file_url`
//! therefore takes the `absoluteURL` first, which reproduces Swift on every
//! probed destination (`..`, `a/../../b`, symlinks, `~`, `file://`, fragments,
//! percent signs, decomposed names). The one remaining difference is that
//! NSURL drops a directory URL's trailing slash where Swift keeps it; the
//! path components, and so every policy decision, are identical.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use objc2::rc::Retained;
use objc2_foundation::{NSObjectProtocol, NSString, NSURL};

use crate::appkit_compat::ns_string;
use crate::fragments::fragment_base::LocalAssetAuthorizer;

/// `LocalAssetRequest`: a canonical file URL and whether it is a safe
/// relative asset inside the document's directory.
#[derive(Debug, Clone)]
pub struct LocalAssetRequest {
    pub url: Retained<NSURL>,
    pub is_safe_relative: bool,
}

impl PartialEq for LocalAssetRequest {
    fn eq(&self, other: &Self) -> bool {
        self.is_safe_relative == other.is_safe_relative && self.url.isEqual(Some(&other.url))
    }
}

impl LocalAssetRequest {
    /// The URL's file-system path, which the app-side authorizer receives.
    pub fn path(&self) -> String {
        self.url.path().map(|path| upleft_swift_text::ns::foundation::to_string(&path)).unwrap_or_default()
    }
}

/// `LocalAssetPolicy`.
pub struct LocalAssetPolicy;

impl LocalAssetPolicy {
    /// `LocalAssetPolicy.request(raw:documentURL:)`: resolves a Markdown
    /// destination and canonicalizes symlinks before any image loader sees
    /// it. Non-file URLs are never local image requests.
    pub fn request(raw: &str, document_url: Option<&NSURL>) -> Option<LocalAssetRequest> {
        if raw.is_empty() {
            return None;
        }
        let document = Self::canonical_file_url(document_url?)?;

        let relative = !upleft_swift_text::has_prefix(raw, "/")
            && !upleft_swift_text::has_prefix(raw, "~")
            && !upleft_swift_text::contains(raw, "://");
        let has_traversal =
            upleft_swift_text::split(raw, '/', usize::MAX, false).iter().any(|piece| upleft_swift_text::str_eq(piece, ".."));

        let candidate: Retained<NSURL> = if upleft_swift_text::contains(raw, "://") {
            let parsed = NSURL::URLWithString(&ns_string(raw))?;
            if !parsed.isFileURL() {
                return None;
            }
            parsed
        } else if upleft_swift_text::has_prefix(raw, "~") {
            NSURL::fileURLWithPath(&ns_string(raw).stringByExpandingTildeInPath())
        } else if upleft_swift_text::has_prefix(raw, "/") {
            NSURL::fileURLWithPath(&ns_string(raw))
        } else {
            let directory = document.URLByDeletingLastPathComponent()?;
            NSURL::fileURLWithPath_relativeToURL(&ns_string(raw), Some(&directory))
        };

        let canonical = Self::canonical_file_url(&candidate)?;
        let safe_relative = relative
            && !has_traversal
            && document.URLByDeletingLastPathComponent().is_some_and(|directory| Self::is_within(&canonical, &directory));
        Some(LocalAssetRequest { url: canonical, is_safe_relative: safe_relative })
    }

    /// `LocalAssetPolicy.allows(_:authorizer:)`.
    pub fn allows(request: &LocalAssetRequest, authorizer: Option<&LocalAssetAuthorizer>) -> bool {
        request.is_safe_relative || authorizer.is_some_and(|authorizer| authorizer(&request.path()))
    }

    /// `LocalAssetPolicy.canonicalFileURL(_:)`: a canonical file URL even
    /// when the final asset is missing, with any existing symlink in its
    /// path resolved.
    pub fn canonical_file_url(url: &NSURL) -> Option<Retained<NSURL>> {
        if !url.isFileURL() {
            return None;
        }
        let absolute = url.absoluteURL()?;
        let standardized = absolute.standardizedURL()?;
        let resolved = standardized.URLByResolvingSymlinksInPath()?;
        resolved.standardizedURL()
    }

    /// `LocalAssetPolicy.isWithin(_:_:)`: path-component containment of
    /// canonical URLs.
    pub fn is_within(child: &NSURL, root: &NSURL) -> bool {
        let (Some(child), Some(root)) = (Self::canonical_file_url(child), Self::canonical_file_url(root)) else {
            return false;
        };
        let (Some(child_parts), Some(root_parts)) = (child.pathComponents(), root.pathComponents()) else {
            return false;
        };
        if child_parts.count() < root_parts.count() {
            return false;
        }
        (0..root_parts.count()).all(|index| {
            let child: Retained<NSString> = child_parts.objectAtIndex(index);
            let root: Retained<NSString> = root_parts.objectAtIndex(index);
            // Swift `String ==`: canonical equivalence.
            upleft_swift_text::str_eq(
                &upleft_swift_text::ns::foundation::to_string(&child),
                &upleft_swift_text::ns::foundation::to_string(&root),
            )
        })
    }
}

/// `URL(fileURLWithPath:)` for a document path.
pub fn file_url(path: &str) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns_string(path))
}
