//! `DocumentQuickLook` scenes (`Scenes/DocumentQuickLookScene.swift`):
//! `QuickLookRequest.resolve` over a list of targets; `panel-model` only.

use std::rc::Rc;

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::ai::path_resolver::PathResolver;
use upleft_app::panels::document_quick_look::QuickLookRequest;
use upleft_core::{NSRange, PathToken};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_text_view_delegate::{ContextTarget, ContextTargetKind};

use super::document_health_view::{document_url, relative};
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct DocumentQuickLookScene {
    results: Vec<Value>,
}

/// `DocumentQuickLookScene.target(_:)`.
pub fn target(value: &Value) -> Option<ContextTarget> {
    let object = value.as_object()?;
    let kind = object.get("kind")?.as_str()?;
    let text = object.get("value").and_then(Value::as_str).unwrap_or("").to_owned();
    let range = NSRange::new(
        object.get("location").and_then(Value::as_i64).unwrap_or(0) as isize,
        object.get("length").and_then(Value::as_i64).unwrap_or(0) as isize,
    );
    let target_kind = match kind {
        "image" => ContextTargetKind::Image(text),
        "pathToken" => ContextTargetKind::PathToken(PathToken::new(
            text,
            object.get("line").and_then(Value::as_i64).map(|line| line as isize),
            None,
        )),
        "link" => ContextTargetKind::Link(text),
        "heading" => ContextTargetKind::Heading(object.get("heading").and_then(Value::as_i64).unwrap_or(0) as usize),
        "codeBlock" => ContextTargetKind::CodeBlock(range),
        "table" => ContextTargetKind::Table(range),
        "selection" => ContextTargetKind::Selection,
        "plain" => ContextTargetKind::Plain,
        _ => return None,
    };
    Some(ContextTarget::new(target_kind, range, None))
}

/// `DocumentQuickLookScene.describe(_:)`.
pub fn describe(request: Option<QuickLookRequest>) -> Value {
    let mut map = Map::new();
    match request {
        None => return Value::Null,
        Some(QuickLookRequest::Lightbox { source }) => {
            map.insert("request".into(), Value::String("lightbox".into()));
            map.insert("source".into(), Value::String(source));
        }
        Some(QuickLookRequest::Panel(url)) => {
            map.insert("request".into(), Value::String("panel".into()));
            map.insert("path".into(), Value::String(relative(&url.path())));
            map.insert("directory".into(), Value::Bool(url.has_directory_path()));
        }
    }
    Value::Object(map)
}

impl PanelScene for DocumentQuickLookScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        _style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let document_url = document_url(scenario);
        let resolver = PathResolver::new(document_url.as_ref());
        self.results = scenario
            .array("targets")
            .iter()
            .map(|value| {
                let Some(target) = target(value) else { return Value::String("invalid target".into()) };
                describe(QuickLookRequest::resolve(
                    &target,
                    document_url.as_ref(),
                    &|token| Some(resolver.resolve(token)),
                    &|url| upleft_foundation::file_manager::file_exists(&url.path()),
                ))
            })
            .collect();
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
        );
        Ok(view)
    }

    fn model(&self) -> Value {
        let mut map = Map::new();
        map.insert("results".into(), Value::Array(self.results.clone()));
        Value::Object(map)
    }
}
