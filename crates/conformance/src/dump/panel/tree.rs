//! `PanelTree` (`oracle/app/.../Panels/PanelTree.swift`): the laid-out view
//! tree of a panel, field for field.

use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{ClassType, Message, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAccessibility, NSButton, NSColor, NSColorSpace, NSControl, NSFont, NSImageView, NSScrollView, NSStackView,
    NSTableView, NSText, NSTextField, NSTextView, NSView, NSWindow,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGColorSpace, CGPath, kCGColorSpaceSRGB};
use objc2_foundation::NSString;
use objc2_quartz_core::{CAGradientLayer, CALayer, CAShapeLayer, CATextLayer};
use serde_json::{Map, Value};

use crate::dump::json::double;

pub const ROW_LIMIT: isize = 400;

fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_owned(), value);
    }
    Value::Object(map)
}

pub fn rect(rect: CGRect) -> Value {
    Value::Array(vec![double(rect.origin.x), double(rect.origin.y), double(rect.size.width), double(rect.size.height)])
}

pub fn size(size: CGSize) -> Value {
    Value::Array(vec![double(size.width), double(size.height)])
}

pub fn point(point: CGPoint) -> Value {
    Value::Array(vec![double(point.x), double(point.y)])
}

fn string(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

/// `PanelTree.className`: `-class` (which, like Swift's `type(of:)`, hides
/// a KVO subclass), whose name for the ported classes equals the Swift
/// unqualified name.
pub fn class_name(object: &AnyObject) -> String {
    let class: &objc2::runtime::AnyClass = unsafe { msg_send![object, class] };
    class.name().to_string_lossy().into_owned()
}

fn is<T: ClassType>(object: &AnyObject) -> bool {
    unsafe { msg_send![object, isKindOfClass: T::class()] }
}

fn cast<T: ClassType + Message>(object: &AnyObject) -> Option<&T> {
    // SAFETY: checked with `isKindOfClass:`.
    is::<T>(object).then(|| unsafe { &*(object as *const AnyObject as *const T) })
}

/// sRGB components, resolved in `view`'s effective appearance.
pub fn color(color: Option<Retained<NSColor>>, view: &NSView) -> Value {
    let Some(color) = color else { return Value::Null };
    let result = RefCell::new(Value::Null);
    let block = RcBlock::new(|| {
        *result.borrow_mut() = match color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) {
            Some(rgb) => Value::Array(vec![
                double(rgb.redComponent()),
                double(rgb.greenComponent()),
                double(rgb.blueComponent()),
                double(rgb.alphaComponent()),
            ]),
            None => Value::String("unconvertible".into()),
        };
    });
    view.effectiveAppearance().performAsCurrentDrawingAppearance(&block);
    drop(block);
    result.into_inner()
}

pub fn cg_color(color: Option<&CGColor>) -> Value {
    let Some(color) = color else { return Value::Null };
    let Some(space) = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })) else {
        return Value::String("unconvertible".into());
    };
    let Some(converted) = (unsafe {
        CGColor::new_copy_by_matching_to_color_space(
            Some(&space),
            objc2_core_graphics::CGColorRenderingIntent::RenderingIntentDefault,
            Some(color),
            None,
        )
    }) else {
        return Value::String("unconvertible".into());
    };
    let count = CGColor::number_of_components(Some(&converted));
    let components = CGColor::components(Some(&converted));
    if components.is_null() {
        return Value::String("unconvertible".into());
    }
    // SAFETY: `components` points at `count` CGFloats owned by the colour.
    let slice = unsafe { std::slice::from_raw_parts(components, count) };
    Value::Array(slice.iter().map(|component| double(*component)).collect())
}

pub fn font(font: Option<Retained<NSFont>>) -> Value {
    let Some(font) = font else { return Value::Null };
    Value::Array(vec![Value::String(font.fontName().to_string()), double(font.pointSize())])
}

pub fn window(window: &NSWindow) -> Value {
    object(vec![
        ("class", Value::String(class_name(window))),
        ("frame", rect(window.frame())),
        ("contentLayoutRect", rect(window.contentLayoutRect())),
        ("styleMask", Value::from(window.styleMask().0 as i64)),
        ("title", Value::String(window.title().to_string())),
        ("opaque", Value::Bool(window.isOpaque())),
        ("hasShadow", Value::Bool(window.hasShadow())),
        ("level", Value::from(window.level() as i64)),
    ])
}

fn layer_delegate_is_view(layer: &CALayer) -> bool {
    let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![layer, delegate] };
    delegate.is_some_and(|delegate| is::<NSView>(&delegate))
}

pub fn layer(layer: &CALayer, view: &NSView) -> Value {
    let mut pairs: Vec<(&str, Value)> = vec![
        ("class", Value::String(class_name(layer))),
        ("frame", rect(layer.frame())),
        ("hidden", Value::Bool(layer.isHidden())),
        ("opacity", double(layer.opacity() as f64)),
        ("cornerRadius", double(layer.cornerRadius())),
        ("masksToBounds", Value::Bool(layer.masksToBounds())),
        ("backgroundColor", cg_color(layer.backgroundColor().as_deref())),
        ("borderWidth", double(layer.borderWidth())),
        ("borderColor", cg_color(layer.borderColor().as_deref())),
        ("shadowOpacity", double(layer.shadowOpacity() as f64)),
        ("shadowRadius", double(layer.shadowRadius())),
        ("zPosition", double(layer.zPosition())),
        ("hasMask", Value::Bool(layer.mask().is_some())),
    ];
    let transform = layer.transform();
    pairs.push((
        "transform",
        Value::Array(vec![
            double(transform.m11),
            double(transform.m12),
            double(transform.m21),
            double(transform.m22),
            double(transform.m41),
            double(transform.m42),
        ]),
    ));
    if let Some(text) = cast::<CATextLayer>(layer) {
        let value: Option<Retained<AnyObject>> = text.string();
        let string = value.and_then(|value| {
            if is::<NSString>(&value) {
                // SAFETY: checked.
                Some(unsafe { &*(Retained::as_ptr(&value) as *const NSString) }.to_string())
            } else {
                None
            }
        });
        pairs.push(("string", string.map_or(Value::Null, Value::String)));
        pairs.push(("fontSize", double(text.fontSize())));
        pairs.push(("foregroundColor", cg_color(text.foregroundColor().as_deref())));
    }
    if let Some(shape) = cast::<CAShapeLayer>(layer) {
        pairs.push(("fillColor", cg_color(shape.fillColor().as_deref())));
        pairs.push(("strokeColor", cg_color(shape.strokeColor().as_deref())));
        pairs.push(("lineWidth", double(shape.lineWidth())));
        pairs.push(("strokeStart", double(shape.strokeStart())));
        pairs.push(("strokeEnd", double(shape.strokeEnd())));
        pairs.push((
            "pathBounds",
            shape.path().map_or(Value::Null, |path| rect(CGPath::path_bounding_box(Some(&path)))),
        ));
    }
    if let Some(gradient) = cast::<CAGradientLayer>(layer) {
        let colors = gradient.colors();
        let colors: Vec<Value> = colors
            .map(|colors| {
                colors
                    .iter()
                    .map(|color| cg_color(Some(unsafe { &*(Retained::as_ptr(&color) as *const CGColor) })))
                    .collect()
            })
            .unwrap_or_default();
        pairs.push(("colors", Value::Array(colors)));
        pairs.push(("startPoint", point(gradient.startPoint())));
        pairs.push(("endPoint", point(gradient.endPoint())));
    }
    let own: Vec<Value> = unsafe { layer.sublayers() }
        .map(|sublayers| {
            sublayers
                .iter()
                .filter(|sublayer| !layer_delegate_is_view(sublayer))
                .map(|sublayer| self::layer(&sublayer, view))
                .collect()
        })
        .unwrap_or_default();
    pairs.push(("sublayers", Value::Array(own)));
    object(pairs)
}

pub fn dump(view: &NSView) -> Value {
    let mut pairs: Vec<(&str, Value)> = vec![
        ("class", Value::String(class_name(view))),
        ("frame", rect(view.frame())),
        ("bounds", rect(view.bounds())),
        ("hidden", Value::Bool(view.isHidden())),
        ("alpha", double(view.alphaValue())),
        ("intrinsicContentSize", size(view.intrinsicContentSize())),
    ];
    if let Some(field) = cast::<NSTextField>(view) {
        pairs.push(("text", Value::String(field.stringValue().to_string())));
        pairs.push(("font", font(field.font())));
        pairs.push(("textColor", color(field.textColor(), view)));
        pairs.push(("alignment", Value::from(field.alignment().0 as i64)));
        pairs.push(("lineBreakMode", Value::from(field.lineBreakMode().0 as i64)));
        pairs.push(("maximumNumberOfLines", Value::from(field.maximumNumberOfLines() as i64)));
        pairs.push(("editable", Value::Bool(field.isEditable())));
        pairs.push(("placeholder", string(field.placeholderString().map(|s| s.to_string()))));
    }
    if let Some(button) = cast::<NSButton>(view) {
        pairs.push(("title", Value::String(button.title().to_string())));
        pairs.push(("state", Value::from(button.state() as i64)));
        pairs.push(("font", font(button.font())));
        pairs.push(("hasImage", Value::Bool(button.image().is_some())));
        pairs.push(("contentTintColor", color(button.contentTintColor(), view)));
        pairs.push(("bezelStyle", Value::from(button.bezelStyle().0 as i64)));
        pairs.push(("bordered", Value::Bool(button.isBordered())));
        pairs.push(("keyEquivalent", Value::String(button.keyEquivalent().to_string())));
    }
    if let Some(control) = cast::<NSControl>(view) {
        pairs.push(("enabled", Value::Bool(control.isEnabled())));
    }
    if let Some(image_view) = cast::<NSImageView>(view) {
        pairs.push(("hasImage", Value::Bool(image_view.image().is_some())));
        pairs.push(("contentTintColor", color(image_view.contentTintColor(), view)));
    }
    if let Some(text_view) = cast::<NSTextView>(view) {
        pairs.push(("string", Value::String(NSText::string(text_view).to_string())));
        pairs.push(("font", font(NSText::font(text_view))));
        pairs.push(("textColor", color(NSText::textColor(text_view), view)));
        pairs.push(("backgroundColor", color(NSText::backgroundColor(text_view), view)));
    }
    if let Some(table) = cast::<NSTableView>(view) {
        pairs.push(("rows", Value::from(table.numberOfRows() as i64)));
        pairs.push(("selectedRow", Value::from(table.selectedRow() as i64)));
        let count = table.numberOfRows().min(ROW_LIMIT);
        pairs.push(("rowRects", Value::Array((0..count).map(|row| rect(table.rectOfRow(row))).collect())));
    }
    if let Some(stack) = cast::<NSStackView>(view) {
        pairs.push(("spacing", double(stack.spacing())));
        pairs.push(("orientation", Value::from(stack.orientation().0 as i64)));
    }
    if let Some(scroll) = cast::<NSScrollView>(view) {
        pairs.push(("documentVisibleRect", rect(scroll.documentVisibleRect())));
    }
    pairs.push(("toolTip", string(view.toolTip().map(|s| s.to_string()))));
    pairs.push(("axRole", string(view.accessibilityRole().map(|s| s.to_string()))));
    pairs.push(("axLabel", string(view.accessibilityLabel().map(|s| s.to_string()))));
    let value = view.accessibilityValue();
    let value = value.and_then(|value| {
        if is::<NSString>(&value) {
            Some(unsafe { &*(Retained::as_ptr(&value) as *const NSString) }.to_string())
        } else {
            None
        }
    });
    pairs.push(("axValue", string(value)));
    pairs.push(("axHelp", string(view.accessibilityHelp().map(|s| s.to_string()))));
    pairs.push(("layer", view.layer().map_or(Value::Null, |layer| self::layer(&layer, view))));
    let subviews: Vec<Value> = view.subviews().iter().map(|subview| dump(&subview)).collect();
    pairs.push(("subviews", Value::Array(subviews)));
    object(pairs)
}

#[allow(unused)]
fn _unused(_: &dyn NSObjectProtocol) {}
