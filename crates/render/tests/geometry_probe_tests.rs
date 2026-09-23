//! Port of `Tests/MarkdownRenderTests/GeometryProbeTests.swift`, the parts
//! outside `MathRenderer` (whose two tests live in `upleft-math`'s
//! `downright_math_tests`).

use objc2::rc::Retained;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSParagraphStyle, NSTextStorage};
use objc2_foundation::NSString;
use upleft_core::health::document_health::DocumentHealth;
use upleft_core::parser::MarkdownParser;
use upleft_core::DirtySet;
use upleft_render::appkit_compat::{attribute_at, keys};
use upleft_render::engine::decoration_engine::DecorationEngine;
use upleft_render::engine::render_metrics;
use upleft_render::render_contracts::{RenderMode, Theme};
use upleft_render::theme::style_sheet::StyleSheet;

fn decorated(text: &str) -> Retained<NSTextStorage> {
    let storage: Retained<NSTextStorage> =
        unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(text)] };
    let document = MarkdownParser::parse(text);
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let sheet = StyleSheet::new(Theme::fallback(), &appearance, None);
    let mut engine = DecorationEngine::new(sheet);
    engine.set_policy(RenderMode::Read.policy());
    engine.decorate(&storage, &document, &DirtySet::wholesale());
    storage
}

fn offset(text: &str, needle: &str) -> usize {
    NSString::from_str(text).rangeOfString(&NSString::from_str(needle)).location
}

fn head_indent(storage: &NSTextStorage, offset: usize) -> Option<f64> {
    if offset >= storage.length() {
        return None;
    }
    attribute_at(storage, keys::paragraph_style(), offset)
        .and_then(|(value, _)| value.downcast::<NSParagraphStyle>().ok())
        .map(|style| style.headIndent())
}

#[test]
fn nested_task_keeps_its_indent() {
    let text = "- [ ] top task\n  - [x] nested task\n    - [ ] doubly nested\n- [ ] sibling task";
    let storage = decorated(text);
    let top = head_indent(&storage, offset(text, "top task")).unwrap();
    let nested = head_indent(&storage, offset(text, "nested task")).unwrap();
    let doubly = head_indent(&storage, offset(text, "doubly nested")).unwrap();
    let sibling = head_indent(&storage, offset(text, "sibling task")).unwrap();
    assert!(nested > top, "nested item lost its indent");
    assert!(doubly > nested, "doubly-nested item lost its indent");
    assert!((sibling - top).abs() < 0.5, "sibling task should match the top level");
}

#[test]
fn paragraph_style_never_leaks_across_children() {
    let text = "- [ ] parent\n  - [x] child one\n  - [ ] child two";
    let storage = decorated(text);
    let child = offset(text, "child one");
    let (value, child_style) = attribute_at(&storage, keys::paragraph_style(), child).expect("a style");
    assert!(value.downcast::<NSParagraphStyle>().is_ok());
    let parent = offset(text, "parent");
    assert!(child_style.location > parent, "child paragraph style leaked back onto the parent line");
}

#[test]
fn code_block_keeps_its_paragraph_style_across_fences() {
    let text = "```swift\nlet x = 1\nlet y = 2\n```";
    let storage = decorated(text);
    let indent = |at: usize| head_indent(&storage, at).unwrap_or(-1.0);
    let fence = indent(offset(text, "```swift"));
    let body = indent(offset(text, "let x = 1"));
    let close = indent(offset(text, "```"));
    assert!(fence > 0.0, "opening fence lost the code paragraph style");
    assert!(body > 0.0, "code body lost the code paragraph style");
    assert!(close > 0.0, "closing fence lost the code paragraph style");
    assert!((fence - body).abs() < 0.5 && (close - body).abs() < 0.5, "fences and body must share one indent");
}

#[test]
fn task_marker_column_reserves_checkbox_geometry() {
    let text = "- [ ] top\n  - [ ] nested";
    let storage = decorated(text);
    let top = head_indent(&storage, offset(text, "top")).unwrap_or(0.0);
    assert!(top >= render_metrics::task_marker_column(), "task marker column is too narrow for the checkbox");
}

#[test]
fn bullet_and_task_at_same_depth_keep_separate_columns() {
    let text = "- alpha\n- [ ] alpha";
    let storage = decorated(text);
    let ns = NSString::from_str(text);
    let bullet_offset = offset(text, "alpha");
    let rest = objc2_foundation::NSRange::new(bullet_offset + 1, ns.length() - bullet_offset - 1);
    let task_offset = ns
        .rangeOfString_options_range(&NSString::from_str("alpha"), objc2_foundation::NSStringCompareOptions(0), rest)
        .location;
    let bullet = head_indent(&storage, bullet_offset).unwrap_or(0.0);
    let task = head_indent(&storage, task_offset).unwrap_or(0.0);
    assert!(task >= render_metrics::task_marker_column(), "task lost its checkbox column to a cached bullet style");
    assert!(bullet < render_metrics::task_marker_column(), "bullet took the task checkbox column");
    assert!(bullet < task, "both rows share one marker column");
}

#[test]
fn malformed_front_matter_is_reported() {
    let bad = DocumentHealth::analyze("t---\ntitle: Sample\n---\n\nBody");
    assert!(bad.iter().any(|diagnostic| diagnostic.id == "frontmatter.malformed-delimiter"));
    let ok = DocumentHealth::analyze("---\ntitle: Sample\n---\n\nBody");
    assert!(!ok.iter().any(|diagnostic| diagnostic.id.starts_with("frontmatter.")));
}

#[test]
fn task_label_excludes_children() {
    let text = "- [ ] Notarise and publish\n  - [ ] Sparkle appcast\n  - [x] Ad-hoc signing for local runs";
    let document = MarkdownParser::parse(text);
    assert_eq!(document.tasks.len(), 3);
    assert_eq!(document.tasks[0].text, "Notarise and publish");
    assert_eq!(document.tasks[1].text, "Sparkle appcast");
    assert_eq!(document.tasks[2].text, "Ad-hoc signing for local runs");
}
