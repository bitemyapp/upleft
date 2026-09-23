//! Port of `App/DocumentWindowController+AssetDoctor.swift`: window actions
//! for the Asset Doctor panel.
//!
//! The panel itself only reports source ranges and proposals. This extension
//! supplies the document, local-only file probe, and the one-edit undo path.
//! The owner may keep an `AssetDoctorView` in any inspector slot and call
//! [`DocumentWindowController::configure_asset_doctor`] after each parse.
//!
//! `AssetDoctorViewDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.
//!
//! Main-thread I/O, as in Swift: [`DocumentWindowController::local_asset_probe`]
//! reads resource values for every local reference while `AssetDoctor`
//! diagnoses, which `configure_asset_doctor` does on the main thread.

use std::rc::{Rc, Weak};

use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak as ObjcWeak, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSResponder, NSWindow, NSWorkspace};
use objc2_foundation::{
    NSArray, NSNumber, NSString, NSURLFileSizeKey, NSURLIsDirectoryKey, NSURLIsRegularFileKey, NSURLResourceKey,
};
use upleft_foundation::url::FileUrl;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;
use upleft_swift_text as swift;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::assets::asset_doctor::{AssetDiagnostic, AssetDoctor, AssetProposalKind, AssetSourceProposal};
use crate::assets::asset_resolver::{AssetMetadata, AssetProbe, AssetReferenceKind, AssetResolutionContext};
use crate::panels::asset_doctor_view::{AssetDoctorView, AssetDoctorViewDelegate};
use crate::panels::panel_chrome::panel_title;
use crate::security::document_trust::{TrustDecision, TrustEffect, TrustRequest, TrustTarget};
use crate::support::commands::Command;

impl DocumentWindowController {
    /// `toggleAssetDoctorPanel()`.
    pub fn toggle_asset_doctor_panel(&self) {
        if let Some(asset_doctor_panel) = self.asset_doctor_panel() {
            self.dismiss_trailing(&asset_doctor_panel);
            self.set_asset_doctor_panel(None);
            return;
        }
        if let Some(folder) = self.markdown_document().url().map(|url| url.deleting_last_path_component()) {
            let path = folder.path();
            let decision = self.trust_decision(&TrustRequest::new(
                TrustEffect::ReadLocalAsset,
                TrustTarget::new(&path, Some(&path), None),
                self.markdown_document().url().as_ref(),
            ));
            if decision != TrustDecision::Allow {
                let weak: ObjcWeak<DocumentWindowController> = ObjcWeak::from(self);
                self.authorize_local_effect(TrustEffect::ReadLocalAsset, &folder, move || {
                    if let Some(this) = weak.load() {
                        this.show_asset_doctor_panel();
                    }
                });
                return;
            }
        }
        self.show_asset_doctor_panel();
    }

    /// `showAssetDoctorPanel()`.
    fn show_asset_doctor_panel(&self) {
        self.set_front_matter_editor(None);
        let panel = AssetDoctorView::new(self.active_style_sheet(), MainThreadMarker::from(self));
        self.set_asset_doctor_panel(Some(panel.clone()));
        self.configure_asset_doctor(&panel);
        self.install_trailing(&panel, Some(&panel_title(Command::AssetDoctor)));
    }

    /// `configureAssetDoctor(_:)`.
    pub fn configure_asset_doctor(&self, view: &AssetDoctorView) {
        let delegates = self.delegates();
        let delegate: Weak<dyn AssetDoctorViewDelegate> =
            Rc::downgrade(&delegates) as Weak<dyn AssetDoctorViewDelegate>;
        view.set_delegate(Some(delegate));
        view.set_style_sheet(self.active_style_sheet());
        let context = AssetResolutionContext::new(
            self.markdown_document().url(),
            self.markdown_document().url().map(|url| url.deleting_last_path_component()),
        );
        view.set_diagnostics(AssetDoctor::diagnose(
            &self.markdown_document().parsed(),
            &context,
            Some(&self.local_asset_probe()),
        ));
    }

    /// `assetDoctorView(_:didSelect:)`.
    pub fn asset_doctor_view_did_select(&self, _view: &AssetDoctorView, diagnostic: &AssetDiagnostic) {
        let range = diagnostic.reference.image_range;
        if !(range.location >= 0 && range.upper_bound() <= self.markdown_document().storage().length() as isize) {
            return;
        }
        self.container_text_view().set_source_selected_ranges(&[range]);
        self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, true);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }

    /// `assetDoctorView(_:didReveal:)`.
    pub fn asset_doctor_view_did_reveal(&self, _view: &AssetDoctorView, diagnostic: &AssetDiagnostic) {
        let kind = diagnostic.reference.kind;
        if !(kind == AssetReferenceKind::RelativeLocal
            || kind == AssetReferenceKind::AbsoluteLocal
            || kind == AssetReferenceKind::FileUrl)
        {
            return;
        }
        let Some(url) = diagnostic.reference.url.clone() else { return };
        let target = url.clone();
        self.authorize_local_effect(TrustEffect::LaunchPathOrEditor, &target, move || {
            let urls = NSArray::from_retained_slice(&[url.to_nsurl()]);
            NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
        });
    }

    /// `assetDoctorView(_:didRequestProposal:for:)`.
    pub fn asset_doctor_view_did_request_proposal(
        &self,
        _view: &AssetDoctorView,
        kind: AssetProposalKind,
        diagnostic: &AssetDiagnostic,
    ) {
        let panel = NSOpenPanel::new(MainThreadMarker::from(self));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        panel.setMessage(Some(&NSString::from_str(if kind == AssetProposalKind::Relink {
            "Choose the replacement image."
        } else {
            "Choose the image path to use."
        })));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(url) = panel.URL().and_then(|url| FileUrl::from_nsurl(&url)) else { return };
        let Some(replacement) = self.relative_asset_path(&url) else { return };
        let proposal = if kind == AssetProposalKind::Relink {
            AssetDoctor::relink_proposal(&diagnostic.reference, &replacement)
        } else {
            AssetDoctor::rename_proposal(&diagnostic.reference, &replacement)
        };
        self.apply_asset_proposal(&proposal);
    }

    /// `assetDoctorView(_:didApply:)`.
    pub fn asset_doctor_view_did_apply(&self, _view: &AssetDoctorView, proposal: &AssetSourceProposal) {
        self.apply_asset_proposal(proposal);
    }

    /// `applyAssetProposal(_:)`.
    fn apply_asset_proposal(&self, proposal: &AssetSourceProposal) {
        if proposal.apply(&self.markdown_document().text()).is_none() {
            return;
        }
        let action_name = if proposal.kind == AssetProposalKind::Relink { "Relink Image" } else { "Rename Image" };
        if !self.markdown_document().replace(proposal.range, &proposal.replacement, Some(action_name)) {
            return;
        }
        self.markdown_document().reparse_now(false);
        self.refresh_derived_ui();
    }

    /// `relativeAssetPath(for:)`. Never `None`, as in Swift; the optional is
    /// the Swift signature's.
    fn relative_asset_path(&self, url: &FileUrl) -> Option<String> {
        let target = url.standardized_file_url().path();
        let Some(document_url) = self.markdown_document().url() else { return Some(target) };
        let base = document_url.deleting_last_path_component().standardized_file_url().path();
        let prefix = if swift::has_suffix(&base, "/") { base } else { base + "/" };
        if swift::has_prefix(&target, &prefix) {
            return Some(swift::drop_first(&target, swift::count(&prefix)).to_owned());
        }
        Some(target)
    }

    /// `localAssetProbe()`: `url.resourceValues(forKeys: [.isRegularFileKey,
    /// .isDirectoryKey, .fileSizeKey])`, read through Foundation.
    pub fn local_asset_probe(&self) -> AssetProbe {
        AssetProbe::new(|url| {
            let values = resource_values(url)?;
            let exists = values.is_regular_file == Some(true) || values.is_directory == Some(true);
            Some(AssetMetadata::new(
                exists,
                values.is_directory == Some(true),
                values.file_size,
                Some(url.path_extension()),
            ))
        })
    }
}

/// The three resource values the probe reads; `None` where Swift's
/// `URLResourceValues` property is `nil`.
struct ProbeValues {
    is_regular_file: Option<bool>,
    is_directory: Option<bool>,
    file_size: Option<i64>,
}

/// `try url.resourceValues(forKeys:)`; `None` where it throws.
fn resource_values(url: &FileUrl) -> Option<ProbeValues> {
    autoreleasepool(|_| {
        let ns_url = url.to_nsurl();
        // SAFETY: Foundation's immutable key constants.
        let (regular, directory, size) = unsafe { (NSURLIsRegularFileKey, NSURLIsDirectoryKey, NSURLFileSizeKey) };
        let keys: Retained<NSArray<NSURLResourceKey>> = NSArray::from_slice(&[regular, directory, size]);
        let values = ns_url.resourceValuesForKeys_error(&keys).ok()?;
        let number = |key: &NSURLResourceKey| -> Option<Retained<NSNumber>> {
            let value: Retained<AnyObject> = values.objectForKey(key)?;
            value.downcast::<NSNumber>().ok()
        };
        Some(ProbeValues {
            is_regular_file: number(regular).map(|value| value.boolValue()),
            is_directory: number(directory).map(|value| value.boolValue()),
            file_size: number(size).map(|value| value.integerValue() as i64),
        })
    })
}

/// `window.makeFirstResponder(responder)`.
pub(crate) fn make_first_responder(window: &NSWindow, responder: &NSResponder) -> bool {
    window.makeFirstResponder(Some(responder))
}

// MARK: - AssetDoctorViewDelegate

impl AssetDoctorViewDelegate for DocumentWindowControllerDelegates {
    fn asset_doctor_view_did_select(&self, view: &AssetDoctorView, diagnostic: &AssetDiagnostic) {
        if let Some(controller) = self.controller() {
            controller.asset_doctor_view_did_select(view, diagnostic);
        }
    }

    fn asset_doctor_view_did_reveal(&self, view: &AssetDoctorView, diagnostic: &AssetDiagnostic) {
        if let Some(controller) = self.controller() {
            controller.asset_doctor_view_did_reveal(view, diagnostic);
        }
    }

    fn asset_doctor_view_did_request_proposal(
        &self,
        view: &AssetDoctorView,
        kind: AssetProposalKind,
        diagnostic: &AssetDiagnostic,
    ) {
        if let Some(controller) = self.controller() {
            controller.asset_doctor_view_did_request_proposal(view, kind, diagnostic);
        }
    }

    fn asset_doctor_view_did_apply(&self, view: &AssetDoctorView, proposal: &AssetSourceProposal) {
        if let Some(controller) = self.controller() {
            controller.asset_doctor_view_did_apply(view, proposal);
        }
    }
}
