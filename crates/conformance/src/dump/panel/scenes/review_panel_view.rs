//! `ReviewPanelView` scenes (`Scenes/ReviewPanelViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSScrollView, NSStackView, NSTableView, NSView};
use objc2_foundation::{NSIndexSet, NSRange as FoundationRange, NSString};
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::PanelSurface;
use upleft_app::panels::review_panel_view::{ReviewPanelView, ReviewPanelViewDelegate};
use upleft_app::review::review_anchor_resolver::{CONTEXT_LENGTH, ReviewAnchorResolver};
use upleft_app::review::review_sidecar::{ReviewItem, ReviewKind, ReviewSidecarEngine, ReviewState};
use upleft_core::NSRange;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
struct Delegate {
    events: RefCell<Vec<String>>,
}

impl Delegate {
    fn push(&self, verb: &str, review: &ReviewItem) {
        self.events.borrow_mut().push(format!("{verb} {} {}", review.title(), review.anchor.range.location));
    }
}

impl ReviewPanelViewDelegate for Delegate {
    fn review_panel_did_select(&self, _panel: &ReviewPanelView, review: &ReviewItem) {
        self.push("select", review);
    }

    fn review_panel_did_apply(&self, _panel: &ReviewPanelView, review: &ReviewItem) {
        self.push("apply", review);
    }

    fn review_panel_did_reject(&self, _panel: &ReviewPanelView, review: &ReviewItem) {
        self.push("reject", review);
    }

    fn review_panel_did_resolve(&self, _panel: &ReviewPanelView, review: &ReviewItem) {
        self.push("resolve", review);
    }
}

fn int(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))
}

#[derive(Default)]
pub struct ReviewPanelViewScene {
    panel: Option<Retained<ReviewPanelView>>,
    delegate: Option<Rc<Delegate>>,
    statuses: Vec<String>,
}

impl PanelScene for ReviewPanelViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = scenario.document_text().map_err(Failure::Error)?;
        let mut source = text.clone();
        for edit in scenario.array("edits") {
            let Some(values) = edit.as_array() else { continue };
            let (Some(location), Some(length), Some(replacement)) =
                (values.first().and_then(int), values.get(1).and_then(int), values.get(2).and_then(Value::as_str))
            else {
                continue;
            };
            if values.len() != 3 {
                continue;
            }
            let string = upleft_app::panels::appkit_support::ns_string(&source);
            source = string
                .stringByReplacingCharactersInRange_withString(
                    FoundationRange::new(location as usize, length as usize),
                    &NSString::from_str(replacement),
                )
                .to_string();
        }
        let reviews: Vec<ReviewItem> = scenario
            .array("reviews")
            .iter()
            .filter_map(|value| {
                let object = value.as_object()?;
                let range: Vec<i64> =
                    object.get("range").and_then(Value::as_array).map(|items| items.iter().filter_map(int).collect())?;
                if range.len() != 2 {
                    return None;
                }
                let kind = ReviewKind::from_raw_value(object.get("kind").and_then(Value::as_str).unwrap_or("comment"))?;
                let mut review = ReviewSidecarEngine::make_review(
                    kind,
                    &text,
                    NSRange::new(range[0] as isize, range[1] as isize),
                    object.get("body").and_then(Value::as_str).unwrap_or(""),
                    object.get("replacement").and_then(Value::as_str),
                )?;
                review.state = ReviewState::from_raw_value(object.get("state").and_then(Value::as_str).unwrap_or("open"))
                    .unwrap_or(ReviewState::Open);
                Some(review)
            })
            .collect();
        let panel =
            if scenario.bool("current") { ReviewPanelView::new_current(mtm) } else { ReviewPanelView::new(style_sheet.clone(), mtm) };
        if scenario.bool("current") {
            panel.set_style_sheet(style_sheet);
        }
        let delegate = Rc::new(Delegate::default());
        let weak: std::rc::Weak<dyn ReviewPanelViewDelegate> =
            Rc::downgrade(&(delegate.clone() as Rc<dyn ReviewPanelViewDelegate>));
        panel.set_delegate(Some(weak));
        self.delegate = Some(delegate);
        if !scenario.bool("noSource") {
            panel.set_source_text(&source);
        }
        panel.set_reviews(reviews.clone());
        let panel_source = panel.source_text();
        self.statuses = reviews
            .iter()
            .map(|review| {
                ReviewAnchorResolver::resolve(&review.anchor, &panel_source, CONTEXT_LENGTH).status.raw_value().to_owned()
            })
            .collect();
        let subviews = panel.subviews();
        let table = subviews.iter().find_map(|view| {
            let scroll = view.downcast::<NSScrollView>().ok()?;
            scroll.documentView()?.downcast::<NSTableView>().ok()
        });
        if let Some(row) = scenario.int("select")
            && let Some(table) = &table
        {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        }
        let buttons: Vec<Retained<NSButton>> = subviews
            .iter()
            .filter_map(|view| view.downcast::<NSStackView>().ok())
            .flat_map(|stack| {
                stack.arrangedSubviews().iter().filter_map(|view| view.downcast::<NSButton>().ok()).collect::<Vec<_>>()
            })
            .collect();
        for name in scenario.strings("press") {
            let mut characters = name.chars();
            let title = match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            };
            let Some(button) = buttons.iter().find(|button| button.title().to_string() == title) else { continue };
            unsafe { button.sendAction_to(button.action(), button.target().as_deref()) };
        }
        self.panel = Some(panel.clone());
        Ok(Retained::into_super(panel))
    }

    fn model(&self) -> Value {
        let Some(panel) = &self.panel else { return Value::Null };
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(panel.preferred_width()));
        map.insert("reviewCount".into(), Value::from(panel.reviews().len() as i64));
        map.insert("statuses".into(), Value::Array(self.statuses.iter().cloned().map(Value::String).collect()));
        let events = self.delegate.as_ref().map(|delegate| delegate.events.borrow().clone()).unwrap_or_default();
        map.insert("events".into(), Value::Array(events.into_iter().map(Value::String).collect()));
        map.insert("fittingSize".into(), tree::size(panel.fittingSize()));
        Value::Object(map)
    }
}
