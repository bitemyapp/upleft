//! The Rust counterpart of `RenderScenario` in
//! `oracle/Sources/downright-oracle/RenderScenario.swift`: one document
//! driven into a particular state through the renderer's public surface, in
//! the same order the Swift scene uses. Every range is UTF-16 and refers to
//! the text after all edits.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use objc2_foundation::NSString;
use serde_json::Value;
use upleft_core::ast_diff::ASTDiff;
use upleft_core::parser::MarkdownParser;
use upleft_core::{ChangeKind, NSRange, ZoomLevel};
use upleft_render::engine::render_metrics;
use upleft_render::render_contracts::{MarkdownRenderConfiguration, MarkdownRevealPolicy, RenderMode};
use upleft_render::view::markdown_text_view::{ChangeMark, MarkdownTextView};
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use super::{Failure, Request};
use crate::capture::CaptureRequest;

pub struct Edit {
    pub range: NSRange,
    pub text: String,
}

pub struct RenderScenario {
    pub document: PathBuf,
    pub mode: String,
    pub theme: String,
    pub dark: bool,
    pub width: f64,
    pub height: f64,
    pub reduce_motion: bool,
    pub configuration: Option<MarkdownRenderConfiguration>,
    pub initial_text: Option<String>,
    pub edits: Vec<Edit>,
    pub folded_headings: Vec<String>,
    pub zoom: Option<ZoomLevel>,
    pub collapse_code: Vec<(isize, bool)>,
    pub focus_source: Option<NSRange>,
    pub change_marks: Vec<ChangeMark>,
    pub search_hits: Vec<NSRange>,
    pub current_search_hit: Option<NSRange>,
    pub speech_highlight: Option<NSRange>,
    pub selection: Vec<NSRange>,
    pub scroll: Option<(isize, ScrollPosition)>,
}

fn range(value: &Value) -> Option<NSRange> {
    let pair = value.as_array()?;
    if pair.len() != 2 {
        return None;
    }
    Some(NSRange::new(pair[0].as_i64()? as isize, pair[1].as_i64()? as isize))
}

fn ranges(value: &Value) -> Vec<NSRange> {
    value.as_array().map_or_else(Vec::new, |items| items.iter().filter_map(range).collect())
}

fn change_kind(raw: &str) -> Option<ChangeKind> {
    match raw {
        "inserted" => Some(ChangeKind::Inserted),
        "deleted" => Some(ChangeKind::Deleted),
        "modified" => Some(ChangeKind::Modified),
        _ => None,
    }
}

impl RenderScenario {
    pub fn load(path: &Path) -> Result<RenderScenario, Failure> {
        let text = std::fs::read_to_string(path)?;
        let json: Value = serde_json::from_str(&text).map_err(|error| Failure::Error(format!("{}: {error}", path.display())))?;
        let document = json["document"]
            .as_str()
            .ok_or_else(|| Failure::Error(format!("{}: a scenario needs a \"document\"", path.display())))?;
        let base = path.parent().unwrap_or(Path::new("."));
        let document = std::fs::canonicalize(base.join(document)).map_err(|error| Failure::Error(error.to_string()))?;
        let configuration = json["configuration"].as_object().map(|configuration| {
            let flag = |key: &str, default: bool| configuration.get(key).and_then(Value::as_bool).unwrap_or(default);
            let reveal_policy = match configuration.get("revealPolicy").and_then(Value::as_str) {
                Some("never") => MarkdownRevealPolicy::Never,
                Some("allCursors") => MarkdownRevealPolicy::AllCursors,
                _ => MarkdownRevealPolicy::PrimaryCaret,
            };
            MarkdownRenderConfiguration::new(
                flag("showInvisibles", false),
                reveal_policy,
                flag("typographicSubstitution", false),
                flag("typewriterScrolling", false),
                flag("reflowHardWrappedParagraphs", true),
                configuration
                    .get("codeCollapseThreshold")
                    .and_then(Value::as_i64)
                    .unwrap_or(render_metrics::CODE_COLLAPSE_LINE_COUNT),
                5,
            )
        });
        let edits = json["edits"]
            .as_array()
            .map_or_else(Vec::new, |edits| {
                edits
                    .iter()
                    .filter_map(|edit| {
                        Some(Edit {
                            range: NSRange::new(edit["location"].as_i64()? as isize, edit["length"].as_i64()? as isize),
                            text: edit["text"].as_str()?.to_owned(),
                        })
                    })
                    .collect()
            });
        let change_marks = json["changeMarks"].as_array().map_or_else(Vec::new, |marks| {
            marks
                .iter()
                .filter_map(|mark| {
                    let mut change = ChangeMark::new(change_kind(mark["kind"].as_str()?)?, range(&mark["range"])?, ranges(&mark["words"]));
                    change.visited = mark["visited"].as_bool().unwrap_or(false);
                    change.deleted_text = mark["deletedText"].as_str().unwrap_or("").to_owned();
                    Some(change)
                })
                .collect()
        });
        let scroll = json["scroll"].as_object().and_then(|scroll| {
            let offset = scroll.get("offset")?.as_i64()? as isize;
            let position = if scroll.get("position").and_then(Value::as_str) == Some("center") {
                ScrollPosition::Center
            } else {
                ScrollPosition::Top
            };
            Some((offset, position))
        });
        Ok(RenderScenario {
            document,
            mode: json["mode"].as_str().unwrap_or("live").to_owned(),
            theme: json["theme"].as_str().unwrap_or("Paper Light").to_owned(),
            dark: json["dark"].as_bool().unwrap_or(false),
            width: json["width"].as_f64().unwrap_or(1000.0),
            height: json["height"].as_f64().unwrap_or(1400.0),
            reduce_motion: json["reduceMotion"].as_bool().unwrap_or(true),
            configuration,
            initial_text: json["initialText"].as_str().map(str::to_owned),
            edits,
            folded_headings: json["foldedHeadings"].as_array().map_or_else(Vec::new, |slugs| {
                slugs.iter().filter_map(|slug| slug.as_str().map(str::to_owned)).collect()
            }),
            zoom: json["zoom"].as_i64().and_then(|raw| ZoomLevel::from_raw_value(raw as isize)),
            collapse_code: json["collapseCode"].as_array().map_or_else(Vec::new, |entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some((entry["offset"].as_i64()? as isize, entry["collapsed"].as_bool().unwrap_or(true)))
                    })
                    .collect()
            }),
            focus_source: range(&json["focusSource"]),
            change_marks,
            search_hits: ranges(&json["searchHits"]),
            current_search_hit: range(&json["currentSearchHit"]),
            speech_highlight: range(&json["speechHighlight"]),
            selection: ranges(&json["selection"]),
            scroll,
        })
    }

    /// The capture parameters, from the scenario rather than from flags.
    pub fn capture_request(&self, request: &Request) -> CaptureRequest {
        let mut capture = request.capture();
        capture.input = self.document.clone();
        capture.dark = self.dark;
        capture.width = self.width;
        capture.height = self.height;
        capture
    }

    pub fn render_mode(&self) -> RenderMode {
        RenderMode::from_raw_value(&self.mode).unwrap_or(RenderMode::Live)
    }

    /// Everything after the first frame, in the Swift scene's order.
    pub fn apply(&self, text_view: &MarkdownTextView) {
        // SAFETY: the text view owns its storage for the whole call.
        let Some(storage) = (unsafe { text_view.textStorage() }) else { return };
        let mut document = text_view.parsed_document();
        for edit in &self.edits {
            storage.beginEditing();
            storage.replaceCharactersInRange_withString(
                objc2_foundation::NSRange::new(edit.range.location as usize, edit.range.length as usize),
                &NSString::from_str(&edit.text),
            );
            storage.endEditing();
            let fresh = MarkdownParser::parse(&storage.string().to_string());
            let dirty = ASTDiff::dirty_set(Some(&document), &fresh);
            text_view.update(fresh.clone(), &dirty, true);
            document = fresh;
            // One frame per edit, as a live stream would get.
            text_view.prepare_for_display();
        }
        if !self.folded_headings.is_empty() {
            text_view.set_folded_heading_slugs(self.folded_headings.iter().cloned().collect::<HashSet<_>>());
        }
        if let Some(zoom) = self.zoom {
            text_view.set_zoom_level(zoom);
        }
        for &(offset, collapsed) in &self.collapse_code {
            text_view.set_code_block_collapsed(collapsed, offset);
        }
        if let Some(range) = self.focus_source {
            text_view.focus_source(range);
        }
        if !self.change_marks.is_empty() {
            text_view.set_change_marks(self.change_marks.clone());
        }
        if !self.search_hits.is_empty() {
            text_view.set_search_hits(self.search_hits.clone());
        }
        if let Some(hit) = self.current_search_hit {
            text_view.set_current_search_hit(Some(hit));
        }
        if let Some(highlight) = self.speech_highlight {
            text_view.set_speech_highlight(Some(highlight));
        }
        if !self.selection.is_empty() {
            text_view.set_source_selected_ranges(&self.selection);
        }
        if let Some((offset, position)) = self.scroll {
            text_view.scroll_to_offset(offset, position, false);
        }
        text_view.prepare_for_display();
        text_view.displayIfNeeded();
    }
}
