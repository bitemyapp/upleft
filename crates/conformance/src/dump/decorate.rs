//! Rust counterpart of `oracle/Sources/downright-oracle/AttributeDump.swift`
//! (`storage`, `value`, `paragraphStyle`, `fragmentPayload`) and the
//! `decorate` command in `main.swift`.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{NSColor, NSFont, NSParagraphStyle, NSShadow, NSTextAttachment, NSTextStorage};
use objc2_foundation::{NSAttributedString, NSAttributedStringEnumerationOptions, NSDictionary, NSNumber, NSString, NSURL};
use serde_json::Value;
use upleft_core::parser::MarkdownParser;
use upleft_core::{DirtySet, NSRange};
use upleft_render::engine::decoration_engine::{DecorationEngine, DecorationResult};
use upleft_render::render_contracts::{FragmentPayload, RenderMode};
use upleft_render::swift_value::{BlockIdentityValue, PathTokenValue};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use super::attribute_dump::{color_json, font_json, number_json};
use super::json::{Object, double};
use super::style_sheet::appearance_named;
use super::{Failure, Request, parse};

/// `AttributeDump.storage(_:)`: every attribute run, keys sorted.
pub fn storage(storage: &NSAttributedString) -> Value {
    let length = storage.length();
    let runs = std::cell::RefCell::new(Vec::new());
    let block = block2::RcBlock::new(
        |attributes: std::ptr::NonNull<NSDictionary<NSString, AnyObject>>,
         range: objc2_foundation::NSRange,
         _stop: std::ptr::NonNull<objc2::runtime::Bool>| {
            // SAFETY: the enumeration hands over a live dictionary.
            let attributes = unsafe { attributes.as_ref() };
            let mut pairs: Vec<(String, Retained<AnyObject>)> = Vec::new();
            for key in attributes.allKeys().iter() {
                if let Some(value) = attributes.objectForKey(&key) {
                    pairs.push((key.to_string(), value));
                }
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut object = Object::new();
            for (key, value) in &pairs {
                object = object.with(key, self::value(value));
            }
            runs.borrow_mut().push(
                Object::new()
                    .with("range", super::json::range(range.location, range.length))
                    .with("attributes", object.build())
                    .build(),
            );
        },
    );
    storage.enumerateAttributesInRange_options_usingBlock(
        objc2_foundation::NSRange::new(0, length),
        NSAttributedStringEnumerationOptions::empty(),
        &block,
    );
    drop(block);
    Object::new()
        .with("length", length as i64)
        .with("runs", Value::Array(runs.into_inner()))
        .build()
}

/// `AttributeDump.value(_:)`.
pub fn value(value: &AnyObject) -> Value {
    if let Some(font) = value.downcast_ref::<NSFont>() {
        return font_json(font);
    }
    if let Some(color) = value.downcast_ref::<NSColor>() {
        return color_json(color);
    }
    if let Some(style) = value.downcast_ref::<NSParagraphStyle>() {
        return paragraph_style(style);
    }
    if let Some(payload) = value.downcast_ref::<FragmentPayload>() {
        return fragment_payload(payload);
    }
    if let Some(identity) = value.downcast_ref::<BlockIdentityValue>() {
        let identity = identity.identity();
        return Object::new()
            .with("type", "BlockIdentity")
            .with("kind", identity.kind as i64)
            .with("ordinal", identity.ordinal as i64)
            .build();
    }
    if let Some(token) = value.downcast_ref::<PathTokenValue>() {
        return Object::new()
            .with("type", "PathToken")
            .with("token", parse::path_token(token.token()))
            .build();
    }
    if let Some(url) = value.downcast_ref::<NSURL>() {
        return Object::new()
            .with("type", "URL")
            .with("absoluteString", url.absoluteString().map_or(String::new(), |s| s.to_string()))
            .build();
    }
    if let Some(shadow) = value.downcast_ref::<NSShadow>() {
        let offset = shadow.shadowOffset();
        return Object::new()
            .with("type", "NSShadow")
            .with("offset", Value::Array(vec![double(offset.width), double(offset.height)]))
            .with("blurRadius", double(shadow.shadowBlurRadius()))
            .with("color", shadow.shadowColor().map_or(Value::Null, |color| color_json(&color)))
            .build();
    }
    if let Some(attachment) = value.downcast_ref::<NSTextAttachment>() {
        let bounds = attachment.bounds();
        return Object::new()
            .with("type", "NSTextAttachment")
            .with("class", attachment.class().name().to_string_lossy().into_owned())
            .with(
                "bounds",
                Value::Array(vec![
                    double(bounds.origin.x),
                    double(bounds.origin.y),
                    double(bounds.size.width),
                    double(bounds.size.height),
                ]),
            )
            .build();
    }
    if let Some(number) = value.downcast_ref::<NSNumber>() {
        return number_json(number);
    }
    if let Some(string) = value.downcast_ref::<NSString>() {
        return Object::new().with("type", "String").with("value", string.to_string()).build();
    }
    Object::new()
        .with("type", "unknown")
        .with("class", value.class().name().to_string_lossy().into_owned())
        .with("description", format!("{value:?}"))
        .build()
}

/// `AttributeDump.paragraphStyle(_:)`.
pub fn paragraph_style(style: &NSParagraphStyle) -> Value {
    let tab_stops: Vec<Value> = style
        .tabStops()
        .iter()
        .map(|tab| Value::Array(vec![(tab.alignment().0 as i64).into(), double(tab.location())]))
        .collect();
    let text_blocks: Vec<Value> = style
        .textBlocks()
        .iter()
        .map(|block| Value::String(block.class().name().to_string_lossy().into_owned()))
        .collect();
    let text_lists: Vec<Value> = style
        .textLists()
        .iter()
        .map(|list| Value::String(list.markerFormat().to_string()))
        .collect();
    Object::new()
        .with("type", "NSParagraphStyle")
        .with("alignment", style.alignment().0 as i64)
        .with("firstLineHeadIndent", double(style.firstLineHeadIndent()))
        .with("headIndent", double(style.headIndent()))
        .with("tailIndent", double(style.tailIndent()))
        .with("lineBreakMode", style.lineBreakMode().0 as i64)
        .with("maximumLineHeight", double(style.maximumLineHeight()))
        .with("minimumLineHeight", double(style.minimumLineHeight()))
        .with("lineSpacing", double(style.lineSpacing()))
        .with("paragraphSpacing", double(style.paragraphSpacing()))
        .with("paragraphSpacingBefore", double(style.paragraphSpacingBefore()))
        .with("baseWritingDirection", style.baseWritingDirection().0 as i64)
        .with("lineHeightMultiple", double(style.lineHeightMultiple()))
        .with("defaultTabInterval", double(style.defaultTabInterval()))
        .with("tabStops", Value::Array(tab_stops))
        .with("hyphenationFactor", double(style.hyphenationFactor() as f64))
        .with("usesDefaultHyphenation", style.usesDefaultHyphenation())
        .with("tighteningFactorForTruncation", double(style.tighteningFactorForTruncation() as f64))
        .with("allowsDefaultTighteningForTruncation", style.allowsDefaultTighteningForTruncation())
        .with("lineBreakStrategy", style.lineBreakStrategy().0 as i64)
        .with("headerLevel", style.headerLevel() as i64)
        .with("textBlocks", Value::Array(text_blocks))
        .with("textLists", Value::Array(text_lists))
        .build()
}

/// `AttributeDump.fragmentPayload(_:)`.
pub fn fragment_payload(payload: &FragmentPayload) -> Value {
    Object::new()
        .with("type", "FragmentPayload")
        .with("kind", payload.kind().raw_value())
        .with("sourceRange", parse::range(payload.source_range()))
        .with("blockIdentity", parse::identity(payload.block_identity()))
        .with("detail", payload.detail())
        .with("hasTableData", payload.table_data().is_some())
        .with("isCollapsed", payload.is_collapsed())
        .build()
}

/// `NSTextStorage(string:)`.
pub fn text_storage(text: &str) -> Retained<NSTextStorage> {
    let string = NSString::from_str(text);
    // SAFETY: `initWithString:` on a freshly allocated NSTextStorage.
    unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*string] }
}

/// The engine the `decorate` family of commands uses: the named theme under
/// the requested appearance, the requested mode's policy.
pub fn engine(request: &Request) -> Result<DecorationEngine, Failure> {
    let appearance = appearance_named(request.dark);
    let store = ThemeStore::shared();
    let Some(theme) = store.themes().into_iter().find(|theme| theme.name == request.theme) else {
        let names: Vec<String> = store.themes().into_iter().map(|theme| theme.name).collect();
        return Err(Failure::Error(format!("unknown theme {}; have {names:?}", request.theme)));
    };
    let sheet = StyleSheet::new(theme, &appearance, Some(true));
    let mut engine = DecorationEngine::new(sheet);
    let mode = RenderMode::from_raw_value(&request.mode).expect("mode validated by Request::parse");
    engine.set_policy(mode.policy());
    Ok(engine)
}

/// `DecorationResult` without its wall-clock time.
pub fn result(result: DecorationResult) -> Value {
    Object::new()
        .with("attributeRanges", result.attribute_ranges as i64)
        .with("fragmentCount", result.fragment_count as i64)
        .build()
}

/// `decorate <file.md> <out.json> [--mode M] [--theme NAME] [--dark]`.
pub fn run(request: &Request) -> Result<(), Failure> {
    let text = super::markup::read_text(&request.input)?;
    let mut engine = engine(request)?;
    let storage = text_storage(&text);
    engine.decorate(&storage, &MarkdownParser::parse(&text), &DirtySet::wholesale());
    super::json::write(&self::storage(&storage), &request.output)?;
    Ok(())
}

/// `[location, length]` for an engine range.
pub fn range(range: NSRange) -> Value {
    parse::range(range)
}
