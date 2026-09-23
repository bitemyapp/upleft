//! Port of `App/DocumentWindowController+ReaderProfiles.swift`.
//!
//! The active profile is presentation state. It is not written into the
//! markdown document and does not select or modify a color theme.
//!
//! Swift keeps a private `ReaderProfileControllerState` object as an
//! associated object, created on first use. Here it is
//! [`ReaderProfileControllerState`] (shared through an `Rc`, as the Swift
//! class reference is), held by [`ReaderProfilesState`].
//!
//! `ReaderProfilePickerDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
#[allow(deprecated)]
use objc2_app_kit::NSToolbarSizeMode;
use objc2_app_kit::{NSAppearanceCustomization, NSApplication, NSToolbarDisplayMode};
use upleft_render::render_contracts::Theme;
use upleft_render::swift_compat::{smax, smin};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::panels::panel_chrome::panel_title;
use crate::panels::reader_profile_picker_view::{ReaderProfilePickerDelegate, ReaderProfilePickerView};
use crate::support::app_paths;
use crate::support::commands::Command;
use crate::support::reader_profiles::{
    JSONReaderProfileStore, ReaderChromeDensity, ReaderMotionPreference, ReaderProfile, ReaderProfileStore,
};

/// `private final class ReaderProfileControllerState`.
pub struct ReaderProfileControllerState {
    pub(crate) profile: RefCell<ReaderProfile>,
    pub(crate) base_theme: RefCell<Option<Theme>>,
    /// The open picker, so the command can close the one it opened. The
    /// picker's delegate reference back to the controller is weak.
    pub(crate) picker: RefCell<Option<Retained<ReaderProfilePickerView>>>,
    pub(crate) store: RefCell<Rc<dyn ReaderProfileStore>>,
}

impl ReaderProfileControllerState {
    /// `ReaderProfileControllerState()`: the first built-in profile and the
    /// JSON store beside the other support files.
    pub fn new() -> ReaderProfileControllerState {
        ReaderProfileControllerState {
            profile: RefCell::new(ReaderProfile::built_ins()[0].clone()),
            base_theme: RefCell::new(None),
            picker: RefCell::new(None),
            store: RefCell::new(Rc::new(JSONReaderProfileStore::new(
                app_paths::support_directory().appending_path_component("reader-profiles.json"),
            ))),
        }
    }
}

impl Default for ReaderProfileControllerState {
    fn default() -> Self {
        ReaderProfileControllerState::new()
    }
}

/// The extension's associated-object state (`ReaderProfileControllerState`,
/// created on first use), held by the controller as
/// `reader_profiles_state()`.
#[derive(Default)]
pub struct ReaderProfilesState {
    pub(crate) controller_state: Option<Rc<ReaderProfileControllerState>>,
}

impl DocumentWindowController {
    /// `readerProfileState`: made on first use.
    pub(crate) fn reader_profile_state(&self) -> Rc<ReaderProfileControllerState> {
        if let Some(state) = self.reader_profiles_state().borrow().controller_state.clone() {
            return state;
        }
        let state = Rc::new(ReaderProfileControllerState::new());
        self.reader_profiles_state().borrow_mut().controller_state = Some(state.clone());
        state
    }

    /// `readerProfile`.
    pub fn reader_profile(&self) -> ReaderProfile {
        self.reader_profile_state().profile.borrow().clone()
    }

    /// `showReaderProfiles()`: the command toggles, like every other panel
    /// command.
    pub fn show_reader_profiles(&self) {
        let existing = self.reader_profile_state().picker.borrow().clone();
        if let Some(picker) = existing {
            self.dismiss_trailing(&picker);
            *self.reader_profile_state().picker.borrow_mut() = None;
            return;
        }
        let store = self.reader_profile_state().store.borrow().clone();
        let picker = ReaderProfilePickerView::new(store, self.active_style_sheet(), MainThreadMarker::from(self));
        let delegates = self.delegates();
        picker.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn ReaderProfilePickerDelegate>));
        *self.reader_profile_state().picker.borrow_mut() = Some(picker.clone());
        // Establishes the Revert baseline: without a selection the picker has
        // nothing to revert a preview to.
        picker.select_profile(&self.reader_profile().id);
        self.install_trailing(&picker, Some(&panel_title(Command::ReaderProfiles)));
    }

    /// `applyReaderProfile(_:)`.
    pub fn apply_reader_profile(&self, profile: &ReaderProfile) {
        let state = self.reader_profile_state();
        *state.profile.borrow_mut() = profile.clone();
        let active_name = self.active_style_sheet().theme.name.clone();
        let rebase = match &*state.base_theme.borrow() {
            None => true,
            Some(base) => !swift::str_eq(&base.name, &active_name),
        };
        if rebase {
            *state.base_theme.borrow_mut() = Some(self.active_style_sheet().theme.clone());
        }

        let mut theme = state.base_theme.borrow().clone().unwrap_or_else(|| self.active_style_sheet().theme.clone());
        let mut typography = theme.typography.clone();
        typography.body_size = smax(10.0, smin(28.0, typography.body_size * profile.typography_scale.value()));
        typography.measure_characters = profile.measure_characters;
        theme.typography = typography;
        let reduce_motion =
            if profile.motion_preference == ReaderMotionPreference::Reduced { Some(true) } else { None };
        let appearance = match self.window() {
            Some(window) => window.effectiveAppearance(),
            None => NSApplication::sharedApplication(MainThreadMarker::from(self)).effectiveAppearance(),
        };
        let sheet = StyleSheet::new(theme, &appearance, reduce_motion);
        self.set_active_style_sheet(Rc::new(sheet));
        self.apply_style_sheet();

        if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
            toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        }
        if let Some(toolbar) = self.window().and_then(|window| window.toolbar()) {
            #[allow(deprecated)]
            toolbar.setSizeMode(if profile.chrome_density == ReaderChromeDensity::Compact {
                NSToolbarSizeMode::Small
            } else {
                NSToolbarSizeMode::Regular
            });
        }
        self.breadcrumb_view().setHidden(profile.chrome_density == ReaderChromeDensity::Compact);
    }

    /// `readerProfilePicker(_:didPreview:)`.
    pub fn reader_profile_picker_did_preview(&self, _picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        self.apply_reader_profile(profile);
    }

    /// `readerProfilePicker(_:didSelect:)`.
    pub fn reader_profile_picker_did_select(&self, _picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        self.apply_reader_profile(profile);
    }
}

// MARK: - ReaderProfilePickerDelegate

impl ReaderProfilePickerDelegate for DocumentWindowControllerDelegates {
    fn reader_profile_picker_did_preview(&self, picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        if let Some(controller) = self.controller() {
            controller.reader_profile_picker_did_preview(picker, profile);
        }
    }

    fn reader_profile_picker_did_select(&self, picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        if let Some(controller) = self.controller() {
            controller.reader_profile_picker_did_select(picker, profile);
        }
    }
}
