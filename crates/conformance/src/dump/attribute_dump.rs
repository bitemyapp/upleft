//! Rust counterpart of `oracle/Sources/downright-oracle/AttributeDump.swift`:
//! the canonical serialisations of `NSFont`, `NSColor` and `NSNumber`
//! attribute values. Floating-point values are compared bit for bit.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSColor, NSColorType, NSFont, NSFontFeatureSettingsAttribute, NSFontVariationAttribute,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use serde_json::Value;

use super::json::{Object, double};

unsafe extern "C" {
    fn CFGetTypeID(object: *const AnyObject) -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFNumberGetType(number: *const AnyObject) -> isize;
}

/// `numberJSON`: `CFBoolean` is a `Bool`, float-typed numbers are `Double`,
/// everything else is `Int`.
pub fn number_json(number: &NSNumber) -> Value {
    let pointer = number as *const NSNumber as *const AnyObject;
    // SAFETY: an NSNumber is a CFNumber or CFBoolean (toll-free bridged).
    let (type_id, boolean_type) = unsafe { (CFGetTypeID(pointer), CFBooleanGetTypeID()) };
    if type_id == boolean_type {
        return Object::new().with("type", "Bool").with("value", number.boolValue()).build();
    }
    // SAFETY: as above; not a CFBoolean, so a CFNumber.
    let kind = unsafe { CFNumberGetType(pointer) };
    // kCFNumberFloat32Type, Float64, Float, Double, CGFloat.
    if matches!(kind, 5 | 6 | 12 | 13 | 16) {
        return Object::new().with("type", "Double").with("value", double(number.doubleValue())).build();
    }
    Object::new().with("type", "Int").with("value", number.integerValue() as i64).build()
}

/// `value(_:)` for the value kinds font descriptors carry.
pub fn value(object: &AnyObject) -> Value {
    if let Some(number) = object.downcast_ref::<NSNumber>() {
        return number_json(number);
    }
    if let Some(font) = object.downcast_ref::<NSFont>() {
        return font_json(font);
    }
    if let Some(color) = object.downcast_ref::<NSColor>() {
        return color_json(color);
    }
    if let Some(string) = object.downcast_ref::<NSString>() {
        return Object::new().with("type", "String").with("value", string.to_string()).build();
    }
    Object::new().with("type", "unknown").with("class", object.class().name().to_string_lossy().into_owned()).build()
}

pub fn font_json(font: &NSFont) -> Value {
    let descriptor = font.fontDescriptor();
    let mut features = Vec::new();
    // SAFETY: AppKit exports the attribute names as immutable globals.
    let (settings_key, variation_key) = unsafe { (NSFontFeatureSettingsAttribute, NSFontVariationAttribute) };
    if let Some(settings) = descriptor.objectForKey(settings_key)
        && let Ok(settings) = settings.downcast::<NSArray>()
    {
        for setting in settings.iter() {
            let Ok(setting) = setting.downcast::<NSDictionary>() else { continue };
            let mut pairs: Vec<(String, Retained<AnyObject>)> = Vec::new();
            for key in setting.allKeys().iter() {
                let Ok(name) = key.clone().downcast::<NSString>() else { continue };
                if let Some(entry) = setting.objectForKey(&key) {
                    pairs.push((name.to_string(), entry));
                }
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut object = Object::new();
            for (key, entry) in pairs {
                object = object.with(&key, value(&entry));
            }
            features.push(object.build());
        }
    }
    let mut variation = Vec::new();
    if let Some(axes) = descriptor.objectForKey(variation_key)
        && let Ok(axes) = axes.downcast::<NSDictionary>()
    {
        let mut pairs: Vec<(i64, f64)> = Vec::new();
        for key in axes.allKeys().iter() {
            let (Ok(axis), Some(entry)) = (key.clone().downcast::<NSNumber>(), axes.objectForKey(&key)) else { continue };
            let Ok(entry) = entry.downcast::<NSNumber>() else { continue };
            pairs.push((axis.integerValue() as i64, entry.doubleValue()));
        }
        pairs.sort_by_key(|(axis, _)| *axis);
        for (axis, amount) in pairs {
            variation.push(Value::Array(vec![axis.into(), double(amount)]));
        }
    }
    Object::new()
        .with("type", "NSFont")
        .with("postScriptName", font.fontName().to_string())
        .with("familyName", font.familyName().map_or(Value::Null, |name| name.to_string().into()))
        .with("pointSize", double(font.pointSize()))
        .with("symbolicTraits", descriptor.symbolicTraits().0 as i64)
        .with("features", Value::Array(features))
        .with("variation", Value::Array(variation))
        .with("ascender", double(font.ascender()))
        .with("descender", double(font.descender()))
        .with("leading", double(font.leading()))
        .build()
}

pub fn color_json(color: &NSColor) -> Value {
    let kind = color.r#type();
    if kind == NSColorType::Catalog {
        return Object::new()
            .with("type", "NSColor")
            .with("colorType", "catalog")
            .with("catalog", color.catalogNameComponent().to_string())
            .with("name", color.colorNameComponent().to_string())
            .build();
    }
    if kind == NSColorType::ComponentBased {
        let space = color.colorSpace();
        let count = color.numberOfComponents().max(0) as usize;
        let mut components = vec![0.0f64; count.max(1)];
        // SAFETY: the buffer holds `numberOfComponents` values.
        unsafe { color.getComponents(std::ptr::NonNull::new(components.as_mut_ptr()).unwrap()) };
        components.truncate(count);
        let space_name = space
            .localizedName()
            .map(|name| name.to_string())
            .unwrap_or_else(|| space.colorSpaceModel().0.to_string());
        return Object::new()
            .with("type", "NSColor")
            .with("colorType", "componentBased")
            .with("colorSpace", space_name)
            .with("components", Value::Array(components.into_iter().map(double).collect()))
            .build();
    }
    if kind == NSColorType::Pattern {
        return Object::new().with("type", "NSColor").with("colorType", "pattern").build();
    }
    Object::new()
        .with("type", "NSColor")
        .with("colorType", "unknown")
        .with("description", format!("{color:?}"))
        .build()
}
