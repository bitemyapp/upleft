//! Port of `App/DocumentWindowController+Share.swift`. Not ported yet.
//!
//! The `NSSharingServicePickerDelegate`/`NSSharingServiceDelegate` methods
//! are declared in `document_window_controller.rs`'s `define_class!` and
//! forward to the methods below.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSSharingContentScope, NSSharingService, NSSharingServicePicker, NSWindow};
use objc2_foundation::NSArray;

use crate::app::document_window_controller::DocumentWindowController;

/// The extension's associated-object state (`sharingPicker`), held by the
/// controller as `share_state()`.
// PORT: DocumentWindowController+Share.swift fills this in.
#[derive(Default)]
pub struct ShareState {}

impl DocumentWindowController {
    /// `sharingServicePicker(_:delegateFor:)`.
    pub fn sharing_service_picker_delegate_for(
        &self,
        _picker: &NSSharingServicePicker,
        _service: &NSSharingService,
    ) -> Option<Retained<AnyObject>> {
        // PORT: DocumentWindowController+Share.swift
        None
    }

    /// `sharingServicePicker(_:didChoose:)`.
    pub fn sharing_service_picker_did_choose(
        &self,
        _picker: &NSSharingServicePicker,
        _service: Option<&NSSharingService>,
    ) {
        // PORT: DocumentWindowController+Share.swift
    }

    /// `sharingService(_:sourceWindowForShareItems:sharingContentScope:)`.
    pub fn sharing_service_source_window(
        &self,
        _service: &NSSharingService,
        _items: &NSArray,
        _scope: NonNull<NSSharingContentScope>,
    ) -> Option<Retained<NSWindow>> {
        // PORT: DocumentWindowController+Share.swift
        None
    }
}
