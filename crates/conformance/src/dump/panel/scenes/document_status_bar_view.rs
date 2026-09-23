//! `DocumentStatusBarView` scenes (`Scenes/DocumentStatusBarViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::NSRange;
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::ns_string;
use upleft_app::panels::document_status_bar_view::DocumentStatusBarView;
use upleft_app::panels::panel_chrome::SourceLineIndex;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct DocumentStatusBarViewScene {
    bar: Option<Retained<DocumentStatusBarView>>,
}

impl PanelScene for DocumentStatusBarViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let bar = DocumentStatusBarView::new(style_sheet.clone(), mtm);
        let cursor: Vec<isize> = scenario
            .array("cursor")
            .iter()
            .filter_map(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
            .map(|value| value as isize)
            .collect();
        if cursor.len() == 2 {
            bar.set_cursor_position(Some((cursor[0], cursor[1])));
        }
        if let Some(offset) = scenario.int("caretOffset") {
            let text = scenario.document_text().map_err(Failure::Error)?;
            let index = SourceLineIndex::new(&text);
            let line = index.line(offset as isize);
            let line_range = ns_string(&text).lineRangeForRange(NSRange::new(offset as usize, 0));
            bar.set_cursor_position(Some((line, offset as isize - line_range.location as isize + 1)));
        }
        if scenario.bool("unsaved") {
            bar.set_has_file_url(false);
        }
        if scenario.bool("saved") {
            bar.set_has_file_url(true);
        }
        if scenario.bool("clearCursor") {
            bar.set_cursor_position(None);
        }
        if scenario.bool("invisible") {
            bar.set_is_visible(false);
        }
        if scenario.bool("reshow") {
            bar.set_is_visible(true);
        }
        if scenario.bool("restyle") {
            bar.set_style_sheet(style_sheet);
        }
        self.bar = Some(bar.clone());
        Ok(Retained::into_super(bar))
    }

    fn model(&self) -> Value {
        let Some(bar) = &self.bar else { return Value::Null };
        let mut map = Map::new();
        map.insert(
            "cursorPosition".into(),
            bar.cursor_position()
                .map_or(Value::Null, |(line, column)| Value::Array(vec![line.into(), column.into()])),
        );
        map.insert("hasFileURL".into(), Value::Bool(bar.has_file_url()));
        map.insert("isVisible".into(), Value::Bool(bar.is_visible()));
        map.insert("intrinsicContentSize".into(), tree::size(bar.intrinsicContentSize()));
        Value::Object(map)
    }
}
