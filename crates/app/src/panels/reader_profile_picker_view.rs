//! Port of `Panels/ReaderProfilePickerView.swift`: a transient,
//! keyboard-first profile picker.  Profiles own reader metrics; themes and
//! colors stay in `ThemeStore`.
//!
//! `store` is `Rc<dyn ReaderProfileStore>` (Swift holds the protocol
//! existential strongly).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSControlSize, NSLayoutAttribute, NSLayoutYAxisAnchor, NSPopUpButton, NSResponder,
    NSStackView, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSRect, NSString};
use upleft_core::contracts::Uuid;
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role};
use super::panel_chrome::{
    PanelBackdrop, PanelFont, PanelMetrics, PanelSurface, enclosing_inspector_host, install_backdrop, panel_title,
};
use crate::ai::change_tracker::uuid_string;
use crate::support::app_paths;
use crate::support::commands::Command;
use crate::support::reader_profiles::{
    JSONReaderProfileStore, ReaderChromeDensity, ReaderMotionPreference, ReaderProfile, ReaderProfileStore,
    ReaderTypographyScale,
};

/// `ReaderProfilePickerDelegate`.
pub trait ReaderProfilePickerDelegate {
    fn reader_profile_picker_did_preview(&self, picker: &ReaderProfilePickerView, profile: &ReaderProfile);
    fn reader_profile_picker_did_select(&self, picker: &ReaderProfilePickerView, profile: &ReaderProfile);
}

pub struct ReaderProfilePickerViewIvars {
    delegate: RefCell<Option<Weak<dyn ReaderProfilePickerDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    selected_profile: RefCell<ReaderProfile>,
    /// The profile that was active when the picker opened.  Every control
    /// here previews live into the document, so there has to be a way back
    /// (§11.4).
    initial_profile: RefCell<Option<ReaderProfile>>,
    custom_profiles: RefCell<Vec<ReaderProfile>>,
    store: Rc<dyn ReaderProfileStore>,
    built_ins: Vec<ReaderProfile>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    detail_label: Retained<NSTextField>,
    profile_popup: Retained<NSPopUpButton>,
    name_field: Retained<NSTextField>,
    save_button: Retained<NSButton>,
    delete_button: Retained<NSButton>,
    revert_button: Retained<NSButton>,
    done_button: Retained<NSButton>,
    controls: RefCell<Vec<Retained<NSPopUpButton>>>,
    is_reloading: Cell<bool>,
}

define_class!(
    /// `ReaderProfilePickerView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ReaderProfilePickerView"]
    #[ivars = ReaderProfilePickerViewIvars]
    pub struct ReaderProfilePickerView;

    unsafe impl NSObjectProtocol for ReaderProfilePickerView {}

    impl ReaderProfilePickerView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        #[unsafe(method(profileChanged:))]
        fn __profile_changed(&self, sender: &NSPopUpButton) {
            self.profile_changed(sender);
        }

        #[unsafe(method(controlChanged:))]
        fn __control_changed(&self, sender: &NSPopUpButton) {
            self.control_changed(sender);
        }

        #[unsafe(method(revert:))]
        fn __revert(&self, _sender: Option<&AnyObject>) {
            self.revert();
        }

        #[unsafe(method(finish:))]
        fn __finish(&self, _sender: Option<&AnyObject>) {
            if let Some(host) = enclosing_inspector_host(self) {
                host.request_close();
            }
        }

        #[unsafe(method(saveCustom:))]
        fn __save_custom(&self, _sender: Option<&AnyObject>) {
            self.save_custom();
        }

        #[unsafe(method(deleteCustom:))]
        fn __delete_custom(&self, _sender: Option<&AnyObject>) {
            self.delete_custom();
        }
    }
);

impl PanelSurface for ReaderProfilePickerView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::LIST_WIDTH
    }
}

fn button(title: &str, mtm: MainThreadMarker) -> Retained<NSButton> {
    unsafe { NSButton::buttonWithTitle_target_action(&ns_string(title), None, None, mtm) }
}

fn titles_array(titles: &[&str]) -> Retained<NSArray<NSString>> {
    let titles: Vec<Retained<NSString>> = titles.iter().map(|title| ns_string(title)).collect();
    NSArray::from_retained_slice(&titles)
}

impl ReaderProfilePickerView {
    /// `ReaderProfilePickerView()`: the JSON store in the support folder
    /// (read on the calling thread, as Swift does).
    pub fn new_default(mtm: MainThreadMarker) -> Retained<ReaderProfilePickerView> {
        let url = app_paths::support_directory().appending_path_component("reader-profiles.json");
        Self::new(Rc::new(JSONReaderProfileStore::new(url)), Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(store:styleSheet:)`; Swift's default sheet is `.current`.
    pub fn new(
        store: Rc<dyn ReaderProfileStore>,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<ReaderProfilePickerView> {
        let built_ins = ReaderProfile::built_ins();
        let title_label = label(&panel_title(Command::ReaderProfiles), mtm);
        let detail_label = label("Presentation settings only", mtm);
        let profile_popup = NSPopUpButton::new(mtm);
        let name_field = NSTextField::new(mtm);
        let save_button = button("Save as Custom", mtm);
        let delete_button = button("Delete Custom", mtm);
        let revert_button = button("Revert", mtm);
        let done_button = button("Done", mtm);
        let custom_profiles = store.load_custom_profiles();
        let selected_profile = ReaderProfile::built_ins()[0].clone();
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(ReaderProfilePickerViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            selected_profile: RefCell::new(selected_profile),
            initial_profile: RefCell::new(None),
            custom_profiles: RefCell::new(custom_profiles),
            store,
            built_ins,
            backdrop,
            title_label,
            detail_label,
            profile_popup,
            name_field,
            save_button,
            delete_button,
            revert_button,
            done_button,
            controls: RefCell::new(Vec::new()),
            is_reloading: Cell::new(false),
        });
        let this: Retained<ReaderProfilePickerView> =
            unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.ivars().delete_button.setHasDestructiveAction(true);
        this.build_interface(mtm);
        this.reload_profile_list();
        this.apply_style();
        set_role(&*this, role::group());
        set_label(&*this, "Reader profile picker");
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn ReaderProfilePickerDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn ReaderProfilePickerDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.apply_style();
    }

    /// `selectedProfile` (`private(set)`).
    pub fn selected_profile(&self) -> ReaderProfile {
        self.ivars().selected_profile.borrow().clone()
    }

    fn set_selected_profile(&self, profile: ReaderProfile) {
        *self.ivars().selected_profile.borrow_mut() = profile;
    }

    /// `profiles`: the built-ins, then the custom profiles.
    pub fn profiles(&self) -> Vec<ReaderProfile> {
        let mut profiles = self.ivars().built_ins.clone();
        profiles.extend(self.ivars().custom_profiles.borrow().iter().cloned());
        profiles
    }

    pub fn custom_profiles(&self) -> Vec<ReaderProfile> {
        self.ivars().custom_profiles.borrow().clone()
    }

    pub fn set_custom_profiles(&self, profiles: Vec<ReaderProfile>) {
        *self.ivars().custom_profiles.borrow_mut() = profiles;
        self.reload_profile_list();
    }

    pub fn select_profile(&self, id: &str) {
        let Some(profile) = self.profiles().into_iter().find(|profile| profile.id == id) else { return };
        self.set_selected_profile(profile.clone());
        // The first selection is the host telling the picker what was
        // already active; that is the state Revert returns to.
        if self.ivars().initial_profile.borrow().is_none() {
            *self.ivars().initial_profile.borrow_mut() = Some(profile.clone());
        }
        self.reload_profile_list();
        self.update_revert_state();
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_select(self, &profile);
        }
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_preview(self, &profile);
        }
    }

    pub fn update_preview(&self) {
        let profile = self.selected_profile();
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_preview(self, &profile);
        }
    }

    pub fn save_custom_profile_for_testing(&self, name: &str) {
        self.ivars().name_field.setStringValue(&ns_string(name));
        self.save_custom();
    }

    fn build_interface(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        install_backdrop(self, &ivars.backdrop);

        for (field, font) in [(&ivars.title_label, PanelFont::title()), (&ivars.detail_label, PanelFont::secondary())] {
            field.setFont(Some(&font));
            field.setTranslatesAutoresizingMaskIntoConstraints(false);
            self.addSubview(field);
        }

        let profile_popup = &ivars.profile_popup;
        self.target(profile_popup, sel!(profileChanged:));
        set_label(&**profile_popup, "Reader profile");
        profile_popup.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(profile_popup);

        let name_field = &ivars.name_field;
        name_field.setPlaceholderString(Some(&ns_string("Custom profile name")));
        name_field.setFont(Some(&PanelFont::row()));
        set_label(&**name_field, "Custom profile name");
        name_field.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(name_field);

        let save_button = &ivars.save_button;
        save_button.setBezelStyle(NSBezelStyle::Push);
        self.target(save_button, sel!(saveCustom:));
        set_label(&**save_button, "Save reader profile as custom");
        save_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(save_button);

        let delete_button = &ivars.delete_button;
        delete_button.setBezelStyle(NSBezelStyle::Push);
        self.target(delete_button, sel!(deleteCustom:));
        set_label(&**delete_button, "Delete custom reader profile");
        delete_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(delete_button);

        let scale_titles: Vec<&str> = ReaderTypographyScale::ALL_CASES.iter().map(|scale| scale.title()).collect();
        self.add_control("Typography scale", &scale_titles, mtm);
        self.add_control("Measure width", &["68 characters", "70 characters", "72 characters"], mtm);
        let density_titles: Vec<&str> = ReaderChromeDensity::ALL_CASES.iter().map(|density| density.title()).collect();
        self.add_control("Chrome density", &density_titles, mtm);
        let motion_titles: Vec<&str> = ReaderMotionPreference::ALL_CASES.iter().map(|motion| motion.title()).collect();
        self.add_control("Motion", &motion_titles, mtm);

        // Every control above writes straight into the live style sheet, so
        // the panel owes the reader both a way back and a way out.
        let revert_button = &ivars.revert_button;
        revert_button.setBezelStyle(NSBezelStyle::Push);
        revert_button.setControlSize(NSControlSize::Small);
        self.target(revert_button, sel!(revert:));
        set_label(&**revert_button, "Revert to the profile that was active");
        let done_button = &ivars.done_button;
        done_button.setBezelStyle(NSBezelStyle::Push);
        done_button.setControlSize(NSControlSize::Small);
        done_button.setKeyEquivalent(&NSString::from_str("\r"));
        self.target(done_button, sel!(finish:));
        set_label(&**done_button, "Close the reader profile picker");

        let views: [Retained<NSView>; 2] = [
            Retained::into_super(Retained::into_super(revert_button.clone())),
            Retained::into_super(Retained::into_super(done_button.clone())),
        ];
        let dismiss_row = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&views), mtm);
        dismiss_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        dismiss_row.setAlignment(NSLayoutAttribute::CenterY);
        dismiss_row.setSpacing(8.0);
        dismiss_row.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&dismiss_row);

        let last_bottom: Retained<NSLayoutYAxisAnchor> = match ivars.controls.borrow().last() {
            Some(control) => control.bottomAnchor(),
            None => save_button.bottomAnchor(),
        };
        activate(&[
            dismiss_row
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            dismiss_row
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            dismiss_row.topAnchor().constraintEqualToAnchor_constant(&last_bottom, 14.0),
            dismiss_row
                .bottomAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&self.bottomAnchor(), -PanelMetrics::INSET),
        ]);

        let title_label = &ivars.title_label;
        let detail_label = &ivars.detail_label;
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 10.0),
            detail_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            detail_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 2.0),
            profile_popup.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            profile_popup
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            profile_popup.topAnchor().constraintEqualToAnchor_constant(&detail_label.bottomAnchor(), 12.0),
            name_field.leadingAnchor().constraintEqualToAnchor(&profile_popup.leadingAnchor()),
            name_field.trailingAnchor().constraintEqualToAnchor(&profile_popup.trailingAnchor()),
            name_field.topAnchor().constraintEqualToAnchor_constant(&profile_popup.bottomAnchor(), 8.0),
            save_button.leadingAnchor().constraintEqualToAnchor(&name_field.leadingAnchor()),
            save_button.topAnchor().constraintEqualToAnchor_constant(&name_field.bottomAnchor(), 6.0),
            delete_button.trailingAnchor().constraintEqualToAnchor(&name_field.trailingAnchor()),
            delete_button.centerYAnchor().constraintEqualToAnchor(&save_button.centerYAnchor()),
        ]);
        // No key equivalent here: this button used to claim ⌘⇧S and take
        // Save As… away from the document for as long as the panel was open
        // (§7.2).
    }

    /// `control.target = self; control.action = action`.
    fn target(&self, control: &objc2_app_kit::NSControl, action: Sel) {
        unsafe {
            control.setTarget(Some(object(self)));
            control.setAction(Some(action));
        }
    }

    fn add_control(&self, text: &str, titles: &[&str], mtm: MainThreadMarker) {
        let name = label(text, mtm);
        name.setFont(Some(&PanelFont::row()));
        name.setTranslatesAutoresizingMaskIntoConstraints(false);
        let popup = NSPopUpButton::new(mtm);
        popup.addItemsWithTitles(&titles_array(titles));
        self.target(&popup, sel!(controlChanged:));
        set_label(&*popup, text);
        popup.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.ivars().controls.borrow_mut().push(popup.clone());
        self.addSubview(&name);
        self.addSubview(&popup);
        let top: Retained<NSLayoutYAxisAnchor> = {
            let controls = self.ivars().controls.borrow();
            let previous = if controls.len() >= 2 { controls.get(controls.len() - 2) } else { None };
            match previous {
                Some(previous) => previous.bottomAnchor(),
                None => self.ivars().save_button.bottomAnchor(),
            }
        };
        activate(&[
            name.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            name.topAnchor().constraintEqualToAnchor_constant(&top, 10.0),
            popup.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            popup.centerYAnchor().constraintEqualToAnchor(&name.centerYAnchor()),
            popup.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&name.trailingAnchor(), 8.0),
            popup.widthAnchor().constraintEqualToConstant(150.0),
        ]);
    }

    fn control(&self, index: usize) -> Option<Retained<NSPopUpButton>> {
        self.ivars().controls.borrow().get(index).cloned()
    }

    fn reload_profile_list(&self) {
        let ivars = self.ivars();
        ivars.is_reloading.set(true);
        let profiles = self.profiles();
        ivars.profile_popup.removeAllItems();
        let names: Vec<&str> = profiles.iter().map(|profile| profile.name.as_str()).collect();
        ivars.profile_popup.addItemsWithTitles(&titles_array(&names));
        let selected_id = self.selected_profile().id;
        let index = profiles.iter().position(|profile| profile.id == selected_id).unwrap_or(0);
        ivars.profile_popup.selectItemAtIndex(index as isize);
        self.load_controls();
        ivars.is_reloading.set(false);
    }

    fn load_controls(&self) {
        let ivars = self.ivars();
        ivars.is_reloading.set(true);
        let selected = self.selected_profile();
        if let Some(control) = self.control(0) {
            let index = ReaderTypographyScale::ALL_CASES.iter().position(|scale| *scale == selected.typography_scale);
            control.selectItemAtIndex(index.unwrap_or(0) as isize);
        }
        // Swift's `Int(_:)` truncates (and traps on a non-finite value,
        // which a decoded profile cannot hold).
        let measure = [68i64, 70, 72].iter().position(|value| *value == selected.measure_characters as i64).unwrap_or(1);
        if let Some(control) = self.control(1) {
            control.selectItemAtIndex(measure as isize);
        }
        if let Some(control) = self.control(2) {
            let index = ReaderChromeDensity::ALL_CASES.iter().position(|density| *density == selected.chrome_density);
            control.selectItemAtIndex(index.unwrap_or(0) as isize);
        }
        if let Some(control) = self.control(3) {
            let index =
                ReaderMotionPreference::ALL_CASES.iter().position(|motion| *motion == selected.motion_preference);
            control.selectItemAtIndex(index.unwrap_or(0) as isize);
        }
        ivars.name_field.setStringValue(&ns_string(if selected.is_built_in { "" } else { &selected.name }));
        ivars.delete_button.setEnabled(!selected.is_built_in);
        ivars.is_reloading.set(false);
    }

    fn profile_changed(&self, sender: &NSPopUpButton) {
        let index = sender.indexOfSelectedItem();
        let profiles = self.profiles();
        if self.ivars().is_reloading.get() || !(index >= 0 && (index as usize) < profiles.len()) {
            return;
        }
        let profile = profiles[index as usize].clone();
        self.set_selected_profile(profile);
        self.load_controls();
        let selected = self.selected_profile();
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_select(self, &selected);
        }
        self.update_preview();
    }

    fn control_changed(&self, sender: &NSPopUpButton) {
        if self.ivars().is_reloading.get() {
            return;
        }
        let index = sender.indexOfSelectedItem();
        if index < 0 {
            return;
        }
        let index = index as usize;
        let mut profile = self.selected_profile();
        let is = |position: usize| self.control(position).is_some_and(|control| std::ptr::eq(&*control, sender));
        if is(0) {
            profile.typography_scale = ReaderTypographyScale::ALL_CASES[index];
        } else if is(1) {
            profile.measure_characters = [68.0, 70.0, 72.0][index];
        } else if is(2) {
            profile.chrome_density = ReaderChromeDensity::ALL_CASES[index];
        } else if is(3) {
            profile.motion_preference = ReaderMotionPreference::ALL_CASES[index];
        }
        self.set_selected_profile(profile);
        self.update_revert_state();
        self.update_preview();
    }

    // MARK: - Revert and dismiss

    fn revert(&self) {
        let Some(initial_profile) = self.ivars().initial_profile.borrow().clone() else { return };
        self.set_selected_profile(initial_profile.clone());
        self.reload_profile_list();
        self.update_revert_state();
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_select(self, &initial_profile);
        }
        self.update_preview();
    }

    fn update_revert_state(&self) {
        let enabled = match &*self.ivars().initial_profile.borrow() {
            Some(initial) => *initial != self.selected_profile(),
            None => false,
        };
        self.ivars().revert_button.setEnabled(enabled);
    }

    fn save_custom(&self) {
        let ivars = self.ivars();
        let text = ivars.name_field.stringValue().to_string();
        let name = upleft_swift_text::trim_whitespaces_and_newlines(&text).to_owned();
        let mut profile = self.selected_profile();
        profile.id = uuid_string(&Uuid::new_v4());
        profile.name = if name.is_empty() { "Custom".to_owned() } else { name };
        profile.is_built_in = false;
        let mut custom = self.custom_profiles();
        custom.push(profile.clone());
        self.set_custom_profiles(custom);
        ivars.store.save_custom_profiles(&self.custom_profiles());
        self.set_selected_profile(profile.clone());
        self.reload_profile_list();
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_select(self, &profile);
        }
        self.update_preview();
    }

    fn delete_custom(&self) {
        let selected = self.selected_profile();
        if selected.is_built_in {
            return;
        }
        let mut custom = self.custom_profiles();
        custom.retain(|profile| profile.id != selected.id);
        self.set_custom_profiles(custom);
        self.ivars().store.save_custom_profiles(&self.custom_profiles());
        self.set_selected_profile(self.ivars().built_ins[0].clone());
        self.reload_profile_list();
        let selected = self.selected_profile();
        if let Some(delegate) = self.delegate() {
            delegate.reader_profile_picker_did_select(self, &selected);
        }
        self.update_preview();
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text));
        ivars.detail_label.setTextColor(Some(&style_sheet.text_secondary));
    }

    /// The profile menu and the four metric menus, for tests and the
    /// conformance scene (they pick an item and send its action, as a click
    /// does).
    pub fn profile_popup_for_testing(&self) -> Retained<NSPopUpButton> {
        self.ivars().profile_popup.clone()
    }

    pub fn controls_for_testing(&self) -> Vec<Retained<NSPopUpButton>> {
        self.ivars().controls.borrow().clone()
    }
}
