//! `UpdateStatusPill` scenes (`Scenes/UpdateStatusPillScene.swift`), plus
//! the helpers the three update scenes share (`UpdateScenes` in Swift).
//!
//! The pill draws from its own style sheet — the current theme against
//! `NSApp`'s appearance, with the *system* Reduce Motion — not the
//! harness's; the scene selects the scenario's theme in `ThemeStore::shared`
//! for this process ([`select_theme`]). With Reduce Motion off the pill
//! animates its width and shell alpha through `animator()`; the scene builds
//! it (and runs the steps) inside a zero-duration `NSAnimationContext`
//! group, so those changes land at once, as they do with Reduce Motion on.
//! The arrival emphasis never plays: the harness never activates the app.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAnimationContext, NSApplication, NSImage, NSProgressIndicator, NSView};
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSJSONReadingOptions, NSJSONSerialization, NSPoint, NSRect, NSSize, NSString,
    NSUserDefaults,
};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{activate, cg, downcast, object};
use upleft_app::panels::update_status_pill::{Presentation, UpdateStatusPill};
use upleft_app::updater::downright_update_driver::UpdateDriverHost;
use upleft_app::updater::update_coordinator::{UpdateCoordinator, UpdatePillModel};
use upleft_app::updater::update_engine::FakeUpdateEngine;
use upleft_app::updater::update_metadata::{UpdateMetadata, Url};
use upleft_app::updater::update_state_machine::UpdateStage;
use upleft_foundation::date::Date;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, repository_root, tree};

#[derive(Default)]
pub struct UpdateStatusPillScene {
    pill: Option<Retained<UpdateStatusPill>>,
}

impl PanelScene for UpdateStatusPillScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        select_theme(&scenario.theme);
        let coordinator = UpdateCoordinator::shared(mtm);
        coordinator.set_suppress_ui_for_testing(true);
        coordinator.tear_down_for_testing();
        let presentation = if scenario.string("presentation").as_deref() == Some("compactWarning") {
            Presentation::CompactWarning
        } else {
            Presentation::Standard
        };
        // A zero-duration group: the pill's `animator()` width and alpha
        // changes land at once instead of racing the settle loop.
        NSAnimationContext::beginGrouping();
        NSAnimationContext::currentContext().setDuration(0.0);
        let early = if scenario.bool("buildFirst") { Some(UpdateStatusPill::new(presentation, mtm)) } else { None };
        run(&scenario.array("steps"), &coordinator, None);
        let pill = early.unwrap_or_else(|| UpdateStatusPill::new(presentation, mtm));
        NSAnimationContext::endGrouping();
        self.pill = Some(pill.clone());

        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
        );
        container.setWantsLayer(true);
        if let Some(layer) = container.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.background)));
        }
        pill.setTranslatesAutoresizingMaskIntoConstraints(false);
        container.addSubview(&pill);
        activate(&[
            pill.trailingAnchor().constraintEqualToAnchor_constant(&container.trailingAnchor(), -12.0),
            pill.centerYAnchor().constraintEqualToAnchor(&container.centerYAnchor()),
        ]);
        Ok(container)
    }

    fn model(&self) -> Value {
        let Some(pill) = &self.pill else { return Value::Null };
        let mtm = MainThreadMarker::new().expect("main thread");
        let label = pill.label_for_testing();
        let progress = pill.progress_indicator_for_testing();
        let icon = pill.icon_view_for_testing();
        let coordinator = UpdateCoordinator::shared(mtm);
        let mut map = Map::new();
        map.insert("pill".into(), pill_json(coordinator.pill_model().as_ref()));
        map.insert("currentModel".into(), pill_json(pill.current_model_for_testing().as_ref()));
        map.insert("title".into(), Value::String(pill.title().to_string()));
        map.insert("intrinsicContentSize".into(), tree::size(pill.intrinsicContentSize()));
        map.insert("hidden".into(), Value::Bool(pill.isHidden()));
        map.insert("labelText".into(), Value::String(label.stringValue().to_string()));
        map.insert("labelHidden".into(), Value::Bool(label.isHidden()));
        map.insert("iconHidden".into(), Value::Bool(icon.isHidden()));
        map.insert("iconHasImage".into(), Value::Bool(icon.image().is_some()));
        map.insert("progressHidden".into(), Value::Bool(progress.isHidden()));
        map.insert("progressStyle".into(), Value::from(progress.style().0 as i64));
        map.insert("progressIndeterminate".into(), Value::Bool(progress.isIndeterminate()));
        map.insert("progressValue".into(), double(progress.doubleValue()));
        map.insert(
            "pendingUpdate".into(),
            coordinator.pending_update().map_or(Value::Null, |update| Value::String(update.display_version_string)),
        );
        Value::Object(map)
    }
}

// MARK: - Shared by the update scenes (`UpdateScenes`)

/// `UpdateScenes.selectTheme(_:)`: selects `name` in `ThemeStore::shared`
/// for this process only. The update surfaces draw from the current theme,
/// not the harness's style sheet; the selection is removed from the defaults
/// again so it never reaches another scene through the shared conform home.
pub fn select_theme(name: &str) {
    ThemeStore::shared().select(name);
    NSUserDefaults::standardUserDefaults().removeObjectForKey(&NSString::from_str("downright.theme.selected"));
}

/// `UpdateScenes.useBundleIcon()`: the bundle's icon as the application
/// icon, as the app-window harness sets it: an oracle binary has no bundle,
/// and AppKit would otherwise show the icon of the folder it runs from (a
/// symbolic link for the Swift oracle, a plain folder for `upleft-oracle`).
pub fn use_bundle_icon(mtm: MainThreadMarker) {
    let path = NSString::from_str(&repository_root().join("vendor/downright/Resources/AppIcon.icns").to_string_lossy());
    let icon = NSImage::initWithContentsOfFile(NSImage::alloc(), &path);
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(icon.as_deref()) };
}

/// `UpdateScenes.feedDescription(_:)`: the text between the first
/// `<![CDATA[` and the next `]]>` of a feed under `corpus/updater/feeds/`.
pub fn feed_description(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(repository_root().join(path)).ok()?;
    let start = text.find("<![CDATA[")? + "<![CDATA[".len();
    let end = start + text[start..].find("]]>")?;
    Some(text[start..end].to_owned())
}

/// `UpdateScenes.metadata(_:)`.
pub fn metadata(object: &Map<String, Value>) -> UpdateMetadata {
    let string = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    let flag = |key: &str| object.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mut description = string("itemDescription");
    if let Some(feed) = string("itemDescriptionFeed") {
        description = feed_description(&feed);
    }
    UpdateMetadata {
        version_string: string("versionString").unwrap_or_else(|| "47".into()),
        display_version_string: string("displayVersionString").unwrap_or_else(|| "1.1.0".into()),
        title: string("title"),
        item_description: description,
        release_notes_url: string("releaseNotesURL").and_then(|url| Url::from_string(&url)),
        info_url: string("infoURL").and_then(|url| Url::from_string(&url)),
        content_length: object.get("contentLength").and_then(Value::as_u64).unwrap_or(0),
        is_information_only: flag("isInformationOnly"),
        is_major_upgrade: false,
        is_critical: flag("isCritical"),
        minimum_system_version: None,
    }
}

/// A JSON value as `JSONSerialization` hands it to Swift.
fn foundation_dictionary(value: Option<&Value>) -> Option<Retained<NSDictionary<NSString, AnyObject>>> {
    let value = value.filter(|value| value.is_object())?;
    let text = serde_json::to_vec(value).ok()?;
    let object =
        NSJSONSerialization::JSONObjectWithData_options_error(&NSData::with_bytes(&text), NSJSONReadingOptions(0)).ok()?;
    let dictionary = object.downcast::<NSDictionary>().ok()?;
    // SAFETY: a JSON object's keys are strings.
    Some(unsafe { Retained::cast_unchecked(dictionary) })
}

/// `UpdateScenes.error(_:)`: `NSError(domain:code:userInfo:)`.
pub fn error(object: &Map<String, Value>) -> Retained<NSError> {
    let domain = object.get("domain").and_then(Value::as_str).unwrap_or("sparkle");
    let code = object.get("code").and_then(Value::as_i64).unwrap_or(0) as isize;
    let user_info = foundation_dictionary(object.get("userInfo"));
    // SAFETY: a string-keyed user info dictionary, as Swift's `[String: Any]`.
    unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(domain), code, user_info.as_deref()) }
}

fn stage(value: Option<&Value>) -> UpdateStage {
    match value.and_then(Value::as_str).unwrap_or("notDownloaded") {
        "downloaded" => UpdateStage::Downloaded,
        "installing" => UpdateStage::Installing,
        _ => UpdateStage::NotDownloaded,
    }
}

/// `UpdateScenes.run(_:on:engine:)`: driver callbacks (and fake-engine
/// settings) in order; every capability handed over is a no-op closure.
pub fn run(steps: &[Value], c: &Rc<UpdateCoordinator>, engine: Option<&FakeUpdateEngine>) {
    for step in steps {
        let Some(step) = step.as_object() else { continue };
        let flag = |key: &str, fallback: bool| step.get(key).and_then(Value::as_bool).unwrap_or(fallback);
        let length = || step.get("length").and_then(Value::as_u64).unwrap_or(0);
        match step.get("do").and_then(Value::as_str).unwrap_or("") {
            "driverDidBeginUserCheck" => c.driver_did_begin_user_check(Box::new(|| {})),
            "driverDidFindUpdate" => c.driver_did_find_update(
                metadata(&step.get("metadata").and_then(Value::as_object).cloned().unwrap_or_default()),
                stage(step.get("stage")),
                flag("userInitiated", true),
                Box::new(|_| {}),
            ),
            "driverDidReceiveReleaseNotes" => c.driver_did_receive_release_notes(
                step.get("text").and_then(Value::as_str).unwrap_or("").as_bytes().to_vec(),
            ),
            "driverDidFailToDownloadReleaseNotes" => {
                let error = unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str("sparkle"), 0, None) };
                c.driver_did_fail_to_download_release_notes(&error);
            }
            "driverDidFindNoUpdate" => c.driver_did_find_no_update(flag("userInitiated", false), Box::new(|| {})),
            "driverDidEncounterError" => c.driver_did_encounter_error(
                &error(&step.get("error").and_then(Value::as_object).cloned().unwrap_or_default()),
                Box::new(|| {}),
            ),
            "driverDidBeginDownload" => c.driver_did_begin_download(Box::new(|| {})),
            "driverDidReceiveExpectedLength" => c.driver_did_receive_expected_length(length()),
            "driverDidReceiveData" => c.driver_did_receive_data(length()),
            "driverDidBeginExtraction" => c.driver_did_begin_extraction(),
            "driverDidReceiveExtractionProgress" => {
                c.driver_did_receive_extraction_progress(step.get("progress").and_then(Value::as_f64).unwrap_or(0.0))
            }
            "driverDidBecomeReadyToRelaunch" => c.driver_did_become_ready_to_relaunch(Box::new(|_| {})),
            "driverDidBeginInstallation" => {
                c.driver_did_begin_installation(flag("applicationTerminated", false), Box::new(|| {}))
            }
            "completeBackgroundDownload" => {
                if let Some(engine) = engine {
                    engine.complete_background_download(step.get("version").and_then(Value::as_str).unwrap_or(""));
                }
            }
            "lastUpdateCheckDate" => {
                if let Some(engine) = engine {
                    let seconds = step.get("value").and_then(Value::as_f64).unwrap_or(0.0);
                    engine
                        .last_update_check_date
                        .set(Some(Date { time_interval_since_reference_date: seconds }));
                }
            }
            other => panic!("unknown update scene step {other}"),
        }
    }
}

/// `UpdateScenes.json(_:)` for a pill model.
pub fn pill_json(pill: Option<&UpdatePillModel>) -> Value {
    let Some(pill) = pill else { return Value::Null };
    let mut map = Map::new();
    match pill {
        UpdatePillModel::UpdateNow { version, is_ready } => {
            map.insert("pill".into(), "updateNow".into());
            map.insert("version".into(), Value::String(version.clone()));
            map.insert("isReady".into(), Value::Bool(*is_ready));
        }
        UpdatePillModel::RestartToUpdate => {
            map.insert("pill".into(), "restartToUpdate".into());
        }
        UpdatePillModel::Progress(label, fraction) => {
            map.insert("pill".into(), "progress".into());
            map.insert("label".into(), Value::String(label.clone()));
            map.insert("fraction".into(), fraction.map_or(Value::Null, double));
        }
        UpdatePillModel::Warning => {
            map.insert("pill".into(), "warning".into());
        }
        UpdatePillModel::Informational(version) => {
            map.insert("pill".into(), "informational".into());
            map.insert("version".into(), Value::String(version.clone()));
        }
    }
    Value::Object(map)
}

/// `UpdateScenes.stopIndicators(in:)`: a spinning indicator never settles,
/// so a capture freezes it where it stands.
pub fn stop_indicators(view: &NSView) {
    if let Some(indicator) = downcast::<NSProgressIndicator>(object(view)) {
        unsafe { indicator.stopAnimation(None) };
    }
    for subview in view.subviews().iter() {
        stop_indicators(&subview);
    }
}
