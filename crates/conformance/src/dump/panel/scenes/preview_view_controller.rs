//! `PreviewViewController` scenes (`Scenes/PreviewViewControllerScene.swift`):
//! the Quick Look preview extension's view controller (upleft-quicklook),
//! driven the way Quick Look drives it and hosted in the harness's
//! off-screen borderless window. See the Swift file for the state keys.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::{DynBlock, RcBlock};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSTextSelectionDataSource, NSView};
use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};
use objc2_foundation::{NSError, NSString, NSURL};
use serde_json::{Map, Value};
use upleft_quicklook::preview_view_controller::PreviewViewController;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, repository_root};

#[derive(Default)]
pub struct PreviewViewControllerScene {
    controller: Option<Retained<PreviewViewController>>,
    outcomes: Vec<Value>,
}

impl PreviewViewControllerScene {
    fn preview(&mut self, path: &std::path::Path, controller: &PreviewViewController) -> Result<(), Failure> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let outcome: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
        let sink = outcome.clone();
        let handler = RcBlock::new(move |error: *mut NSError| {
            // SAFETY: Quick Look's handler receives a live error or nil.
            let value = match unsafe { error.as_ref() } {
                Some(error) => {
                    let mut object = Map::new();
                    object.insert("domain".into(), Value::String(error.domain().to_string()));
                    object.insert("code".into(), Value::from(error.code() as i64));
                    Value::Object(object)
                }
                None => Value::Null,
            };
            *sink.borrow_mut() = Some(value);
        });
        let handler_ref: &DynBlock<dyn Fn(*mut NSError)> = &handler;
        // SAFETY: QLPreviewingController's selector and argument types.
        let _: () = unsafe { objc2::msg_send![controller, preparePreviewOfFileAtURL: &*url, completionHandler: handler_ref] };
        let deadline = Instant::now() + Duration::from_secs(30);
        while outcome.borrow().is_none() && Instant::now() < deadline {
            // `RunLoop.current.run(mode: .default, before: now + 0.005)`.
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, 0.005, true);
        }
        let Some(value) = outcome.borrow_mut().take() else {
            return Err(Failure::Error("the preview's completion handler never ran".into()));
        };
        self.outcomes.push(value);
        Ok(())
    }
}

impl PanelScene for PreviewViewControllerScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        _style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let Some(document_path) = scenario.document_path.clone() else {
            return Err(Failure::Error("a PreviewViewController scenario needs a document".into()));
        };
        let controller = PreviewViewController::new(mtm);
        self.controller = Some(controller.clone());
        if let Some(count) = scenario.int("repeat") {
            let text = scenario.document_text().map_err(Failure::Error)?.repeat(count.max(0) as usize);
            let temporary =
                std::env::temp_dir().join(format!("upleft-quicklook-preview-{}.md", std::process::id()));
            std::fs::write(&temporary, text)?;
            let result = self.preview(&temporary, &controller);
            let _ = std::fs::remove_file(&temporary);
            result?;
        } else {
            // `repositoryRoot.appendingPathComponent(documentPath)`.
            self.preview(&repository_root().join(&document_path), &controller)?;
        }
        if let Some(then) = scenario.string("then") {
            self.preview(&repository_root().join(then), &controller)?;
        }
        if scenario.bool("fallback") {
            controller.fall_back_to_plain_text_for_testing();
        }
        controller.retire_memory_watch_for_testing();
        Ok(controller.view())
    }

    /// `beforeSettleCheck()`: lay the whole document out and size the text
    /// view to it, then let the controller's own `viewDidLayout` refresh the
    /// gutter.
    fn before_settle_check(&mut self) {
        let Some(controller) = &self.controller else { return };
        let Some(container) = controller.container_for_testing() else { return };
        container.setNeedsLayout(true);
        container.layoutSubtreeIfNeeded();
        if let Some(layout) = container.text_view().textLayoutManager() {
            layout.ensureLayoutForRange(&layout.documentRange());
        }
        container.text_view().resize_to_fit_content();
        controller.view().setNeedsLayout(true);
        controller.view().layoutSubtreeIfNeeded();
    }

    fn model(&self) -> Value {
        let Some(controller) = &self.controller else { return Value::Null };
        let gutter = controller.density_gutter_for_testing();
        let container = controller.container_for_testing();
        let gutter_shown = match (&container, &gutter) {
            (Some(container), Some(gutter)) => container
                .leading_accessory()
                .is_some_and(|accessory| std::ptr::eq(&*accessory, &**gutter as &NSView)),
            _ => false,
        };
        let mut map = Map::new();
        map.insert("outcomes".into(), Value::Array(self.outcomes.clone()));
        map.insert("storageLength".into(), Value::from(controller.storage_for_testing().length() as i64));
        map.insert("container".into(), Value::Bool(container.is_some()));
        map.insert("fallback".into(), Value::Bool(controller.fallback_text_view_for_testing().is_some()));
        map.insert("noticeBar".into(), Value::Bool(controller.notice_bar_for_testing().is_some()));
        map.insert("gutterShown".into(), Value::Bool(gutter_shown));
        map.insert(
            "currentHeadingIndex".into(),
            controller.current_heading_index_for_testing().map_or(Value::Null, |index| Value::from(index as i64)),
        );
        map.insert("metricsSummary".into(), gutter.as_ref().map_or(Value::Null, |gutter| Value::String(gutter.metrics_summary())));
        map.insert(
            "visibleRange".into(),
            gutter.as_ref().map_or(Value::Null, |gutter| {
                let (lower, upper) = gutter.visible_range();
                Value::Array(vec![double(lower), double(upper)])
            }),
        );
        map.insert("readProgress".into(), gutter.as_ref().map_or(Value::Null, |gutter| double(gutter.read_progress())));
        map.insert(
            "outline".into(),
            gutter.as_ref().map_or(Value::Null, |gutter| {
                Value::Array(
                    gutter
                        .outline_entries()
                        .iter()
                        .map(|entry| {
                            Value::Array(vec![
                                Value::String(entry.title.clone()),
                                Value::from(entry.level as i64),
                                double(entry.fraction),
                                Value::Bool(entry.is_current),
                            ])
                        })
                        .collect(),
                )
            }),
        );
        Value::Object(map)
    }
}
