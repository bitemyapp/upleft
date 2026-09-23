//! `DocumentHealthView` scenes (`Scenes/DocumentHealthViewScene.swift`),
//! plus the helpers the diagnostics-group scenes share
//! (`DiagnosticsSceneSupport` in the Swift file).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::msg_send;
use objc2_app_kit::{NSTableView, NSView};
use objc2_core_foundation::CGFloat;
use serde_json::{Map, Value};
use upleft_app::assets::asset_resolver::{AssetMetadata, AssetProbe};
use upleft_app::panels::appkit_support::accessibility_label;
use upleft_app::panels::document_health_view::DocumentHealthView;
use upleft_core::health::document_health::DocumentHealth;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, repository_root, tree};

#[derive(Default)]
pub struct DocumentHealthViewScene {
    view: Option<Retained<DocumentHealthView>>,
}

impl PanelScene for DocumentHealthViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = scenario.document_text().map_err(Failure::Error)?;
        let view = DocumentHealthView::new(style_sheet.clone(), mtm);
        view.set_source_text(&text);
        let mut findings = DocumentHealth::analyze_document(&MarkdownParser::parse(&text));
        if let Some(limit) = scenario.int("limit") {
            findings.truncate(limit.max(0) as usize);
        }
        view.set_diagnostics(findings);
        if let Some(select) = scenario.int("select") {
            view.select_finding_for_testing(select as isize);
        }
        if scenario.bool("ignore") {
            view.ignore_selection_for_testing();
        }
        if scenario.bool("apply") {
            view.apply_safe_fixes_for_testing();
        }
        if scenario.bool("reset") {
            view.reset_ignored_findings();
        }
        if scenario.bool("restyle") {
            view.set_style_sheet(style_sheet);
        }
        self.view = Some(view.clone());
        Ok(Retained::into_super(view))
    }

    fn model(&self) -> Value {
        let Some(view) = &self.view else { return Value::Null };
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(view.preferred_width()));
        map.insert(
            "findings".into(),
            Value::Array(view.diagnostics().into_iter().map(|diagnostic| Value::String(diagnostic.id)).collect()),
        );
        map.insert("rows".into(), table_rows(view));
        Value::Object(map)
    }
}

// MARK: - Shared helpers (`DiagnosticsSceneSupport`)

pub const ROW_LIMIT: isize = 60;

/// `DiagnosticsSceneSupport.tableRows(_:)`: every row the panel's data
/// source reports, asked of a fresh table.
pub fn table_rows(source: &AnyObject) -> Value {
    let mtm = MainThreadMarker::new().expect("scenes run on the main thread");
    let table = NSTableView::new(mtm);
    let count: isize = unsafe { msg_send![source, numberOfRowsInTableView: &*table] };
    let mut rows = Vec::new();
    for row in 0..count.min(ROW_LIMIT) {
        let cell: Option<Retained<NSView>> =
            unsafe { msg_send![source, tableView: &*table, viewForTableColumn: std::ptr::null::<AnyObject>(), row: row] };
        let group: bool = unsafe { msg_send![source, tableView: &*table, isGroupRow: row] };
        let selectable: bool = unsafe { msg_send![source, tableView: &*table, shouldSelectRow: row] };
        let height: CGFloat = unsafe { msg_send![source, tableView: &*table, heightOfRow: row] };
        let mut map = Map::new();
        map.insert("group".into(), Value::Bool(group));
        map.insert("selectable".into(), Value::Bool(selectable));
        map.insert("height".into(), double(height));
        map.insert("class".into(), cell.as_ref().map_or(Value::Null, |cell| Value::String(tree::class_name(cell))));
        map.insert(
            "axLabel".into(),
            cell.as_ref().and_then(|cell| accessibility_label(&**cell)).map_or(Value::Null, Value::String),
        );
        map.insert(
            "toolTip".into(),
            cell.as_ref().and_then(|cell| cell.toolTip()).map_or(Value::Null, |tip| Value::String(tip.to_string())),
        );
        rows.push(Value::Object(map));
    }
    let mut map = Map::new();
    map.insert("count".into(), Value::from(count as i64));
    map.insert("rows".into(), Value::Array(rows));
    Value::Object(map)
}

/// `DiagnosticsSceneSupport.documentURL(_:)`.
pub fn document_url(scenario: &PanelScenario) -> Option<FileUrl> {
    let path = scenario.document_path.as_ref()?;
    let root = FileUrl::from_path_is_directory(&repository_root().to_string_lossy(), true);
    Some(root.appending_path_component(path))
}

/// `DiagnosticsSceneSupport.probe()`.
pub fn probe() -> AssetProbe {
    AssetProbe::new(|url: &FileUrl| {
        let path = url.path();
        let is_directory = upleft_foundation::file_manager::file_exists_is_directory(&path)?;
        let size = std::fs::metadata(&path).ok().map(|metadata| metadata.len() as i64);
        Some(AssetMetadata::new(true, is_directory, size, Some(url.path_extension())))
    })
}

/// `DiagnosticsSceneSupport.relative(_:)`.
pub fn relative(path: &str) -> String {
    let root = repository_root().to_string_lossy().into_owned();
    match path.strip_prefix(&format!("{root}/")) {
        Some(rest) => rest.to_owned(),
        None => path.to_owned(),
    }
}
