//! Port of `Assets/AssetDoctor.swift`: diagnostics for a document's image
//! references, and reversible source edits that relink or rename one.
//!
//! `URL` is [`FileUrl`] (every URL here is a resolved file URL, see
//! `asset_resolver`). Swift `String` comparisons (`==`, `hasPrefix`,
//! `Dictionary` keys) keep their canonical-equivalence semantics through
//! `upleft-swift-text`.

use std::collections::HashMap;

use upleft_core::{NSRange, ParsedDocument};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift, ns::NSStringExt};

pub use crate::assets::asset_resolver::{
    AssetMetadata, AssetProbe, AssetReference, AssetReferenceKind, AssetReferenceParser, AssetResolutionContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetDiagnosticCode {
    Missing,
    OutsideWorkspace,
    AbsolutePath,
    Duplicate,
    UnsupportedFormat,
    LargeFile,
    MissingAlt,
    Unsafe,
    Malformed,
}

impl AssetDiagnosticCode {
    pub fn raw_value(&self) -> &'static str {
        match self {
            AssetDiagnosticCode::Missing => "missing",
            AssetDiagnosticCode::OutsideWorkspace => "outsideWorkspace",
            AssetDiagnosticCode::AbsolutePath => "absolutePath",
            AssetDiagnosticCode::Duplicate => "duplicate",
            AssetDiagnosticCode::UnsupportedFormat => "unsupportedFormat",
            AssetDiagnosticCode::LargeFile => "largeFile",
            AssetDiagnosticCode::MissingAlt => "missingAlt",
            AssetDiagnosticCode::Unsafe => "unsafe",
            AssetDiagnosticCode::Malformed => "malformed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl AssetDiagnosticSeverity {
    pub fn raw_value(&self) -> &'static str {
        match self {
            AssetDiagnosticSeverity::Info => "info",
            AssetDiagnosticSeverity::Warning => "warning",
            AssetDiagnosticSeverity::Error => "error",
        }
    }
}

/// `AssetDiagnostic`.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetDiagnostic {
    pub id: String,
    pub code: AssetDiagnosticCode,
    pub severity: AssetDiagnosticSeverity,
    pub message: String,
    pub range: NSRange,
    pub reference: AssetReference,
}

impl AssetDiagnostic {
    /// `init(code:message:range:reference:)`.
    pub fn new(code: AssetDiagnosticCode, message: impl Into<String>, range: NSRange, reference: AssetReference) -> AssetDiagnostic {
        let severity = match code {
            AssetDiagnosticCode::Malformed | AssetDiagnosticCode::Unsafe => AssetDiagnosticSeverity::Error,
            AssetDiagnosticCode::Missing
            | AssetDiagnosticCode::OutsideWorkspace
            | AssetDiagnosticCode::AbsolutePath
            | AssetDiagnosticCode::UnsupportedFormat
            | AssetDiagnosticCode::LargeFile
            | AssetDiagnosticCode::MissingAlt => AssetDiagnosticSeverity::Warning,
            _ => AssetDiagnosticSeverity::Info,
        };
        AssetDiagnostic {
            id: format!("asset:{}:{}:{}", code.raw_value(), range.location, range.length),
            code,
            severity,
            message: message.into(),
            range,
            reference,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetProposalKind {
    Relink,
    Rename,
}

impl AssetProposalKind {
    pub fn raw_value(&self) -> &'static str {
        match self {
            AssetProposalKind::Relink => "relink",
            AssetProposalKind::Rename => "rename",
        }
    }
}

/// A reversible source edit. Applying it validates the source before it
/// changes anything, so a stale proposal cannot overwrite a newer edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetSourceProposal {
    pub kind: AssetProposalKind,
    pub range: NSRange,
    pub expected_source: String,
    pub replacement: String,
    pub inverse_replacement: String,
}

impl AssetSourceProposal {
    /// `apply(to:)`.
    pub fn apply(&self, source: &str) -> Option<String> {
        let mut text = swift::ns::utf16(source);
        // `text.substring(with: range) == expectedSource` compares Swift Strings.
        if !(self.range.location >= 0
            && self.range.upper_bound() <= text.as_slice().length()
            && swift::str_eq(&text.as_slice().substring(self.range), &self.expected_source))
        {
            return None;
        }
        text.splice(self.range.as_usize_range(), self.replacement.encode_utf16());
        Some(swift::ns::string_from_utf16(&text))
    }

    /// `inverse(in:)`.
    pub fn inverse(&self, source: &str) -> Option<String> {
        let inverse = AssetSourceProposal {
            kind: self.kind,
            range: self.range,
            expected_source: self.replacement.clone(),
            replacement: self.inverse_replacement.clone(),
            inverse_replacement: self.expected_source.clone(),
        };
        inverse.apply(source)
    }
}

/// `markdownSafeDestination(_:)`.
fn markdown_safe_destination(source: &str) -> String {
    let source = swift::replacing_occurrences(source, "%", "%25");
    let source = swift::replacing_occurrences(&source, " ", "%20");
    let source = swift::replacing_occurrences(&source, "(", "%28");
    swift::replacing_occurrences(&source, ")", "%29")
}

/// `AssetDoctor`.
pub struct AssetDoctor;

impl AssetDoctor {
    /// `references(in:context:)`.
    pub fn references(document: &ParsedDocument, context: &AssetResolutionContext) -> Vec<AssetReference> {
        AssetReferenceParser::references(document, context)
    }

    /// `diagnose(_:context:probe:)`.
    pub fn diagnose(
        document: &ParsedDocument,
        context: &AssetResolutionContext,
        probe: Option<&AssetProbe>,
    ) -> Vec<AssetDiagnostic> {
        let refs = Self::references(document, context);
        let mut diagnostics: Vec<AssetDiagnostic> = Vec::new();
        let mut identities: HashMap<String, (FileUrl, AssetReference)> = HashMap::new();
        for reference in refs {
            if swift::trim_whitespaces_and_newlines(&reference.alt_text).is_empty() {
                diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::MissingAlt,
                    "Image has no alt text.",
                    reference.image_range,
                    reference.clone(),
                ));
            }
            match reference.kind {
                AssetReferenceKind::Malformed => diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::Malformed,
                    "Image destination is malformed.",
                    reference.destination_range,
                    reference.clone(),
                )),
                AssetReferenceKind::Unsafe => diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::Unsafe,
                    "Image destination is unsafe.",
                    reference.destination_range,
                    reference.clone(),
                )),
                AssetReferenceKind::RemoteHttp | AssetReferenceKind::DataUrl => continue,
                AssetReferenceKind::AbsoluteLocal => diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::AbsolutePath,
                    "Absolute image paths are not portable.",
                    reference.destination_range,
                    reference.clone(),
                )),
                AssetReferenceKind::RelativeLocal | AssetReferenceKind::FileUrl => {}
            }

            let Some(url) = reference.url.clone() else { continue };
            if let Some(root) = &context.workspace_root
                && !Self::is_inside(&url, root)
            {
                diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::OutsideWorkspace,
                    "Image is outside the workspace.",
                    reference.destination_range,
                    reference.clone(),
                ));
            }
            let Some(probe) = probe else { continue };
            let Some(metadata) = probe.metadata(&url) else {
                diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::Missing,
                    "Image asset was not found.",
                    reference.destination_range,
                    reference.clone(),
                ));
                continue;
            };
            if !metadata.exists || metadata.is_directory {
                diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::Missing,
                    "Image asset was not found.",
                    reference.destination_range,
                    reference.clone(),
                ));
            } else if let Some(size) = metadata.byte_size
                && size > context.maximum_bytes
            {
                diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::LargeFile,
                    "Image asset is larger than the configured limit.",
                    reference.destination_range,
                    reference.clone(),
                ));
            }
            if let Some(extension) = metadata.file_extension.as_deref().map(swift::lowercased)
                && !context.supported_extensions.contains(&extension)
            {
                diagnostics.push(AssetDiagnostic::new(
                    AssetDiagnosticCode::UnsupportedFormat,
                    "Image format is not supported.",
                    reference.destination_range,
                    reference.clone(),
                ));
            }
            if let Some(identity) = metadata.content_identity {
                match swift::dict_get(&identities, &identity) {
                    Some((previous_url, previous_reference)) if !swift::str_eq(&previous_url.path(), &url.path()) => {
                        let message = format!("Image has the same content as line {}.", previous_reference.line);
                        diagnostics.push(AssetDiagnostic::new(
                            AssetDiagnosticCode::Duplicate,
                            message,
                            reference.destination_range,
                            reference.clone(),
                        ));
                    }
                    _ => swift::dict_insert(&mut identities, identity, (url, reference)),
                }
            }
        }
        diagnostics
    }

    /// `relinkProposal(for:to:)`.
    pub fn relink_proposal(reference: &AssetReference, replacement: &str) -> AssetSourceProposal {
        AssetSourceProposal {
            kind: AssetProposalKind::Relink,
            range: reference.destination_range,
            expected_source: reference.source_text.clone(),
            replacement: markdown_safe_destination(replacement),
            inverse_replacement: reference.source_text.clone(),
        }
    }

    /// `renameProposal(for:to:)`.
    pub fn rename_proposal(reference: &AssetReference, replacement: &str) -> AssetSourceProposal {
        AssetSourceProposal {
            kind: AssetProposalKind::Rename,
            range: reference.destination_range,
            expected_source: reference.source_text.clone(),
            replacement: markdown_safe_destination(replacement),
            inverse_replacement: reference.source_text.clone(),
        }
    }

    fn is_inside(url: &FileUrl, root: &FileUrl) -> bool {
        let path = url.standardized_file_url().path();
        let root_path = root.standardized_file_url().path();
        swift::str_eq(&path, &root_path)
            || swift::has_prefix(
                &path,
                &if swift::has_suffix(&root_path, "/") { root_path.clone() } else { format!("{root_path}/") },
            )
    }
}
